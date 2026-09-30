#!/usr/bin/env python3
"""Check the Codex sandbox HTTP handshake with disposable permission profiles."""

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import tempfile
from threading import Thread


class Health(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200 if self.path == "/health" else 404)
        self.end_headers()
        self.wfile.write(b"ok" if self.path == "/health" else b"not found")

    def log_message(self, *_args):
        pass


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--codex", default="codex")
    parser.add_argument("--appa", type=Path, default=Path(__file__).resolve().parents[2] / "target/debug/appa")
    args = parser.parse_args()
    if not args.appa.is_file():
        parser.error(f"build appa first: {args.appa}")
    server = ThreadingHTTPServer(("127.0.0.1", 0), Health)
    worker = Thread(target=server.serve_forever, daemon=True)
    worker.start()
    try:
        with tempfile.TemporaryDirectory(prefix="appa-strict-profile-") as directory:
            project = Path(directory)
            home = project / "codex-home"
            home.mkdir()
            environment = dict(os.environ, CODEX_HOME=str(home))
            url = f"http://127.0.0.1:{server.server_port}"
            cases = [
                ("allowed", True, "allow"),
                ("network_off", False, "allow"),
                ("domain_denied", True, "deny"),
            ]
            results = []
            for name, network, domain in cases:
                home.joinpath("config.toml").write_text(
                    '[features]\nnetwork_proxy=true\n'
                    '[permissions.appa]\nextends=":workspace"\n'
                    f'[permissions.appa.network]\nenabled={str(network).lower()}\n'
                    f'[permissions.appa.network.domains]\n"127.0.0.1"="{domain}"\n',
                    encoding="utf-8",
                )
                command = [
                    args.codex, "sandbox", "-P", "appa", "--include-managed-config",
                    "--enable", "network_proxy", "--", str(args.appa.resolve()),
                    "codex-probe", "--url", url,
                ]
                result = subprocess.run(
                    command, cwd=project, env=environment, capture_output=True,
                    text=True, timeout=20, check=False,
                )
                passed = (result.returncode == 0 and result.stdout.strip() == "ok") if name == "allowed" else result.returncode != 0
                results.append({
                    "profile": name,
                    "status": "pass" if passed else "fail",
                    "exit": result.returncode,
                    "diagnostic": result.stderr.strip()[-250:],
                })
        print(json.dumps(results, indent=2))
        return 0 if all(result["status"] == "pass" for result in results) else 1
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    raise SystemExit(main())
