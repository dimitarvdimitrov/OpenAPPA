#!/usr/bin/env python3
"""Start one protected, persistent Codex CLI invocation."""

import argparse
import json
import os
from pathlib import Path
import secrets
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


def protected_home(source: Path) -> Path:
    home = source / "appa-protected"
    home.mkdir(mode=0o700, parents=True, exist_ok=True)
    if home.is_symlink() or not home.is_dir():
        raise RuntimeError("the protected Codex home is not a directory")
    home.chmod(0o700)
    auth = source / "auth.json"
    if not auth.is_file():
        raise RuntimeError("Codex login is unavailable in the source home")
    if auth.stat().st_nlink != 1:
        raise RuntimeError("the Codex login file has another hard link")
    link = home / "auth.json"
    if not link.exists() and not link.is_symlink():
        link.symlink_to(auth)
    if link.resolve() != auth.resolve():
        raise RuntimeError("the protected Codex login link points elsewhere")
    return home


def profile(
    home: Path, source: Path, appa: Path, url: str = "http://127.0.0.1:9",
    proof: Path | None = None, name: str | None = None,
) -> None:
    proof = proof or home / "missing-proof"
    denied = sorted({source, home, (source / "auth.json").resolve().parent}, key=str)
    policy = (
        'default_permissions = "appa_protected"\n'
        '[features]\nnetwork_proxy = true\nhooks = true\nmulti_agent = true\nmulti_agent_v2 = true\n'
        '[permissions.appa_protected]\nextends = ":workspace"\n'
        '[permissions.appa_protected.filesystem]\n'
    )
    policy += "".join(json.dumps(str(path)) + ' = "deny"\n' for path in denied)
    policy += (
        '[permissions.appa_protected.network]\nenabled = true\n'
        '[permissions.appa_protected.network.domains]\n"127.0.0.1" = "allow"\n'
    )
    policy += "[hooks]\n"
    for event in EVENTS:
        hook_command = shlex.join([
            "env", "APPA_GATE=1", f"APPA_CODEX_PROOF_FILE={proof}",
            str(appa), "hook", "--adapter", "codex", "--deployment-url", url,
            "--expected-event", event,
        ])
        hook = '[{hooks=[{type="command",command=' + json.dumps(hook_command) + '}]}]'
        policy += f"{event} = {hook}\n"
    policy += f'[mcp_servers.appa]\nurl = {json.dumps(url + "/mcp")}\n'
    target = home / (f"{name}.config.toml" if name else "config.toml")
    temporary = home / f"{target.name}.{secrets.token_hex(8)}.new"
    temporary.write_text(policy)
    temporary.replace(target)


def preflight(home: Path, cwd: Path, proof: Path, environment: dict[str, str]) -> None:
    with tempfile.TemporaryDirectory(prefix="appa-profile-check-") as directory:
        control = Path(directory) / "control"
        control.write_text("available")
        access = subprocess.run(
            ["codex", "sandbox", "-P", "appa_protected", "-C", str(cwd),
             "/bin/sh", "-c", 'cat "$1"', "appa-check", str(control)],
            cwd=cwd, env=environment, text=True, capture_output=True, timeout=20, check=False,
        )
        if access.returncode != 0 or access.stdout != "available":
            raise RuntimeError("the Codex profile did not allow the control read")
        link = Path(directory) / "proof-link"
        link.symlink_to(proof)
        targets = (
            proof,
            link,
            cwd / os.path.relpath(proof, cwd),
            (home.parent / "auth.json").resolve(),
        )
        for target in targets:
            check = subprocess.run(
                ["codex", "sandbox", "-P", "appa_protected", "-C", str(cwd),
                 "/bin/sh", "-c", 'cat "$1" >/dev/null', "appa-check", str(target)],
                cwd=cwd, env=environment, text=True, capture_output=True, timeout=20, check=False,
            )
            if check.returncode == 0 or "Operation not permitted" not in check.stderr:
                raise RuntimeError("the Codex profile did not deny its private state")
        for command in ("ps -axo pid,command >/dev/null", "sysctl kern.proc.all >/dev/null"):
            check = subprocess.run(
                ["codex", "sandbox", "-P", "appa_protected", "-C", str(cwd),
                 "/bin/sh", "-c", command],
                cwd=cwd, env=environment, text=True, capture_output=True, timeout=20, check=False,
            )
            if check.returncode == 0 or "Operation not permitted" not in check.stderr:
                raise RuntimeError("the Codex profile permits process inspection")


def runtime_config(root: Path, home: Path) -> Path:
    battery = home / "batteries/codex"
    battery.mkdir(parents=True, exist_ok=True)
    shutil.copy2(root / "marketplace/batteries/codex/appa.toml", battery / "appa.toml")
    policy = home / "appa.toml"
    default = (root / "marketplace/plugins/codex/default.appa.toml").read_text()
    policy.write_text('include = ["batteries/codex/appa.toml"]\n' + default)
    return policy


def run(cwd: Path, prompt: str | None, resume: str | None, exec_mode: bool) -> int:
    root = Path(__file__).resolve().parents[2]
    appa = root / "target/debug/appa"
    if not appa.is_file():
        raise RuntimeError("Build appa first with cargo build -p appa")
    if not cwd.is_dir():
        raise RuntimeError(f"No such directory: {cwd}")
    source = Path(os.environ.get("CODEX_HOME", str(Path.home() / ".codex"))).resolve()
    home = protected_home(source)
    if cwd == home or home in cwd.parents or cwd == source or source in cwd.parents:
        raise RuntimeError("the working directory overlaps the denied Codex home")
    profile(home, source, appa)
    invocation = home / "invocations" / secrets.token_hex(16)
    invocation.mkdir(mode=0o700, parents=True)
    proof = invocation / "proof"
    proof.write_text(secrets.token_hex(32))
    proof.chmod(0o600)
    environment = dict(os.environ, CODEX_HOME=str(home), APPA_GATE="1", APPA_CODEX_PROOF_FILE=str(proof))
    environment.pop("APPA_RUNTIME_URL", None)
    environment.pop("APPA_CODEX_EXPECTED_ROOT", None)
    if resume:
        environment["APPA_CODEX_EXPECTED_ROOT"] = "codex:" + resume
    runtime = None
    try:
        preflight(home, cwd, proof, environment)
        policy = runtime_config(root, home)
        log = (invocation / "runtime.log").open("w")
        try:
            runtime = subprocess.Popen(
                [str(appa), "runtime", "--adapter", "codex", "--config", str(policy),
                 "--db", str(home / "appa.db"), "--listen", "127.0.0.1:0"],
                cwd=cwd, env=environment, stdout=subprocess.PIPE, stderr=log, text=True,
            )
            with selectors.DefaultSelector() as selector:
                selector.register(runtime.stdout, selectors.EVENT_READ)
                if not selector.select(timeout=15):
                    raise RuntimeError("APPA runtime did not report its address")
            url = runtime.stdout.readline().strip()
            if not url.startswith("http://127.0.0.1:"):
                raise RuntimeError("APPA runtime did not start")
            profile(home, source, appa, url, proof, invocation.name)
            command = ["codex", "--profile", invocation.name]
            if exec_mode:
                command.append("exec")
                if resume:
                    command.extend(["resume", resume, "--skip-git-repo-check"])
                else:
                    command.extend(["--skip-git-repo-check", "-C", str(cwd)])
            else:
                command.append("--no-daemon")
                if resume:
                    command.extend(["resume", resume])
            command.extend(["--strict-config", "--dangerously-bypass-hook-trust"])
            if not exec_mode:
                command.extend(["-C", str(cwd)])
            if prompt:
                command.append(prompt)
            return subprocess.run(command, cwd=cwd, env=environment, check=False).returncode
        finally:
            log.close()
    finally:
        if runtime is not None:
            runtime.terminate()
            try:
                runtime.wait(timeout=3)
            except subprocess.TimeoutExpired:
                runtime.kill()
                runtime.wait()
        proof.unlink(missing_ok=True)
        (home / f"{invocation.name}.config.toml").unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cwd", type=Path, default=Path.cwd())
    parser.add_argument("--resume", metavar="SESSION_UUID")
    parser.add_argument("--exec", action="store_true", dest="exec_mode")
    parser.add_argument("prompt", nargs="?")
    args = parser.parse_args()
    try:
        return run(args.cwd.resolve(), args.prompt, args.resume, args.exec_mode)
    except (OSError, subprocess.TimeoutExpired, RuntimeError) as error:
        print(f"Protected Codex launch refused: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
