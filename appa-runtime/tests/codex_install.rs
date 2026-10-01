#![cfg(unix)]

mod common;

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use common::repo_root;

struct Fixture {
    root: tempfile::TempDir,
    config: PathBuf,
    endpoint: String,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let codex = root.path().join("codex-home");
        let claude = root.path().join("claude-home");
        fs::create_dir_all(&codex).unwrap();
        fs::create_dir_all(&claude).unwrap();
        fs::write(
            codex.join("hooks.json"),
            r#"{"hooks":{"PreToolUse":[{"matcher":"Read","hooks":[{"type":"command","command":"mine"}]}]}}"#,
        )
        .unwrap();
        fs::write(
            codex.join("config.toml"),
            "model = 'test'\napproval_policy = 'never'\napprovals_reviewer = 'auto_review'\n[mcp_servers.other]\nurl = 'http://127.0.0.1:9999/mcp'\n",
        )
        .unwrap();
        fs::write(claude.join("settings.json"), "{\"theme\":\"dark\"}\n").unwrap();
        let config = root.path().join("config/codex/appa.toml");
        let battery = config.parent().unwrap().join("batteries/codex/appa.toml");
        fs::create_dir_all(battery.parent().unwrap()).unwrap();
        fs::copy(repo_root().join("marketplace/batteries/codex/appa.toml"), &battery).unwrap();
        let default = fs::read_to_string(repo_root().join("marketplace/plugins/codex/default.appa.toml")).unwrap();
        fs::write(&config, format!("include = [\"batteries/codex/appa.toml\"]\n{default}")).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        Self { root, config, endpoint }
    }

    fn process(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_appa"));
        command
            .args(args)
            .env("HOME", self.root.path())
            .env("CODEX_HOME", self.root.path().join("codex-home"))
            .env("CLAUDE_CONFIG_DIR", self.root.path().join("claude-home"))
            .env("APPA_CONFIG_DIR", self.root.path().join("config"))
            .env("APPA_DATA_DIR", self.root.path().join("data"))
            .env("APPA_ENDPOINT", &self.endpoint);
        command
    }

    fn command(&self, args: &[&str]) -> Output {
        self.process(args).output().unwrap()
    }

    fn profile(&self) -> &Path {
        self.root.path()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(address) = self.endpoint.strip_prefix("http://")
            && let Ok(mut stream) = std::net::TcpStream::connect(address)
        {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(1)));
            let _ =
                stream.write_all(b"GET /binary-fingerprint HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
            let mut answer = String::new();
            let _ = stream.take(4096).read_to_string(&mut answer);
            if let Some(body) = answer.split("\r\n\r\n").nth(1)
                && let Some(pid) = body.split_whitespace().nth(1).and_then(|pid| pid.parse::<i32>().ok())
            {
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
            }
        }
    }
}

#[test]
fn direct_codex_activation_reinstall_and_removal_preserve_both_profiles() {
    let fixture = Fixture::new();
    let config = fixture.config.to_str().unwrap();
    for _ in 0..2 {
        let output = fixture.command(&["activate-codex", "--config", config]);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    }
    let hooks: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.profile().join("codex-home/hooks.json")).unwrap()).unwrap();
    assert_eq!(hooks["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
    let pre = hooks["hooks"]["PreToolUse"][1]["hooks"][0]["command"].as_str().unwrap();
    assert!(pre.contains("codex-hook --event PreToolUse"), "{pre}");
    let start = hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(start.contains("codex-hook --event SessionStart"), "{start}");
    assert!(!start.contains("--ensure-runtime"), "{start}");
    let host_config = fs::read_to_string(fixture.profile().join("codex-home/config.toml")).unwrap();
    assert!(host_config.contains("[mcp_servers.other]"));
    assert!(host_config.contains("[mcp_servers.appa]"));
    assert!(host_config.contains("[permissions.appa.network.domains]"));
    assert!(fixture.profile().join("codex-home/skills/appa-guide/SKILL.md").exists());
    assert_eq!(
        fs::read_to_string(
            fixture
                .profile()
                .join("codex-home/skills/appa-guide/references/contracts.md")
        )
        .unwrap(),
        fs::read_to_string(repo_root().join("website/content/docs/contracts.md")).unwrap(),
    );
    assert_eq!(
        fs::read_to_string(fixture.profile().join("claude-home/settings.json")).unwrap(),
        "{\"theme\":\"dark\"}\n"
    );

    let removed = fixture.command(&["remove-codex", "--config", config]);
    assert!(removed.status.success(), "{}", String::from_utf8_lossy(&removed.stderr));
    let hooks: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.profile().join("codex-home/hooks.json")).unwrap()).unwrap();
    assert_eq!(hooks["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    let host_config = fs::read_to_string(fixture.profile().join("codex-home/config.toml")).unwrap();
    assert!(host_config.contains("[mcp_servers.other]"));
    assert!(!host_config.contains("[mcp_servers.appa]"));
    assert!(!host_config.contains("[permissions.appa]"));
    assert!(!fixture.profile().join("codex-home/skills/appa-guide/SKILL.md").exists());
    assert!(
        !fixture
            .profile()
            .join("codex-home/skills/appa-guide/references/contracts.md")
            .exists()
    );
    assert_eq!(
        fs::read_to_string(fixture.profile().join("claude-home/settings.json")).unwrap(),
        "{\"theme\":\"dark\"}\n"
    );
}

#[test]
fn launcher_inherits_host_mode_after_the_sandbox_check() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let config = fixture.config.to_str().unwrap();
    let installed = fixture.command(&["activate-codex", "--config", config]);
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );

    let bin = fixture.profile().join("fake-bin");
    fs::create_dir(&bin).unwrap();
    let fake_codex = bin.join("codex");
    let calls = fixture.profile().join("codex-calls.txt");
    fs::write(
        &fake_codex,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$APPA_DENY_SANDBOX\" = 1 ]; then exit 1; fi\nexit 0\n",
            calls.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fake_codex, fs::Permissions::from_mode(0o755)).unwrap();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let path = std::env::join_paths(paths).unwrap();
    let launch = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_appa"));
        command
            .arg("codex")
            .env("HOME", fixture.profile())
            .env("CODEX_HOME", fixture.profile().join("codex-home"))
            .env("APPA_CONFIG_DIR", fixture.profile().join("config"))
            .env("APPA_DATA_DIR", fixture.profile().join("data"))
            .env("APPA_ENDPOINT", &fixture.endpoint)
            .env("PATH", &path);
        command
    };
    let host_config_path = fixture.profile().join("codex-home/config.toml");
    let original_config = fs::read(&host_config_path).unwrap();
    let incompatible = launch().args(["--", "--ask-for-approval", "never"]).output().unwrap();
    assert!(!incompatible.status.success());
    assert!(String::from_utf8_lossy(&incompatible.stderr).contains("APPA requires approval_policy=on-request"));
    assert!(!calls.exists(), "a conflicting option must fail before Codex starts");
    let strict = launch().env("APPA_DENY_SANDBOX", "1").output().unwrap();
    assert!(!strict.status.success());
    assert!(String::from_utf8_lossy(&strict.stderr).contains("command sandbox cannot reach"));

    let output = launch()
        .args([
            "--",
            "-c",
            "features.code_mode_host=false",
            "--search",
            "--enable",
            "apps",
            "--enable=browser_use",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let calls = fs::read_to_string(calls).unwrap();
    let mut calls = calls.lines();
    assert!(calls.next().unwrap().starts_with("sandbox "));
    assert!(calls.next().unwrap().starts_with("sandbox "));
    let launch = calls.next().unwrap();
    assert!(launch.contains("features.hooks=true"), "{launch}");
    assert!(launch.contains("features.code_mode_host=false"), "{launch}");
    assert!(!launch.contains("features.code_mode_host=true"), "{launch}");
    assert!(!launch.contains("features.apps=false"), "{launch}");
    assert!(!launch.contains("features.browser_use=false"), "{launch}");
    assert!(launch.contains("features.multi_agent=false"), "{launch}");
    assert!(launch.contains("approval_policy=\"on-request\""), "{launch}");
    assert!(launch.contains("approvals_reviewer=\"user\""), "{launch}");
    assert!(
        launch.contains("mcp_servers.appa.tools.execute_remedy_plan.approval_mode=\"approve\""),
        "{launch}"
    );
    assert!(!launch.contains("default_tools_approval_mode"), "{launch}");
    assert!(
        launch.contains("--search --enable apps --enable=browser_use"),
        "{launch}"
    );
    assert!(calls.next().is_none());
    assert_eq!(fs::read(host_config_path).unwrap(), original_config);
}

#[test]
#[cfg(feature = "daemon")]
fn terminal_footer_tracks_labels_and_restores_the_terminal() {
    terminal_footer_case(false);
}

#[test]
#[cfg(feature = "daemon")]
fn terminal_footer_restores_the_terminal_after_a_signal() {
    terminal_footer_case(true);
}

#[cfg(feature = "daemon")]
fn terminal_footer_case(interrupt: bool) {
    appa_runtime::tls::install_crypto_provider();
    use std::os::unix::fs::PermissionsExt;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let fixture = Fixture::new();
    fs::write(&fixture.config, "[externals]\ntimeout_ms = 5000\nmax_body_bytes = 65536\n[policy]\nversion = 2\n[[policy.tool]]\nname = 'host/codex/audit_probe'\ndelta = { trust = 'suspicious', audience = ['internal'] }\n").unwrap();
    let installed = fixture.command(&["activate-codex", "--config", fixture.config.to_str().unwrap()]);
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    let bin = fixture.profile().join("fake-bin");
    fs::create_dir(&bin).unwrap();
    let fake = bin.join("codex");
    fs::write(
        &fake,
        r#"#!/usr/bin/env python3
import json, os, subprocess, sys, tty
if sys.argv[1] == 'sandbox':
    sys.exit(0)
tty.setraw(0)
def hook(event, **fields):
    payload = dict(hook_event_name=event, session_id='footer-test', **fields)
    result = subprocess.run([os.environ['APPA_TEST_BIN'], 'codex-hook', '--event', event,
        '--deployment-url', os.environ['APPA_ENDPOINT']], input=json.dumps(payload).encode(), capture_output=True)
    assert result.returncode == 0, result.stderr
    answer = json.loads(result.stdout or '{}')
    with open(os.environ['APPA_TEST_HOOK_LOG'], 'a') as log:
        log.write(event + ': ' + json.dumps(answer) + '\n')
    return answer
hook('SessionStart')
hook('UserPromptSubmit', prompt='hello')
size = os.get_terminal_size()
sys.stdout.write('\x1b[?1049h\x1b[2J\x1b[Hviewport:%dx%d' % (size.columns, size.lines))
sys.stdout.flush()
while True:
    key = os.read(0, 1)
    if key == b'r':
        size = os.get_terminal_size()
        sys.stdout.write('\x1b[Hviewport:%dx%d' % (size.columns, size.lines))
        sys.stdout.flush()
    elif key == b'l':
        sys.stdout.write('\x1b[2;1Hlabel-update')
        sys.stdout.flush()
        hook('PreToolUse', tool_name='audit_probe', tool_input={}, tool_use_id='read')
        sys.stdout.write('\x1b[2;1Hoffer-ready ')
        sys.stdout.flush()
    elif key == b'p':
        hook('PreToolUse', tool_name='audit_probe', tool_input={}, tool_use_id='read')
        hook('PostToolUse', tool_name='audit_probe', tool_input={}, tool_use_id='read', tool_response='data')
    elif key == b'q':
        sys.exit(7)
    elif key == b't':
        os.kill(os.getppid(), 15)
"#,
    )
    .unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let path = std::env::join_paths(paths).unwrap();
    let size = portable_pty::PtySize {
        rows: 12,
        cols: 80,
        ..portable_pty::PtySize::default()
    };
    let pair = portable_pty::native_pty_system().openpty(size).unwrap();
    let mut command = portable_pty::CommandBuilder::new(env!("CARGO_BIN_EXE_appa"));
    command.args(["codex", "--"]);
    for (key, value) in fixture.process(&[]).get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        }
    }
    command.env("PATH", path);
    command.env("APPA_TEST_BIN", env!("CARGO_BIN_EXE_appa"));
    let hook_log = fixture.profile().join("hook-log");
    command.env("APPA_TEST_HOOK_LOG", &hook_log);
    let mut child = pair.slave.spawn_command(command).unwrap();
    // Kill a stalled fixture if an assertion fails.
    struct KillOnDrop(Box<dyn portable_pty::ChildKiller + Send + Sync>);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            let _ = self.0.kill();
        }
    }
    let _cleanup = KillOnDrop(child.clone_killer());
    let original = pair.master.get_termios().unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0; 16384];
        while let Ok(length) = reader.read(&mut buffer) {
            if length == 0 || send.send(buffer[..length].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut terminal = vt100::Parser::new(12, 80, 0);
    let read_until = |terminal: &mut vt100::Parser, needle: &str| {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !terminal.screen().contents().contains(needle) {
            let bytes = receive
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|error| {
                    panic!(
                        "missing {needle}: {error}; screen: {}; hooks: {}",
                        terminal.screen().contents(),
                        fs::read_to_string(&hook_log).unwrap_or_default()
                    )
                });
            terminal.process(&bytes);
        }
    };
    read_until(&mut terminal, "trust:trusted  audience:public");
    assert!(terminal.screen().contents().contains("viewport:80x10"));
    writer.write_all(b"l").unwrap();
    read_until(&mut terminal, "offer-ready");
    let log = fs::read_to_string(&hook_log).unwrap();
    let answer: serde_json::Value =
        serde_json::from_str(log.lines().last().unwrap().strip_prefix("PreToolUse: ").unwrap()).unwrap();
    let feedback = answer["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    let offer = common::offers(feedback)[0].0.clone();
    let control = serde_json::json!({"hook_event_name": "PreToolUse", "session_id": "footer-test",
        "tool_name": "mcp__appa__execute_remedy_plan", "tool_use_id": "accept-labels",
        "tool_input": {"offer_id": offer}});
    let mut hook = fixture
        .process(&["hook", "--adapter", "codex", "--deployment-url", &fixture.endpoint])
        .env("APPA_GATE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    hook.stdin
        .take()
        .unwrap()
        .write_all(control.to_string().as_bytes())
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&hook.wait_with_output().unwrap().stdout).unwrap(),
        serde_json::json!({})
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            use rmcp::service::{ClientLifecycleMode, ClientServiceExt};
            struct Client;
            impl rmcp::ClientHandler for Client {}
            let transport =
                rmcp::transport::StreamableHttpClientTransport::from_uri(format!("{}/mcp", fixture.endpoint));
            let client = Client
                .serve_with_lifecycle(transport, ClientLifecycleMode::Initialize)
                .await
                .unwrap();
            let mut params = rmcp::model::CallToolRequestParams::default();
            params.name = "execute_remedy_plan".into();
            params.arguments = serde_json::json!({"offer_id": offer}).as_object().cloned();
            let result = client.call_tool(params).await.unwrap();
            assert_ne!(result.is_error, Some(true), "{:?}", result.content);
            client.cancel().await.unwrap();
        });
    writer.write_all(b"p").unwrap();
    read_until(&mut terminal, "trust:suspicious  audience:internal");
    pair.master
        .resize(portable_pty::PtySize {
            rows: 15,
            cols: 70,
            ..size
        })
        .unwrap();
    terminal.screen_mut().set_size(15, 70);
    std::thread::sleep(Duration::from_millis(100));
    writer.write_all(b"r").unwrap();
    read_until(&mut terminal, "viewport:70x13");
    assert!(
        terminal
            .screen()
            .contents()
            .contains("trust:suspicious  audience:internal")
    );
    writer.write_all(if interrupt { b"t" } else { b"q" }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the terminal wrapper did not exit");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.exit_code(), if interrupt { 143 } else { 7 });
    while let Ok(bytes) = receive.try_recv() {
        terminal.process(&bytes);
    }
    assert!(!terminal.screen().alternate_screen());
    assert!(!terminal.screen().hide_cursor());
    assert_eq!(pair.master.get_termios().unwrap(), original);
}

#[test]
fn codex_hook_guard_denies_a_worker_failure() {
    use std::process::Stdio;

    let mut child = Command::new(env!("CARGO_BIN_EXE_appa"))
        .args([
            "codex-hook",
            "--event",
            "PreToolUse",
            "--deployment-url",
            "http://127.0.0.1:1",
        ])
        .env("APPA_GATE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(br#"{"hook_event_name":"PreToolUse","session_id":"s1","tool_name":"Bash","tool_input":{"command":"printf secret"}}"#).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let answer: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(answer["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("printf secret"));
}

fn success(output: Output) -> String {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn hook_decision(fixture: &Fixture, session: &str) -> String {
    let mut child = fixture
        .process(&["hook", "--adapter", "codex", "--deployment-url", &fixture.endpoint])
        .env("APPA_GATE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let event = serde_json::json!({
        "hook_event_name": "PreToolUse", "session_id": session,
        "tool_name": "audit_probe", "tool_use_id": session,
        "cwd": fixture.profile(), "tool_input": {}
    });
    child
        .stdin
        .take()
        .unwrap()
        .write_all(event.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let answer: serde_json::Value = serde_json::from_str(&success(output)).unwrap();
    if answer == serde_json::json!({}) {
        "allow".to_owned()
    } else {
        answer["hookSpecificOutput"]["permissionDecision"]
            .as_str()
            .unwrap_or_else(|| panic!("missing permission decision: {answer}"))
            .to_owned()
    }
}

fn remedy_hook(fixture: &Fixture, event: serde_json::Value) -> serde_json::Value {
    let mut child = fixture
        .process(&["hook", "--adapter", "codex", "--deployment-url", &fixture.endpoint])
        .env("APPA_GATE", "1")
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
    serde_json::from_str(&success(child.wait_with_output().unwrap())).unwrap()
}

#[derive(Clone)]
struct RemedyReviewer {
    answer: rmcp::model::ElicitationAction,
    reviews: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl rmcp::ClientHandler for RemedyReviewer {
    fn get_info(&self) -> rmcp::model::ClientConfig {
        let mut info = rmcp::model::ClientConfig::default();
        info.capabilities.elicitation = Some(Default::default());
        info
    }

    async fn create_elicitation(
        &self,
        request: rmcp::model::ElicitRequestParams,
        _: rmcp::service::RequestContext<rmcp::service::RoleClient>,
    ) -> Result<rmcp::model::ElicitResult, rmcp::ErrorData> {
        if let rmcp::model::ElicitRequestParams::FormElicitationParams { message, .. } = request {
            self.reviews.lock().unwrap().push(message);
        }
        Ok(rmcp::model::ElicitResult::new(self.answer.clone()))
    }
}

async fn execute_codex_remedy(fixture: &Fixture, offer: &str, reviewer: RemedyReviewer) -> String {
    use rmcp::service::{ClientLifecycleMode, ClientServiceExt};

    appa_runtime::tls::install_crypto_provider();
    let arguments = serde_json::json!({"offer_id": offer});
    assert_eq!(
        remedy_hook(
            fixture,
            serde_json::json!({
                "hook_event_name": "PreToolUse", "session_id": "remedy-test",
                "tool_name": "mcp__appa__execute_remedy_plan", "tool_use_id": "remedy",
                "cwd": fixture.profile(), "tool_input": arguments,
            })
        ),
        serde_json::json!({}),
    );
    let transport = rmcp::transport::StreamableHttpClientTransport::from_uri(format!("{}/mcp", fixture.endpoint));
    let client = reviewer
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![rmcp::model::ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .unwrap();
    let mut params = rmcp::model::CallToolRequestParams::default();
    params.name = "execute_remedy_plan".into();
    params.arguments = arguments.as_object().cloned();
    let result = client.call_tool(params).await.unwrap();
    assert_ne!(result.is_error, Some(true), "{:?}", result.content);
    client.cancel().await.unwrap();
    format!("{:?}", result.content)
}

#[tokio::test]
async fn codex_remedies_sanitize_autonomously_and_require_human_approval() {
    let fixture = Fixture::new();
    fs::write(
        &fixture.config,
        r#"
[policy]
version = 2
[[policy.tool]]
name = "host/codex/read_private"
delta = { audience = ["self"] }
[[policy.tool]]
name = "host/codex/publish"
requires = { attention = ["signoff"] }
[[policy.sanitizer]]
name = "redact-secrets"
on = ["tool_output"]
[policy.sanitizer.permits]
audience = { from = ["self"], to = ["public"] }
[policy.deployment]
confined_results = ["host/codex/read_private"]
[[policy.authority]]
name = "operator"
hint = "The person at the keyboard."
[policy.authority.permits]
attention = ["signoff"]
[externals]
timeout_ms = 2000
review_timeout_ms = 5000
max_body_bytes = 65536
[externals.sanitizers.redact-secrets]
builtin = "redact-secrets"
[externals.authorities.operator]
builtin = "hitl"
"#,
    )
    .unwrap();
    success(fixture.command(&["activate-codex", "--config", fixture.config.to_str().unwrap()]));
    let event = |tool: &str| {
        serde_json::json!({
            "hook_event_name": "PreToolUse", "session_id": "remedy-test",
            "tool_name": tool, "tool_use_id": tool, "cwd": fixture.profile(), "tool_input": {},
        })
    };
    let blocked = remedy_hook(&fixture, event("read_private"));
    let feedback = blocked["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    assert!(feedback.contains("redact-secrets"), "{feedback}");
    let offer = common::last_offer(feedback);
    let reviewer = RemedyReviewer {
        answer: rmcp::model::ElicitationAction::Accept,
        reviews: Default::default(),
    };
    let authorized = execute_codex_remedy(&fixture, &offer.0, reviewer.clone()).await;
    assert!(authorized.contains("Authorized"), "{authorized}");
    assert!(
        reviewer.reviews.lock().unwrap().is_empty(),
        "sanitization must not request approval"
    );
    assert_eq!(remedy_hook(&fixture, event("read_private")), serde_json::json!({}));
    let mut result = event("read_private");
    result["hook_event_name"] = serde_json::json!("PostToolUse");
    result["tool_response"] = serde_json::json!({"token": "fixture-secret", "status": "ready"});
    let delivered = remedy_hook(&fixture, result).to_string();
    assert!(delivered.contains("redacted-secret"), "{delivered}");
    assert!(delivered.contains("ready"), "{delivered}");
    assert!(!delivered.contains("fixture-secret"), "{delivered}");

    let blocked = remedy_hook(&fixture, event("publish"));
    let feedback = blocked["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap();
    let offer = common::last_offer(feedback);
    let canceled = RemedyReviewer {
        answer: rmcp::model::ElicitationAction::Cancel,
        ..reviewer.clone()
    };
    let answer = execute_codex_remedy(&fixture, &offer.0, canceled).await;
    assert!(!answer.contains("Authorized"), "dismissal must not authorize: {answer}");
    let authorized = execute_codex_remedy(&fixture, &offer.0, reviewer.clone()).await;
    assert!(authorized.contains("Authorized"), "{authorized}");
    let reviews = reviewer.reviews.lock().unwrap();
    assert_eq!(reviews.len(), 2);
    assert!(
        reviews
            .iter()
            .all(|review| review.contains("publish") && review.contains("signoff"))
    );
    assert_eq!(remedy_hook(&fixture, event("publish")), serde_json::json!({}));
}

#[test]
fn installed_custom_policy_reloads_before_launch_and_through_the_explicit_command() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let custom = fixture.profile().join("custom.appa.toml");
    let allowed = "[externals]\ntimeout_ms = 5000\nmax_body_bytes = 65536\n[policy]\nversion = 2\n[[policy.tool]]\nname = 'host/codex/audit_probe'\n";
    fs::write(&custom, allowed).unwrap();
    success(fixture.command(&["activate-codex", "--config", custom.to_str().unwrap()]));
    let described = success(fixture.command(&["describe", "--adapter", "codex", "--check"]));
    assert!(described.contains(custom.to_str().unwrap()), "{described}");
    let first = success(fixture.command(&["codex-policy-key"]));
    assert_eq!(hook_decision(&fixture, "before-update"), "allow");

    // This represents the approved root edit. The check validates disk only.
    fs::write(&custom, format!("{allowed}requires = {{ attention = ['blocked'] }}\n")).unwrap();
    success(fixture.command(&["describe", "--adapter", "codex", "--check"]));
    assert_eq!(success(fixture.command(&["codex-policy-key"])), first);

    let bin = fixture.profile().join("fake-bin");
    fs::create_dir(&bin).unwrap();
    let fake = bin.join("codex");
    fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
    let launched = fixture
        .process(&["codex", "--"])
        .env("PATH", std::env::join_paths(paths).unwrap())
        .output()
        .unwrap();
    assert!(
        launched.status.success(),
        "{}",
        String::from_utf8_lossy(&launched.stderr)
    );
    let second = success(fixture.command(&["codex-policy-key"]));
    assert_ne!(first, second);
    assert!(String::from_utf8_lossy(&launched.stderr).contains(&second));
    assert_eq!(hook_decision(&fixture, "after-launch-update"), "deny");

    fs::write(&custom, allowed).unwrap();
    assert_eq!(success(fixture.command(&["codex-policy-key"])), second);
    assert_eq!(success(fixture.command(&["codex-reload"])), first);
    assert_eq!(hook_decision(&fixture, "after-explicit-reload"), "allow");

    fs::write(&custom, "invalid TOML").unwrap();
    assert!(!fixture.command(&["codex-reload"]).status.success());
    assert!(!fixture.process(&["codex", "--"]).output().unwrap().status.success());
    assert_eq!(success(fixture.command(&["codex-policy-key"])), first);

    // An unreadable receipt cannot silently select the default policy.
    fs::write(fixture.profile().join("data/codex/install-receipt.json"), "broken").unwrap();
    assert!(
        !fixture
            .command(&["describe", "--adapter", "codex", "--check"])
            .status
            .success()
    );
    assert!(
        fixture
            .command(&[
                "describe",
                "--adapter",
                "codex",
                "--config",
                fixture.config.to_str().unwrap(),
                "--check"
            ])
            .status
            .success()
    );
}

#[test]
fn hook_edit_diagnostic_supports_targeted_restoration_and_preserves_custom_hooks() {
    let fixture = Fixture::new();
    let config = fixture.config.to_str().unwrap();
    success(fixture.command(&["activate-codex", "--config", config]));
    let path = fixture.profile().join("codex-home/hooks.json");
    let receipt_path = fixture.profile().join("data/codex/install-receipt.json");
    let receipt: serde_json::Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    let mut hooks: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let foreign = serde_json::json!({"type": "command", "command": "custom-in-shared-group"});
    hooks["hooks"]["PreToolUse"][1]["hooks"][0]["timeout"] = serde_json::json!(131);
    hooks["hooks"]["PreToolUse"][1]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(foreign.clone());
    let edited = serde_json::to_vec_pretty(&hooks).unwrap();
    fs::write(&path, &edited).unwrap();
    for arguments in [
        vec!["codex", "--"],
        vec!["activate-codex", "--config", config],
        vec!["remove-codex", "--config", config],
    ] {
        let refused = fixture.command(&arguments);
        assert!(!refused.status.success());
        let error = String::from_utf8_lossy(&refused.stderr);
        for expected in [
            "PreToolUse",
            path.to_str().unwrap(),
            receipt_path.to_str().unwrap(),
            "Restore only the affected",
            "Do not delete the receipt",
        ] {
            assert!(error.contains(expected), "{error}");
        }
        assert_eq!(fs::read(&path).unwrap(), edited);
        assert!(receipt_path.exists());
    }
    // Execute the diagnostic procedure: retain the edit in a backup and move
    // the unrelated hook to its own group before restoring just the APPA group.
    fs::write(path.with_extension("json.backup"), &edited).unwrap();
    let matcher = hooks["hooks"]["PreToolUse"][1]["matcher"].clone();
    hooks["hooks"]["PreToolUse"][1] = receipt["hooks"]["PreToolUse"].clone();
    hooks["hooks"]["PreToolUse"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"matcher": matcher, "hooks": [foreign]}));
    fs::write(&path, serde_json::to_vec_pretty(&hooks).unwrap()).unwrap();
    success(fixture.command(&["activate-codex", "--config", config]));
    success(fixture.command(&["remove-codex", "--config", config]));
    assert_eq!(fs::read(path.with_extension("json.backup")).unwrap(), edited);
    let remaining: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(remaining["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
    assert_eq!(
        remaining["hooks"]["PreToolUse"][1]["hooks"][0]["command"],
        "custom-in-shared-group"
    );
    assert_eq!(remaining["hooks"]["PreToolUse"][1]["matcher"], "*");
}
