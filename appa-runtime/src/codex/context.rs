//! Execution context supplied explicitly in the command, before policy dispatch.

use std::path::Path;

use appa_runtime_api::HookEvent;
use serde::{Deserialize, Serialize};

const HEADER: &str = "# appa-codex-exec-v1";

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Specification {
    pub command: String,
    pub shell: String,
    pub cwd: String,
    pub login: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    workdir: String,
    shell: String,
    login: bool,
}

pub(crate) fn prepare(event: &mut HookEvent, default_shell: &str) -> Result<Specification, String> {
    let HookEvent::ToolCall { call, .. } = event else {
        return Err("the call is not a Codex command".into());
    };
    if call.tool != "host/codex/appa_exec" {
        return Err("the call is not a Codex command".into());
    }
    let mut arguments: serde_json::Value =
        serde_json::from_str(call.arguments.get()).map_err(|_| "the Codex command arguments are invalid")?;
    let raw = arguments
        .get("command")
        .and_then(serde_json::Value::as_str)
        .filter(|command| !command.trim().is_empty() && !command.contains('\0'))
        .ok_or("the Codex command is missing or invalid")?;
    let first = raw.lines().next().unwrap();
    let (command, metadata) = if first.trim_start().starts_with(HEADER) {
        let json = first
            .strip_prefix(HEADER)
            .and_then(|rest| rest.strip_prefix(' '))
            .ok_or("the Codex execution header is invalid")?;
        let metadata: Metadata =
            serde_json::from_str(json).map_err(|error| format!("the Codex execution header is invalid: {error}"))?;
        let (_, command) = raw
            .split_once('\n')
            .ok_or("the Codex execution header has no command")?;
        if command.lines().any(|line| line.trim_start().starts_with(HEADER)) {
            return Err("the Codex execution header is duplicated".into());
        }
        (command.to_owned(), Some(metadata))
    } else {
        (raw.to_owned(), None)
    };
    if command.trim().is_empty() {
        return Err("the Codex execution header has no command".into());
    }
    let string_option = |key: &str| -> Result<Option<&str>, String> {
        arguments
            .get(key)
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| format!("the Codex {key} must be a string"))
            })
            .transpose()
    };
    let supplied_cwd = string_option("workdir")?;
    let supplied_shell = string_option("shell")?;
    let supplied_login = arguments
        .get("login")
        .map(|value| value.as_bool().ok_or("the Codex login setting must be a boolean"))
        .transpose()?;
    let tty = arguments
        .get("tty")
        .map(|value| value.as_bool().ok_or("the Codex tty setting must be a boolean"))
        .transpose()?;
    if tty == Some(true) || arguments.get("env").is_some() {
        return Err("terminal commands and environment overrides are outside the buffered Codex command path".into());
    }
    let (cwd, shell, login) = if let Some(metadata) = metadata {
        if supplied_cwd.is_some_and(|value| value != metadata.workdir)
            || supplied_shell.is_some_and(|value| value != metadata.shell)
            || supplied_login.is_some_and(|value| value != metadata.login)
        {
            return Err("the Codex execution header conflicts with the command arguments".into());
        }
        (metadata.workdir, metadata.shell, metadata.login)
    } else {
        (
            supplied_cwd
                .or(call.cwd.as_deref())
                .ok_or("the Codex hook supplied no working directory")?
                .to_owned(),
            supplied_shell.unwrap_or(default_shell).to_owned(),
            supplied_login.unwrap_or(true),
        )
    };
    if cwd.contains('\0') || !Path::new(&cwd).is_absolute() || !Path::new(&cwd).is_dir() {
        return Err("the Codex working directory must be an existing absolute directory".into());
    }
    validate_shell(&shell)?;
    // Selectors, annotators, and relative-path context providers all see the
    // exact command and context that the sandboxed child will receive.
    arguments["command"] = serde_json::json!(command);
    arguments["workdir"] = serde_json::json!(cwd);
    arguments["shell"] = serde_json::json!(shell);
    arguments["login"] = serde_json::json!(login);
    call.arguments = serde_json::value::to_raw_value(&arguments).map_err(|error| error.to_string())?;
    call.cwd = Some(cwd.clone());
    Ok(Specification {
        command,
        shell,
        cwd,
        login,
    })
}

pub(crate) fn validate_shell(shell: &str) -> Result<(), String> {
    let path = Path::new(shell);
    if shell.contains('\0')
        || !path.is_absolute()
        || !matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some("sh" | "bash" | "zsh")
        )
        || !path.is_file()
    {
        return Err("the Codex command shell is missing or unsupported".into());
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use appa_runtime_api::{Actor, ProposedCall, TrajectoryId};

    fn event(arguments: serde_json::Value, cwd: &Path) -> HookEvent {
        HookEvent::ToolCall {
            actor: Actor {
                root: TrajectoryId("codex:context".into()),
                child: None,
            },
            call: ProposedCall {
                tool: "host/codex/appa_exec".into(),
                arguments: serde_json::value::to_raw_value(&arguments).unwrap(),
                cwd: Some(cwd.to_string_lossy().into_owned()),
            },
            call_id: Some("call".into()),
            spawn: false,
            ruling: None,
        }
    }

    #[test]
    fn supplied_hook_options_use_the_requested_context() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let mut event = event(
            serde_json::json!({
                "command": "pwd", "workdir": nested, "shell": "/bin/bash", "login": false
            }),
            root.path(),
        );
        let spec = prepare(&mut event, "/bin/sh").unwrap();
        assert_eq!(spec.cwd, nested.to_str().unwrap());
        assert_eq!(spec.shell, "/bin/bash");
        assert!(!spec.login);
        let HookEvent::ToolCall { call, .. } = event else {
            panic!("call")
        };
        assert_eq!(call.cwd.as_deref(), nested.to_str());
    }

    #[test]
    fn header_normalizes_the_policy_call_and_plain_commands_keep_disclosed_defaults() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let metadata = serde_json::json!({"workdir": nested, "shell": "/bin/bash", "login": false});
        let mut with_header = event(
            serde_json::json!({
                "command": format!("{HEADER} {metadata}\ncat .env")
            }),
            root.path(),
        );
        let spec = prepare(&mut with_header, "/bin/sh").unwrap();
        assert_eq!(spec.command, "cat .env");
        assert_eq!(spec.shell, "/bin/bash");
        assert!(!spec.login);
        let HookEvent::ToolCall { call, .. } = with_header else {
            panic!("call")
        };
        let arguments: serde_json::Value = serde_json::from_str(call.arguments.get()).unwrap();
        assert_eq!(arguments["command"], "cat .env");
        assert_eq!(arguments["workdir"], nested.to_str().unwrap());
        assert_eq!(arguments["shell"], "/bin/bash");
        assert_eq!(arguments["login"], false);
        assert_eq!(call.cwd.as_deref(), nested.to_str());

        let mut plain = event(serde_json::json!({"command": "pwd"}), root.path());
        let spec = prepare(&mut plain, "/bin/sh").unwrap();
        assert_eq!(spec.cwd, root.path().to_str().unwrap());
        assert_eq!(spec.shell, "/bin/sh");
        assert!(spec.login);
        assert!(
            serde_json::from_value::<Specification>(serde_json::json!({
                "command":"touch never", "shell":"/bin/sh", "cwd":root.path()
            }))
            .is_err(),
            "an old specification without login cannot execute"
        );
    }

    #[test]
    fn invalid_or_conflicting_context_refuses_before_dispatch() {
        let root = tempfile::tempdir().unwrap();
        let valid = serde_json::json!({"workdir": root.path(), "shell": "/bin/sh", "login": false});
        let header = format!("{HEADER} {valid}\npwd");
        for arguments in [
            serde_json::json!({"command": format!("{HEADER} {{}}\npwd")}),
            serde_json::json!({"command": format!("{HEADER} {valid}\n{header}")}),
            serde_json::json!({"command": header, "login": true}),
            serde_json::json!({"command": header, "workdir": "/"}),
            serde_json::json!({"command": header, "shell": "/bin/bash"}),
            serde_json::json!({"command": header, "env": {}}),
            serde_json::json!({"command": "pwd", "workdir": "nested"}),
            serde_json::json!({"command": "pwd", "login": "false"}),
            serde_json::json!({"command": "pwd", "tty": true}),
            serde_json::json!({"command": "pwd", "shell": "/bin/fish"}),
            serde_json::json!({"command": format!("{HEADER} {{\"workdir\":\"/\",\"shell\":\"/bin/sh\",\"login\":false,\"login\":true}}\npwd")}),
            serde_json::json!({"command": format!("{HEADER} {{\"workdir\":\"/\",\"shell\":\"/bin/sh\",\"login\":false,\"env\":{{}}}}\npwd")}),
        ] {
            assert!(
                prepare(&mut event(arguments.clone(), root.path()), "/bin/sh").is_err(),
                "{arguments}"
            );
        }
    }
}
