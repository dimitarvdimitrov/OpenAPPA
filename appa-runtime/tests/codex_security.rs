mod common;

use std::sync::{Arc, Mutex};

use appa_runtime::{
    api::{AuditEvent, RemedyOutcome, Runtime},
    config::Config,
    hooks,
};
use appa_runtime_api::{HookDecision, HookEvent, ProposedCall};
use axum::{Router, routing::post};
use common::{actor, audit_len, offer_of, propose, ran, raw, repo_root, root, serve};

#[derive(Clone)]
enum AnnotationResponse {
    Produced(serde_json::Value),
    Failed,
    Malformed,
    Missing,
    Slow,
}

// Replace the model transport, while retaining the shipped declarations and hints.
async fn deployment(
    dir: &tempfile::TempDir,
    answer: serde_json::Value,
) -> (Config, Arc<Runtime>, Arc<Mutex<Vec<serde_json::Value>>>) {
    deployment_response(dir, AnnotationResponse::Produced(answer)).await
}

async fn deployment_response(
    dir: &tempfile::TempDir,
    answer: AnnotationResponse,
) -> (Config, Arc<Runtime>, Arc<Mutex<Vec<serde_json::Value>>>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    let url = serve(Router::new().route(
        "/annotate",
        post(move |body: String| {
            let seen = seen.clone();
            let answer = answer.clone();
            async move {
                seen.lock().unwrap().push(serde_json::from_str(&body).unwrap());
                use axum::http::StatusCode;
                match answer {
                    AnnotationResponse::Produced(answer) => (
                        StatusCode::OK,
                        serde_json::json!({ "version": 1, "answer": answer }).to_string(),
                    ),
                    AnnotationResponse::Failed => (StatusCode::INTERNAL_SERVER_ERROR, "failed".into()),
                    AnnotationResponse::Malformed => (StatusCode::OK, "not json".into()),
                    AnnotationResponse::Missing => (StatusCode::OK, r#"{"version":1}"#.into()),
                    AnnotationResponse::Slow => {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        (StatusCode::OK, r#"{"version":1}"#.into())
                    }
                }
            }
        }),
    ))
    .await;

    let mut battery: toml::Value = std::fs::read_to_string(repo_root().join("marketplace/batteries/codex/appa.toml"))
        .unwrap()
        .parse::<toml::Table>()
        .map(toml::Value::Table)
        .unwrap();
    let mut default: toml::Value =
        std::fs::read_to_string(repo_root().join("marketplace/plugins/codex/default.appa.toml"))
            .unwrap()
            .parse::<toml::Table>()
            .map(toml::Value::Table)
            .unwrap();
    let mut bindings = toml::map::Map::new();
    for policy in [&mut battery, &mut default] {
        for annotator in policy["policy"]["annotator"].as_array_mut().unwrap() {
            assert_eq!(annotator["builtin"].as_str(), Some("codex"));
            annotator.as_table_mut().unwrap().remove("builtin");
            bindings.insert(
                annotator["name"].as_str().unwrap().to_string(),
                toml::Value::Table(toml::map::Map::from_iter([(
                    "url".into(),
                    toml::Value::String(format!("{url}/annotate")),
                )])),
            );
        }
    }
    default["externals"]
        .as_table_mut()
        .unwrap()
        .insert("annotators".into(), toml::Value::Table(bindings));
    default["externals"]["authorities"]["hitl"]["builtin"] = "approve".into();
    default["externals"]["timeout_ms"] = 150.into();
    default["policy"]["tool"].as_array_mut().unwrap().push(
        r#"name = "deployment/publish"
delta = {}
effects = ["deployment-release"]"#
            .parse::<toml::Table>()
            .map(toml::Value::Table)
            .unwrap(),
    );
    default["policy"]["tool"].as_array_mut().unwrap().push(
        r#"name = "deployment/read-private"
delta = { audience = ["self"] }"#
            .parse::<toml::Table>()
            .map(toml::Value::Table)
            .unwrap(),
    );
    default["policy"].as_table_mut().unwrap().insert(
        "deployment".into(),
        r#"starting_label = { trust = "trusted", audience = "public" }"#
            .parse::<toml::Table>()
            .map(toml::Value::Table)
            .unwrap(),
    );
    default.as_table_mut().unwrap().insert(
        "include".into(),
        toml::Value::Array(vec![toml::Value::String("battery.toml".into())]),
    );
    std::fs::write(dir.path().join("battery.toml"), toml::to_string(&battery).unwrap()).unwrap();
    let path = dir.path().join("appa.toml");
    std::fs::write(&path, toml::to_string(&default).unwrap()).unwrap();
    let config = Config::load(&path).unwrap();
    let runtime = Arc::new(Runtime::open(config.clone(), dir.path().join("runtime.db"), None).unwrap());
    assert_eq!(
        hooks::handle(
            &runtime,
            HookEvent::SessionStart {
                root: root(),
                principal: None
            }
        )
        .await,
        HookDecision::Ack
    );
    (config, runtime, requests)
}

fn call(tool: &str, command: &str) -> ProposedCall {
    ProposedCall {
        tool: format!("host/codex/{tool}"),
        arguments: raw(serde_json::json!({ "command": command })),
        cwd: Some("/tmp/project".into()),
    }
}

#[tokio::test]
async fn both_command_classifiers_admit_deployment_declared_effects() {
    let dir = tempfile::tempdir().unwrap();
    let (config, runtime, requests) = deployment(
        &dir,
        serde_json::json!({
            "delta": {},
            "requires": { "history": [], "attention": [] },
            "emits": ["deployment-release"],
        }),
    )
    .await;
    let policy = appa_policy::Config::from_toml_str(&toml::to_string(config.policy_file().value()).unwrap()).unwrap();
    for name in ["codex.command-requirements", "codex.repository-requirements"] {
        let mandate = policy
            .engine()
            .registry()
            .annotator_mandate(&appa_engine::names::AnnotatorName::new(name))
            .unwrap();
        assert!(
            mandate.effects().any(|effect| effect.as_str() == "deployment-release"),
            "{name}"
        );
    }
    for command in ["deploy-service production", "gh release create v1"] {
        let proposed = call("appa_exec", command);
        assert_eq!(
            hooks::handle(
                &runtime,
                HookEvent::ToolCall {
                    actor: actor(),
                    call: proposed,
                    call_id: Some(command.into()),
                    spawn: false,
                    ruling: None,
                },
            )
            .await,
            HookDecision::AllowCall { spawn: None }
        );
    }
    let releases = runtime
        .audit(&root())
        .unwrap()
        .into_iter()
        .filter_map(|entry| match entry.event {
            AuditEvent::Released { effects, .. } => Some(effects),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(releases, vec![vec!["deployment-release".to_string()]; 2]);
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["name"], "codex.command-requirements");
    assert_eq!(requests[1]["name"], "codex.repository-requirements");
    for request in requests.iter() {
        assert_eq!(
            request["declaration"]["effects"],
            serde_json::json!(["deployment-release"])
        );
    }
}

#[tokio::test]
async fn a_trusted_mixed_patch_with_hitl_attention_requires_fresh_review() {
    let dir = tempfile::tempdir().unwrap();
    let (config, runtime, requests) = deployment(
        &dir,
        serde_json::json!({
            "delta": {},
            "requires": { "trust": "trusted", "history": [], "attention": ["hitl"] },
            "emits": [],
        }),
    )
    .await;
    let policy = appa_policy::Config::from_toml_str(&toml::to_string(config.policy_file().value()).unwrap()).unwrap();
    let mandate = policy
        .engine()
        .registry()
        .annotator_mandate(&appa_engine::names::AnnotatorName::new("codex.patch-requirements"))
        .unwrap();
    assert_eq!(mandate.marks().map(|mark| mark.as_str()).collect::<Vec<_>>(), ["hitl"]);
    let patch = "*** Begin Patch\n*** Update File: .codex/config.toml\n@@\n+[mcp_servers.external]\n+url = \"https://example.com/mcp\"\n*** Update File: README.md\n@@\n+Text from an external source.\n*** End Patch";
    let proposed = call("apply_patch", patch);

    for iteration in 0..2 {
        let decision = propose(&runtime, proposed.clone()).await;
        assert_eq!(runtime.status(&root()).unwrap().trust, "trusted");
        let HookDecision::DenyCall { feedback, .. } = &decision else {
            panic!("patch {iteration}: {decision:?}");
        };
        assert!(
            feedback.contains("hitl"),
            "the attention gap must require hitl: {feedback}"
        );
        assert!(
            !feedback.contains("below the required floor"),
            "trust alone cannot explain this denial: {feedback}"
        );
        assert!(matches!(
            runtime.execute_remedy(&actor(), offer_of(&decision)).await,
            RemedyOutcome::Authorized { .. }
        ));
        assert_eq!(
            propose(&runtime, proposed.clone()).await,
            HookDecision::AllowCall { spawn: None }
        );
        ran(&runtime, proposed.clone()).await;
    }

    let reviews = runtime
        .audit(&root())
        .unwrap()
        .iter()
        .filter(|entry| matches!(&entry.event, AuditEvent::Ruled { authority, .. } if authority == "hitl"))
        .count();
    assert_eq!(reviews, 2, "each distinct patch proposal requires a new ruling");
    let requests = requests.lock().unwrap();
    assert!(!requests.is_empty());
    for request in requests.iter() {
        assert_eq!(request["name"], "codex.patch-requirements");
        assert_eq!(request["artifact"]["args"]["arguments"]["command"], patch);
        let hint = request["declaration"]["hint"].as_str().unwrap();
        assert!(hint.contains("For hook or MCP edits, require fresh attention hitl even when trusted"));
        assert_eq!(request["declaration"]["attention_marks"], serde_json::json!(["hitl"]));
        assert!(hint.contains("Preserve trust and audience for every file in mixed patches"));
        assert!(hint.contains("additions, deletions, renames"));
    }
}

fn annotation(delta: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "delta": delta,
        "requires": { "history": [], "attention": [] },
        "emits": [],
    })
}

#[tokio::test]
async fn unknown_tools_receive_complete_calls_and_policy_admission() {
    let dir = tempfile::tempdir().unwrap();
    let (_, runtime, requests) = deployment(&dir, annotation(serde_json::json!({}))).await;
    for tool in ["host/codex/image_genimagegen", "mcp/new/read", "host/codex/unknown"] {
        let proposed = ProposedCall {
            tool: tool.into(),
            arguments: raw(serde_json::json!({"prompt": "draw a private diagram", "options": {"quality": "high"}})),
            cwd: Some("/tmp/project".into()),
        };
        assert_eq!(
            propose(&runtime, proposed.clone()).await,
            HookDecision::AllowCall { spawn: None }
        );
        ran(&runtime, proposed).await;
    }
    {
        let seen = requests.lock().unwrap();
        assert_eq!(seen.len(), 3);
        for (request, tool) in seen
            .iter()
            .zip(["host/codex/image_genimagegen", "mcp/new/read", "host/codex/unknown"])
        {
            assert_eq!(request["name"], "codex.undeclared-tool");
            assert_eq!(request["artifact"]["args"]["name"], tool);
            assert_eq!(
                request["artifact"]["args"]["arguments"]["prompt"],
                "draw a private diagram"
            );
            assert_eq!(request["artifact"]["args"]["arguments"]["options"]["quality"], "high");
        }
    }

    let restricted = tempfile::tempdir().unwrap();
    let (_, runtime, _) = deployment(&restricted, annotation(serde_json::json!({"audience": ["self"]}))).await;
    let decision = propose(&runtime, call("unknown", "read private data")).await;
    assert!(matches!(decision, HookDecision::DenyCall { .. }), "{decision:?}");
}

#[tokio::test]
async fn wildcard_annotation_failures_refuse_without_an_audit_change() {
    for response in [
        AnnotationResponse::Failed,
        AnnotationResponse::Malformed,
        AnnotationResponse::Missing,
        AnnotationResponse::Slow,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (_, runtime, requests) = deployment_response(&dir, response).await;
        let before = audit_len(&runtime);
        let decision = propose(&runtime, call("unknown", "unchecked call")).await;
        let HookDecision::Refuse { detail } = decision else {
            panic!("failed wildcard annotation: {decision:?}");
        };
        assert!(detail.contains("codex.undeclared-tool"), "{detail}");
        assert_eq!(audit_len(&runtime), before);
        assert_eq!(requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn explicit_web_contract_bypasses_classifier_and_marks_results_suspicious() {
    let dir = tempfile::tempdir().unwrap();
    let (_, runtime, requests) = deployment_response(&dir, AnnotationResponse::Failed).await;
    let proposed = ProposedCall {
        tool: "host/codex/webrun".into(),
        arguments: raw(serde_json::json!({"search_query": [{"q": "public documentation"}]})),
        cwd: None,
    };
    let decision = propose(&runtime, proposed.clone()).await;
    let HookDecision::DenyCall { feedback, .. } = &decision else {
        panic!("a web result requires acceptance of lower trust: {decision:?}");
    };
    assert!(feedback.contains("suspicious"), "{feedback}");
    assert!(matches!(
        runtime.execute_remedy(&actor(), offer_of(&decision)).await,
        RemedyOutcome::Authorized { .. }
    ));
    assert_eq!(
        propose(&runtime, proposed.clone()).await,
        HookDecision::AllowCall { spawn: None }
    );
    ran(&runtime, proposed.clone()).await;
    assert_eq!(runtime.status(&root()).unwrap().trust, "suspicious");
    assert!(
        requests.lock().unwrap().is_empty(),
        "the explicit web rule needs no annotation"
    );

    // A static private read narrows the session before the next web query.
    let private_read = ProposedCall {
        tool: "deployment/read-private".into(),
        arguments: raw(serde_json::json!({})),
        cwd: None,
    };
    let decision = propose(&runtime, private_read.clone()).await;
    assert!(matches!(
        runtime.execute_remedy(&actor(), offer_of(&decision)).await,
        RemedyOutcome::Authorized { .. }
    ));
    assert_eq!(
        propose(&runtime, private_read.clone()).await,
        HookDecision::AllowCall { spawn: None }
    );
    ran(&runtime, private_read).await;
    assert_eq!(
        runtime.status(&root()).unwrap().audience,
        "self",
        "a private read narrows the session"
    );
    let decision = propose(&runtime, proposed).await;
    let HookDecision::DenyCall { feedback, .. } = decision else {
        panic!("a private session cannot send a web query: {decision:?}");
    };
    assert!(feedback.contains("public"), "{feedback}");
    assert!(requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn default_local_inspection_denial_offers_an_acceptance_remedy() {
    let dir = tempfile::tempdir().unwrap();
    let (_, runtime, _) = deployment(&dir, annotation(serde_json::json!({"audience": ["self"]}))).await;
    let proposed = call("appa_exec", "ls -la .");
    let decision = propose(&runtime, proposed.clone()).await;
    assert!(matches!(decision, HookDecision::DenyCall { .. }), "{decision:?}");
    assert!(matches!(
        runtime.execute_remedy(&actor(), offer_of(&decision)).await,
        RemedyOutcome::Authorized { .. }
    ));
    assert_eq!(
        propose(&runtime, proposed).await,
        HookDecision::AllowCall { spawn: None }
    );
}
