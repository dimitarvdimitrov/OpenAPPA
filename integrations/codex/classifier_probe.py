#!/usr/bin/env python3
"""Opt-in saved-login Codex Annotator check with synthetic calls only."""

import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--appa", type=Path, default=Path(__file__).resolve().parents[2] / "target/debug/appa")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    if not args.appa.is_file():
        parser.error(f"build appa first: {args.appa}")
    with tempfile.TemporaryDirectory(prefix="appa-codex-classifier-") as directory:
        root = Path(directory)
        battery = root / "batteries/codex/appa.toml"
        battery.parent.mkdir(parents=True)
        shutil.copy2(repo / "marketplace/batteries/codex/appa.toml", battery)
        policy = root / "appa.toml"
        policy.write_text(
            'include = ["batteries/codex/appa.toml"]\n'
            + (repo / "marketplace/plugins/codex/default.appa.toml").read_text()
        )
        calls = [
            {"id": "finite-command", "tool": "host/codex/appa_exec",
             "arguments": {"command": "printf APPA_CLASSIFIER_MARKER"}, "cwd": str(root)},
            {"id": "patch", "tool": "host/codex/apply_patch",
             "arguments": {"command": "*** Begin Patch\n*** Add File: demo.txt\n+APPA_SYNTHETIC\n*** End Patch"},
             "cwd": str(root)},
        ]
        try:
            completed = subprocess.run(
                [str(args.appa.resolve()), "runtime", "--adapter", "codex", "--config", str(policy),
                 "annotate", "--concurrency", "1"],
                input="".join(json.dumps(call) + "\n" for call in calls),
                cwd=root, capture_output=True, text=True, timeout=180,
            )
        except subprocess.TimeoutExpired:
            print(json.dumps({"status": "timeout"}))
            return 1
        answers = []
        for line in completed.stdout.splitlines():
            try:
                answer = json.loads(line)
            except json.JSONDecodeError:
                continue
            answers.append({"id": answer.get("id"), "outcome": answer.get("outcome"),
                            "annotator": answer.get("annotator"), "latency_ms": answer.get("latency_ms")})
        valid = (
            completed.returncode == 0 and len(answers) == 2
            and {answer["id"] for answer in answers} == {"finite-command", "patch"}
            and all(answer["outcome"] == "answer" for answer in answers)
        )
        print(json.dumps({"status": "pass" if valid else "fail", "answers": answers}, indent=2))
        return 0 if valid else 1


if __name__ == "__main__":
    raise SystemExit(main())
