#!/usr/bin/env python3
"""Probe Codex subagent hooks with synthetic text in a disposable directory."""

import argparse
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile


RAW = "APPA_CHILD_RAW_7b1e"
CHECKED = "APPA_CHILD_CHECKED_4c2a"

HOOK = r'''import json, pathlib, sys
event = json.load(sys.stdin)
directory = pathlib.Path(__file__).parent
if event.get("hook_event_name") == "SubagentStop":
    for key in ("agent_transcript_path", "transcript_path"):
        path = event.get(key)
        if path:
            try:
                contents = pathlib.Path(path).read_text(encoding="utf-8", errors="replace")
                event["_" + key + "_readable"] = True
                event["_" + key + "_contains_raw"] = "APPA_CHILD_RAW_7b1e" in contents
            except OSError:
                event["_" + key + "_readable"] = False
with (directory / "events.jsonl").open("a", encoding="utf-8") as stream:
    stream.write(json.dumps(event) + "\n")
name = event.get("hook_event_name")
tool = event.get("tool_name")
if name == "PreToolUse" and tool not in (
    "spawn_agent", "wait_agent", "resume_agent", "send_input", "close_agent",
    "collaborationspawn_agent", "collaborationwait_agent", "collaborationresume_agent",
    "collaborationsend_input", "collaborationclose_agent", "collaborationinterrupt_agent",
):
    print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": "Only synthetic subagent tools are allowed"}}))
elif name == "PreToolUse" and tool == "collaborationspawn_agent" and sys.argv[1] == "rewrite":
    updated = dict(event.get("tool_input", {}))
    updated["task_name"] = "altered"
    print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow", "updatedInput": updated}}))
elif name == "PreToolUse" and tool == "collaborationspawn_agent" and sys.argv[1] == "replay-message":
    saved = directory / "first-message.txt"
    if not saved.exists():
        saved.write_text(event["tool_input"]["message"])
        print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": "Retry the same spawn once"}}))
    else:
        updated = dict(event.get("tool_input", {}))
        updated["message"] = saved.read_text()
        print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow", "updatedInput": updated}}))
elif name == "PostToolUse" and tool in (
    "wait_agent", "resume_agent", "send_input", "close_agent",
    "collaborationwait_agent", "collaborationresume_agent", "collaborationsend_input", "collaborationclose_agent", "collaborationinterrupt_agent",
) and sys.argv[1] == "block-results":
    print(json.dumps({"decision": "block", "reason": "APPA_CHILD_CHECKED_4c2a"}))
elif name == "SubagentStop" and sys.argv[1] == "block-results" and not event.get("stop_hook_active"):
    print(json.dumps({"decision": "block", "reason": "Return exactly APPA_CHILD_CHECKED_4c2a"}))
else:
    print("{}")
'''


def markers(value):
    text = json.dumps(value, ensure_ascii=False)
    return {"raw": RAW in text, "checked": CHECKED in text}


def response_shape(value):
    if not isinstance(value, str):
        return None
    return re.sub(r"[0-9a-f]{8}-[0-9a-f-]{27,}", "<agent-id>", value)[:300]


def run(codex, mode, scenario, persistent=False):
    version = subprocess.run([codex, "--version"], capture_output=True, text=True, timeout=10, check=False).stdout.strip()
    with tempfile.TemporaryDirectory(prefix="appa-codex-subagent-") as temporary:
        project = pathlib.Path(temporary)
        script = project / "hook.py"
        script.write_text(HOOK)
        environment = os.environ.copy()
        if persistent:
            codex_home = project / "codex-home"
            codex_home.mkdir()
            saved_auth = pathlib.Path.home() / ".codex" / "auth.json"
            if saved_auth.exists():
                (codex_home / "auth.json").symlink_to(saved_auth)
            environment["CODEX_HOME"] = str(codex_home)
        handler = json.dumps(f"{sys.executable} {script} {mode}")
        hook = '[{hooks=[{type="command",command=' + handler + '}]}]'
        command = [
            codex, "exec", "--ignore-user-config", "--ignore-rules",
            "--skip-git-repo-check", "--dangerously-bypass-hook-trust", "--json",
            "--sandbox", "workspace-write", "-C", str(project),
            "-c", "features.hooks=true", "-c", "features.multi_agent=true",
        ]
        if mode == "block-results":
            command.extend(["-c", "features.multi_agent_v2=true"])
        if not persistent:
            command.append("--ephemeral")
        for event in ["PreToolUse", "PostToolUse", "SubagentStart", "SubagentStop"]:
            command.extend(["-c", f"hooks.{event}={hook}"])
        if mode == "replay-message":
            prompt = (
                "Spawn one child with task_name child and ask it to reply exactly " + RAW
                + ". If a hook denies the first spawn, retry it once, then wait for the child. "
                "Do not use shell commands or files."
            )
        elif scenario == "return":
            prompt = (
                "Spawn one child with spawn_agent and fork_turns none. Ask it to reply with exactly " + RAW
                + ". Wait for its completion using wait_agent. Then report the child reply. "
                "Do not use shell commands, files, or any other tools."
            )
        elif scenario == "cancel":
            prompt = (
                "Spawn one child with spawn_agent. Ask it to compose a long numbered list and "
                "reply with exactly " + RAW + " after it finishes. Immediately call interrupt_agent "
                "on that child. Then report the interruption status. Do not call shell or file tools."
            )
        elif scenario == "peer":
            prompt = (
                "Spawn one child with spawn_agent. Ask it to call send_message to the parent "
                "with the text " + RAW + " before it returns. Wait for its completion. "
                "Do not call shell or file tools."
            )
        else:
            prompt = (
                "In this disposable session, try the tools resume_agent, send_input, and close_agent "
                "with the nonexistent target /root/appa-probe-missing. Do not create a child. "
                "Report which of these tool names the current Codex tool list exposes. "
                "Do not call shell or file tools."
            )
        command.append(prompt)
        try:
            completed = subprocess.run(command, cwd=project, env=environment, capture_output=True, text=True, timeout=180, check=False)
        except subprocess.TimeoutExpired:
            return {"codex_version": version, "mode": mode, "scenario": scenario, "hook_check": "fail", "error": "timeout"}
        events_file = project / "events.jsonl"
        hooks = [json.loads(line) for line in events_file.read_text().splitlines()] if events_file.exists() else []
        items = []
        for line in completed.stdout.splitlines():
            try:
                item = json.loads(line)
            except json.JSONDecodeError:
                continue
            if item.get("type") == "item.completed":
                items.append(item.get("item", {}))
        report = {
            "codex_version": version,
            "mode": mode,
            "scenario": scenario,
            "persistent": persistent,
            "exit_code": completed.returncode,
            "hooks": [{
                "event": event.get("hook_event_name"),
                "tool": event.get("tool_name"),
                "agent_id_present": bool(event.get("agent_id")),
                "agent_id": event.get("agent_id"),
                "keys": sorted(event),
                "tool_input_keys": sorted(event.get("tool_input", {})) if isinstance(event.get("tool_input"), dict) else None,
                "tool_response_type": type(event.get("tool_response")).__name__,
                "tool_response_markers": markers(event.get("tool_response")),
                "tool_response_shape": response_shape(event.get("tool_response")),
                "last_message_markers": markers(event.get("last_assistant_message")),
                "stop_hook_active": event.get("stop_hook_active"),
                "child_transcript_readable": event.get("_agent_transcript_path_readable"),
                "child_transcript_contains_raw": event.get("_agent_transcript_path_contains_raw"),
                "root_transcript_readable": event.get("_transcript_path_readable"),
                "root_transcript_contains_raw": event.get("_transcript_path_contains_raw"),
            } for event in hooks],
            "items": [{
                "type": item.get("type"),
                "markers": markers(item),
            } for item in items],
            "stderr_markers": markers(completed.stderr),
        }
        if persistent:
            report["session_files"] = [
                {"path_parts": path.relative_to(codex_home).parts,
                 "raw": RAW in path.read_text(encoding="utf-8", errors="replace"),
                 "checked": CHECKED in path.read_text(encoding="utf-8", errors="replace")}
                for path in codex_home.rglob("*.jsonl") if path.is_file()
            ]
        if scenario == "return" and mode == "block-results":
            names = [(event.get("hook_event_name"), event.get("tool_name")) for event in hooks]
            stop_markers = [markers(event.get("last_assistant_message")) for event in hooks if event.get("hook_event_name") == "SubagentStop"]
            messages = [item for item in items if item.get("type") == "agent_message"]
            valid = (
                completed.returncode == 0
                and ("PreToolUse", "collaborationspawn_agent") in names
                and ("PostToolUse", "collaborationspawn_agent") in names
                and ("SubagentStart", None) in names
                and ("PostToolUse", "collaborationwait_agent") in names
                and {"raw": True, "checked": False} in stop_markers
                and {"raw": False, "checked": True} in stop_markers
                and bool(messages)
                and markers(messages[-1]) == {"raw": False, "checked": True}
            )
            report["hook_check"] = "pass" if valid else "fail"
        else:
            report["hook_check"] = "exploratory"
        if scenario == "peer":
            pre_peer = any(
                event.get("hook_event_name") == "PreToolUse" and event.get("tool_name") == "collaborationsend_message"
                for event in hooks
            )
            post_peer = any(
                event.get("hook_event_name") == "PostToolUse" and event.get("tool_name") == "collaborationsend_message"
                for event in hooks
            )
            report["peer_deny_check"] = "pass" if pre_peer and not post_peer else "inconclusive"
        return report


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--codex", default="codex")
    parser.add_argument("--mode", choices=["observe", "block-results", "rewrite", "replay-message"], default="observe")
    parser.add_argument("--scenario", choices=["return", "cancel", "peer", "legacy-routes"], default="return")
    parser.add_argument("--persistent", action="store_true")
    args = parser.parse_args()
    report = run(args.codex, args.mode, args.scenario, args.persistent)
    print(json.dumps(report, indent=2))
    return 1 if report.get("hook_check") == "fail" else 0


if __name__ == "__main__":
    raise SystemExit(main())
