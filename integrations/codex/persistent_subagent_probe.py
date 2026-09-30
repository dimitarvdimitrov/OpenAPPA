#!/usr/bin/env python3
"""Verify protected persistent Codex spawn, checked return, and resume."""

import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import sys
import tempfile


def facts(database: Path) -> list[tuple[int, object]]:
    with sqlite3.connect(database) as connection:
        rows = connection.execute("SELECT seq, facts FROM logs ORDER BY seq").fetchall()
    decoded = []
    for seq, raw in rows:
        value = json.loads(raw)
        for fact in value if isinstance(value, list) else value.get("facts", []):
            decoded.append((seq, fact))
    return decoded


def profile_read(home: Path, work: Path, target: Path) -> bool:
    result = subprocess.run(
        ["codex", "sandbox", "-P", "appa_protected", "-C", str(work),
         "/bin/sh", "-c", 'cat "$1"', "appa-probe", str(target)],
        cwd=work, env=dict(os.environ, CODEX_HOME=str(home)), capture_output=True, text=True,
        timeout=20, check=False,
    )
    return result.returncode != 0 and "Operation not permitted" in result.stderr


def process_list_denied(home: Path, work: Path) -> bool:
    for command in ("ps -axo pid,command >/dev/null", "sysctl kern.proc.all >/dev/null"):
        result = subprocess.run(
            ["codex", "sandbox", "-P", "appa_protected", "-C", str(work),
             "/bin/sh", "-c", command],
            cwd=work, env=dict(os.environ, CODEX_HOME=str(home)), capture_output=True,
            text=True, timeout=20, check=False,
        )
        if result.returncode == 0 or "Operation not permitted" not in result.stderr:
            return False
    return True


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    if not (root / "target/debug/appa").is_file():
        print(json.dumps({"status": "fail", "reason": "build appa first"}))
        return 1
    with tempfile.TemporaryDirectory(prefix="appa-codex-persistent-live-") as directory:
        base = Path(directory)
        source = base / "source"
        source.mkdir()
        work = base / "work"
        work.mkdir()
        (source / "auth.json").symlink_to(Path.home() / ".codex/auth.json")
        environment = dict(os.environ, CODEX_HOME=str(source))
        launcher = root / "integrations/codex/run_persistent.py"
        prompt = (
            "Spawn one child with task_name child and fork_turns none. "
            "Ask it to return one line APPA_CHILD_<eight random digits>. "
            "Call wait_agent, then reply with exactly the child's line. "
            "Use no shell or file tools."
        )
        first = subprocess.run(
            [sys.executable, str(launcher), "--cwd", str(work), "--exec", prompt],
            cwd=work, env=environment, capture_output=True, text=True, timeout=240, check=False,
        )
        home = source / "appa-protected"
        database = home / "appa.db"
        if first.returncode != 0 or not database.is_file():
            print(json.dumps({"status": "fail", "phase": "spawn", "exit": first.returncode,
                              "stderr_tail": first.stderr[-500:]}))
            return 1
        observed = facts(database)
        launched = [seq for seq, fact in observed if "ForkLaunched" in fact]
        opened = [seq for seq, fact in observed if "ForkOpened" in fact]
        returns = [(seq, fact["ChildReturn"]["value"]["body"])
                   for seq, fact in observed if "ChildReturn" in fact]
        wait = [(seq, fact["ValueAdmitted"]["value"]["body"])
                for seq, fact in observed if "ValueAdmitted" in fact
                and fact["ValueAdmitted"].get("provenance", {}).get("ToolResult")]
        roots = list(home.rglob("rollout-*.jsonl"))
        root_sessions = [path for path in roots if '"thread_source":"user"' in path.read_text()[:2500]]
        root_id = None
        if root_sessions:
            root_id = re.search(r"([0-9a-f]{8}-[0-9a-f-]{27})", root_sessions[0].name)
        child_value = returns[0][1] if returns else ""
        checked = (
            bool(launched and opened and returns and wait and root_id)
            and launched[0] <= opened[0] < returns[0][0] < wait[-1][0]
            and re.fullmatch(r"APPA_CHILD_\d{8}", child_value) is not None
            and first.stdout.strip() == child_value
            and json.loads(wait[-1][1]) == {"message": "Wait completed.", "timed_out": False}
        )
        link = work / "private-link"
        link.symlink_to(home, target_is_directory=True)
        profile = all(profile_read(home, work, target) for target in (
            home / "config.toml", link / "config.toml", work / "../source/appa-protected/config.toml",
            (source / "auth.json").resolve(),
        ))
        resume = subprocess.run(
            [sys.executable, str(launcher), "--cwd", str(work), "--exec", "--resume", root_id.group(1),
             "Say only APPA_RESUME_OK."],
            cwd=work, env=environment, capture_output=True, text=True, timeout=180, check=False,
        ) if root_id else None
        resumed = resume is not None and resume.returncode == 0 and resume.stdout.strip() == "APPA_RESUME_OK"
        direct = subprocess.run(
            ["codex", "exec", "resume", root_id.group(1), "--skip-git-repo-check",
             "--dangerously-bypass-hook-trust", "--strict-config", "Say APPA_DIRECT_RESUME_UNCHECKED."],
            cwd=work, env=dict(os.environ, CODEX_HOME=str(home)), capture_output=True, text=True,
            timeout=60, check=False,
        ) if root_id else None
        direct_denied = direct is not None and "APPA_DIRECT_RESUME_UNCHECKED" not in direct.stdout
        with sqlite3.connect(database) as connection:
            count = connection.execute(
                "SELECT count(*) FROM logs WHERE CAST(facts AS TEXT) LIKE '%protected_codex_root%'"
            ).fetchone()[0]
        processes = process_list_denied(home, work)
        valid = checked and profile and processes and resumed and direct_denied and count >= 2
        print(json.dumps({"status": "pass" if valid else "fail", "checked_return": checked,
                          "profile_reads_denied": profile, "resume": resumed,
                          "process_lists_denied": processes,
                          "direct_resume_denied": direct_denied,
                          "protected_records": count,
                          "first_stderr_tail": first.stderr[-500:] if not checked else "",
                          "resume_stderr_tail": resume.stderr[-500:] if resume and not resumed else "",
                          "direct_stderr_tail": direct.stderr[-500:] if direct and not direct_denied else ""}))
        return 0 if valid else 1


if __name__ == "__main__":
    raise SystemExit(main())
