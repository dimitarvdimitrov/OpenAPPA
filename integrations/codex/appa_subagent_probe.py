#!/usr/bin/env python3
"""Run the enabled Codex child workflow against a disposable APPA runtime."""

import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile


MARKER = "APPA_CHILD_CHECKED_4c2a"

HOOK = r'''import json, os, pathlib, subprocess, sys
event = json.load(sys.stdin)
malformed_stop_kind = os.environ.get("APPA_MALFORMED_STOP")
malformed_stop = (malformed_stop_kind in ("identity", "message")
                  and event.get("hook_event_name") == "SubagentStop"
                  and not (pathlib.Path(__file__).parent / "malformed-stop-once").exists())
forwarded_event = dict(event)
if malformed_stop:
    (pathlib.Path(__file__).parent / "malformed-stop-once").touch()
    forwarded_event.pop("agent_id" if malformed_stop_kind == "identity" else "last_assistant_message", None)
answer = subprocess.run([sys.argv[1], "hook", "--adapter", "codex", "--deployment-url", sys.argv[2]],
                        input=json.dumps(forwarded_event), text=True, capture_output=True, timeout=30, env=os.environ)
record = {"event": event.get("hook_event_name"), "tool": event.get("tool_name"),
          "malformed_stop": malformed_stop,
          "session_id": event.get("session_id"),
          "ephemeral": event.get("transcript_path") is None,
          "agent_id": event.get("agent_id"),
          "fork_turns": (event.get("tool_input") or {}).get("fork_turns"),
          "input": event.get("tool_input"),
          "response": str(event.get("tool_response"))[:500],
          "child_marker": "APPA_CHILD_CHECKED_4c2a" in str(event.get("last_assistant_message")),
          "result_marker": "APPA_CHILD_CHECKED_4c2a" in str(event.get("tool_response")),
          "decision": answer.stdout.strip(), "exit": answer.returncode}
with (pathlib.Path(__file__).parent / "hooks.jsonl").open("a") as stream:
    stream.write(json.dumps(record) + "\n")
sys.stdout.write(answer.stdout)
sys.stderr.write(answer.stderr)
sys.exit(answer.returncode)
'''


def main() -> int:
    persistent_denial = "--persistent-denial" in sys.argv
    persistent_wait_denial = "--persistent-wait-denial" in sys.argv
    missing_stop_message = "--missing-stop-message" in sys.argv
    malformed_stop = "--malformed-stop" in sys.argv or missing_stop_message
    persistent = persistent_denial or persistent_wait_denial
    root = Path(__file__).resolve().parents[2]
    appa = root / "target/debug/appa"
    if not appa.is_file():
        print(json.dumps({"status": "fail", "reason": "build target/debug/appa first"}))
        return 1
    with tempfile.TemporaryDirectory(prefix="appa-codex-child-live-") as directory:
        project = Path(directory)
        home = project / "codex-home"
        home.mkdir()
        auth = Path.home() / ".codex" / "auth.json"
        if auth.exists():
            (home / "auth.json").symlink_to(auth)
        policy = project / "appa.toml"
        policy.write_text(
            '[policy]\nversion = 2\n'
            '[[policy.tool]]\nname = "host/codex/collaborationspawn_agent"\ndelta = {}\n'
            '[[policy.tool]]\nname = "host/codex/collaborationwait_agent"\ndelta = {}\n'
            '[policy.deployment]\ncontext_control = true\nauto_return_as_spoken = true\n'
            '[externals]\ntimeout_ms = 5000\nmax_body_bytes = 65536\n'
        )
        runtime = subprocess.Popen(
            [str(appa), "runtime", "--adapter", "codex", "--config", str(policy),
             "--db", str(project / "appa.db"), "--listen", "127.0.0.1:0"],
            cwd=project, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        try:
            url = runtime.stdout.readline().strip()
            if not url.startswith("http://127.0.0.1:"):
                raise RuntimeError("APPA runtime did not start")
            hook_script = project / "hook.py"
            hook_script.write_text(HOOK)
            hook_command = shlex.join([sys.executable, str(hook_script), str(appa), url])
            hook = '[{hooks=[{type="command",command=' + json.dumps(hook_command) + '}]}]'
            command = [
                "codex", "exec", "--ignore-user-config", "--ignore-rules",
                "--skip-git-repo-check", "--dangerously-bypass-hook-trust", "--json",
                "--sandbox", "workspace-write",
                "-C", str(project),
                "-c", "features.hooks=true", "-c", "features.multi_agent=true",
                "-c", "features.multi_agent_v2=true",
                "-c", f'mcp_servers.appa.url="{url}/mcp"',
            ]
            if not persistent:
                command.insert(2, "--ephemeral")
            for event in ["SessionStart", "PreToolUse", "PostToolUse", "SubagentStart", "SubagentStop", "Stop"]:
                command.extend(["-c", f"hooks.{event}={hook}"])
            command.append((
                "Call wait_agent with a short timeout for /root/appa-probe-missing. "
                "If the hook denies the call, do not retry. Reply DENIED."
            ) if persistent_wait_denial else (
                "Spawn one child with task_name child and fork_turns none. If the hook denies this call, do not retry. "
                "Reply DENIED. Use no shell or file tools."
            ) if persistent_denial else (
                "Spawn one child with task_name child and fork_turns none. Ask it to reply exactly " + MARKER + ". "
                + ("If its return is blocked, ask it to reply exactly " + MARKER + " again. " if malformed_stop else "")
                + "Then call wait_agent and report the child's final answer. Use no shell or file tools."
            ))
            environment = dict(os.environ, APPA_GATE="1")
            if malformed_stop:
                environment["APPA_MALFORMED_STOP"] = "message" if missing_stop_message else "identity"
            if persistent:
                environment["CODEX_HOME"] = str(home)
            completed = subprocess.run(
                command, cwd=project, env=environment, capture_output=True, text=True, timeout=240,
            )
            hooks_file = project / "hooks.jsonl"
            hooks = [json.loads(line) for line in hooks_file.read_text().splitlines()] if hooks_file.exists() else []
            items = []
            for line in completed.stdout.splitlines():
                try:
                    item = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if item.get("type") == "item.completed":
                    items.append(item.get("item", {}))
            names = [(entry["event"], entry["tool"]) for entry in hooks]
            final = [item for item in items if item.get("type") == "agent_message"]
            transcript_home = home if persistent else Path.home() / ".codex"
            session_ids = {entry["session_id"] for entry in hooks if entry.get("session_id")}
            session_ids.update(entry["agent_id"] for entry in hooks if entry.get("agent_id"))
            session_files = [path for path in transcript_home.rglob("rollout-*.jsonl")
                             if any(identity in path.name for identity in session_ids)]
            valid_workflow = (
                completed.returncode == 0
                and ("PostToolUse", "collaborationspawn_agent") in names
                and any(entry["event"] == "PreToolUse" and entry["tool"] == "collaborationspawn_agent"
                        and entry["fork_turns"] == "none" for entry in hooks)
                and ("SubagentStart", None) in names
                and ("SubagentStop", None) in names
                and ("PostToolUse", "collaborationwait_agent") in names
                and any(entry["child_marker"] for entry in hooks if entry["event"] == "SubagentStop")
                and (not malformed_stop or (
                    any(entry["malformed_stop"] and entry["exit"] == 0
                        and '"decision":"block"' in entry["decision"].replace(" ", "") for entry in hooks)
                    and any(entry["event"] == "SubagentStop" and not entry["malformed_stop"] for entry in hooks)
                ))
                and bool(final) and MARKER in json.dumps(final[-1])
                and all(entry["ephemeral"] and entry["exit"] == 0 for entry in hooks)
                and not session_files
            )
            target = "collaborationwait_agent" if persistent_wait_denial else "collaborationspawn_agent"
            valid_denial = (
                completed.returncode == 0
                and any(entry["event"] == "PreToolUse" and entry["tool"] == target
                        and entry["exit"] == 0 and '"permissionDecision":"deny"' in entry["decision"].replace(" ", "")
                        for entry in hooks)
                and ("PostToolUse", target) not in names
                and ("SubagentStart", None) not in names
                and bool(session_files)
            )
            valid = valid_denial if persistent else valid_workflow
            print(json.dumps({
                "status": "pass" if valid else "fail", "codex_exit": completed.returncode,
                "mode": "persistent_wait_denial" if persistent_wait_denial else "persistent_denial" if persistent_denial else "missing_stop_message" if missing_stop_message else "malformed_stop" if malformed_stop else "workflow",
                "hooks": [{"event": entry["event"], "tool": entry["tool"], "exit": entry["exit"],
                           "ephemeral": entry["ephemeral"], "child_marker": entry["child_marker"],
                           "malformed_stop": entry["malformed_stop"],
                           "fork_turns": entry["fork_turns"],
                           "result_marker": entry["result_marker"],
                           "decision_kind": "deny" if '"permissionDecision":"deny"' in entry["decision"].replace(" ", "")
                           else "block" if '"decision":"block"' in entry["decision"].replace(" ", "")
                           else "allow" if '"permissionDecision":"allow"' in entry["decision"].replace(" ", "")
                           else "other"} for entry in hooks],
                "items": [item.get("type") for item in items],
                "final_marker": bool(final) and MARKER in json.dumps(final[-1]),
                "session_files": len(session_files),
                "stderr_tail": completed.stderr[-500:] if not valid else "",
            }, indent=2))
            return 0 if valid else 1
        finally:
            runtime.terminate()
            try:
                runtime.wait(timeout=3)
            except subprocess.TimeoutExpired:
                runtime.kill()
                runtime.wait()


if __name__ == "__main__":
    raise SystemExit(main())
