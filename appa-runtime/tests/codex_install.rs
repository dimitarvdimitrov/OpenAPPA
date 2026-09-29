#![cfg(unix)]

mod common;

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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
            "model = 'test'\n[mcp_servers.other]\nurl = 'http://127.0.0.1:9999/mcp'\n",
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

    fn command(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_appa"))
            .args(args)
            .env("HOME", self.root.path())
            .env("CODEX_HOME", self.root.path().join("codex-home"))
            .env("CLAUDE_CONFIG_DIR", self.root.path().join("claude-home"))
            .env("APPA_CONFIG_DIR", self.root.path().join("config"))
            .env("APPA_DATA_DIR", self.root.path().join("data"))
            .env("APPA_ENDPOINT", &self.endpoint)
            .output()
            .unwrap()
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
    let host_config = fs::read_to_string(fixture.profile().join("codex-home/config.toml")).unwrap();
    assert!(host_config.contains("[mcp_servers.other]"));
    assert!(host_config.contains("[mcp_servers.appa]"));
    assert!(host_config.contains("[permissions.appa.network.domains]"));
    assert!(fixture.profile().join("codex-home/skills/appa-guide/SKILL.md").exists());
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
    assert_eq!(
        fs::read_to_string(fixture.profile().join("claude-home/settings.json")).unwrap(),
        "{\"theme\":\"dark\"}\n"
    );
}
