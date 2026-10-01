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
async fn composed_default_constrains_commands_and_blocks_unverified_subagents() {
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
    assert!(tools.iter().any(|tool| tool["name"].as_str() == Some("*")));
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
    assert!(command_annotator.get("effects").is_none());
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
        matches!(spawn, HookDecision::DenyCall { .. }),
        "unverified spawn: {spawn:?}"
    );

    for (index, name) in [
        "appa_stdin",
        "wait",
        "send_input",
        "send_message",
        "close_agent",
        "resume_agent",
    ]
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
        let HookDecision::DenyCall { feedback, .. } = decision else {
            panic!("{name}: {decision:?}");
        };
        assert!(feedback.contains("blocked"), "{name}: {feedback}");
    }
}

fn compiled(config: &Config) -> appa_policy::Config {
    appa_policy::Config::from_toml_str(&toml::to_string(config.policy_file().value()).unwrap()).unwrap()
}

fn selected<'a>(
    policy: &'a appa_policy::Config,
    host: &str,
    command: &str,
) -> &'a appa_engine::contract::ToolDeclaration {
    let call = policy
        .engine()
        .resolve_call(
            appa_engine::value::ToolName::new(host),
            &serde_json::to_vec(&serde_json::json!({ "command": command })).unwrap(),
        )
        .unwrap();
    policy.engine().registry().declaration(&call).unwrap()
}

#[test]
fn repository_commands_match_claude_and_preserve_credential_precedence() {
    let dir = tempfile::tempdir().unwrap();
    let (codex, _) = composed(&dir);
    let codex = compiled(&codex);
    let claude = Config::load_from(
        &repo_root().join("marketplace/plugins/claude-code/default.appa.toml"),
        &[repo_root().join("marketplace/batteries")],
    )
    .unwrap();
    let claude = compiled(&claude);

    for command in [
        "git push origin main",
        "cd repo && git push origin main",
        "git -C repo push origin main",
        "git -c push.default=current push",
        "gh pr view 42 --repo acme/private",
        "gh pr create --title Test",
        "gh issue view 12",
        "gh issue comment 12 --body Test",
        "gh release create v1",
        "gh api repos/acme/private/issues/12",
        "gh repo view acme/private",
    ] {
        let codex_rule = selected(&codex, "host/codex/appa_exec", command);
        let claude_rule = selected(&claude, "host/claude-code/Bash", command);
        assert_eq!(
            codex_rule.annotator().map(|name| name.as_str()),
            Some("codex.repository-requirements"),
            "{command}"
        );
        assert_eq!(
            claude_rule.annotator().map(|name| name.as_str()),
            Some("claude-code.bash-repository-requirements"),
            "{command}"
        );
        assert_eq!(
            codex_rule.name().as_str(),
            claude_rule
                .name()
                .as_str()
                .replace("host/claude-code/Bash", "host/codex/appa_exec"),
            "{command}"
        );
        assert_eq!(codex_rule.tags(), claude_rule.tags());
    }

    for command in [
        "gh pr create --body-file .env",
        "gh issue create --body-file ~/.aws/config",
        "gh api repos/acme/private --input ~/.config/gh/hosts.yml",
        "git push origin main && cat ~/.netrc",
    ] {
        let codex_rule = selected(&codex, "host/codex/appa_exec", command);
        let claude_rule = selected(&claude, "host/claude-code/Bash", command);
        let annotation = codex_rule.declared().expect("credentials select the static rule");
        let equivalent = claude_rule.declared().unwrap();
        assert_eq!(annotation.delta, equivalent.delta, "{command}");
        assert_eq!(annotation.tags, equivalent.tags, "{command}");
        assert_eq!(annotation.tags[0].as_str(), "credentials");
    }

    for command in ["git status", "gh auth status", "cargo test", "cat README.md"] {
        assert_eq!(
            selected(&codex, "host/codex/appa_exec", command)
                .annotator()
                .map(|name| name.as_str()),
            Some("codex.command-requirements"),
            "{command}"
        );
    }
}

#[test]
fn repository_classifier_preserves_claude_instructions_and_default_mandate() {
    let dir = tempfile::tempdir().unwrap();
    let (config, _) = composed(&dir);
    let codex = compiled(&config);
    let claude = Config::load_from(
        &repo_root().join("marketplace/plugins/claude-code/default.appa.toml"),
        &[repo_root().join("marketplace/batteries")],
    )
    .unwrap();
    let claude = compiled(&claude);
    let (_, repository) = codex
        .annotators()
        .find(|(name, _)| name.as_str() == "codex.repository-requirements")
        .unwrap();
    let (_, equivalent) = claude
        .annotators()
        .find(|(name, _)| name.as_str() == "claude-code.bash-repository-requirements")
        .unwrap();
    assert_eq!(repository.builtin, Some(appa_policy::AnnotatorBuiltin::Codex));
    let hint = repository.hint.as_ref().unwrap().as_str();
    assert!(hint.contains("context.github"));
    assert!(hint.contains("For repository writes, use the destination visibility"));
    assert!(hint.contains("public or unknown, require audience public"));
    assert!(hint.contains("private or internal, require audience internal"));
    assert!(hint.contains("For reads, preserve the session's audience"));
    assert!(hint.contains("every author as a repository owner, member, collaborator, or bot"));
    assert!(equivalent.hint.as_ref().unwrap().as_str().contains("context.github"));
    assert!(
        repository.inputs.is_empty(),
        "the classifier receives the complete call"
    );

    let mandate = codex
        .engine()
        .registry()
        .annotator_mandate(&appa_engine::names::AnnotatorName::new("codex.repository-requirements"))
        .unwrap();
    assert_eq!(mandate.trust_ranks().count(), 2);
    for audience in ["self", "internal"] {
        assert!(mandate.audiences().entries().any(|entry| entry == audience));
    }
    let generic_mandate = codex
        .engine()
        .registry()
        .annotator_mandate(&appa_engine::names::AnnotatorName::new("codex.command-requirements"))
        .unwrap();
    assert_eq!(mandate, generic_mandate);
    assert_eq!(mandate.effects().count(), 0);
    let authored = config.policy_file().value()["annotator"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"].as_str() == Some("codex.repository-requirements"))
        .unwrap();
    assert!(authored.get("effects").is_none());
}

#[test]
fn missing_credential_rules_match_claude_and_codex_login_stays_private() {
    let dir = tempfile::tempdir().unwrap();
    let (config, _) = composed(&dir);
    let codex = compiled(&config);
    let claude = Config::load_from(
        &repo_root().join("marketplace/plugins/claude-code/default.appa.toml"),
        &[repo_root().join("marketplace/batteries")],
    )
    .unwrap();
    let claude = compiled(&claude);

    for command in [
        "cat ~/.claude.json",
        "cat /home/user/.claude.json.backup",
        "cat .credentials.json",
        "gh pr create --body-file /tmp/.credentials.json",
        "databricks configure --token",
        "databricks --profile deployment configure",
    ] {
        let annotation = selected(&codex, "host/codex/appa_exec", command).declared().unwrap();
        let equivalent = selected(&claude, "host/claude-code/Bash", command).declared().unwrap();
        assert_eq!(annotation.delta, equivalent.delta, "{command}");
        assert_eq!(annotation.tags, equivalent.tags, "{command}");
        assert_eq!(annotation.tags[0].as_str(), "credentials");
    }

    let equivalent = selected(&claude, "host/claude-code/Bash", "cat .credentials.json")
        .declared()
        .unwrap();
    for command in [
        "cat .codex/auth.json",
        "cat ./.codex/auth.json",
        "cat ~/.codex/auth.json",
        "cat /home/user/.codex/auth.json",
        "gh pr create --body-file /home/user/.codex/auth.json",
    ] {
        let annotation = selected(&codex, "host/codex/appa_exec", command).declared().unwrap();
        assert_eq!(annotation.delta, equivalent.delta, "{command}");
        assert_eq!(annotation.tags, equivalent.tags, "{command}");
    }
}

#[test]
fn explicit_contracts_precede_the_wildcard_for_commands_and_web() {
    let dir = tempfile::tempdir().unwrap();
    let (config, _) = composed(&dir);
    let policy = compiled(&config);
    for (tool, annotator) in [
        ("host/codex/appa_exec", Some("codex.command-requirements")),
        ("host/codex/apply_patch", Some("codex.patch-requirements")),
        ("host/codex/view_image", Some("codex.local-read")),
        ("host/codex/update_plan", None),
        ("host/codex/webrun", None),
        ("mcp/appa/yell", None),
    ] {
        let rule = selected(&policy, tool, "printf test");
        assert_eq!(rule.name().as_str(), tool);
        assert_eq!(rule.annotator().map(|name| name.as_str()), annotator);
    }
    for tool in ["host/codex/image_genimagegen", "host/codex/unknown", "mcp/new/read"] {
        let rule = selected(&policy, tool, "input");
        assert_eq!(rule.name().as_str(), "*");
        assert_eq!(rule.annotator().unwrap().as_str(), "codex.undeclared-tool");
    }
    let (_, fallback) = policy
        .annotators()
        .find(|(name, _)| name.as_str() == "codex.undeclared-tool")
        .unwrap();
    assert_eq!(fallback.builtin, Some(appa_policy::AnnotatorBuiltin::Codex));
    assert!(fallback.inputs.is_empty(), "the wildcard receives the complete call");
    let mandate = policy
        .engine()
        .registry()
        .annotator_mandate(&appa_engine::names::AnnotatorName::new("codex.undeclared-tool"))
        .unwrap();
    assert_eq!(mandate.marks().map(|mark| mark.as_str()).collect::<Vec<_>>(), ["hitl"]);
    assert_eq!(
        mandate.effects().count(),
        0,
        "unknown tools cannot declare deployment effects"
    );
}
