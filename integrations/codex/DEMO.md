# Set up OpenAPPA with Codex

This flow covers installation, hook trust, launch, guide use, and removal.
The protected-launch result comes from Codex CLI 0.159.0 on macOS 26.6.2 arm64.
An isolated host-mode probe reproduced the shell error on CLI 0.159.2.
The installation and sandbox checks used disposable profiles.
An authenticated session with manually trusted hooks remains unverified.

## Install

1. Install `appa` and Codex CLI on a Unix host.
2. Sign in to Codex with your usual account.
3. Run `codex --version` and record the CLI release.
4. Run `appa plugin install codex`.

The installer adds APPA hooks, an HTTP MCP server, an `appa` permission profile, and `/appa-guide` to the active Codex profile.
It creates a separate Codex policy and runtime. It preserves unrelated profile entries.
The installed policy resides in the platform APPA config directory under `codex/appa.toml`.

## Trust the hooks

1. Open Codex and run `/hooks`.
2. Review and trust the installed APPA hooks.
3. Exit that Codex session.

If an update changes the hooks, repeat this review.
The APPA launcher cannot inspect the Codex trust state through a supported noninteractive interface.

## Launch and inspect

1. Open a disposable project directory.
2. Run `appa codex -- -C .`.
3. Read the launch result and record any sandbox handshake error.
4. If Codex opens, run `/appa-guide init`.
5. Ask the guide to report the installed policy path, available tools, battery suggestions, and coverage limits.

The launcher checks HTTP access from `codex sandbox` to the APPA runtime before it opens Codex.
The installed permission profile permits access to `127.0.0.1` through Codex's proxy.
That exception covers every port on the host. It does not permit public destinations.

On the tested CLI 0.159.0 and 0.159.2 releases, `features.code_mode_host=false` also disables the shell tool.
The launcher sets this value until the enabled host mode passes hook coverage checks.
If Codex reports `code-mode host is disabled`, stop the command demonstration at this step.
The guide cannot complete `appa describe --adapter codex` through a disabled shell tool.
Report the guide inspection as unavailable. Do not describe this session as fully protected.

The command wrapper also does not forward later stdin or support terminal programs.
Native Windows command containment and strict or managed permission profiles remain unverified.

## Remove

1. Exit the Codex session.
2. Run `appa plugin remove codex`.

Removal unregisters installer-owned hooks, the MCP server, the permission profile, and the guide.
It retains the Codex policy and data. Run `appa runtime --adapter codex stop` to stop the separate runtime.

## Scope of the demonstration

Disposable probes verified installation, the sandbox HTTP path, and a synthetic model-to-wrapper call on CLI 0.159.0.
On CLI 0.159.2, a read-only call in a disposable project returned `code-mode host is disabled` before it ran `pwd`.
The synthetic call enabled the code-mode host and bypassed hook trust only in its temporary fixture.
It does not prove that the protected launcher supports a complete, manually trusted session.
