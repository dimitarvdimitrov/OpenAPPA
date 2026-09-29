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

use super::endpoint::{Endpoint, endpoint_health, reconcile_policy, verify_runtime_deployment};
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

fn remove_owned_hooks(document: &mut Value, previous: &Receipt, path: &Path) -> Result<(), InitError> {
    let Some(hooks) = document.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Err(profile_error(path, "an installed APPA hook group is missing"));
    };
    for (event, group) in &previous.hooks {
        let Some(groups) = hooks.get_mut(event).and_then(Value::as_array_mut) else {
            return Err(profile_error(
                path,
                format!("installed {event} hook group was removed or changed"),
            ));
        };
        let Some(index) = groups.iter().position(|candidate| candidate == group) else {
            return Err(profile_error(path, format!("installed {event} hook group was edited")));
        };
        groups.remove(index);
        if groups.is_empty() {
            hooks.remove(event);
        }
    }
    if hooks.is_empty() {
        document.as_object_mut().expect("object checked").remove("hooks");
    }
    Ok(())
}

fn shell_literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn powershell_literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn hook_command(
    binary: &Path,
    url: &str,
    config: &Path,
    data: &Path,
    start: bool,
    windows: bool,
) -> Result<String, InitError> {
    let path = binary
        .to_str()
        .ok_or_else(|| profile_error(binary, "path must be UTF-8 for Codex hooks"))?;
    let config = config
        .to_str()
        .ok_or_else(|| profile_error(config, "path must be UTF-8 for Codex hooks"))?;
    let data = data
        .to_str()
        .ok_or_else(|| profile_error(data, "path must be UTF-8 for Codex hooks"))?;
    let literal = if windows { powershell_literal } else { shell_literal };
    let mut parts = vec![
        "hook".to_owned(),
        "--adapter".into(),
        "codex".into(),
        "--deployment-url".into(),
        literal(url),
    ];
    if start {
        parts.extend([
            "--ensure-runtime".into(),
            "--config".into(),
            literal(config),
            "--data-dir".into(),
            literal(data),
        ]);
    }
    let head = if windows {
        format!("& {}", literal(path))
    } else {
        literal(path)
    };
    Ok(format!("{head} {}", parts.join(" ")))
}

fn hook_groups(binary: &Path, url: &str, config: &Path, data: &Path) -> Result<BTreeMap<String, Value>, InitError> {
    let mut groups = BTreeMap::new();
    let events = [
        ("SessionStart", None, true, 150),
        ("UserPromptSubmit", None, false, 130),
        ("PreToolUse", Some("*"), false, 130),
        ("PostToolUse", Some("*"), false, 130),
        ("SubagentStart", None, false, 130),
        ("SubagentStop", None, false, 130),
        ("Stop", None, false, 3),
        ("SessionEnd", None, false, 3),
    ];
    for (event, matcher, start, timeout) in events {
        let unix = hook_command(binary, url, config, data, start, false)?;
        let windows = hook_command(binary, url, config, data, start, true)?;
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
    wanted: &Receipt,
    compensation: &mut Compensation,
) -> Result<(), InitError> {
    let destination = profile_destination(path)?;
    let mut document = read_hooks(&destination)?;
    if let Some(previous) = previous {
        remove_owned_hooks(&mut document, previous, path)?;
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
        hooks: hook_groups(&binary, endpoint.url(), &config, &data)?,
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
        install_hooks(&hooks_path, previous.as_ref(), &wanted, &mut compensation)?;
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
    remove_owned_hooks(&mut hooks, &receipt, &hooks_path)?;
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
    let (hooks, _, receipt) = match profile_paths() {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("appa codex: {error}");
            return ExitCode::FAILURE;
        }
    };
    if !hooks.exists() || !receipt.exists() {
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
    if installed.hooks.iter().any(|(event, group)| {
        !current["hooks"][event]
            .as_array()
            .is_some_and(|groups| groups.contains(group))
    }) {
        eprintln!("appa codex: installed hooks changed; rerun appa plugin install codex and review /hooks");
        return ExitCode::FAILURE;
    }
    let endpoint = match Endpoint::resolve_for(AdapterName::Codex) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            eprintln!("appa codex: {error}");
            return ExitCode::FAILURE;
        }
    };
    let data = match codex_data_dir() {
        Ok(data) => data,
        Err(error) => {
            eprintln!("appa codex: {error}");
            return ExitCode::FAILURE;
        }
    };
    let binary = data.join("bin").join(appa_filename());
    let target = RuntimeTarget {
        url: endpoint.url().to_owned(),
        user_owned: false,
    };
    let deployment = crate::runtime_start::Deployment {
        config: installed.config.clone(),
        data_dir: data,
    };
    if let Err(error) = crate::runtime_start::ensure_for(&target, &deployment, &binary, &[], AdapterName::Codex) {
        eprintln!("appa codex: runtime check failed: {error}");
        return ExitCode::FAILURE;
    }
    if let Err(error) = verify_runtime_deployment(&binary, &installed.config, &endpoint) {
        eprintln!("appa codex: {error}");
        return ExitCode::FAILURE;
    }
    let probe_options = match probe_options(&arguments) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("appa codex: {message}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(message) = sandbox_probe(&binary, endpoint.url(), &probe_options) {
        eprintln!("appa codex: {message}");
        return ExitCode::FAILURE;
    }
    eprintln!(
        "appa codex: review and trust the installed hooks with /hooks; Codex has no supported noninteractive trust check"
    );
    let status = Command::new("codex")
        .args([
            "-c",
            "default_permissions=\"appa\"",
            "-c",
            "features.network_proxy=true",
            "-c",
            "features.apps=false",
            "-c",
            "features.browser_use=false",
            "-c",
            "features.code_mode_host=false",
            "-c",
            "features.multi_agent=false",
        ])
        .args(arguments)
        .env("APPA_GATE", "1")
        .status();
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
        if matches!(
            option.as_ref(),
            "--dangerously-bypass-approvals-and-sandbox" | "--dangerously-bypass-hook-trust" | "--search"
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
            if (option == "--disable" && matches!(value_text.as_ref(), "hooks" | "network_proxy"))
                || (option == "--enable"
                    && matches!(
                        value_text.as_ref(),
                        "apps" | "browser_use" | "code_mode_host" | "multi_agent"
                    ))
                || (matches!(option.as_ref(), "-c" | "--config") && forbidden_config_override(&value_text))
            {
                return Err("a Codex option disables hooks or replaces the protected permission profile");
            }
            options.extend([arguments[index].clone(), value.clone()]);
            index += 2;
            continue;
        }
        if matches!(option.as_ref(), "--disable=hooks" | "--disable=network_proxy")
            || ["apps", "browser_use", "code_mode_host", "multi_agent"]
                .iter()
                .any(|feature| option == format!("--enable={feature}"))
            || option.strip_prefix("--config=").is_some_and(forbidden_config_override)
        {
            return Err("a Codex option disables hooks or replaces the protected permission profile");
        }
        if ["--cd=", "--profile=", "--config=", "--enable=", "--disable="]
            .iter()
            .any(|prefix| option.starts_with(prefix))
        {
            options.push(arguments[index].clone());
        }
        index += 1;
    }
    Ok(options)
}

fn forbidden_config_override(value: &str) -> bool {
    let Some((key, setting)) = value.split_once('=') else {
        return false;
    };
    let key = key.trim();
    let setting = setting.trim();
    key == "default_permissions"
        || (matches!(key, "features.hooks" | "features.network_proxy") && setting == "false")
        || (matches!(
            key,
            "features.apps" | "features.browser_use" | "features.code_mode_host" | "features.multi_agent"
        ) && setting == "true")
}

fn sandbox_probe(binary: &Path, url: &str, options: &[OsString]) -> Result<(), String> {
    let mut child = Command::new("codex")
        .arg("sandbox")
        .args(["-P", "appa", "--enable", "network_proxy"])
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
            hooks: hook_groups(
                Path::new("/tmp/appa"),
                "http://127.0.0.1:8766",
                Path::new("/tmp/appa.toml"),
                root.path(),
            )
            .unwrap(),
            mcp_url: "http://127.0.0.1:8766/mcp".into(),
            config: root.path().join("appa.toml"),
        };
        let mut undo = Compensation::default();
        install_hooks(&hooks_path, None, &first, &mut undo).unwrap();
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
        remove_owned_hooks(&mut removed, &first, &hooks_path).unwrap();
        assert_eq!(removed, foreign);
        edit_mcp(&config_path, Some(&first), None, &mut Compensation::default()).unwrap();
        assert!(!fs::read_to_string(&config_path).unwrap().contains("[mcp_servers.appa]"));
    }

    #[test]
    fn edited_owned_hook_is_refused_before_removal() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("hooks.json");
        let receipt = Receipt {
            hooks: hook_groups(
                Path::new("/tmp/appa"),
                "http://127.0.0.1:8766",
                Path::new("/tmp/appa.toml"),
                root.path(),
            )
            .unwrap(),
            mcp_url: "http://127.0.0.1:8766/mcp".into(),
            config: root.path().join("appa.toml"),
        };
        let mut hooks = json!({"hooks": {"PreToolUse": [{"hooks": []}]}});
        assert!(remove_owned_hooks(&mut hooks, &receipt, &path).is_err());
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
            hooks: hook_groups(
                Path::new("/tmp/appa"),
                "http://127.0.0.1:8766",
                Path::new("/tmp/appa.toml"),
                root.path(),
            )
            .unwrap(),
            mcp_url: "http://127.0.0.1:8766/mcp".into(),
            config: root.path().join("appa.toml"),
        };
        let mut undo = Compensation::default();
        install_hooks(&hooks, None, &receipt, &mut undo).unwrap();
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
            vec!["--enable", "browser_use"],
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
    }
}
