#!/usr/bin/env python3
"""Opt-in synthetic Codex -> hook -> sandboxed APPA proxy -> Codex probe.

This disposable compatibility probe uses the saved Codex login and a trust
bypass only for its temporary hook definitions. It is not a protected launch.
"""

import argparse
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile


MARKER = "APPA_E2E_MARKER"


def run(codex: str, appa: Path, timeout: int) -> dict:
    with tempfile.TemporaryDirectory(prefix="appa-codex-live-") as directory:
        project = Path(directory)
        policy = project / "policy.toml"
        policy.write_text(
            '[policy]\nversion = 2\n[[policy.tool]]\n'
            'name = "host/codex/appa_exec"\n'
            '[externals]\ntimeout_ms = 5000\nmax_body_bytes = 65536\n'
        )
        runtime = subprocess.Popen(
            [str(appa), "runtime", "--adapter", "codex", "--config", str(policy),
             "--db", str(project / "appa.db"), "--listen", "127.0.0.1:0"],
            cwd=project, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
        )
        try:
            url = runtime.stdout.readline().strip()
            if not url.startswith("http://127.0.0.1:"):
                return {"status": "fail", "reason": "runtime did not start on loopback"}
            hook = " ".join(
                shlex.quote(part) for part in
                [str(appa), "hook", "--adapter", "codex", "--deployment-url", url]
            )
            hook_group = '[{hooks=[{type="command",command=' + json.dumps(hook) + ',timeout=130}]}]'
            command = [
                codex, "exec", "--ephemeral", "--ignore-user-config", "--ignore-rules",
                "--skip-git-repo-check", "--dangerously-bypass-hook-trust", "--json",
                "-C", str(project), "-c", "features.hooks=true",
                "-c", "features.network_proxy=true",
                "-c", 'permissions.appa.extends=":workspace"',
                "-c", "permissions.appa.network.enabled=true",
                "-c", 'permissions.appa.network.domains={"127.0.0.1"="allow"}',
                "-c", 'default_permissions="appa"',
            ]
            for event in ("SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "Stop"):
                command.extend(["-c", f"hooks.{event}={hook_group}"])
            command.append(
                f"Use the shell tool to execute `printf {MARKER}` exactly once. "
                "Then give the observed stdout."
            )
            environment = dict(os.environ, APPA_GATE="1", SHELL="/bin/sh")
            environment.pop("APPA_RUNTIME_URL", None)
            completed = subprocess.run(
                command, cwd=project, env=environment, capture_output=True,
                text=True, timeout=timeout, check=False,
            )
            events = []
            for line in completed.stdout.splitlines():
                try:
                    events.append(json.loads(line))
                except json.JSONDecodeError:
                    pass
            items = [event.get("item", {}) for event in events if event.get("type") == "item.completed"]
            commands = [item for item in items if item.get("type") == "command_execution"]
            messages = [item.get("text", "") for item in items if item.get("type") == "agent_message"]
            valid = (
                completed.returncode == 0 and len(commands) == 1
                and "codex-exec" in (commands[0].get("command") or "")
                and "printf" not in (commands[0].get("command") or "")
                and commands[0].get("aggregated_output") == MARKER
                and messages and MARKER in messages[-1]
            )
            return {
                "status": "pass" if valid else "fail",
                "codex_exit": completed.returncode,
                "command_count": len(commands),
                "rewritten": bool(commands and "codex-exec" in (commands[0].get("command") or "")),
                "admitted_marker": bool(commands and commands[0].get("aggregated_output") == MARKER),
                "model_reported_marker": bool(messages and MARKER in messages[-1]),
            }
        finally:
            runtime.terminate()
            try:
                runtime.wait(timeout=3)
            except subprocess.TimeoutExpired:
                runtime.kill()
                runtime.wait()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--codex", default="codex")
    parser.add_argument("--appa", type=Path, default=Path(__file__).resolve().parents[2] / "target/debug/appa")
    parser.add_argument("--timeout", type=int, default=150)
    args = parser.parse_args()
    if not args.appa.is_file():
        parser.error(f"build appa first: {args.appa}")
    try:
        report = run(args.codex, args.appa.resolve(), args.timeout)
    except subprocess.TimeoutExpired:
        report = {"status": "fail", "reason": "Codex timed out"}
    print(json.dumps(report, indent=2))
    return 0 if report["status"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
