//! The sandboxed half of a Codex command. Child bytes stay private until
//! the runtime admits a complete result.

use std::io::{Read, Write};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::loopback_http::{Deadline, Endpoint, request_for_sandboxed_wrapper};

use super::context::{Specification, validate_shell};
use super::jobs::MAX_OUTPUT;

#[cfg(unix)]
static CANCELLED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn cancel(_signal: libc::c_int) {
    CANCELLED.store(true, Ordering::Relaxed);
}

#[cfg(unix)]
fn install_cancellation() {
    // Only touch an atomic in the handler. The main loop kills the child's
    // process group, including ordinary descendants in that group.
    unsafe {
        libc::signal(libc::SIGINT, cancel as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, cancel as *const () as libc::sighandler_t);
    }
}

#[cfg(unix)]
fn was_cancelled() -> bool {
    CANCELLED.load(Ordering::Relaxed)
}

#[cfg(not(unix))]
fn was_cancelled() -> bool {
    false
}

#[cfg(unix)]
fn kill_child_group(child: &mut ChildGroup) {
    if child.cleaned {
        return;
    }
    unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
    let _ = child.kill();
    child.cleaned = true;
}

#[cfg(not(unix))]
fn kill_child_group(child: &mut ChildGroup) {
    let _ = child.kill();
    child.cleaned = true;
}

struct ChildGroup {
    child: std::process::Child,
    cleaned: bool,
}

impl std::ops::Deref for ChildGroup {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.child
    }
}

impl std::ops::DerefMut for ChildGroup {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}

impl Drop for ChildGroup {
    fn drop(&mut self) {
        kill_child_group(self);
        let _ = self.child.wait();
    }
}

#[derive(Serialize)]
struct Report {
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
}

#[derive(Deserialize)]
struct Admission {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

fn capture<R: Read>(mut reader: R, total: Arc<AtomicUsize>, overflow: Arc<AtomicBool>) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            return Ok(output);
        }
        let previous = total.fetch_add(count, Ordering::Relaxed);
        if previous.saturating_add(count) > MAX_OUTPUT {
            overflow.store(true, Ordering::Relaxed);
            return Ok(Vec::new());
        }
        output.extend_from_slice(&chunk[..count]);
    }
}

fn execute(specification: Specification, mut still_authorized: impl FnMut() -> bool) -> Result<Report, String> {
    if specification.command.is_empty() || specification.cwd.is_empty() {
        return Err("the approved command specification is incomplete".into());
    }
    validate_shell(&specification.shell)?;
    super::containment::check_text(&specification.command)?;
    let shell = std::path::Path::new(&specification.shell);
    // Child stdin stays closed. This proxy supports finite commands without
    // later input.
    #[cfg(unix)]
    let mut command = Command::new(shell);
    #[cfg(not(unix))]
    let mut command = Command::new("cmd.exe");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.arg(if specification.login { "-lc" } else { "-c" });
        // A fresh session removes the inherited controlling terminal. The
        // child cannot bypass our pipes by opening /dev/tty.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                #[cfg(target_os = "linux")]
                super::containment::restrict()?;
                Ok(())
            });
        }
        install_cancellation();
    }
    #[cfg(not(unix))]
    command.arg("/C");
    // A Stop/Interrupt can arrive after /consume and before process creation.
    // Check immediately before spawn so a cancelled job never uses the first
    // periodic poll as a launch grace period.
    if !still_authorized() {
        return Ok(Report {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
        });
    }
    let child = command
        // Login startup files can change directories. Restore the approved
        // directory after startup, before the actual command.
        .arg(format!(
            "cd {} || exit $?\n{}",
            super::jobs::shell_literal(&specification.cwd),
            specification.command
        ))
        .current_dir(&specification.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("the approved command could not start: {error}"))?;
    let mut child = ChildGroup { child, cleaned: false };
    let total = Arc::new(AtomicUsize::new(0));
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout = child.stdout.take().ok_or("the child has no stdout pipe")?;
    let stderr = child.stderr.take().ok_or("the child has no stderr pipe")?;
    let (out_tx, out_rx) = std::sync::mpsc::channel();
    let (err_tx, err_rx) = std::sync::mpsc::channel();
    let _out_thread = {
        let total = total.clone();
        let overflow = overflow.clone();
        std::thread::spawn(move || {
            let _ = out_tx.send(capture(stdout, total, overflow));
        })
    };
    let _err_thread = {
        let total = total.clone();
        let overflow = overflow.clone();
        std::thread::spawn(move || {
            let _ = err_tx.send(capture(stderr, total, overflow));
        })
    };
    let mut next_check = Instant::now() + Duration::from_millis(250);
    let status = loop {
        if Instant::now() >= next_check {
            if !still_authorized() {
                kill_child_group(&mut child);
                let _ = child.wait();
                return Ok(Report {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: None,
                });
            }
            next_check = Instant::now() + Duration::from_millis(250);
        }
        if overflow.load(Ordering::Relaxed) || was_cancelled() {
            kill_child_group(&mut child);
            break child.wait().map_err(|error| error.to_string())?;
        }
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) => break status,
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    // The shell may have exited while a background descendant still holds a
    // pipe open. Terminate the original group before settling the result.
    kill_child_group(&mut child);
    // Bound capture even if a platform escape or an external process retains
    // a pipe. A drain timeout is not evidence of descendant teardown.
    let stdout = match out_rx.recv_timeout(Duration::from_secs(1)) {
        Ok(Ok(bytes)) => bytes,
        _ => {
            return Ok(Report {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: None,
            });
        }
    };
    let stderr = match err_rx.recv_timeout(Duration::from_secs(1)) {
        Ok(Ok(bytes)) => bytes,
        _ => {
            return Ok(Report {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: None,
            });
        }
    };
    if overflow.load(Ordering::Relaxed) || was_cancelled() {
        return Ok(Report {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
        });
    }
    let base64 = base64::engine::general_purpose::STANDARD;
    Ok(Report {
        stdout: base64.encode(stdout),
        stderr: base64.encode(stderr),
        exit_code: status.code(),
    })
}

fn run_inner(url: &str, handle: &str) -> Result<i32, String> {
    if !url.starts_with("http://127.0.0.1:") || uuid::Uuid::parse_str(handle).is_err() {
        return Err("invalid Codex job endpoint or handle".into());
    }
    let endpoint = Endpoint::parse(url)?;
    let proxy = std::env::var("HTTP_PROXY")
        .or_else(|_| std::env::var("http_proxy"))
        .ok();
    let request = |method: &str, path: &str, body: &[u8], seconds| {
        request_for_sandboxed_wrapper(
            &endpoint,
            method,
            path,
            body,
            &Deadline::spanning(Duration::from_secs(seconds)),
            proxy.as_deref(),
        )
    };
    let path = format!("/codex/job/{handle}/consume");
    let answer = request("GET", &path, b"", 5)?;
    if !answer.is_success() {
        return Err(format!("the Codex job was not available (status {})", answer.status));
    }
    let specification: Specification =
        serde_json::from_slice(&answer.body).map_err(|error| format!("invalid Codex job specification: {error}"))?;
    let running_path = format!("/codex/job/{handle}/running");
    let result = execute(specification, || {
        request("GET", &running_path, b"", 3).is_ok_and(|answer| answer.status == 204)
    });
    let (report, failure) = match result {
        Ok(report) => (report, None),
        Err(error) => (
            Report {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: None,
            },
            Some(error),
        ),
    };
    let body = serde_json::to_vec(&report).map_err(|error| error.to_string())?;
    let path = format!("/codex/job/{handle}/report");
    let answer = request("POST", &path, &body, 120)?;
    if let Some(error) = failure {
        return Err(error);
    }
    if !answer.is_success() {
        return Err(format!(
            "the runtime withheld the command result (status {})",
            answer.status
        ));
    }
    let admission: Admission =
        serde_json::from_slice(&answer.body).map_err(|error| format!("invalid Codex result admission: {error}"))?;
    let base64 = base64::engine::general_purpose::STANDARD;
    let stdout = base64.decode(admission.stdout).map_err(|_| "invalid admitted stdout")?;
    let stderr = base64.decode(admission.stderr).map_err(|_| "invalid admitted stderr")?;
    if stdout.len().saturating_add(stderr.len()) > MAX_OUTPUT {
        return Err("the admitted result is too large".into());
    }
    std::io::stdout()
        .write_all(&stdout)
        .map_err(|error| error.to_string())?;
    std::io::stderr()
        .write_all(&stderr)
        .map_err(|error| error.to_string())?;
    Ok(admission.exit_code)
}

pub fn run(url: &str, handle: &str) -> ExitCode {
    match run_inner(url, handle) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(error) => {
            eprintln!("OpenAPPA withheld the Codex command: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn output_overflow_stops_the_child_and_withholds_all_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("continued-after-overflow");
        let report = execute(
            Specification {
                command: format!(
                    "dd if=/dev/zero bs=1024 count={} 2>/dev/null; sleep 2; touch '{}'",
                    MAX_OUTPUT / 1024 + 1,
                    marker.display()
                ),
                shell: "/bin/sh".into(),
                login: true,
                cwd: dir.path().to_string_lossy().into_owned(),
            },
            || true,
        )
        .unwrap();
        assert_eq!(report.exit_code, None);
        assert!(report.stdout.is_empty());
        assert!(report.stderr.is_empty());
        assert!(!marker.exists());
    }

    fn alive(pid: i32) -> bool {
        let output = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let state = String::from_utf8_lossy(&output.stdout);
        !state.trim().is_empty() && !state.trim().starts_with('Z')
    }

    struct ProbeProcess(i32);
    impl Drop for ProbeProcess {
        fn drop(&mut self) {
            // Clean up even if an assertion exposes a containment regression.
            unsafe { libc::kill(self.0, libc::SIGKILL) };
        }
    }

    #[test]
    fn ordinary_background_work_can_finish_before_shell_exit() {
        let dir = tempfile::tempdir().unwrap();
        let report = execute(
            Specification {
                command: "(sleep 0.1; echo done > finished; printf child) & wait; printf parent".into(),
                shell: "/bin/sh".into(),
                login: false,
                cwd: dir.path().to_string_lossy().into_owned(),
            },
            || true,
        )
        .unwrap();
        assert_eq!(report.exit_code, Some(0));
        assert_eq!(std::fs::read_to_string(dir.path().join("finished")).unwrap(), "done\n");
        assert_eq!(
            base64::engine::general_purpose::STANDARD.decode(report.stdout).unwrap(),
            b"childparent"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_spawn_attributes_are_blocked_and_ordinary_threads_still_work() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("spawn.py"),
            "import os,ctypes,threading\nassert ctypes.CDLL(None).prctl(39,0,0,0,0)==1\nfor options in [{'setsid':True},{'setpgroup':0}]:\n try:\n  p=os.posix_spawn('/bin/sh',['sh','-c','sleep 0.3; echo escaped > late'],os.environ,**options)\n except PermissionError:\n  continue\n os.kill(p,9); os.waitpid(p,0)\n raise AssertionError('spawn attributes escaped')\nt=threading.Thread(target=lambda: open('thread','w').write('done'))\nt.start(); t.join()\n"
        ).unwrap();
        let report = execute(
            Specification {
                command: "python3 spawn.py".into(),
                shell: "/bin/sh".into(),
                login: false,
                cwd: dir.path().to_string_lossy().into_owned(),
            },
            || true,
        )
        .unwrap();
        assert_eq!(
            report.exit_code,
            Some(0),
            "{}",
            String::from_utf8_lossy(&base64::engine::general_purpose::STANDARD.decode(report.stderr).unwrap())
        );
        assert_eq!(std::fs::read_to_string(dir.path().join("thread")).unwrap(), "done");
        std::thread::sleep(Duration::from_millis(400));
        assert!(!dir.path().join("late").exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn scripts_cannot_escape_group_cleanup_with_or_without_output_pipes() {
        assert!(
            Command::new("python3")
                .arg("--version")
                .output()
                .unwrap()
                .status
                .success()
        );
        for operation in ["os.setsid()", "os.setpgid(0, 0)"] {
            for pipes in [
                "pass",
                "os.dup2(os.open(os.devnull, os.O_WRONLY), 1); os.dup2(1, 2)",
                "os.close(1); os.close(2)",
            ] {
                for cancelled in [false, true] {
                    let dir = tempfile::tempdir().unwrap();
                    // The attempted detach lives in a script, outside the text check.
                    // Catch EPERM and keep running to prove cleanup kills the child.
                    std::fs::write(dir.path().join("child.py"), format!(
                        "import os,time\ntry:\n {operation}\nexcept PermissionError:\n open('blocked','w').write('yes')\n{pipes}\nopen('pid','w').write(str(os.getpid()))\nopen('ready','w').write('yes')\ntime.sleep(1.2)\nopen('late','w').write('escaped')\n"
                    )).unwrap();
                    let command = format!(
                        "python3 child.py & i=0; while [ ! -f ready ] && [ \"$i\" -lt 200 ]; do sleep 0.01; i=$((i+1)); done; {}",
                        if cancelled { "wait" } else { ":" }
                    );
                    let report = execute(
                        Specification {
                            command,
                            shell: "/bin/sh".into(),
                            login: false,
                            cwd: dir.path().to_string_lossy().into_owned(),
                        },
                        || !cancelled || !dir.path().join("ready").exists(),
                    )
                    .unwrap();
                    let pid = std::fs::read_to_string(dir.path().join("pid"))
                        .unwrap()
                        .parse()
                        .unwrap();
                    let process = ProbeProcess(pid);
                    assert!(dir.path().join("blocked").exists(), "{operation} escaped");
                    assert_eq!(report.exit_code, if cancelled { None } else { Some(0) });
                    std::thread::sleep(Duration::from_millis(1300));
                    assert!(!alive(process.0), "descendant {} survived", process.0);
                    assert!(!dir.path().join("late").exists(), "descendant wrote after cleanup");
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_session_escape_remains_a_limit_with_any_pipe_mode() {
        for pipes in [
            "pass",
            "os.dup2(os.open(os.devnull, os.O_WRONLY), 1); os.dup2(1, 2)",
            "os.close(1); os.close(2)",
        ] {
            for cancelled in [false, true] {
                let dir = tempfile::tempdir().unwrap();
                std::fs::write(dir.path().join("escape.py"), format!(
                    "import os,time\nos.setsid()\n{pipes}\nopen('pid','w').write(str(os.getpid()))\nopen('ready','w').write('yes')\ntime.sleep(2)\nopen('late','w').write('escaped')\n"
                )).unwrap();
                let report = execute(Specification {
                    command: format!("python3 escape.py & i=0; while [ ! -f ready ] && [ $i -lt 200 ]; do sleep 0.01; i=$((i+1)); done; {}", if cancelled { "wait" } else { ":" }),
                    shell: "/bin/sh".into(), login: false,
                    cwd: dir.path().to_string_lossy().into_owned(),
                }, || !cancelled || !dir.path().join("ready").exists()).unwrap();
                let process = ProbeProcess(
                    std::fs::read_to_string(dir.path().join("pid"))
                        .unwrap()
                        .parse()
                        .unwrap(),
                );
                assert_eq!(
                    report.exit_code,
                    if cancelled || pipes == "pass" { None } else { Some(0) }
                );
                assert!(alive(process.0), "update the macOS containment limit if this changes");
                std::thread::sleep(Duration::from_millis(2100));
                assert!(dir.path().join("late").exists(), "the escaped process could not write");
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_spawn_attributes_remain_an_explicit_containment_limit() {
        let dir = tempfile::tempdir().unwrap();
        // POSIX_SPAWN_SETSID also bypasses the syscall-only Seatbelt design
        // evaluated for macOS. Keep this evidence until the boundary changes.
        std::fs::write(dir.path().join("spawn.py"),
            "import os\np=os.posix_spawn('/bin/sh',['sh','-c','sleep 0.3; echo survived > late'],os.environ,setsid=True)\nopen('pid','w').write(str(p))\n"
        ).unwrap();
        let report = execute(
            Specification {
                command: "python3 spawn.py >/dev/null 2>&1".into(),
                shell: "/bin/sh".into(),
                login: false,
                cwd: dir.path().to_string_lossy().into_owned(),
            },
            || true,
        )
        .unwrap();
        let process = ProbeProcess(
            std::fs::read_to_string(dir.path().join("pid"))
                .unwrap()
                .parse()
                .unwrap(),
        );
        assert_eq!(report.exit_code, Some(0));
        assert!(
            alive(process.0),
            "update the documented macOS limit if this boundary changes"
        );
        std::thread::sleep(Duration::from_millis(500));
        assert!(dir.path().join("late").exists());
    }

    #[test]
    fn cancellation_stops_an_ordinary_grandchild() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("too-late");
        let command = format!("(sleep 1; touch '{}') & wait", marker.display());
        let started = Instant::now();
        let report = execute(
            Specification {
                command,
                shell: "/bin/sh".into(),
                login: true,
                cwd: dir.path().to_string_lossy().into_owned(),
            },
            || started.elapsed() < Duration::from_millis(300),
        )
        .unwrap();
        assert_eq!(report.exit_code, None);
        std::thread::sleep(Duration::from_secs(1));
        assert!(!marker.exists(), "cancelled grandchild kept running");
    }

    #[test]
    fn cancellation_between_consume_and_spawn_never_launches_child() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("launched");
        let report = execute(
            Specification {
                command: format!("touch '{}'", marker.display()),
                shell: "/bin/sh".into(),
                login: true,
                cwd: dir.path().to_string_lossy().into_owned(),
            },
            || false,
        )
        .unwrap();
        assert_eq!(report.exit_code, None);
        assert!(!marker.exists());
    }
}
