use std::io::Write;
use std::process::{Command, Stdio};

fn hook(expected: &str, body: &[u8]) -> serde_json::Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_appa"))
        .args([
            "hook",
            "--adapter",
            "codex",
            "--deployment-url",
            "http://127.0.0.1:9",
            "--expected-event",
            expected,
        ])
        .env("APPA_GATE", "1")
        .env_remove("APPA_CODEX_PROOF_FILE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(body).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{expected}: {:?}", output.status);
    serde_json::from_slice(&output.stdout).expect("one valid Codex response")
}

#[test]
fn an_unavailable_runtime_denies_a_call_and_withholds_results() {
    let pre = hook(
        "PreToolUse",
        br#"{"hook_event_name":"PreToolUse","session_id":"s1","tool_name":"collaborationspawn_agent","tool_use_id":"c1","transcript_path":"/tmp/rollout","tool_input":{"task_name":"child","message":"x","fork_turns":"none"}}"#,
    );
    assert_eq!(pre["hookSpecificOutput"]["permissionDecision"], "deny");

    let post = hook(
        "PostToolUse",
        br#"{"hook_event_name":"PostToolUse","session_id":"s1","tool_name":"collaborationwait_agent","tool_use_id":"c2","transcript_path":"/tmp/rollout","tool_input":{},"tool_response":"raw child text"}"#,
    );
    assert_eq!(post["decision"], "block");
    assert!(!post.to_string().contains("raw child text"));

    let stop = hook(
        "SubagentStop",
        br#"{"hook_event_name":"SubagentStop","session_id":"s1","agent_id":"child","last_assistant_message":"raw child text"}"#,
    );
    assert_eq!(stop["decision"], "block");
    assert!(!stop.to_string().contains("raw child text"));
}

#[test]
fn the_fixed_entrypoint_refuses_a_mismatched_or_oversized_event() {
    let mismatch = hook("PreToolUse", br#"{"hook_event_name":"PostToolUse"}"#);
    assert_eq!(mismatch["hookSpecificOutput"]["permissionDecision"], "deny");
    let oversized = hook("PostToolUse", &vec![b'x'; 1024 * 1024 + 1]);
    assert_eq!(oversized["decision"], "block");
}
