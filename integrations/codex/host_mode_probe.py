#!/usr/bin/env python3
"""Probe local Codex tool routes with synthetic data in temporary projects."""

import argparse
import binascii
import hashlib
import json
import pathlib
import platform
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib


HOOK = '''import json, pathlib, sys
event = json.load(sys.stdin)
path = pathlib.Path(__file__).with_name("events.jsonl")
record = {"event": event.get("hook_event_name"), "tool": event.get("tool_name"),
          "input_keys": sorted(event.get("tool_input", {})),
          "response_type": type(event.get("tool_response")).__name__}
with path.open("a", encoding="utf-8") as output:
    output.write(json.dumps(record) + "\\n")
print("{}")
'''

PROMPTS = {
    "inventory": 'Call functions.exec once with `text(ALL_TOOLS.map(x => x.name).join(","))`. Report its exact output.',
    "shell": "Call the shell once to execute `printf APPA_ROUTE_SHELL`. Report the output. Use no other tool.",
    "patch": "Use apply_patch once to create marker.txt with APPA_PATCH_MARKER. Do not use the shell.",
    "image": "Use view_image once to inspect marker.png. Do not use the shell.",
    "web": "Use the web tool once to open https://example.com. Report its title. Do not use the shell.",
    "imagegen": "Use the image generation tool once to make a red square with no text. Do not use the shell.",
    "late_input": (
        'Call functions.exec once with this JavaScript: '
        'let r=await tools.exec_command({cmd:"sh",tty:true,yield_time_ms:500}); '
        'text(r); if(r.session_id){let x=await tools.write_stdin('
        '{session_id:r.session_id,chars:"printf APPA_LATE_INPUT\\nexit\\n",yield_time_ms:1000});text(x);} '
        'Use no other tool.'
    ),
    "host_off": "Call the shell once to execute `printf APPA_ROUTE_SHELL`. Report the result.",
}

EXPECTED = {
    "shell": "Bash",
    "patch": "apply_patch",
    "image": "view_image",
    "web": "webrun",
    "imagegen": "image_genimagegen",
    "late_input": "Bash",
}


def make_image(path):
    def chunk(name, payload):
        checksum = binascii.crc32(name + payload) & 0xFFFFFFFF
        return struct.pack("!I", len(payload)) + name + payload + struct.pack("!I", checksum)

    pixels = b"\x00\xff\x00\x00\xff\x00\x00\x00\xff\x00\xff\xff\xff\x00"
    header = struct.pack("!2I5B", 2, 2, 8, 2, 0, 0, 0)
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(pixels)) + chunk(b"IEND", b""))


def run_route(codex, route, timeout):
    with tempfile.TemporaryDirectory(prefix=f"appa-host-{route}-") as directory:
        project = pathlib.Path(directory)
        hook_path = project / "hook.py"
        hook_path.write_text(HOOK, encoding="utf-8")
        make_image(project / "marker.png")
        handler = json.dumps(f"{sys.executable} {hook_path}")
        group = '[{hooks=[{type="command",command=' + handler + '}]}]'
        command = [
            codex, "exec", "--ephemeral", "--ignore-user-config", "--ignore-rules",
            "--skip-git-repo-check", "--dangerously-bypass-hook-trust", "--json",
            "-C", directory, "--sandbox", "workspace-write",
            "-c", "features.hooks=true", "-c", "features.code_mode=true",
            "-c", f"features.code_mode_host={'false' if route == 'host_off' else 'true'}",
            "-c", "features.apps=false", "-c", "features.browser_use=false",
            "-c", "features.multi_agent=false",
            "-c", f"hooks.PreToolUse={group}", "-c", f"hooks.PostToolUse={group}",
            PROMPTS[route],
        ]
        try:
            completed = subprocess.run(
                command, cwd=project, capture_output=True, text=True,
                timeout=timeout, check=False,
            )
        except subprocess.TimeoutExpired:
            return {"route": route, "status": "timeout"}
        event_path = project / "events.jsonl"
        events = [json.loads(line) for line in event_path.read_text().splitlines()] if event_path.exists() else []
        items = []
        for line in completed.stdout.splitlines():
            try:
                item = json.loads(line)
            except json.JSONDecodeError:
                continue
            if item.get("type") == "item.completed":
                items.append(item.get("item", {}))
        messages = [item.get("text", "") for item in items if item.get("type") == "agent_message"]
        tools = [item.get("type") for item in items if item.get("type") not in ("agent_message", "error")]
        expected = EXPECTED.get(route)
        pre = [event for event in events if event["event"] == "PreToolUse"]
        post = [event for event in events if event["event"] == "PostToolUse"]
        if route == "inventory":
            passed = completed.returncode == 0 and bool(messages)
        elif route == "host_off":
            passed = completed.returncode == 0 and not events and "code-mode host is disabled" in completed.stderr
        else:
            passed = (completed.returncode == 0 and len(pre) == len(post) == 1
                      and pre[0]["tool"] == post[0]["tool"] == expected)
            if route == "patch":
                passed = passed and (project / "marker.txt").read_text().strip() == "APPA_PATCH_MARKER"
            if route == "late_input":
                passed = passed and any("APPA_LATE_INPUT" in message for message in messages)
        return {
            "route": route, "status": "pass" if passed else "fail",
            "codex_exit": completed.returncode,
            "events": events,
            "item_types": tools,
            "inventory": messages[-1] if route == "inventory" and messages else None,
            "stderr_tail": completed.stderr[-300:] if not passed else None,
        }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--codex", default="codex")
    parser.add_argument("--route", choices=[*PROMPTS, "all"], default="all")
    parser.add_argument("--timeout", type=int, default=180)
    args = parser.parse_args()
    binary = pathlib.Path(shutil.which(args.codex) or args.codex).resolve()
    version = subprocess.run([str(binary), "--version"], capture_output=True, text=True, check=False)
    routes = list(PROMPTS) if args.route == "all" else [args.route]
    results = [run_route(str(binary), route, args.timeout) for route in routes]
    print(json.dumps({
        "codex": version.stdout.strip(),
        "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "os": platform.platform(),
        "routes": results,
    }, indent=2))
    return 0 if all(result["status"] == "pass" for result in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
