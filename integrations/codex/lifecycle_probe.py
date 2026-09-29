#!/usr/bin/env python3
"""Exercise Codex hook routing against a disposable real APPA runtime."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--appa", type=Path, default=Path(__file__).resolve().parents[2] / "target/debug/appa")
    args = parser.parse_args()
    if not args.appa.is_file():
        parser.error(f"build appa first: {args.appa}")
    appa = args.appa.resolve()
    with tempfile.TemporaryDirectory(prefix="appa-codex-lifecycle-") as directory:
        root = Path(directory)
        policy = root / "policy.toml"
        policy.write_text(
            '[policy]\nversion = 2\n'
            '[[policy.tool]]\nname = "mcp/demo/echo"\n'
            '[[policy.tool]]\nname = "host/codex/apply_patch"\n'
            '[externals]\ntimeout_ms = 5000\nmax_body_bytes = 65536\n'
        )
        runtime = subprocess.Popen(
            [str(appa), "runtime", "--adapter", "codex", "--config", str(policy),
             "--db", str(root / "appa.db"), "--listen", "127.0.0.1:0"],
            cwd=root, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
        )
        try:
            url = runtime.stdout.readline().strip()
            if not url.startswith("http://127.0.0.1:"):
                print(json.dumps({"status": "fail", "reason": "runtime did not start"}))
                return 1

            def hook(event):
                environment = dict(os.environ, APPA_GATE="1", SHELL="/bin/sh")
                environment.pop("APPA_RUNTIME_URL", None)
                result = subprocess.run(
                    [str(appa), "hook", "--adapter", "codex", "--deployment-url", url],
                    cwd=root, env=environment, input=json.dumps(event), capture_output=True,
                    text=True, timeout=15,
                )
                return result.returncode, json.loads(result.stdout) if result.stdout else {}

            session = "lifecycle-probe"
            start = {"hook_event_name": "SessionStart", "session_id": session}
            first_start = hook(start)
            echo = {"session_id": session, "tool_name": "mcp__demo__echo", "tool_use_id": "c1",
                    "tool_input": {"text": "APPA_MCP_MARKER"}, "cwd": str(root)}
            mcp_pre = hook(dict(echo, hook_event_name="PreToolUse"))
            mcp_post = hook(dict(echo, hook_event_name="PostToolUse", tool_response="APPA_MCP_MARKER"))
            compact = hook({"hook_event_name": "PreCompact", "session_id": session})
            post_compact = hook({"hook_event_name": "PostCompact", "session_id": session})
            resume = hook(start)
            patch = {"session_id": session, "tool_name": "apply_patch", "tool_use_id": "c2",
                     "tool_input": {"command": "*** Begin Patch\n*** End Patch"}, "cwd": str(root)}
            patch_pre = hook(dict(patch, hook_event_name="PreToolUse"))
            patch_post = hook(dict(patch, hook_event_name="PostToolUse", tool_response="Success"))
            error = dict(echo, tool_use_id="c3")
            error_pre = hook(dict(error, hook_event_name="PreToolUse"))
            error_post = hook(dict(error, hook_event_name="PostToolUse", tool_response={
                "isError": True, "content": [{"type": "text", "text": "APPA_ERROR_MARKER"}],
            }))
            unknown = hook({"hook_event_name": "PreToolUse", "session_id": session,
                            "tool_name": "unknown_tool", "tool_use_id": "c4", "tool_input": {}})
            peer = hook({"hook_event_name": "PreToolUse", "session_id": session,
                         "tool_name": "send_message", "tool_use_id": "c5", "tool_input": {"message": "x"}})
            report = {
                "initial_session": first_start == (0, {}),
                "mcp_success": mcp_pre == (0, {}) and mcp_post == (0, {}),
                "compact_reopen": compact[0] == 0 and post_compact == (0, {}) and resume == (0, {}),
                "patch_hook_pair": patch_pre == (0, {}) and patch_post == (0, {}),
                "mcp_error_withheld_when_post_arrives": error_pre == (0, {}) and error_post[1].get("decision") == "block",
                "unknown_tool_denied": unknown[1].get("hookSpecificOutput", {}).get("permissionDecision") == "deny",
                "peer_message_denied": peer[1].get("hookSpecificOutput", {}).get("permissionDecision") == "deny",
            }
            report["status"] = "pass" if all(report.values()) else "fail"
            if report["status"] == "fail":
                report["responses"] = {"mcp_pre": mcp_pre, "mcp_post": mcp_post,
                                       "patch_pre": patch_pre, "patch_post": patch_post}
            print(json.dumps(report, indent=2))
            return 0 if report["status"] == "pass" else 1
        finally:
            runtime.terminate()
            try:
                runtime.wait(timeout=3)
            except subprocess.TimeoutExpired:
                runtime.kill()
                runtime.wait()


if __name__ == "__main__":
    raise SystemExit(main())
