use appa_runtime_api::{
    Actor, HookEvent, OutcomeBody, ParseRefusal, ProposedCall, SpawnRef, ToolOutcome, TrajectoryId,
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Input {
    hook_event_name: String,
    session_id: String,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    tool_name: Option<String>,
    #[serde(default)]
    tool_use_id: Option<String>,
    #[serde(default)]
    tool_input: Option<Box<serde_json::value::RawValue>>,
    #[serde(default)]
    tool_response: Option<serde_json::Value>,
    #[serde(default)]
    last_assistant_message: Option<String>,
}

fn malformed(detail: &str) -> ParseRefusal {
    ParseRefusal::Malformed { detail: detail.into() }
}

impl Input {
    fn root(&self) -> TrajectoryId {
        TrajectoryId(format!("codex:{}", self.session_id))
    }

    fn child(&self) -> Option<TrajectoryId> {
        self.agent_id
            .as_deref()
            .filter(|id| !id.is_empty())
            .map(|id| TrajectoryId(format!("codex:{}:{id}", self.session_id)))
    }

    fn actor(&self) -> Actor {
        Actor {
            root: self.root(),
            child: self.child(),
        }
    }

    fn call(&self) -> Result<ProposedCall, ParseRefusal> {
        let raw = self
            .tool_name
            .as_deref()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| malformed("tool hook has no tool_name"))?;
        Ok(ProposedCall {
            tool: raw.to_owned(),
            arguments: self
                .tool_input
                .clone()
                .ok_or_else(|| malformed("tool hook has no tool_input"))?,
            cwd: self.cwd.clone().filter(|cwd| !cwd.is_empty()),
        })
    }

    fn call_id(&self) -> Result<Option<String>, ParseRefusal> {
        Ok(Some(
            self.tool_use_id
                .clone()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| malformed("tool hook has no tool_use_id"))?,
        ))
    }
}

pub(crate) fn parse(body: &[u8]) -> Result<Option<HookEvent>, ParseRefusal> {
    let input: Input = serde_json::from_slice(body).map_err(|error| ParseRefusal::Unreadable {
        detail: error.to_string(),
    })?;
    if input.session_id.is_empty() {
        return Err(malformed("hook has no session_id"));
    }
    let event = match input.hook_event_name.as_str() {
        "SessionStart" => HookEvent::SessionStart {
            root: input.root(),
            principal: None,
        },
        "UserPromptSubmit" => HookEvent::Prompt {
            actor: input.actor(),
            text: input
                .prompt
                .clone()
                .ok_or_else(|| malformed("prompt hook has no prompt"))?,
        },
        "PreToolUse" => {
            let call = input.call()?;
            let spawn = matches!(call.tool.as_str(), "spawn_agent" | "collaborationspawn_agent");
            if spawn && !fresh_spawn_context(&call) {
                return Err(malformed("Codex subagents require fork_turns none"));
            }
            HookEvent::ToolCall {
                actor: input.actor(),
                call,
                call_id: input.call_id()?,
                spawn,
                ruling: None,
            }
        }
        "PostToolUse" => {
            let call = input.call()?;
            if call.tool == "collaborationwait_agent" && !safe_wait_response(input.tool_response.as_ref()) {
                return Err(malformed("Codex wait returned an unchecked child result"));
            }
            let outcome = match input.tool_response.as_ref() {
                None => ToolOutcome::Indeterminate,
                Some(response) if call.tool.starts_with("mcp__") && mcp_error(response) => ToolOutcome::Indeterminate,
                Some(response) => ToolOutcome::Success {
                    // The runtime wire carries a JSON value. Codex often
                    // supplies a plain string for patch and MCP results, so
                    // preserve that string as a JSON string value.
                    body: OutcomeBody::Available(response.to_string()),
                },
            };
            if matches!(call.tool.as_str(), "spawn_agent" | "collaborationspawn_agent") {
                if !fresh_spawn_context(&call) {
                    return Err(malformed("Codex subagents require fork_turns none"));
                }
                // The collaboration result supplies a launch receipt, not a verified
                // identity. Only SubagentStart can bind its child to this parent.
                let child = (call.tool != "collaborationspawn_agent")
                    .then(|| {
                        input
                            .tool_response
                            .as_ref()
                            .and_then(|response| response.get("agent_id").or_else(|| response.get("agentId")))
                            .and_then(serde_json::Value::as_str)
                            .filter(|id| !id.is_empty())
                            .map(|id| TrajectoryId(format!("codex:{}:{id}", input.session_id)))
                    })
                    .flatten();
                HookEvent::SpawnResult {
                    actor: input.actor(),
                    call,
                    call_id: input.call_id()?,
                    outcome,
                    child,
                    value: None,
                }
            } else {
                HookEvent::ToolResult {
                    actor: input.actor(),
                    call,
                    call_id: input.call_id()?,
                    outcome,
                }
            }
        }
        "SubagentStart" => HookEvent::ChildStart {
            root: input.root(),
            child: input
                .child()
                .ok_or_else(|| malformed("subagent start has no agent_id"))?,
            spawn: SpawnRef::InFlight,
        },
        "SubagentStop" => HookEvent::ChildEnd {
            root: input.root(),
            child: input
                .child()
                .ok_or_else(|| malformed("subagent stop has no agent_id"))?,
            value: Some(
                input
                    .last_assistant_message
                    .clone()
                    .ok_or_else(|| malformed("subagent stop has no last_assistant_message"))?,
            ),
        },
        "Stop" | "SessionEnd" | "Interrupt" | "PreCompact" => HookEvent::TurnEnd { actor: input.actor() },
        "PostCompact" | "PermissionRequest" => return Ok(None),
        _ => return Err(malformed("unsupported Codex hook event")),
    };
    Ok(Some(event))
}

fn fresh_spawn_context(call: &ProposedCall) -> bool {
    serde_json::from_str::<serde_json::Value>(call.arguments.get())
        .ok()
        .and_then(|arguments| {
            arguments
                .get("fork_turns")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .as_deref()
        == Some("none")
}

fn safe_wait_response(response: Option<&serde_json::Value>) -> bool {
    let Some(serde_json::Value::String(response)) = response else {
        return false;
    };
    let Ok(serde_json::Value::Object(fields)) = serde_json::from_str(response) else {
        return false;
    };
    fields.len() == 2
        && fields.get("message").and_then(serde_json::Value::as_str) == Some("Wait completed.")
        && fields.get("timed_out").and_then(serde_json::Value::as_bool) == Some(false)
}

fn mcp_error(response: &serde_json::Value) -> bool {
    response
        .get("isError")
        .or_else(|| response.get("is_error"))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interruption_and_session_end_close_outstanding_calls() {
        for hook in ["Interrupt", "SessionEnd", "PreCompact"] {
            let body = serde_json::json!({"hook_event_name": hook, "session_id": "s1"});
            assert!(matches!(
                parse(body.to_string().as_bytes()).unwrap(),
                Some(HookEvent::TurnEnd { .. })
            ));
        }
    }

    #[test]
    fn calls_keep_host_identity_and_arguments() {
        let body = br#"{"hook_event_name":"PreToolUse","session_id":"s1","tool_name":"Bash","tool_use_id":"c1","tool_input":{"command":"echo hi"},"cwd":"/tmp"}"#;
        let event = parse(body).unwrap().unwrap();
        let HookEvent::ToolCall {
            actor, call, call_id, ..
        } = event
        else {
            panic!("tool call")
        };
        assert_eq!(actor.root.0, "codex:s1");
        assert_eq!(call_id.as_deref(), Some("c1"));
        assert_eq!(call.cwd.as_deref(), Some("/tmp"));
        assert_eq!(call.tool, "Bash");
        assert_eq!(call.arguments.get(), r#"{"command":"echo hi"}"#);
    }

    #[test]
    fn a_codex_string_result_crosses_the_wire_as_a_json_string() {
        let body = br#"{"hook_event_name":"PostToolUse","session_id":"s1","tool_name":"Bash","tool_use_id":"c1","tool_input":{"command":"echo hi"},"tool_response":"hello\n"}"#;
        let event = parse(body).unwrap().unwrap();
        appa_runtime_api::WireEvent::from_event(appa_runtime_api::AdapterName::Codex, &event)
            .expect("a Codex string result crosses the runtime wire");
        let HookEvent::ToolResult { outcome, .. } = event else {
            panic!("tool result")
        };
        assert_eq!(
            outcome,
            ToolOutcome::Success {
                body: OutcomeBody::Available("\"hello\\n\"".into())
            }
        );
    }

    #[test]
    fn an_observed_mcp_error_keeps_effects_unsettled() {
        for spelling in ["isError", "is_error"] {
            let mut response = serde_json::json!({"content": [{"type":"text", "text":"partial write"}]});
            response[spelling] = serde_json::json!(true);
            let body = serde_json::json!({
                "hook_event_name": "PostToolUse", "session_id": "s1", "tool_name": "mcp__fs__write",
                "tool_use_id": "c1", "tool_input": {"path": "x"},
                "tool_response": response,
            });
            let event = parse(&serde_json::to_vec(&body).unwrap()).unwrap().unwrap();
            let HookEvent::ToolResult { outcome, .. } = event else {
                panic!("tool result")
            };
            assert_eq!(outcome, ToolOutcome::Indeterminate);
        }
    }

    #[test]
    fn invalid_required_fields_refuse() {
        for body in [
            br#"{"hook_event_name":"PreToolUse","session_id":"s1","tool_name":"Bash","tool_input":{}}"#.as_slice(),
            br#"{"hook_event_name":"PostToolUse","session_id":"s1","tool_use_id":"c1","tool_name":"Bash"}"#,
        ] {
            assert!(parse(body).is_err());
        }
    }

    #[test]
    fn observed_collaboration_spawn_has_no_verified_child_id_in_its_result() {
        let before = br#"{"hook_event_name":"PreToolUse","session_id":"s1","transcript_path":null,"tool_name":"collaborationspawn_agent","tool_use_id":"c1","tool_input":{"task_name":"child","message":"report","fork_turns":"none"}}"#;
        let Some(HookEvent::ToolCall { spawn: true, .. }) = parse(before).unwrap() else {
            panic!("the observed Codex tool must retain its spawn identity")
        };
        let after = br#"{"hook_event_name":"PostToolUse","session_id":"s1","transcript_path":null,"tool_name":"collaborationspawn_agent","tool_use_id":"c1","tool_input":{"task_name":"child","message":"report","fork_turns":"none"},"tool_response":"{\"task_name\":\"/root/child\"}"}"#;
        let Some(HookEvent::SpawnResult { child: None, .. }) = parse(after).unwrap() else {
            panic!("the task path is not a verified child ID")
        };
        let start = br#"{"hook_event_name":"SubagentStart","session_id":"s1","agent_id":"child-uuid"}"#;
        let Some(HookEvent::ChildStart {
            child,
            spawn: SpawnRef::InFlight,
            ..
        }) = parse(start).unwrap()
        else {
            panic!("the child start binds only through the pending spawn")
        };
        assert_eq!(child.0, "codex:s1:child-uuid");
    }

    #[test]
    fn collaboration_spawn_result_cannot_supply_a_child_identity() {
        for response in [
            serde_json::json!({"agent_id":"forged", "text":"raw child text"}),
            serde_json::json!({"task_name":"/root/child", "agentId":"forged"}),
        ] {
            let body = serde_json::json!({
                "hook_event_name":"PostToolUse", "session_id":"s1", "transcript_path":null,
                "tool_name":"collaborationspawn_agent", "tool_use_id":"c1",
                "tool_input":{"task_name":"child","message":"report","fork_turns":"none"},
                "tool_response":response,
            });
            let Some(HookEvent::SpawnResult { child: None, .. }) = parse(body.to_string().as_bytes()).unwrap() else {
                panic!("a collaboration result cannot bind a child: {body}")
            };
        }
    }

    #[test]
    fn persistent_session_calls_keep_the_same_identity_and_arguments() {
        for path in [
            serde_json::Value::String("/tmp/rollout.jsonl".into()),
            serde_json::Value::Bool(false),
        ] {
            for tool in ["collaborationspawn_agent", "collaborationwait_agent"] {
                let body = serde_json::json!({
                    "hook_event_name":"PreToolUse", "session_id":"s1", "transcript_path":path,
                    "tool_name":tool, "tool_use_id":"c1",
                    "tool_input": if tool == "collaborationspawn_agent" {
                        serde_json::json!({"task_name":"child","message":"report","fork_turns":"none"})
                    } else { serde_json::json!({"timeout_ms":1000}) }
                });
                let Some(HookEvent::ToolCall {
                    actor, call, call_id, ..
                }) = parse(body.to_string().as_bytes()).unwrap()
                else {
                    panic!("persistent collaboration call did not parse: {body}")
                };
                assert_eq!(actor.root.0, "codex:s1");
                assert_eq!(call.tool, tool);
                assert_eq!(call_id.as_deref(), Some("c1"));
            }
        }
    }

    #[test]
    fn ephemeral_spawn_refuses_inherited_parent_turns() {
        for fork_turns in [serde_json::Value::Null, serde_json::json!("all"), serde_json::json!(3)] {
            let body = serde_json::json!({
                "hook_event_name":"PreToolUse", "session_id":"s1", "transcript_path":null,
                "tool_name":"collaborationspawn_agent", "tool_use_id":"c1",
                "tool_input":{"task_name":"child","message":"report","fork_turns":fork_turns}
            });
            assert!(parse(body.to_string().as_bytes()).is_err(), "{body}");
        }
    }

    #[test]
    fn wait_result_admits_only_the_observed_completion_status() {
        for response in [
            r#"{"message":"Wait completed.","timed_out":false}"#,
            r#"{"message":"Wait completed.","timed_out":false,"child":"raw"}"#,
            r#"{"message":"raw child text","timed_out":false}"#,
            r#"{"message":"Wait timed out.","timed_out":true}"#,
        ] {
            let body = serde_json::json!({
                "hook_event_name":"PostToolUse", "session_id":"s1", "transcript_path":null,
                "tool_name":"collaborationwait_agent", "tool_use_id":"c1", "tool_input":{},
                "tool_response":response
            });
            assert_eq!(
                parse(body.to_string().as_bytes()).is_ok(),
                response == r#"{"message":"Wait completed.","timed_out":false}"#
            );
        }
    }

    #[test]
    fn child_return_and_parent_cancellation_have_separate_actors() {
        let stop = br#"{"hook_event_name":"SubagentStop","session_id":"s1","agent_id":"child-uuid","last_assistant_message":"private"}"#;
        let Some(HookEvent::ChildEnd { root, child, value }) = parse(stop).unwrap() else {
            panic!("a child stop must carry its return")
        };
        assert_eq!(root.0, "codex:s1");
        assert_eq!(child.0, "codex:s1:child-uuid");
        assert_eq!(value.as_deref(), Some("private"));

        let interrupt = br#"{"hook_event_name":"Interrupt","session_id":"s1"}"#;
        let Some(HookEvent::TurnEnd { actor }) = parse(interrupt).unwrap() else {
            panic!("the parent interruption must end its turn")
        };
        assert_eq!(actor.root.0, root.0);
        assert!(actor.child.is_none());
    }

    #[test]
    fn child_stop_requires_a_reported_message() {
        for value in [None, Some(serde_json::Value::Null)] {
            let mut body = serde_json::json!({
                "hook_event_name":"SubagentStop", "session_id":"s1", "agent_id":"child-uuid"
            });
            if let Some(value) = value {
                body["last_assistant_message"] = value;
            }
            assert!(parse(body.to_string().as_bytes()).is_err(), "{body}");
        }
        let empty = br#"{"hook_event_name":"SubagentStop","session_id":"s1","agent_id":"child-uuid","last_assistant_message":""}"#;
        assert!(matches!(parse(empty), Ok(Some(HookEvent::ChildEnd { value: Some(value), .. })) if value.is_empty()));
    }
}
