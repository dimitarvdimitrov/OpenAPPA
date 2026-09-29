mod common;

use std::sync::Arc;

use appa_runtime::{api::Runtime, config::Config, hooks};
use appa_runtime_api::{Actor, CanonicalTool, HookDecision, HookEvent, ProposedCall, TrajectoryId};
use common::{raw, repo_root};

fn composed(dir: &tempfile::TempDir) -> (Config, Arc<Runtime>) {
    let battery = dir.path().join("batteries/codex");
    std::fs::create_dir_all(&battery).unwrap();
    std::fs::copy(
        repo_root().join("marketplace/batteries/codex/appa.toml"),
        battery.join("appa.toml"),
    )
    .unwrap();
    let default = std::fs::read_to_string(repo_root().join("marketplace/plugins/codex/default.appa.toml")).unwrap();
    let root = dir.path().join("appa.toml");
    std::fs::write(&root, format!("include = [\"batteries/codex/appa.toml\"]\n{default}")).unwrap();
    let config = Config::load(&root).expect("the Codex default and battery compose");
    let runtime = Arc::new(Runtime::open(config.clone(), dir.path().join("appa.db"), None).unwrap());
    (config, runtime)
}

fn actor() -> Actor {
    Actor {
        root: TrajectoryId("codex:policy-test".into()),
        child: None,
    }
}

fn call(name: &str, input: serde_json::Value) -> ProposedCall {
    ProposedCall {
        tool: name.into(),
        arguments: raw(input),
        cwd: Some("/tmp".into()),
    }
}

#[tokio::test]
async fn composed_default_constrains_commands_and_refuses_unverified_subagents() {
    let dir = tempfile::tempdir().unwrap();
    let (config, runtime) = composed(&dir);
    let policy = config.policy_file().value();
    let tools = policy["tool"].as_array().unwrap();
    let command = tools
        .iter()
        .position(|tool| tool["name"].as_str() == Some("host/codex/appa_exec"))
        .unwrap();
    let credential = tools
        .iter()
        .position(|tool| tool["name"].as_str() == Some("host/codex/appa_exec(command:*.env*)"))
        .unwrap();
    assert!(
        credential < command,
        "credential selectors must precede the generic command"
    );
    assert_eq!(tools[command]["annotator"].as_str(), Some("codex.command-requirements"));
    assert!(
        !tools
            .iter()
            .any(|tool| matches!(tool["name"].as_str(), Some("*") | Some("host/codex/spawn_agent")))
    );
    assert_eq!(
        policy["deployment"]["confined_results"][0].as_str(),
        Some("host/codex/appa_exec")
    );
    assert!(
        policy["annotator"]
            .as_array()
            .unwrap()
            .iter()
            .all(|annotation| annotation["builtin"].as_str() == Some("codex"))
    );
    let command_annotator = policy["annotator"]
        .as_array()
        .unwrap()
        .iter()
        .find(|annotation| annotation["name"].as_str() == Some("codex.command-requirements"))
        .unwrap();
    assert!(command_annotator["effects"].as_array().is_some_and(Vec::is_empty));
    let identify = appa_adapter_codex::adapter().identify_tool;
    for (host, canonical) in [
        ("apply_patch", "host/codex/apply_patch"),
        ("view_image", "host/codex/view_image"),
        ("mcp__appa__yell", "mcp/appa/yell"),
    ] {
        assert_eq!(
            identify(host).unwrap().canonical,
            CanonicalTool::parse(canonical).unwrap()
        );
        assert!(
            tools.iter().any(|tool| tool["name"].as_str() == Some(canonical)),
            "no policy for {host}"
        );
    }

    assert_eq!(
        hooks::handle(
            &runtime,
            HookEvent::SessionStart {
                root: actor().root,
                principal: None
            }
        )
        .await,
        HookDecision::Ack
    );
    let credential = hooks::handle(
        &runtime,
        HookEvent::ToolCall {
            actor: actor(),
            call: call("host/codex/appa_exec", serde_json::json!({"command": "cat .env"})),
            call_id: Some("credential".into()),
            spawn: false,
            ruling: None,
        },
    )
    .await;
    assert!(
        matches!(credential, HookDecision::DenyCall { .. }),
        "credential command: {credential:?}"
    );

    let spawn = hooks::handle(
        &runtime,
        HookEvent::ToolCall {
            actor: actor(),
            call: call("host/codex/spawn_agent", serde_json::json!({"message": "read a file"})),
            call_id: Some("spawn".into()),
            spawn: true,
            ruling: None,
        },
    )
    .await;
    assert!(
        matches!(spawn, HookDecision::Refuse { .. }),
        "unverified spawn: {spawn:?}"
    );

    for (index, name) in ["send_input", "send_message", "unknown_host_tool"]
        .into_iter()
        .enumerate()
    {
        let decision = hooks::handle(
            &runtime,
            HookEvent::ToolCall {
                actor: actor(),
                call: call(
                    &format!("host/codex/{name}"),
                    serde_json::json!({"message": "unchecked child text"}),
                ),
                call_id: Some(format!("unknown-{index}")),
                spawn: false,
                ruling: None,
            },
        )
        .await;
        assert!(matches!(decision, HookDecision::Refuse { .. }), "{name}: {decision:?}");
    }
}
