#!/usr/bin/env python3
"""Check the installed Codex sandbox profile with disposable files and HTTP servers."""

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import tempfile
from threading import Thread


class Handler(BaseHTTPRequestHandler):
    requests = 0

    def do_GET(self):
        type(self).requests += 1
        self.send_response(200)
        self.end_headers()
        self.wfile.write(b"APPA_SANDBOX_MARKER")

    def log_message(self, *args):
        pass


def serve(host):
    server = ThreadingHTTPServer((host, 0), Handler)
    worker = Thread(target=server.serve_forever, daemon=True)
    worker.start()
    return server


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--codex", default="codex")
    args = parser.parse_args()
    # macOS grants its system temporary directory independently of the
    # workspace. Keep the sibling target under HOME to test the workspace rule.
    with tempfile.TemporaryDirectory(prefix="appa-sandbox-guards-", dir=Path.home()) as directory:
        root = Path(directory)
        project = root / "project"
        project.mkdir()
        home = root / "codex-home"
        home.mkdir()
        (home / "config.toml").write_text(
            '[features]\nnetwork_proxy = true\n'
            '[permissions.probe]\nextends = ":workspace"\n'
            '[permissions.probe.network]\nenabled = true\n'
            '[permissions.probe.network.domains]\n"127.0.0.1" = "allow"\n'
        )
        env = dict(os.environ, CODEX_HOME=str(home))

        def sandbox(command):
            return subprocess.run(
                [args.codex, "sandbox", "-P", "probe", "-C", str(project),
                 "/bin/sh", "-c", command],
                cwd=project, env=env, capture_output=True, text=True, timeout=20,
            )

        inside = project / "inside.txt"
        outside = root / "outside.txt"
        inside_result = sandbox("printf APPA_INSIDE > inside.txt")
        outside_result = sandbox(f"printf APPA_OUTSIDE > '{outside}'")
        allowed = serve("127.0.0.1")
        try:
            allowed_url = f"http://127.0.0.1:{allowed.server_port}/"
            allowed_result = sandbox(f"curl --noproxy '' --fail --silent --show-error --max-time 4 '{allowed_url}'")
            Handler.requests = 0
            unlisted_url = f"http://localhost:{allowed.server_port}/"
            unlisted_result = sandbox(f"curl --noproxy '' --fail --silent --show-error --max-time 4 '{unlisted_url}'")
            unlisted_requests = Handler.requests
        finally:
            allowed.shutdown()
            allowed.server_close()
        public_result = sandbox(
            "curl --noproxy '' --fail --silent --show-error --max-time 5 "
            "--output /dev/null --write-out '%{http_code}' https://example.com/"
        )
        report = {
            "inside_write": inside_result.returncode == 0 and inside.read_text() == "APPA_INSIDE" if inside.exists() else False,
            "outside_write_denied": outside_result.returncode != 0 and not outside.exists(),
            "allowed_loopback_http": allowed_result.returncode == 0 and allowed_result.stdout == "APPA_SANDBOX_MARKER",
            "unlisted_hostname_denied": unlisted_result.returncode != 0 and unlisted_requests == 0,
            "public_host_rejected": public_result.returncode != 0 and "403" in public_result.stderr,
            "public_http_status": public_result.stdout.strip(),
            "public_error_tail": public_result.stderr.strip()[-200:],
        }
        report["status"] = (
            "pass"
            if all(report[key] for key in (
                "inside_write", "outside_write_denied", "allowed_loopback_http",
                "unlisted_hostname_denied", "public_host_rejected",
            )) else "fail"
        )
        print(json.dumps(report, indent=2))
        return 0 if report["status"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
