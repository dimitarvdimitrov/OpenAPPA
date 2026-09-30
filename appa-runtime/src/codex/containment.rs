//! Inherited process-group restrictions. Command text is only an early UX check.

pub(crate) const DETACHED_ERROR: &str =
    "Detached background processes are unsupported. Run this command in the foreground.";

pub(crate) fn check_text(command: &str) -> Result<(), String> {
    // Deliberately recognize only literal launches at shell command boundaries.
    // Quoted arguments, comments, scripts, aliases and computed names are not
    // evidence of containment. The child restriction supplies that separately.
    static LAUNCH: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?m)(?:^|[;&|()]|\n)[ \t]*(?:(?:exec|command)[ \t]+)?(?:/[A-Za-z0-9_./-]+/)?(?:(?:setsid|disown|systemd-run)(?:[ \t;&|()]|$)|launchctl[ \t]+(?:submit|bootstrap|kickstart)(?:[ \t]|$))",
        )
        .unwrap()
    });
    // Mask quoted text and comments so examples and command arguments do not
    // look like launches. Abstain on here-documents rather than parse data as
    // shell code. Computed or quoted executable names can bypass this aid.
    if command.contains("<<") {
        return Ok(());
    }
    let mut visible = String::with_capacity(command.len());
    let mut quote = None;
    let mut escaped = false;
    let mut comment = false;
    let mut boundary = true;
    for ch in command.chars() {
        if comment {
            if ch == '\n' {
                comment = false;
                visible.push(ch);
            } else {
                visible.push('x');
            }
        } else if escaped {
            visible.push('x');
            escaped = false;
        } else if let Some(end) = quote {
            visible.push('x');
            if ch == end {
                quote = None;
            } else if ch == '\\' && end == '"' {
                escaped = true;
            }
        } else if ch == '\\' {
            visible.push('x');
            escaped = true;
        } else if ch == '\'' || ch == '"' {
            visible.push('x');
            quote = Some(ch);
        } else if ch == '#' && boundary {
            comment = true;
            visible.push('x');
        } else {
            visible.push(ch);
        }
        boundary = ch.is_whitespace() || ";&|()".contains(ch);
    }
    if LAUNCH.is_match(&visible) {
        return Err(DETACHED_ERROR.into());
    }
    static NOHUP: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?m)(?:^|[;&|()]|\n)[ \t]*(?:/[A-Za-z0-9_./-]+/)?nohup[ \t]+").unwrap()
    });
    for launch in NOHUP.find_iter(&visible) {
        let rest = &visible.as_bytes()[launch.end()..];
        for (index, byte) in rest.iter().enumerate() {
            if b";|()\n".contains(byte) {
                break;
            }
            if *byte != b'&' || index > 0 && b"<>".contains(&rest[index - 1]) {
                continue;
            }
            match rest.get(index + 1) {
                Some(b'&') => break,    // foreground command with &&
                Some(b'>') => continue, // combined output redirection
                _ => return Err(DETACHED_ERROR.into()),
            }
        }
    }
    Ok(())
}

// Use libc, which the runtime already requires. No helper, root privilege,
// libseccomp package, user namespace or cgroup delegation is required.
#[cfg(all(target_os = "linux", any(target_arch = "aarch64", target_arch = "x86_64")))]
pub(crate) fn restrict() -> std::io::Result<()> {
    const LD_W_ABS: u16 = 0x20;
    const JMP_JEQ_K: u16 = 0x15;
    const JMP_JSET_K: u16 = 0x45;
    const RET_K: u16 = 0x06;
    const ALLOW: u32 = 0x7fff0000;
    const ERRNO: u32 = 0x00050000;
    #[cfg(target_arch = "aarch64")]
    const ARCH: u32 = 0xc00000b7;
    #[cfg(target_arch = "x86_64")]
    const ARCH: u32 = 0xc000003e;
    const fn instruction(code: u16, jt: u8, jf: u8, k: u32) -> libc::sock_filter {
        libc::sock_filter { code, jt, jf, k }
    }
    // All storage exists before prctl. This function runs after fork and must
    // not allocate or acquire a lock. Reject alternate ABIs, including x32.
    let filter = [
        instruction(LD_W_ABS, 0, 0, 4), // seccomp_data.arch
        instruction(JMP_JEQ_K, 1, 0, ARCH),
        instruction(RET_K, 0, 0, ERRNO | libc::EPERM as u32),
        instruction(LD_W_ABS, 0, 0, 0), // seccomp_data.nr
        instruction(JMP_JSET_K, 0, 1, 0x40000000),
        instruction(RET_K, 0, 0, ERRNO | libc::EPERM as u32),
        instruction(JMP_JEQ_K, 0, 1, libc::SYS_setsid as u32),
        instruction(RET_K, 0, 0, ERRNO | libc::EPERM as u32),
        instruction(JMP_JEQ_K, 0, 1, libc::SYS_setpgid as u32),
        instruction(RET_K, 0, 0, ERRNO | libc::EPERM as u32),
        instruction(JMP_JEQ_K, 0, 1, libc::SYS_unshare as u32),
        instruction(RET_K, 0, 0, ERRNO | libc::EPERM as u32),
        instruction(JMP_JEQ_K, 0, 1, libc::SYS_setns as u32),
        instruction(RET_K, 0, 0, ERRNO | libc::EPERM as u32),
        instruction(JMP_JEQ_K, 0, 1, libc::SYS_ptrace as u32),
        instruction(RET_K, 0, 0, ERRNO | libc::EPERM as u32),
        instruction(JMP_JEQ_K, 0, 1, libc::SYS_process_vm_writev as u32),
        instruction(RET_K, 0, 0, ERRNO | libc::EPERM as u32),
        // clone3's flags are behind a pointer. ENOSYS permits libc's fallback
        // to clone, whose namespace flags we can examine without a race.
        instruction(JMP_JEQ_K, 0, 1, libc::SYS_clone3 as u32),
        instruction(RET_K, 0, 0, ERRNO | libc::ENOSYS as u32),
        instruction(JMP_JEQ_K, 0, 3, libc::SYS_clone as u32),
        instruction(LD_W_ABS, 0, 0, 16),           // seccomp_data.args[0], low flags
        instruction(JMP_JSET_K, 0, 1, 0x7e020000), // CLONE_NEW* namespaces
        instruction(RET_K, 0, 0, ERRNO | libc::EPERM as u32),
        instruction(RET_K, 0, 0, ALLOW),
    ];
    let program = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_ptr().cast_mut(),
    };
    unsafe {
        if libc::prctl(
            libc::PR_SET_NO_NEW_PRIVS,
            1 as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
        ) != 0
            || libc::prctl(
                libc::PR_SET_SECCOMP,
                2 as libc::c_ulong,
                &program,
                0 as libc::c_ulong,
                0 as libc::c_ulong,
            ) != 0
        {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(all(target_os = "linux", not(any(target_arch = "aarch64", target_arch = "x86_64"))))]
pub(crate) fn restrict() -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Codex process restrictions support Linux aarch64 and x86_64 only",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_launches_receive_a_foreground_hint() {
        for command in [
            "setsid sleep 2",
            "/usr/bin/setsid sleep 2",
            "echo ok; disown",
            "command setsid x",
            "exec setsid x",
            "nohup sleep 2 &",
            "nohup sleep 2 & wait",
            "nohup sleep 2 >/dev/null 2>&1 &",
            "systemd-run --user sleep 2",
            "launchctl submit -l example -- sleep 2",
        ] {
            assert_eq!(check_text(command).unwrap_err(), DETACHED_ERROR);
        }
        for command in [
            "sleep 0.1 & wait",
            "nohup printf hello",
            "nohup printf hello 2>&1",
            "nohup printf hello && sleep 0.1 & wait",
            "launchctl list",
            "printf '%s' setsid",
            "printf 'example; setsid x'",
            "# setsid x\necho ok",
            "python3 script.py",
            "echo disown",
            "cat <<EOF\nsetsid example\nEOF",
        ] {
            assert!(check_text(command).is_ok(), "{command}");
        }
    }
}
