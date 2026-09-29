#!/usr/bin/env python3
"""Probe live Codex pre-hook denial and failure in disposable projects.

This makes authenticated Codex model calls. It writes only synthetic markers
inside temporary directories and never changes the user's Codex profile.
"""

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tempfile


MARKER = "APPA_HOOK_PROBE_MARKER"
HANDLER = '''import json, pathlib, sys, time
mode, log = sys.argv[1:]
event = json.load(sys.stdin)
with pathlib.Path(log).open("a", encoding="utf-8") as output:
    output.write(json.dumps({"event": event.get("hook_event_name"), "tool": event.get("tool_name"), "response_type": type(event.get("tool_response")).__name__}) + "\\n")
if event.get("hook_event_name") == "PreToolUse":
    if mode in ("deny", "deny_patch"):
        print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": "synthetic denial"}}))
    elif mode == "crash":
        sys.exit(1)
    elif mode == "malformed":
        print("{not valid JSON")
    elif mode == "timeout":
        time.sleep(3)
    else:
        print("{}")
else:
    print("{}")
'''


def one_case(codex: str, mode: str) -> dict:
    with tempfile.TemporaryDirectory(prefix=f"appa-hook-{mode}-") as directory:
        project = Path(directory)
        handler = project / "hook.py"
        handler.write_text(HANDLER, encoding="utf-8")
        log = project / "events.jsonl"
        marker = project / "marker.txt"
        hook = f"{sys.executable} {handler} {mode} {log}"
        group = '[{hooks=[{type="command",command=' + json.dumps(hook) + ',timeout=1}]}]'
        command = [
            codex, "exec", "--ephemeral", "--ignore-user-config", "--ignore-rules",
            "--skip-git-repo-check", "--json", "--sandbox", "workspace-write",
            "-C", str(project), "-c", "features.hooks=true",
            "-c", f"hooks.PreToolUse={group}", "-c", f"hooks.PostToolUse={group}",
        ]
        if mode != "untrusted":
            command.insert(6, "--dangerously-bypass-hook-trust")
        if mode in ("deny_patch", "observe_patch"):
            command.append(
                "Use the apply_patch tool, not the shell, exactly once to create "
                f"marker.txt containing {MARKER}. Then report whether it ran."
            )
        else:
            command.append(
                f"Use the shell tool exactly once to run `printf {MARKER} > marker.txt` "
                "in this directory. Then report whether the command ran."
            )
        try:
            completed = subprocess.run(
                command, cwd=project, capture_output=True, text=True,
                timeout=90, check=False,
            )
        except subprocess.TimeoutExpired:
            return {"case": mode, "status": "timeout"}
        hooks = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
        pre = [event for event in hooks if event["event"] == "PreToolUse"]
        post = [event for event in hooks if event["event"] == "PostToolUse"]
        items = []
        for line in completed.stdout.splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if event.get("type") == "item.completed":
                items.append(event.get("item", {}).get("type"))
        return {
            "case": mode,
            "status": "observed" if completed.returncode == 0 else "codex_error",
            "codex_exit": completed.returncode,
            "pre_hook_count": len(pre),
            "pre_tool_names": sorted({event["tool"] for event in pre}),
            "post_response_types": sorted({event["response_type"] for event in post}),
            "file_written": marker.is_file(),
            "marker_written": marker.is_file() and marker.read_text().strip() == MARKER,
            "command_item_count": items.count("command_execution"),
        }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--codex", default="codex")
    args = parser.parse_args()
    results = [one_case(args.codex, mode) for mode in (
        "deny", "deny_patch", "observe_patch", "crash", "malformed", "timeout", "untrusted",
    )]
    print(json.dumps(results, indent=2))
    denied = results[0]
    patch = results[1]
    observed_patch = results[2]
    return 0 if (
        denied["pre_hook_count"] == 1 and not denied["file_written"]
        and patch["pre_tool_names"] == ["apply_patch"] and not patch["file_written"]
        and observed_patch["pre_tool_names"] == ["apply_patch"]
        and observed_patch["post_response_types"] == ["str"]
        and observed_patch["marker_written"]
        and all(case["marker_written"] for case in results[3:])
        and results[-1]["pre_hook_count"] == 0
    ) else 1


if __name__ == "__main__":
    raise SystemExit(main())
