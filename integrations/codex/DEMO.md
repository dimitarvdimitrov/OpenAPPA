# Set up OpenAPPA with Codex

This flow covers installation, hook trust, launch, guide use, and removal.
It describes the intended `appa codex --` path with the Codex shell tool available.
Complete the manual trust and end-to-end checks below before you describe a session as protected.

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
4. Ask Codex to use its shell tool to run `printf 'appa-shell-check\n'`.
5. Run `/appa-guide init`.
6. Ask the guide to report the installed policy path, available tools, battery suggestions, and coverage limits.

The launcher checks HTTP access from `codex sandbox` to the APPA runtime before it opens Codex.
The installed permission profile permits access to `127.0.0.1` through Codex's proxy.
That exception covers every port on the host. It does not permit public destinations.

The intended launcher keeps Codex's code-mode host available for shell requests.
The guide uses the shell tool to run `appa describe --adapter codex` and find the installed policy.
If a command fails, record that command and its exact error. Stop the dependent steps.

## Check the protected path

1. Verify that `/hooks` shows the APPA hooks as trusted in the active Codex profile.
2. Verify that the shell marker comes from an APPA pre-hook rewrite and an admitted result.
3. Verify a policy denial with a synthetic file write in the disposable project.
4. Verify that the denied file does not exist and that Codex receives the denial.
5. Record the Codex CLI release, platform, profile, commands, and observed results.

A visible shell marker alone does not prove that APPA checked the call and result.
Do not describe the session as protected until a manually trusted end-to-end check passes.
Do not use a synthetic hook-trust bypass as evidence for that check.

The command wrapper also does not forward later stdin or support terminal programs.
Native Windows command containment and strict or managed permission profiles remain unverified.

## Remove

1. Exit the Codex session.
2. Run `appa plugin remove codex`.

Removal unregisters installer-owned hooks, the MCP server, the permission profile, and the guide.
It retains the Codex policy and data. Run `appa runtime --adapter codex stop` to stop the separate runtime.
