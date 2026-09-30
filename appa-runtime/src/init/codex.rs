//! Direct Codex CLI registration in the active user's profile.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use appa_runtime_api::AdapterName;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::endpoint::{Endpoint, endpoint_health, reconcile_policy, serving_policy_key, verify_runtime_deployment};
use super::paths::{appa_filename, codex_config_dir, codex_data_dir};
use super::{Compensation, InitError, Undo, file_before, install_runtime, write_state};
use crate::runtime_url::RuntimeTarget;

#[derive(Serialize, Deserialize)]
struct Receipt {
    hooks: BTreeMap<String, Value>,
    mcp_url: String,
    config: PathBuf,
}

fn profile_error(path: &Path, message: impl std::fmt::Display) -> InitError {
    InitError::CodexProfile {
        path: path.to_owned(),
        message: message.to_string(),
    }
}

fn profile_paths() -> Result<(PathBuf, PathBuf, PathBuf), InitError> {
    let dir = codex_config_dir()?;
    Ok((
        dir.join("hooks.json"),
        dir.join("config.toml"),
        codex_data_dir()?.join("install-receipt.json"),
    ))
}

/// Keep a user's profile symlink in place when updating its destination.
fn profile_destination(path: &Path) -> Result<PathBuf, InitError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            fs::canonicalize(path).map_err(|error| profile_error(path, format!("cannot resolve profile link: {error}")))
        }
        Ok(_) => Ok(path.to_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path.to_owned()),
        Err(error) => Err(profile_error(path, format!("cannot inspect profile path: {error}"))),
    }
}

fn check_profile_destination(path: &Path, expected: &Path) -> Result<(), InitError> {
    if profile_destination(path)? != expected {
        return Err(profile_error(
            path,
            "profile link changed during the operation; rerun the command",
        ));
    }
    Ok(())
}

fn read_receipt(path: &Path) -> Result<Option<Receipt>, InitError> {
    file_before(path)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(|error| profile_error(path, error)))
        .transpose()
}

fn read_hooks(path: &Path) -> Result<Value, InitError> {
    let document = file_before(path)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(|error| profile_error(path, error)))
        .transpose()?
        .unwrap_or_else(|| json!({}));
    if !document.is_object() || document.get("hooks").is_some_and(|value| !value.is_object()) {
        return Err(profile_error(
            path,
            "hooks.json must be an object with an optional hooks object",
        ));
    }
    Ok(document)
}

fn verify_owned_hooks(document: &Value, previous: &Receipt, path: &Path, receipt_path: &Path) -> Result<(), InitError> {
    for (event, group) in &previous.hooks {
        if !document["hooks"][event]
            .as_array()
            .is_some_and(|groups| groups.contains(group))
        {
            return Err(profile_error(
                path,
                format!(
                    "The installed {event} APPA hook group is missing or edited. The receipt is {}.\n\
                 1. Back up hooks.json and the receipt.\n\
                 2. Compare hooks[{event}] with receipt.hooks[{event}].\n\
                 3. Preserve custom values in the backup.\n\
                 4. Move unrelated hooks from that group into a separate group with their original matcher.\n\
                 5. Restore only the affected APPA group from the receipt.\n\
                 6. Retry the original command.\n\
                 7. After reinstall, review and trust the hooks through /hooks.\n\
                 Do not delete the receipt. Reinstall can duplicate foreign registrations.",
                    receipt_path.display()
                ),
            ));
        }
    }
    Ok(())
}

fn remove_owned_hooks(
    document: &mut Value,
    previous: &Receipt,
    path: &Path,
    receipt_path: &Path,
) -> Result<(), InitError> {
    verify_owned_hooks(document, previous, path, receipt_path)?;
    for (event, group) in &previous.hooks {
        let hooks = document["hooks"].as_object_mut().expect("verified hooks");
        let groups = hooks[event].as_array_mut().expect("verified event");
        let index = groups
            .iter()
            .position(|candidate| candidate == group)
            .expect("verified group");
        groups.remove(index);
        if groups.is_empty() {
            hooks.remove(event);
        }
    }
    if document
        .get("hooks")
        .and_then(Value::as_object)
        .is_some_and(|hooks| hooks.is_empty())
    {
        document.as_object_mut().expect("object checked").remove("hooks");
    }
    Ok(())
}

/// Resolve the policy from the activation receipt. Refuse malformed state.
pub fn resolved_codex_config_path() -> Result<PathBuf, InitError> {
    let receipt = codex_data_dir()?.join("install-receipt.json");
    Ok(read_receipt(&receipt)?.map_or_else(super::paths::installed_codex_config_path, |receipt| receipt.config))
}

fn installed_receipt() -> Result<Receipt, InitError> {
    let (_, _, path) = profile_paths()?;
    read_receipt(&path)?
        .ok_or_else(|| profile_error(&path, "install the Codex plugin first: appa plugin install codex"))
}

fn installed_runtime(receipt: &Receipt, reconcile: bool) -> Result<Endpoint, InitError> {
    let endpoint = Endpoint::resolve_for(AdapterName::Codex)?;
    let data = codex_data_dir()?;
    let binary = data.join("bin").join(appa_filename());
    let policy = if reconcile {
        Some(super::config::verify_config(&receipt.config)?)
    } else {
        None
    };
    if reconcile {
        let target = RuntimeTarget {
            url: endpoint.url().to_owned(),
            user_owned: false,
        };
        let deployment = crate::runtime_start::Deployment {
            config: receipt.config.clone(),
            data_dir: data,
        };
        crate::runtime_start::ensure_for(&target, &deployment, &binary, &[], AdapterName::Codex)
            .map_err(|error| InitError::Starter(error.to_string()))?;
    }
    verify_runtime_deployment(&binary, &receipt.config, &endpoint)?;
    if let Some(policy) = policy {
        reconcile_policy(&endpoint, &receipt.config, &policy)?;
    }
    Ok(endpoint)
}

/// Reconcile the installed policy and return its verified active key.
pub fn reload_codex_policy() -> Result<String, InitError> {
    serving_policy_key(&installed_runtime(&installed_receipt()?, true)?)
}

/// Return the active key without a policy change or runtime startup.
pub fn codex_policy_key() -> Result<String, InitError> {
    serving_policy_key(&installed_runtime(&installed_receipt()?, false)?)
}

fn shell_literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn powershell_literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn hook_command(binary: &Path, event: &str, url: &str, windows: bool) -> Result<String, InitError> {
    let path = binary
        .to_str()
        .ok_or_else(|| profile_error(binary, "path must be UTF-8 for Codex hooks"))?;
    let literal = if windows { powershell_literal } else { shell_literal };
    let parts = [
        "codex-hook".to_owned(),
        "--event".into(),
        event.into(),
        "--deployment-url".into(),
        literal(url),
    ];
    let head = if windows {
        format!("& {}", literal(path))
    } else {
        literal(path)
    };
    Ok(format!("{head} {}", parts.join(" ")))
}

fn hook_groups(binary: &Path, url: &str) -> Result<BTreeMap<String, Value>, InitError> {
    let mut groups = BTreeMap::new();
    let events = [
        ("SessionStart", None, 150),
        ("UserPromptSubmit", None, 130),
        ("PreToolUse", Some("*"), 130),
        ("PostToolUse", Some("*"), 130),
        ("SubagentStart", None, 130),
        ("SubagentStop", None, 130),
        ("Stop", None, 3),
        ("SessionEnd", None, 3),
    ];
    for (event, matcher, timeout) in events {
        let unix = hook_command(binary, event, url, false)?;
        let windows = hook_command(binary, event, url, true)?;
        let mut group =
            json!({"hooks": [{"type":"command", "command":unix, "commandWindows":windows, "timeout":timeout}]});
        if let Some(matcher) = matcher {
            group["matcher"] = json!(matcher);
        }
        groups.insert(event.into(), group);
    }
    Ok(groups)
}

fn install_hooks(
    path: &Path,
    previous: Option<&Receipt>,
    receipt_path: &Path,
    wanted: &Receipt,
    compensation: &mut Compensation,
) -> Result<(), InitError> {
    let destination = profile_destination(path)?;
    let mut document = read_hooks(&destination)?;
    if let Some(previous) = previous {
        remove_owned_hooks(&mut document, previous, path, receipt_path)?;
    }
    let hooks = document
        .as_object_mut()
        .expect("checked")
        .entry("hooks")
        .or_insert_with(|| json!({}));
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| profile_error(path, "hooks must be an object"))?;
    for (event, group) in &wanted.hooks {
        let groups = hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or_else(|| profile_error(path, format!("{event} hooks must be an array")))?;
        groups.push(group.clone());
    }
    let before = file_before(&destination)?;
    check_profile_destination(path, &destination)?;
    compensation.record(Undo::File {
        path: destination.clone(),
        before,
    });
    write_state(
        &destination,
        &serde_json::to_vec_pretty(&document).expect("JSON encodes"),
    )?;
    check_profile_destination(path, &destination)
}

fn edit_mcp(
    path: &Path,
    previous: Option<&Receipt>,
    wanted: Option<&str>,
    compensation: &mut Compensation,
) -> Result<(), InitError> {
    let destination = profile_destination(path)?;
    let before = file_before(&destination)?;
    let source = before
        .as_deref()
        .map(std::str::from_utf8)
        .transpose()
        .map_err(|error| profile_error(path, error))?
        .unwrap_or("");
    let mut document = source
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| profile_error(path, error))?;
    if let Some(profile) = document
        .get("permissions")
        .and_then(|permissions| permissions.get("appa"))
    {
        let expected = profile.get("extends").and_then(toml_edit::Item::as_str) == Some(":workspace")
            && profile
                .get("network")
                .and_then(|network| network.get("enabled"))
                .and_then(toml_edit::Item::as_bool)
                == Some(true)
            && profile
                .get("network")
                .and_then(|network| network.get("domains"))
                .and_then(|domains| domains.get("127.0.0.1"))
                .and_then(toml_edit::Item::as_str)
                == Some("allow")
            && profile.as_table().is_some_and(|table| table.len() == 2)
            && profile
                .get("network")
                .and_then(toml_edit::Item::as_table)
                .is_some_and(|table| table.len() == 2)
            && profile
                .get("network")
                .and_then(|network| network.get("domains"))
                .and_then(toml_edit::Item::as_table)
                .is_some_and(|table| table.len() == 1);
        if previous.is_none() || !expected {
            return Err(profile_error(
                path,
                "permissions.appa was edited or is not the profile this installer wrote",
            ));
        }
    } else if previous.is_some() {
        return Err(profile_error(
            path,
            "installed permissions.appa profile was removed or edited",
        ));
    }
    if let Some(current) = document.get("mcp_servers").and_then(|servers| servers.get("appa")) {
        let actual = current.get("url").and_then(toml_edit::Item::as_str).unwrap_or("");
        if previous.is_none_or(|receipt| receipt.mcp_url != actual)
            || current.as_table().is_none_or(|entry| entry.len() != 1)
        {
            return Err(profile_error(
                path,
                "mcp_servers.appa was edited or is not the registration this installer wrote",
            ));
        }
    } else if previous.is_some() {
        return Err(profile_error(
            path,
            "installed mcp_servers.appa entry was removed or edited",
        ));
    }
    if previous.is_some_and(|receipt| wanted == Some(receipt.mcp_url.as_str())) {
        return Ok(());
    }
    if document.get("mcp_servers").is_none() {
        document["mcp_servers"] = toml_edit::table();
    }
    document["mcp_servers"]
        .as_table_mut()
        .ok_or_else(|| profile_error(path, "mcp_servers must be a table"))?
        .remove("appa");
    if let Some(url) = wanted {
        document["mcp_servers"]["appa"] = toml_edit::table();
        document["mcp_servers"]["appa"]["url"] = toml_edit::value(url);
        if document.get("permissions").is_none() {
            document["permissions"] = toml_edit::table();
        }
        document["permissions"]["appa"] = toml_edit::table();
        document["permissions"]["appa"]["network"] = toml_edit::table();
        document["permissions"]["appa"]["network"]["domains"] = toml_edit::table();
        document["permissions"]["appa"]["extends"] = toml_edit::value(":workspace");
        document["permissions"]["appa"]["network"]["enabled"] = toml_edit::value(true);
        document["permissions"]["appa"]["network"]["domains"]["127.0.0.1"] = toml_edit::value("allow");
    } else if let Some(permissions) = document.get_mut("permissions").and_then(toml_edit::Item::as_table_mut) {
        permissions.remove("appa");
        if permissions.is_empty() {
            document.remove("permissions");
        }
    }
    check_profile_destination(path, &destination)?;
    compensation.record(Undo::File {
        path: destination.clone(),
        before,
    });
    write_state(&destination, document.to_string().as_bytes())?;
    check_profile_destination(path, &destination)
}

/// Register Codex without touching Claude's profile, config, or runtime.
pub fn activate_codex(config: &Path) -> Result<String, InitError> {
    let config = std::path::absolute(config).map_err(|source| InitError::AbsolutePath {
        path: config.to_owned(),
        source,
    })?;
    let policy = super::config::verify_config(&config)?;
    let endpoint = Endpoint::resolve_for(AdapterName::Codex)?;
    let data = codex_data_dir()?;
    let binary = data.join("bin").join(appa_filename());
    let (hooks_path, config_path, receipt_path) = profile_paths()?;
    let _lock = super::lock_claude_profile(hooks_path.parent().expect("hooks parent"))?;
    let previous = read_receipt(&receipt_path)?;
    let wanted = Receipt {
        hooks: hook_groups(&binary, endpoint.url())?,
        mcp_url: format!("{}/mcp", endpoint.url()),
        config: config.clone(),
    };
    let appa = std::env::current_exe().map_err(InitError::CurrentExecutable)?;
    fs::create_dir_all(binary.parent().expect("binary parent")).map_err(|source| InitError::WriteFile {
        path: binary.clone(),
        source,
    })?;
    fs::create_dir_all(hooks_path.parent().expect("hooks parent")).map_err(|source| InitError::WriteFile {
        path: hooks_path.clone(),
        source,
    })?;
    let mut compensation = Compensation::default();
    let activation = (|| {
        install_runtime(&appa, &binary, &mut compensation)?;
        install_hooks(
            &hooks_path,
            previous.as_ref(),
            &receipt_path,
            &wanted,
            &mut compensation,
        )?;
        edit_mcp(
            &config_path,
            previous.as_ref(),
            Some(&wanted.mcp_url),
            &mut compensation,
        )?;
        super::skill::install_codex(hooks_path.parent().expect("profile"), &mut compensation)?;
        let before = file_before(&receipt_path)?;
        compensation.record(Undo::File {
            path: receipt_path.clone(),
            before,
        });
        write_state(
            &receipt_path,
            &serde_json::to_vec_pretty(&wanted).expect("receipt encodes"),
        )?;
        let target = RuntimeTarget {
            url: endpoint.url().to_owned(),
            user_owned: false,
        };
        let deployment = crate::runtime_start::Deployment {
            config: config.clone(),
            data_dir: data.clone(),
        };
        let running_before = endpoint_health(&endpoint)?.as_deref() == Some("ok");
        crate::runtime_start::ensure_for(&target, &deployment, &binary, &[], AdapterName::Codex)
            .map_err(|error| InitError::Starter(error.to_string()))?;
        let pid = verify_runtime_deployment(&binary, &config, &endpoint)?;
        if !running_before {
            compensation.record(Undo::Runtime {
                pid,
                endpoint: endpoint.clone(),
            });
        }
        reconcile_policy(&endpoint, &config, &policy)?;
        Ok::<_, InitError>(())
    })();
    match activation {
        Ok(()) => {
            compensation.commit();
            Ok(format!(
                "OpenAPPA activated for Codex at {}. Review and trust its hooks with `/hooks` before launching a protected session.",
                hooks_path.display()
            ))
        }
        Err(error) => match compensation.unwind() {
            Ok(()) => Err(error),
            Err(recovery) => Err(InitError::Recovery {
                operation: Box::new(error),
                recovery: Box::new(recovery),
            }),
        },
    }
}

/// Remove only entries recorded by this Codex activation.
pub fn codex_remove() -> Result<(), InitError> {
    let (hooks_path, config_path, receipt_path) = profile_paths()?;
    let _lock = super::lock_claude_profile(hooks_path.parent().expect("hooks parent"))?;
    let Some(receipt) = read_receipt(&receipt_path)? else {
        return Ok(());
    };
    let hooks_destination = profile_destination(&hooks_path)?;
    let mut hooks = read_hooks(&hooks_destination)?;
    remove_owned_hooks(&mut hooks, &receipt, &hooks_path, &receipt_path)?;
    let mut compensation = Compensation::default();
    let removal = (|| {
        edit_mcp(&config_path, Some(&receipt), None, &mut compensation)?;
        check_profile_destination(&hooks_path, &hooks_destination)?;
        compensation.record(Undo::File {
            path: hooks_destination.clone(),
            before: file_before(&hooks_destination)?,
        });
        write_state(
            &hooks_destination,
            &serde_json::to_vec_pretty(&hooks).expect("JSON encodes"),
        )?;
        check_profile_destination(&hooks_path, &hooks_destination)?;
        super::skill::remove_codex(hooks_path.parent().expect("profile"), &mut compensation)?;
        compensation.record(Undo::File {
            path: receipt_path.clone(),
            before: file_before(&receipt_path)?,
        });
        fs::remove_file(&receipt_path).map_err(|source| InitError::WriteFile {
            path: receipt_path.clone(),
            source,
        })
    })();
    match removal {
        Ok(()) => {
            compensation.commit();
            Ok(())
        }
        Err(error) => match compensation.unwind() {
            Ok(()) => Err(error),
            Err(recovery) => Err(InitError::Recovery {
                operation: Box::new(error),
                recovery: Box::new(recovery),
            }),
        },
    }
}

/// Start Codex with the installed profile. The hook trust review remains a
/// Codex action; this launcher never bypasses it.
pub fn launch_codex(arguments: Vec<OsString>) -> ExitCode {
    let probe_options = match probe_options(&arguments) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("appa codex: {message}");
            return ExitCode::FAILURE;
        }
    };
    let (hooks, _, receipt) = match profile_paths() {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("appa codex: {error}");
            return ExitCode::FAILURE;
        }
    };
    if !receipt.exists() {
        eprintln!("appa codex: install the Codex plugin first: appa plugin install codex");
        return ExitCode::FAILURE;
    }
    let installed = match read_receipt(&receipt) {
        Ok(Some(installed)) => installed,
        Ok(None) => {
            eprintln!("appa codex: install the Codex plugin first");
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("appa codex: {error}");
            return ExitCode::FAILURE;
        }
    };
    if !installed.config.exists() {
        eprintln!("appa codex: installed policy {} is missing", installed.config.display());
        return ExitCode::FAILURE;
    }
    let current = match read_hooks(&hooks) {
        Ok(current) => current,
        Err(error) => {
            eprintln!("appa codex: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = verify_owned_hooks(&current, &installed, &hooks, &receipt) {
        eprintln!("appa codex: {error}");
        return ExitCode::FAILURE;
    }
    let endpoint = match installed_runtime(&installed, true) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            eprintln!("appa codex: {error}");
            return ExitCode::FAILURE;
        }
    };
    let binary = match codex_data_dir() {
        Ok(data) => data.join("bin").join(appa_filename()),
        Err(error) => {
            eprintln!("appa codex: {error}");
            return ExitCode::FAILURE;
        }
    };
    match serving_policy_key(&endpoint) {
        Ok(key) => eprintln!("appa codex: active policy key {key}"),
        Err(error) => {
            eprintln!("appa codex: {error}");
            return ExitCode::FAILURE;
        }
    }
    if let Err(message) = sandbox_probe(&binary, endpoint.url(), &probe_options) {
        eprintln!("appa codex: {message}");
        return ExitCode::FAILURE;
    }
    let mut command = Command::new("codex");
    command
        .args([
            "-c",
            "default_permissions=\"appa\"",
            "-c",
            "features.network_proxy=true",
            "-c",
            "features.hooks=true",
            "-c",
            "features.multi_agent=false",
            "-c",
            "approval_policy=\"on-request\"",
            "-c",
            "approvals_reviewer=\"user\"",
            "-c",
            "mcp_servers.appa.tools.execute_remedy_plan.approval_mode=\"approve\"",
        ])
        .args(&arguments)
        .env("APPA_GATE", "1")
        .env_remove("APPA_CODEX_STATUS_FILE");
    #[cfg(unix)]
    if crate::codex_terminal::is_interactive(&arguments) {
        return match crate::codex_terminal::run(&command, endpoint.url()) {
            Ok(code) => ExitCode::from(code),
            Err(error) => {
                eprintln!("appa codex: {error}");
                ExitCode::FAILURE
            }
        };
    }
    let status = command.status();
    match status {
        Ok(status) => ExitCode::from(status.code().unwrap_or(1).clamp(0, 255) as u8),
        Err(error) => {
            eprintln!("appa codex: cannot start Codex: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Carry config and project selection into the sandbox handshake. Refuse flags
/// that would make a protected launch bypass its hook or command sandbox.
fn probe_options(arguments: &[OsString]) -> Result<Vec<OsString>, &'static str> {
    let mut options = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let option = arguments[index].to_string_lossy();
        if option == "--" {
            break;
        }
        if matches!(option.as_ref(), "-a" | "--ask-for-approval") {
            let Some(value) = arguments.get(index + 1) else {
                return Err("a Codex option is missing its value");
            };
            if value != "on-request" {
                return Err(APPROVAL_ERROR);
            }
            index += 2;
            continue;
        }
        if let Some(value) = option
            .strip_prefix("--ask-for-approval=")
            .or_else(|| option.strip_prefix("-a").filter(|value| !value.is_empty()))
            && value.strip_prefix('=').unwrap_or(value) != "on-request"
        {
            return Err(APPROVAL_ERROR);
        }
        if matches!(
            option.as_ref(),
            "--dangerously-bypass-approvals-and-sandbox" | "--dangerously-bypass-hook-trust"
        ) {
            return Err("this Codex option bypasses a protected tool or hook path");
        }
        if matches!(option.as_ref(), "--sandbox" | "-s") || option.starts_with("--sandbox=") {
            return Err(
                "select the protected appa permission profile through `appa codex`; a separate --sandbox mode cannot be verified",
            );
        }
        if matches!(
            option.as_ref(),
            "-C" | "--cd" | "-p" | "--profile" | "-c" | "--config" | "--enable" | "--disable"
        ) {
            let Some(value) = arguments.get(index + 1) else {
                return Err("a Codex option is missing its value");
            };
            let value_text = value.to_string_lossy();
            if matches!(option.as_ref(), "-c" | "--config") {
                check_approval_override(&value_text)?;
            }
            if (option == "--disable" && matches!(value_text.as_ref(), "hooks" | "network_proxy"))
                || (option == "--enable" && value_text == "multi_agent")
                || (matches!(option.as_ref(), "-c" | "--config") && forbidden_config_override(&value_text))
            {
                return Err("a Codex option disables hooks or replaces the protected permission profile");
            }
            options.extend([arguments[index].clone(), value.clone()]);
            index += 2;
            continue;
        }
        if let Some(value) = option
            .strip_prefix("--config=")
            .or_else(|| option.strip_prefix("-c").filter(|value| !value.is_empty()))
        {
            let value = value.strip_prefix('=').unwrap_or(value);
            check_approval_override(value)?;
            if forbidden_config_override(value) {
                return Err("a Codex option disables hooks or replaces the protected permission profile");
            }
            options.extend([OsString::from("-c"), OsString::from(value)]);
            index += 1;
            continue;
        }
        if matches!(option.as_ref(), "--disable=hooks" | "--disable=network_proxy") || option == "--enable=multi_agent"
        {
            return Err("a Codex option disables hooks or replaces the protected permission profile");
        }
        if ["--cd=", "--profile=", "--enable=", "--disable="]
            .iter()
            .any(|prefix| option.starts_with(prefix))
        {
            options.push(arguments[index].clone());
        }
        index += 1;
    }
    Ok(options)
}

const APPROVAL_ERROR: &str = "APPA requires approval_policy=on-request and approvals_reviewer=user for human review. Remove the conflicting approval option.";
const REMEDY_APPROVAL_ERROR: &str = "APPA requires execute_remedy_plan approval_mode=approve so remedies reach its policy checks. Remove the conflicting MCP approval option.";

/// Parse parent tables and quoted keys too, so an inline table cannot replace a protected setting.
fn check_approval_override(value: &str) -> Result<(), &'static str> {
    let Some((key, setting)) = value.split_once('=') else {
        return Ok(());
    };
    let Ok(keys) = toml_edit::Key::parse(key.trim()) else {
        return Ok(());
    };
    let keys = keys.iter().map(toml_edit::Key::get).collect::<Vec<_>>();
    // Codex accepts unquoted string values when TOML value parsing fails.
    let document = value.parse::<toml::Table>().or_else(|_| {
        format!("{} = {}", key.trim(), serde_json::to_string(setting.trim()).unwrap()).parse::<toml::Table>()
    });
    let Ok(document) = document else {
        return Ok(()); // Codex reports invalid config syntax.
    };
    for (path, expected, error) in [
        (&["approval_policy"][..], "on-request", APPROVAL_ERROR),
        (&["approvals_reviewer"][..], "user", APPROVAL_ERROR),
        (
            &["mcp_servers", "appa", "tools", "execute_remedy_plan", "approval_mode"][..],
            "approve",
            REMEDY_APPROVAL_ERROR,
        ),
    ] {
        if !path.starts_with(&keys) && !keys.starts_with(path) {
            continue;
        }
        let mut value = document.get(path[0]);
        for part in &path[1..] {
            value = match value {
                Some(toml::Value::Table(table)) => table.get(*part),
                Some(_) => return Err(error),
                None => None,
            };
        }
        if value.and_then(toml::Value::as_str) != Some(expected) {
            return Err(error);
        }
    }
    Ok(())
}

fn forbidden_config_override(value: &str) -> bool {
    let Some((key, setting)) = value.split_once('=') else {
        return false;
    };
    let key = key.trim();
    let setting = setting.trim();
    key == "default_permissions"
        || (matches!(key, "features.hooks" | "features.network_proxy") && setting == "false")
        || (key == "features.multi_agent" && setting == "true")
}

fn sandbox_probe(binary: &Path, url: &str, options: &[OsString]) -> Result<(), String> {
    let mut child = Command::new("codex")
        .arg("sandbox")
        .args(["-P", "appa", "--include-managed-config", "--enable", "network_proxy"])
        .args(options)
        .arg("--")
        .arg(binary)
        .args(["codex-probe", "--url", url])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot start Codex sandbox handshake: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err("the Codex command sandbox cannot reach the APPA runtime through the appa profile; check its 127.0.0.1 network rule and managed restrictions".into()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("the Codex sandbox HTTP handshake timed out".into());
            }
            Err(error) => return Err(format!("cannot check the Codex sandbox: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_and_remove_preserve_foreign_codex_profile_entries() {
        let root = tempfile::tempdir().unwrap();
        let hooks_path = root.path().join("hooks.json");
        let config_path = root.path().join("config.toml");
        let foreign =
            json!({"hooks": {"PreToolUse": [{"matcher":"Read", "hooks":[{"type":"command", "command":"mine"}]}]}});
        fs::write(&hooks_path, serde_json::to_vec(&foreign).unwrap()).unwrap();
        fs::write(&config_path, "model = 'test'\n").unwrap();
        let first = Receipt {
            hooks: hook_groups(Path::new("/tmp/appa"), "http://127.0.0.1:8766").unwrap(),
            mcp_url: "http://127.0.0.1:8766/mcp".into(),
            config: root.path().join("appa.toml"),
        };
        let mut undo = Compensation::default();
        install_hooks(&hooks_path, None, &root.path().join("receipt.json"), &first, &mut undo).unwrap();
        edit_mcp(&config_path, None, Some(&first.mcp_url), &mut undo).unwrap();
        let installed = read_hooks(&hooks_path).unwrap();
        assert!(
            installed["hooks"]["PreToolUse"]
                .as_array()
                .unwrap()
                .contains(&foreign["hooks"]["PreToolUse"][0])
        );
        assert!(fs::read_to_string(&config_path).unwrap().contains("model = 'test'"));
        let mut removed = installed;
        remove_owned_hooks(&mut removed, &first, &hooks_path, &root.path().join("receipt.json")).unwrap();
        assert_eq!(removed, foreign);
        edit_mcp(&config_path, Some(&first), None, &mut Compensation::default()).unwrap();
        assert!(!fs::read_to_string(&config_path).unwrap().contains("[mcp_servers.appa]"));
    }

    #[test]
    fn hook_mismatches_name_the_affected_event_without_partial_removal() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("hooks.json");
        let receipt_path = root.path().join("receipt.json");
        let receipt = Receipt {
            hooks: hook_groups(Path::new("/tmp/appa"), "http://127.0.0.1:8766").unwrap(),
            mcp_url: "http://127.0.0.1:8766/mcp".into(),
            config: root.path().join("appa.toml"),
        };
        let groups: BTreeMap<_, _> = receipt
            .hooks
            .iter()
            .map(|(event, group)| (event.clone(), json!([group])))
            .collect();
        let original = json!({"hooks": groups});
        let mutations: [fn(&mut Value); 8] = [
            |doc| doc["hooks"]["PreToolUse"][0]["hooks"][0]["timeout"] = json!(131),
            |doc| doc["hooks"]["PreToolUse"][0]["hooks"][0]["command"] = json!("edited command"),
            |doc| doc["hooks"]["PreToolUse"][0]["matcher"] = json!("Bash"),
            |doc| doc["hooks"]["PreToolUse"][0]["custom"] = json!(true),
            |doc| {
                doc["hooks"]["PreToolUse"][0]["hooks"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"command": "foreign"}));
            },
            |doc| doc["hooks"]["PreToolUse"] = json!([]),
            |doc| {
                doc["hooks"].as_object_mut().unwrap().remove("PreToolUse");
            },
            |doc| doc["hooks"]["PreToolUse"] = json!({}),
        ];
        for mutate in mutations {
            let mut document = original.clone();
            mutate(&mut document);
            let before = document.clone();
            let error = remove_owned_hooks(&mut document, &receipt, &path, &receipt_path)
                .unwrap_err()
                .to_string();
            assert!(error.contains("PreToolUse"), "{error}");
            assert!(error.contains(receipt_path.to_str().unwrap()), "{error}");
            assert_eq!(document, before);
        }
        let mut document = original;
        document["hooks"]["PreToolUse"]
            .as_array_mut()
            .unwrap()
            .push(json!({"hooks": [{"command": "foreign"}]}));
        remove_owned_hooks(&mut document, &receipt, &path, &receipt_path).unwrap();
        assert_eq!(
            document["hooks"]["PreToolUse"],
            json!([{"hooks": [{"command": "foreign"}]}])
        );
    }

    #[test]
    fn edited_owned_hook_is_refused_before_removal() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("hooks.json");
        let receipt = Receipt {
            hooks: hook_groups(Path::new("/tmp/appa"), "http://127.0.0.1:8766").unwrap(),
            mcp_url: "http://127.0.0.1:8766/mcp".into(),
            config: root.path().join("appa.toml"),
        };
        let mut hooks = json!({"hooks": {"PreToolUse": [{"hooks": []}]}});
        assert!(remove_owned_hooks(&mut hooks, &receipt, &path, &root.path().join("receipt.json")).is_err());
    }

    #[test]
    fn edited_mcp_entry_is_preserved_and_refused() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config.toml");
        let original = "[mcp_servers.appa]\nurl = 'http://127.0.0.1:8766/mcp'\nenabled = false\n";
        fs::write(&config, original).unwrap();
        let receipt = Receipt {
            hooks: BTreeMap::new(),
            mcp_url: "http://127.0.0.1:8766/mcp".into(),
            config: root.path().join("appa.toml"),
        };
        let mut undo = Compensation::default();
        assert!(edit_mcp(&config, Some(&receipt), None, &mut undo).is_err());
        assert_eq!(fs::read_to_string(config).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn profile_links_survive_hook_and_mcp_edits() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("dotfiles");
        fs::create_dir(&target).unwrap();
        let hooks_target = target.join("hooks.json");
        let config_target = target.join("config.toml");
        let hooks = root.path().join("hooks.json");
        let config = root.path().join("config.toml");
        fs::write(&hooks_target, b"{}\n").unwrap();
        fs::write(&config_target, b"model = 'test'\n").unwrap();
        std::os::unix::fs::symlink(&hooks_target, &hooks).unwrap();
        std::os::unix::fs::symlink(&config_target, &config).unwrap();
        let receipt = Receipt {
            hooks: hook_groups(Path::new("/tmp/appa"), "http://127.0.0.1:8766").unwrap(),
            mcp_url: "http://127.0.0.1:8766/mcp".into(),
            config: root.path().join("appa.toml"),
        };
        let mut undo = Compensation::default();
        install_hooks(&hooks, None, &root.path().join("receipt.json"), &receipt, &mut undo).unwrap();
        edit_mcp(&config, None, Some(&receipt.mcp_url), &mut undo).unwrap();
        assert_eq!(fs::read_link(&hooks).unwrap(), hooks_target);
        assert_eq!(fs::read_link(&config).unwrap(), config_target);
        assert!(fs::read_to_string(&hooks_target).unwrap().contains("PreToolUse"));
        assert!(fs::read_to_string(&config_target).unwrap().contains("mcp_servers.appa"));
        undo.unwind().unwrap();
        assert_eq!(fs::read_to_string(&hooks_target).unwrap(), "{}\n");
        assert_eq!(fs::read_to_string(&config_target).unwrap(), "model = 'test'\n");
        assert_eq!(fs::read_link(&hooks).unwrap(), hooks_target);
        assert_eq!(fs::read_link(&config).unwrap(), config_target);
    }

    #[test]
    fn launcher_rejects_overrides_of_protected_controls() {
        for arguments in [
            vec!["--disable", "hooks"],
            vec!["--enable", "multi_agent"],
            vec!["-c", "default_permissions = ':workspace'"],
            vec!["--config=features.network_proxy=false"],
            vec!["--dangerously-bypass-hook-trust"],
            vec!["--sandbox", "danger-full-access"],
        ] {
            let arguments = arguments.into_iter().map(OsString::from).collect::<Vec<_>>();
            assert!(probe_options(&arguments).is_err(), "{arguments:?}");
        }
        let allowed = vec![
            OsString::from("-C"),
            OsString::from("/tmp/project"),
            OsString::from("--no-alt-screen"),
        ];
        assert_eq!(probe_options(&allowed).unwrap(), allowed[..2]);
        for arguments in [
            vec!["--enable", "apps"],
            vec!["--enable=browser_use"],
            vec!["--config=features.apps=true"],
            vec!["-c", "features.browser_use=true"],
            vec!["--search"],
            vec!["--disable", "code_mode_host"],
            vec!["--disable=code_mode_host"],
            vec!["--config=features.code_mode_host=false"],
        ] {
            let arguments = arguments.into_iter().map(OsString::from).collect::<Vec<_>>();
            assert!(probe_options(&arguments).is_ok(), "{arguments:?}");
        }
    }

    #[test]
    fn launcher_rejects_conflicting_approval_options() {
        for arguments in [
            vec!["-a", "never"],
            vec!["-anever"],
            vec!["--ask-for-approval=never"],
            vec!["--ask-for-approval", "untrusted"],
            vec!["-c", "approval_policy='never'"],
            vec!["-capproval_policy=never"],
            vec!["--config=\"approval_policy\" = 'never'"],
            vec!["-c", "approval_policy.granular.mcp_elicitations=false"],
            vec!["-c", "approvals_reviewer=auto_review"],
            vec!["-c", "mcp_servers.appa.tools.execute_remedy_plan.approval_mode=prompt"],
            vec!["--config=mcp_servers.appa.tools={execute_remedy_plan={approval_mode='auto'}}"],
            vec!["--config=mcp_servers.appa={url='http://localhost/mcp'}"],
        ] {
            let arguments = arguments.into_iter().map(OsString::from).collect::<Vec<_>>();
            let error = probe_options(&arguments).unwrap_err();
            assert!(error.contains("approval"), "{arguments:?}: {error}");
        }
        for arguments in [
            vec!["-a", "on-request"],
            vec!["--ask-for-approval=on-request"],
            vec!["-c", "approval_policy=on-request"],
            vec!["-c", "approvals_reviewer='user'"],
            vec!["--config=mcp_servers.appa.tools.execute_remedy_plan.approval_mode='approve'"],
            vec!["-c", "mcp_servers.other.tools.write.approval_mode='prompt'"],
            vec!["--", "--ask-for-approval=never"],
        ] {
            let arguments = arguments.into_iter().map(OsString::from).collect::<Vec<_>>();
            assert!(probe_options(&arguments).is_ok(), "{arguments:?}");
        }
    }
}
