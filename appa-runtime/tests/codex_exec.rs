//! The child can produce secret bytes; only the runtime's admitted replacement
//! reaches the host command result.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;

use base64::Engine as _;

mod common;

fn read_request(socket: &mut TcpStream) -> (String, Vec<u8>) {
    let mut reader = BufReader::new(socket.try_clone().unwrap());
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().unwrap();
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    (first, body)
}

fn answer(socket: &mut TcpStream, json: serde_json::Value) {
    let body = json.to_string();
    write!(
        socket,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
}

#[test]
fn wrapper_emits_only_the_admitted_result() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = uuid::Uuid::new_v4().to_string();
    let cwd = std::env::current_dir().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let (request, _) = read_request(&mut socket);
        assert!(request.contains("/consume"));
        answer(
            &mut socket,
            serde_json::json!({"command":"printf secret; printf hidden >&2", "shell":"/bin/sh", "login":true, "cwd":cwd}),
        );
        let (mut socket, body) = loop {
            let (mut socket, _) = listener.accept().unwrap();
            let (request, body) = read_request(&mut socket);
            if request.contains("/running") {
                socket
                    .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
                    .unwrap();
                continue;
            }
            assert!(request.contains("/report"));
            break (socket, body);
        };
        let report: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let base64 = base64::engine::general_purpose::STANDARD;
        assert_eq!(base64.decode(report["stdout"].as_str().unwrap()).unwrap(), b"secret");
        assert_eq!(base64.decode(report["stderr"].as_str().unwrap()).unwrap(), b"hidden");
        answer(
            &mut socket,
            serde_json::json!({
                "stdout": base64.encode("admitted"), "stderr": "", "exit_code": 0
            }),
        );
    });
    let output = Command::new(env!("CARGO_BIN_EXE_appa"))
        .args(["codex-exec", "--url", &url, &handle])
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout, b"admitted");
    assert!(output.stderr.is_empty());
}

#[test]
fn outer_pipe_input_does_not_reach_the_child_or_echo() {
    use std::process::Stdio;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = uuid::Uuid::new_v4().to_string();
    let cwd = std::env::current_dir().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let (request, _) = read_request(&mut socket);
        assert!(request.contains("/consume"));
        answer(
            &mut socket,
            serde_json::json!({
                "command":"IFS= read -r input || :; printf '%s' \"$input\"",
                "shell":"/bin/sh", "login":true, "cwd":cwd
            }),
        );
        let (mut socket, body) = loop {
            let (mut socket, _) = listener.accept().unwrap();
            let (request, body) = read_request(&mut socket);
            if request.contains("/running") {
                socket
                    .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
                    .unwrap();
                continue;
            }
            assert!(request.contains("/report"));
            break (socket, body);
        };
        let report: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(report["stdout"], "");
        assert_eq!(report["stderr"], "");
        answer(
            &mut socket,
            serde_json::json!({
                "stdout": base64::engine::general_purpose::STANDARD.encode("closed"),
                "stderr": "", "exit_code": 0
            }),
        );
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_appa"))
        .args(["codex-exec", "--url", &url, &handle])
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"NO-ECHO-MARKER\n").unwrap();
    let output = child.wait_with_output().unwrap();
    server.join().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout, b"closed");
    assert!(output.stderr.is_empty());
}

#[test]
fn lost_result_admission_never_releases_child_output() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = uuid::Uuid::new_v4().to_string();
    let cwd = std::env::current_dir().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let (request, _) = read_request(&mut socket);
        assert!(request.contains("/consume"));
        answer(
            &mut socket,
            serde_json::json!({"command":"printf private-stdout; printf private-stderr >&2", "shell":"/bin/sh", "login":true, "cwd":cwd}),
        );
        let (mut socket, _) = listener.accept().unwrap();
        let (request, _) = read_request(&mut socket);
        assert!(request.contains("/running"));
        socket
            .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .unwrap();
        let (mut socket, _) = listener.accept().unwrap();
        let (request, body) = read_request(&mut socket);
        assert!(request.contains("/report"));
        let report: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let base64 = base64::engine::general_purpose::STANDARD;
        assert_eq!(
            base64.decode(report["stdout"].as_str().unwrap()).unwrap(),
            b"private-stdout"
        );
        assert_eq!(
            base64.decode(report["stderr"].as_str().unwrap()).unwrap(),
            b"private-stderr"
        );
        // The runtime disconnects after it receives the report.
        drop(socket);
    });
    let output = Command::new(env!("CARGO_BIN_EXE_appa"))
        .args(["codex-exec", "--url", &url, &handle])
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.windows(7).any(|bytes| bytes == b"private"));
}

#[test]
fn codex_hook_prepares_and_settles_a_runtime_owned_command() {
    use std::process::Stdio;

    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("policy.toml");
    std::fs::write(&config, "[policy]\nversion = 2\n[[policy.tool]]\nname = \"host/codex/appa_exec\"\n[externals]\ntimeout_ms = 5000\nmax_body_bytes = 105000000\n").unwrap();
    let mut runtime = Command::new(env!("CARGO_BIN_EXE_appa"))
        .args(["runtime", "--adapter", "codex", "--config"])
        .arg(&config)
        .arg("--db")
        .arg(dir.path().join("appa.db"))
        .args(["--listen", "127.0.0.1:0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let url = common::served_url(&mut runtime);
    let hook = |event: serde_json::Value| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_appa"))
            .args(["hook", "--adapter", "codex", "--deployment-url", &url])
            .env("APPA_GATE", "1")
            .env("SHELL", "/bin/sh")
            .env_remove("APPA_RUNTIME_URL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(event.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        if event["hook_event_name"] == "Stop" {
            assert!(output.stdout.is_empty());
            return serde_json::json!({});
        }
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let original = concat!(
        "dd if=/dev/zero bs=1048576 count=50 2>/dev/null | tr '\\000' x; ",
        "dd if=/dev/zero bs=1048576 count=50 2>/dev/null | tr '\\000' x >&2"
    );
    let pre = hook(serde_json::json!({
        "hook_event_name":"PreToolUse", "session_id":"s1", "tool_name":"Bash",
        "tool_use_id":"c1", "cwd":dir.path(), "tool_input":{"command":original}
    }));
    assert_eq!(pre["hookSpecificOutput"]["permissionDecision"], "allow", "{pre}");
    let wrapper = pre["hookSpecificOutput"]["updatedInput"]["command"].as_str().unwrap();
    let wrapper_for_first = wrapper.to_owned();
    assert!(!wrapper.contains(original));
    let result = Command::new("/bin/sh")
        .arg("-c")
        .arg(wrapper)
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .output()
        .unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    // Exactly 100 MiB across both streams must survive capture and admission.
    assert_eq!(result.stdout.len(), 50 * 1024 * 1024);
    assert_eq!(result.stderr.len(), 50 * 1024 * 1024);
    assert!(result.stdout.iter().all(|byte| *byte == b'x'));
    assert!(result.stderr.iter().all(|byte| *byte == b'x'));
    let pre = hook(serde_json::json!({
        "hook_event_name":"PreToolUse", "session_id":"s1", "tool_name":"Bash",
        "tool_use_id":"c2", "cwd":dir.path(),
        "tool_input":{"command":"printf diagnostic; exit 7"}
    }));
    let wrapper = pre["hookSpecificOutput"]["updatedInput"]["command"].as_str().unwrap();
    let result = Command::new("/bin/sh")
        .arg("-c")
        .arg(wrapper)
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(7));
    assert_eq!(result.stdout, b"diagnostic");
    let post = hook(serde_json::json!({
        "hook_event_name":"PostToolUse", "session_id":"s1", "tool_name":"Bash",
        "tool_use_id":"c2", "cwd":dir.path(), "tool_input":{"command":wrapper},
        "tool_response":{"stdout":"diagnostic", "exit_code":7}
    }));
    assert_eq!(post, serde_json::json!({}));
    let duplicate = hook(serde_json::json!({
        "hook_event_name":"PostToolUse", "session_id":"s1", "tool_name":"Bash",
        "tool_use_id":"c2", "cwd":dir.path(), "tool_input":{"command":wrapper},
        "tool_response":{"stdout":"diagnostic", "exit_code":7}
    }));
    assert_eq!(duplicate, serde_json::json!({}));
    for (session, agent) in [("other-session", None), ("s1", Some("other-agent"))] {
        let mut foreign = serde_json::json!({
            "hook_event_name":"PostToolUse", "session_id":session, "tool_name":"Bash",
            "tool_use_id":"c2", "cwd":dir.path(), "tool_input":{"command":wrapper},
            "tool_response":{"stdout":"diagnostic", "exit_code":7}
        });
        if let Some(agent) = agent {
            foreign["agent_id"] = serde_json::json!(agent);
        }
        assert_eq!(hook(foreign)["decision"], "block");
    }

    let pre = hook(serde_json::json!({
        "hook_event_name":"PreToolUse", "session_id":"s1", "tool_name":"Bash",
        "tool_use_id":"c3", "cwd":dir.path(),
        "tool_input":{"command":"sleep 5; printf late-secret"}
    }));
    let wrapper = pre["hookSpecificOutput"]["updatedInput"]["command"].as_str().unwrap();
    let running = Command::new("/bin/sh")
        .arg("-c")
        .arg(wrapper)
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    let _ = hook(serde_json::json!({"hook_event_name":"Stop", "session_id":"s1"}));
    let result = running.wait_with_output().unwrap();
    assert!(!result.status.success());
    assert!(!result.stdout.windows(11).any(|bytes| bytes == b"late-secret"));
    assert!(!result.stderr.windows(11).any(|bytes| bytes == b"late-secret"));
    let late = hook(serde_json::json!({
        "hook_event_name":"PostToolUse", "session_id":"s1", "tool_name":"Bash",
        "tool_use_id":"c1", "cwd":dir.path(), "tool_input":{"command":wrapper_for_first},
        "tool_response":"original-result"
    }));
    assert_eq!(late["decision"], "block");
    let _ = runtime.kill();
    let _ = runtime.wait();
}

#[test]
fn explicit_context_preserves_directory_shell_login_and_credential_selectors() {
    use std::process::Stdio;

    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("policy.toml");
    std::fs::write(&config, "[policy]\nversion = 2\n[[policy.tool]]\nname = \"host/codex/appa_exec(command:cat .env)\"\ndelta = { audience = [\"self\"] }\ntags = [\"credentials\"]\n[[policy.tool]]\nname = \"host/codex/appa_exec\"\n[externals]\ntimeout_ms = 5000\nmax_body_bytes = 65536\n").unwrap();
    let mut runtime = Command::new(env!("CARGO_BIN_EXE_appa"))
        .args(["runtime", "--adapter", "codex", "--config"])
        .arg(&config)
        .arg("--db")
        .arg(dir.path().join("appa.db"))
        .args(["--listen", "127.0.0.1:0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let url = common::served_url(&mut runtime);
    let hook = |event: serde_json::Value| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_appa"))
            .args(["hook", "--adapter", "codex", "--deployment-url", &url])
            .env("APPA_GATE", "1")
            .env("SHELL", "/bin/sh")
            .env_remove("APPA_RUNTIME_URL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(event.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        if !output.status.success() {
            assert_eq!(output.status.code(), Some(2));
            assert_eq!(event["hook_event_name"], "PreToolUse");
            if !output.stdout.is_empty() {
                let answer: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                assert!(answer["hookSpecificOutput"]["updatedInput"].is_null(), "{answer}");
            }
            return serde_json::json!({"denied":true});
        }
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        if event["hook_event_name"] == "Stop" {
            assert!(output.stdout.is_empty());
            return serde_json::json!({});
        }
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let nested = dir.path().join("nested ' quoted");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(dir.path().join("selected.txt"), "wrong-root").unwrap();
    std::fs::write(nested.join("selected.txt"), "nested-result").unwrap();
    let startup = dir.path().join("bash-startup");
    std::fs::write(&startup, "export APPA_CWD_STARTUP=loaded\ncd /\n").unwrap();
    for login in [false, true] {
        let metadata = serde_json::json!({"workdir": nested, "shell": "/bin/bash", "login": login});
        let command = format!(
            "# appa-codex-exec-v1 {metadata}\nprintf '%s\\n' \"$0\"; if shopt -q login_shell; then printf 'login\\n'; else printf 'plain\\n'; fi; printf '%s\\n' \"${{APPA_CWD_STARTUP-absent}}\"; pwd; cat selected.txt"
        );
        let pre = hook(serde_json::json!({
            "hook_event_name":"PreToolUse", "session_id":"s1", "tool_name":"Bash",
            "tool_use_id":format!("context-{login}"), "cwd":dir.path(), "tool_input":{"command":command}
        }));
        assert_eq!(pre["hookSpecificOutput"]["permissionDecision"], "allow", "{pre}");
        let wrapper = pre["hookSpecificOutput"]["updatedInput"]["command"].as_str().unwrap();
        let result = Command::new("/bin/sh")
            .args(["-c", wrapper])
            .env("BASH_ENV", &startup)
            .env_remove("HTTP_PROXY")
            .env_remove("http_proxy")
            .output()
            .unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        let actual = String::from_utf8(result.stdout).unwrap();
        assert!(
            actual.starts_with(if login {
                "/bin/bash\nlogin\n"
            } else {
                "/bin/bash\nplain\n"
            }),
            "{actual}"
        );
        assert!(actual.contains(nested.to_str().unwrap()), "{actual}");
        assert!(actual.contains("\nloaded\n"), "the startup file did not run: {actual}");
        assert!(actual.ends_with("nested-result"), "{actual}");
        assert!(!actual.contains("wrong-root"));
    }
    // Exact credential selectors must see the payload without the metadata header.
    let metadata = serde_json::json!({"workdir": nested, "shell": "/bin/bash", "login": false});
    for (id, command) in [
        ("credentials-plain", "cat .env".to_owned()),
        (
            "credentials-context",
            format!("# appa-codex-exec-v1 {metadata}\ncat .env"),
        ),
        ("context-invalid", "# appa-codex-exec-v1 {}\nprintf never".to_owned()),
    ] {
        let denied = hook(serde_json::json!({
            "hook_event_name":"PreToolUse", "session_id":"s1", "tool_name":"Bash",
            "tool_use_id":id, "cwd":dir.path(), "tool_input":{"command":command}
        }));
        assert!(
            denied["denied"] == true || denied["hookSpecificOutput"]["permissionDecision"] == "deny",
            "{denied}"
        );
        assert!(denied["hookSpecificOutput"]["updatedInput"].is_null(), "{denied}");
    }

    let _ = runtime.kill();
    let _ = runtime.wait();
}

#[test]
fn effectful_nonzero_command_does_not_release_its_diagnostics() {
    use std::process::Stdio;

    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("policy.toml");
    std::fs::write(&config, "[policy]\nversion = 2\n[[policy.tool]]\nname = \"host/codex/appa_exec\"\neffects = [\"touch\"]\n[externals]\ntimeout_ms = 5000\nmax_body_bytes = 65536\n").unwrap();
    let mut runtime = Command::new(env!("CARGO_BIN_EXE_appa"))
        .args(["runtime", "--adapter", "codex", "--config"])
        .arg(&config)
        .arg("--db")
        .arg(dir.path().join("appa.db"))
        .args(["--listen", "127.0.0.1:0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let url = common::served_url(&mut runtime);
    let event = serde_json::json!({
        "hook_event_name":"PreToolUse", "session_id":"effectful", "tool_name":"Bash",
        "tool_use_id":"c1", "cwd":dir.path(),
        "tool_input":{"command":"printf private-diagnostic; exit 7"}
    });
    let mut hook = Command::new(env!("CARGO_BIN_EXE_appa"))
        .args(["hook", "--adapter", "codex", "--deployment-url", &url])
        .env("APPA_GATE", "1")
        .env("SHELL", "/bin/sh")
        .env_remove("APPA_RUNTIME_URL")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    hook.stdin
        .take()
        .unwrap()
        .write_all(event.to_string().as_bytes())
        .unwrap();
    let output = hook.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let answer: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let wrapper = answer["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .unwrap();
    let result = Command::new("/bin/sh")
        .arg("-c")
        .arg(wrapper)
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!result.stdout.windows(18).any(|bytes| bytes == b"private-diagnostic"));
    assert!(!result.stderr.windows(18).any(|bytes| bytes == b"private-diagnostic"));
    let _ = runtime.kill();
    let _ = runtime.wait();
}

#[test]
fn post_hook_is_withheld_after_runtime_restart() {
    use std::process::Stdio;

    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("policy.toml");
    let db = dir.path().join("appa.db");
    std::fs::write(&config, "[policy]\nversion = 2\n[[policy.tool]]\nname = \"host/codex/appa_exec\"\n[externals]\ntimeout_ms = 5000\nmax_body_bytes = 65536\n").unwrap();
    let start = || {
        let mut child = Command::new(env!("CARGO_BIN_EXE_appa"))
            .args(["runtime", "--adapter", "codex", "--config"])
            .arg(&config)
            .arg("--db")
            .arg(&db)
            .args(["--listen", "127.0.0.1:0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let url = common::served_url(&mut child);
        (child, url)
    };
    let hook = |url: &str, event: serde_json::Value| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_appa"))
            .args(["hook", "--adapter", "codex", "--deployment-url", url])
            .env("APPA_GATE", "1")
            .env("SHELL", "/bin/sh")
            .env_remove("APPA_RUNTIME_URL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(event.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        if output.stdout.is_empty() {
            return serde_json::json!({});
        }
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let (mut first, first_url) = start();
    let pre = hook(
        &first_url,
        serde_json::json!({
            "hook_event_name":"PreToolUse", "session_id":"restart", "tool_name":"Bash",
            "tool_use_id":"c1", "cwd":dir.path(), "tool_input":{"command":"printf admitted"}
        }),
    );
    let wrapper = pre["hookSpecificOutput"]["updatedInput"]["command"].as_str().unwrap();
    let result = Command::new("/bin/sh")
        .arg("-c")
        .arg(wrapper)
        .env_remove("HTTP_PROXY")
        .env_remove("http_proxy")
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, b"admitted");
    first.kill().unwrap();
    first.wait().unwrap();
    let (mut second, second_url) = start();
    let post = hook(
        &second_url,
        serde_json::json!({
            "hook_event_name":"PostToolUse", "session_id":"restart", "tool_name":"Bash",
            "tool_use_id":"c1", "cwd":dir.path(), "tool_input":{"command":wrapper},
            "tool_response":"admitted"
        }),
    );
    assert_eq!(post["decision"], "block");
    hook(
        &second_url,
        serde_json::json!({"hook_event_name":"Interrupt", "session_id":"restart"}),
    );
    second.kill().unwrap();
    second.wait().unwrap();
    let (mut third, third_url) = start();
    let late = hook(
        &third_url,
        serde_json::json!({
            "hook_event_name":"PostToolUse", "session_id":"restart", "tool_name":"Bash",
            "tool_use_id":"c1", "cwd":dir.path(), "tool_input":{"command":wrapper},
            "tool_response":"admitted"
        }),
    );
    assert_eq!(late["decision"], "block");
    third.kill().unwrap();
    third.wait().unwrap();
}

// These tests use a permissive policy. Protection must come from the proxy.
#[test]
fn detached_launch_refusal_and_lifecycle_cleanup_need_no_deny_policy() {
    use std::process::{Child, Stdio};
    use std::time::{Duration, Instant};

    struct Cleanup(Child);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    struct Descendant(i32);
    impl Drop for Descendant {
        fn drop(&mut self) {
            unsafe { libc::kill(self.0, libc::SIGKILL) };
        }
    }
    for end in ["Interrupt", "Stop", "UserPromptSubmit", "restart"] {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("policy.toml");
        let db = dir.path().join("appa.db");
        std::fs::write(&config, "[policy]\nversion = 2\n[[policy.tool]]\nname = \"host/codex/appa_exec\"\n[externals]\ntimeout_ms = 5000\nmax_body_bytes = 65536\n").unwrap();
        let start = |address: &str| {
            let mut child = Command::new(env!("CARGO_BIN_EXE_appa"))
                .args(["runtime", "--adapter", "codex", "--config"])
                .arg(&config)
                .arg("--db")
                .arg(&db)
                .args(["--listen", address])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let url = common::served_url(&mut child);
            (Cleanup(child), url)
        };
        let (mut runtime, url) = start("127.0.0.1:0");
        let hook = |event: serde_json::Value| {
            let mut child = Command::new(env!("CARGO_BIN_EXE_appa"))
                .args(["hook", "--adapter", "codex", "--deployment-url", &url])
                .env("APPA_GATE", "1")
                .env("SHELL", "/bin/sh")
                .env_remove("APPA_RUNTIME_URL")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(event.to_string().as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            if !output.status.success() {
                let reason = String::from_utf8_lossy(&output.stderr).into_owned();
                assert!(
                    reason.contains("Detached background processes are unsupported."),
                    "{reason}"
                );
                return serde_json::json!({"refusal":reason});
            }
            if output.stdout.is_empty() {
                return serde_json::json!({});
            }
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
        };
        let pre_event = |id: &str, command: &str| {
            serde_json::json!({
                "hook_event_name":"PreToolUse", "session_id":"lifetime", "tool_name":"Bash",
                "tool_use_id":id, "cwd":dir.path(), "tool_input":{"command":command}
            })
        };
        let refused = hook(pre_event("refused", "touch never; setsid sleep 3"));
        assert!(refused.get("refusal").is_some(), "{refused}");
        assert!(refused.to_string().contains("Run this command in the foreground."));
        assert!(!dir.path().join("never").exists());

        // Catch the denied syscall. Keep the child alive with closed pipes so
        // this test checks teardown independently of output capture.
        let detach = if cfg!(target_os = "linux") {
            "try:\n os.setsid()\nexcept PermissionError:\n pass\n"
        } else {
            ""
        };
        std::fs::write(dir.path().join("child.py"),
            format!("import os,time\n{detach}os.close(1); os.close(2)\nopen('pid','w').write(str(os.getpid()))\nopen('ready','w').write('yes')\ntime.sleep(2)\nopen('late','w').write('survived')\n")
        ).unwrap();
        let pre = hook(pre_event("running", "python3 child.py & wait"));
        let wrapper = pre["hookSpecificOutput"]["updatedInput"]["command"].as_str().unwrap();
        let mut command = Cleanup(
            Command::new("/bin/sh")
                .args(["-c", wrapper])
                .current_dir(dir.path())
                .env_remove("HTTP_PROXY")
                .env_remove("http_proxy")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dir.path().join("ready").exists() && Instant::now() < deadline {
            assert!(
                command.0.try_wait().unwrap().is_none(),
                "wrapper exited before the child started"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(dir.path().join("ready").exists());
        let process = Descendant(
            std::fs::read_to_string(dir.path().join("pid"))
                .unwrap()
                .parse()
                .unwrap(),
        );
        if end == "restart" {
            runtime.0.kill().unwrap();
            runtime.0.wait().unwrap();
            // Bind the replacement to the same URL. The old handle must fail
            // even when its authorization poll reaches the replacement runtime.
            let (replacement, _) = start(url.strip_prefix("http://").unwrap());
            runtime = replacement;
        } else {
            hook(serde_json::json!({"hook_event_name":end, "session_id":"lifetime", "prompt":"new prompt"}));
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while command.0.try_wait().unwrap().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            command.0.try_wait().unwrap().is_some(),
            "{end} did not stop the wrapper"
        );
        std::thread::sleep(Duration::from_millis(2100));
        let state = Command::new("ps")
            .args(["-o", "stat=", "-p", &process.0.to_string()])
            .output()
            .unwrap();
        let state = String::from_utf8_lossy(&state.stdout);
        assert!(
            state.trim().is_empty() || state.trim().starts_with('Z'),
            "{end}: descendant survived"
        );
        assert!(
            !dir.path().join("late").exists(),
            "{end}: descendant wrote after cancellation"
        );
        drop(runtime);
    }
}
