//! A small Codex hook supervisor. Codex can ignore a failed hook process, so
//! worker failures need a valid host response before the host timeout expires.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const MAX_EVENT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ANSWER_BYTES: u64 = 4 * 1024 * 1024;

fn deadline_for(event: &str) -> Duration {
    match event {
        "SessionStart" => Duration::from_secs(145),
        "Stop" | "SessionEnd" => Duration::from_secs(2),
        _ => Duration::from_secs(125),
    }
}

fn fallback(event: &str, reason: &str) -> Value {
    match event {
        "PreToolUse" => json!({"hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason
        }}),
        "SessionStart" | "UserPromptSubmit" => json!({"continue": false, "stopReason": reason}),
        "Stop" | "SessionEnd" => json!({}),
        _ => json!({"decision": "block", "reason": reason}),
    }
}

fn valid_answer(event: &str, input: &[u8], answer: &[u8]) -> Result<Value, String> {
    let response: Value = serde_json::from_slice(answer).map_err(|_| "the hook worker returned invalid JSON")?;
    if !response.is_object() {
        return Err("the hook worker returned a non-object answer".into());
    }
    if event == "PreToolUse"
        && serde_json::from_slice::<Value>(input)
            .ok()
            .is_some_and(|call| call["tool_name"] == "Bash")
    {
        let decision = &response["hookSpecificOutput"];
        let denied = decision["permissionDecision"] == "deny";
        let rewritten = decision["permissionDecision"] == "allow"
            && decision["updatedInput"]["command"]
                .as_str()
                .is_some_and(|command| !command.is_empty());
        if !denied && !rewritten {
            return Err("the Bash hook returned no denial or command wrapper".into());
        }
    }
    Ok(response)
}

fn supervise(worker: &Path, event: &str, url: &str, input: &[u8], limit: Duration) -> Result<Value, String> {
    let mut source = tempfile::tempfile().map_err(|error| format!("cannot create hook input: {error}"))?;
    source
        .write_all(input)
        .map_err(|error| format!("cannot stage hook input: {error}"))?;
    source
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("cannot reset hook input: {error}"))?;
    let mut answer = tempfile::tempfile().map_err(|error| format!("cannot create hook output: {error}"))?;
    let mut command = Command::new(worker);
    command
        .args(["hook", "--adapter", "codex", "--deployment-url", url])
        .stdin(Stdio::from(source))
        .stdout(Stdio::from(answer.try_clone().map_err(|error| error.to_string())?))
        .stderr(Stdio::inherit());
    let mut child =
        crate::child_process::spawn(&mut command).map_err(|error| format!("cannot start hook worker: {error}"))?;
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(status)) => return Err(format!("hook worker exited with {status}")),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("hook worker exceeded the {} second limit", limit.as_secs()));
            }
            Err(error) => return Err(format!("cannot wait for hook worker: {error}")),
        }
    }
    answer
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("cannot read hook output: {error}"))?;
    let mut bytes = Vec::new();
    answer
        .take(MAX_ANSWER_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read hook output: {error}"))?;
    if bytes.len() as u64 > MAX_ANSWER_BYTES {
        return Err("the hook worker answer is too large".into());
    }
    valid_answer(event, input, &bytes)
}

pub fn run(event: &str, url: &str) -> ExitCode {
    if !crate::hook_client::session_is_gated() {
        return ExitCode::SUCCESS;
    }
    let mut input = Vec::new();
    let result = std::io::stdin()
        .take(MAX_EVENT_BYTES + 1)
        .read_to_end(&mut input)
        .map_err(|error| format!("cannot read the Codex hook event: {error}"))
        .and_then(|_| {
            if input.len() as u64 > MAX_EVENT_BYTES {
                Err("the Codex hook event is too large".into())
            } else {
                let worker = std::env::current_exe().map_err(|error| format!("cannot locate hook worker: {error}"))?;
                supervise(&worker, event, url, &input, deadline_for(event))
            }
        });
    let response = match result {
        Ok(response) => response,
        Err(reason) => {
            eprintln!("OpenAPPA Codex hook denied the event: {reason}");
            fallback(event, &reason)
        }
    };
    match std::io::stdout().write_all(response.to_string().as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("OpenAPPA Codex hook could not answer: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_requires_an_explicit_denial_or_wrapper() {
        let input = br#"{"tool_name":"Bash"}"#;
        assert!(valid_answer("PreToolUse", input, b"{}").is_err());
        assert!(
            valid_answer(
                "PreToolUse",
                input,
                br#"{"hookSpecificOutput":{"permissionDecision":"deny"}}"#
            )
            .is_ok()
        );
        assert!(valid_answer("PreToolUse", input, br#"{"hookSpecificOutput":{"permissionDecision":"allow","updatedInput":{"command":"appa codex-exec"}}}"#).is_ok());
    }

    #[test]
    fn fallback_uses_codex_denial_shape() {
        assert_eq!(
            fallback("PreToolUse", "failed")["hookSpecificOutput"]["permissionDecision"],
            "deny"
        );
        assert_eq!(fallback("SessionStart", "failed")["continue"], false);
        assert_eq!(fallback("PostToolUse", "failed")["decision"], "block");
    }

    #[cfg(unix)]
    #[test]
    fn worker_crash_timeout_and_bad_output_get_denials() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let worker = dir.path().join("worker");
        let input = br#"{"hook_event_name":"PreToolUse","tool_name":"Bash"}"#;
        for (script, expected, limit) in [
            ("#!/bin/sh\nexit 1\n", "exited", Duration::from_secs(2)),
            ("#!/bin/sh\nsleep 1\n", "limit", Duration::from_millis(100)),
            ("#!/bin/sh\nprintf 'bad json'\n", "invalid JSON", Duration::from_secs(2)),
        ] {
            std::fs::write(&worker, script).unwrap();
            std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o755)).unwrap();
            let result = supervise(&worker, "PreToolUse", "http://127.0.0.1:1", input, limit);
            let error = result.unwrap_err();
            assert!(error.contains(expected), "{error}");
            assert_eq!(
                fallback("PreToolUse", &error)["hookSpecificOutput"]["permissionDecision"],
                "deny"
            );
        }
    }
}
