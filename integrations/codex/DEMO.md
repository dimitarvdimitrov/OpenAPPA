# Set up OpenAPPA with Codex

This flow uses Codex CLI 0.159.0 and a local `appa` build or installation. It
registers Codex hooks, a separate policy and runtime, and the `/appa-guide`
skill. The command workflow is currently blocked in the protected launcher by
the host-mode issue in [Phase 7 findings](PHASE7.md). Do not present the
session as fully protected until that gate is resolved.

## Install and inspect

1. Sign in to Codex with your usual Codex login. Install `appa` on `PATH`, or
   build this checkout with `cargo build -p appa` and use `target/debug/appa`
   in place of `appa` below.
2. Run `appa plugin install codex`. This starts the separate Codex runtime,
   writes its policy under the platform OpenAPPA config directory at
   `codex/appa.toml`, adds the `appa` permission profile and MCP server to the
   active Codex profile, and installs `/appa-guide`. It preserves unrelated
   profile entries.
3. Open Codex and use `/hooks` to review and trust the installed APPA hooks.
   Codex requires trust again if those hook definitions change. Exit that
   session after the review.
4. From the project directory, run `appa codex -- -C .`. The launcher verifies
   the Codex runtime and checks loopback HTTP from a sandboxed process before
   opening Codex. It prints a reminder that hook trust cannot be checked
   noninteractively.
5. In the Codex session, open `/appa-guide init`. The guide is installed, but
   its command-side inspection cannot complete on the tested release while
   the protected launcher disables the shell tool. It must report that limit;
   it cannot treat the session as fully protected.

The installed permission profile allows sandboxed commands to reach all
services on `127.0.0.1`, across ports, through Codex's proxy. It does not open
public destinations or disable the filesystem sandbox. The shell wrapper
buffers finite command output until APPA admits it, but it requires a working
Codex shell tool. On the tested 0.159.0 build, disabling `code_mode_host` also
disables that tool, so the current launcher cannot demonstrate an ordinary
protected shell call. Later stdin, terminal programs, and native Windows
command execution are also pending.

## Repeatable disposable checks

These checks use synthetic markers and temporary projects:

```sh
cargo build -p appa
python3 integrations/codex/probe.py
python3 integrations/codex/proxy_probe.py
python3 integrations/codex/sandbox_guards_probe.py
python3 integrations/codex/lifecycle_probe.py
python3 integrations/codex/classifier_probe.py
python3 integrations/codex/hook_failure_probe.py
python3 integrations/codex/live_e2e_probe.py
```

The classifier, hook-failure, and last checks make authenticated model calls
using the saved Codex login. The last check applies a temporary permission
profile and bypasses hook trust only for disposable probe hooks. It verifies
a Codex call, APPA rewrite, sandboxed wrapper, admitted output, and
model-visible marker. These are compatibility probes, not evidence that
`appa codex` currently offers complete coverage.

To unregister the installed hooks, MCP entry and guide while retaining the
policy and data, run `appa plugin remove codex`. If you also want to stop the
separate runtime, run `appa runtime --adapter codex stop`.
