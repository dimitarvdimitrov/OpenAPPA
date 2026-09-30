#!/usr/bin/env python3
"""Run one protected, ephemeral Codex session with the repository policy."""

import argparse
import json
import os
from pathlib import Path
import selectors
import shlex
import shutil
import subprocess
import sys
import tempfile


EVENTS = (
    "SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse",
    "SubagentStart", "SubagentStop", "Stop", "Interrupt", "SessionEnd",
    "PreCompact", "PostCompact",
)


def run(cwd: Path, prompt: str) -> int:
    root = Path(__file__).resolve().parents[2]
    appa = root / "target/debug/appa"
    if not appa.is_file():
        print("Build appa first: cargo build -p appa", file=sys.stderr)
        return 1
    if not cwd.is_dir():
        print(f"No such directory: {cwd}", file=sys.stderr)
        return 1
    with tempfile.TemporaryDirectory(prefix="appa-codex-protected-") as directory:
        deployment = Path(directory)
        battery = deployment / "batteries/codex"
        battery.mkdir(parents=True)
        shutil.copy2(root / "marketplace/batteries/codex/appa.toml", battery / "appa.toml")
        default = (root / "marketplace/plugins/codex/default.appa.toml").read_text()
        policy = deployment / "appa.toml"
        policy.write_text('include = ["batteries/codex/appa.toml"]\n' + default)
        with (deployment / "runtime.log").open("w") as log:
            runtime = subprocess.Popen(
                [str(appa), "runtime", "--adapter", "codex", "--config", str(policy),
                 "--db", str(deployment / "appa.db"), "--listen", "127.0.0.1:0"],
                stdout=subprocess.PIPE, stderr=log, text=True,
            )
            try:
                with selectors.DefaultSelector() as selector:
                    selector.register(runtime.stdout, selectors.EVENT_READ)
                    if not selector.select(timeout=15):
                        raise RuntimeError("APPA runtime did not report its address")
                url = runtime.stdout.readline().strip()
                if not url.startswith("http://127.0.0.1:"):
                    raise RuntimeError("APPA runtime did not start")
                hook_command = shlex.join([
                    str(appa), "hook", "--adapter", "codex", "--deployment-url", url,
                ])
                hook = '[{hooks=[{type="command",command=' + json.dumps(hook_command) + '}]}]'
                command = [
                    "codex", "exec", "--ephemeral", "--ignore-user-config", "--ignore-rules",
                    "--skip-git-repo-check", "--dangerously-bypass-hook-trust",
                    "--sandbox", "workspace-write", "-C", str(cwd),
                    "-c", "features.hooks=true", "-c", "features.multi_agent=true",
                    "-c", "features.multi_agent_v2=true",
                    "-c", f'mcp_servers.appa.url="{url}/mcp"',
                ]
                for event in EVENTS:
                    command.extend(["-c", f"hooks.{event}={hook}"])
                environment = dict(os.environ, APPA_GATE="1")
                return subprocess.run(command + [prompt], cwd=cwd, env=environment, check=False).returncode
            except RuntimeError as error:
                print(error, file=sys.stderr)
                log.flush()
                print((deployment / "runtime.log").read_text()[-2000:], file=sys.stderr)
                return 1
            finally:
                runtime.terminate()
                try:
                    runtime.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    runtime.kill()
                    runtime.wait()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cwd", type=Path, default=Path.cwd())
    parser.add_argument("prompt", help="Complete instruction for the protected Codex session")
    args = parser.parse_args()
    return run(args.cwd.resolve(), args.prompt)


if __name__ == "__main__":
    raise SystemExit(main())
