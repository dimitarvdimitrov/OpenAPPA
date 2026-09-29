---
title: Codex
category: Works with
order: 7
description: Install OpenAPPA hooks and a separate policy for Codex CLI sessions.
---

The Codex integration connects synchronous Codex hooks to an OpenAPPA runtime on `127.0.0.1:8766`. It uses a separate policy and data directory from Claude Code. A successful `PreToolUse` command rewrite starts an APPA execution wrapper, which buffers command output until the runtime admits it.

**Current command limitation:** On the tested Codex CLI 0.159.0 macOS build, disabling `code_mode_host` also disables the shell tool. The protected launcher disables that feature while its unhooked-operation coverage is being checked, so ordinary protected shell work is not yet demonstrated. Installation, sandbox HTTP, and a disposable synthetic model-to-wrapper probe passed; an authenticated, manually trusted protected session has not. See the [setup demo](https://github.com/archestra-ai/OpenAPPA/blob/main/integrations/codex/DEMO.md) and [Phase 7 findings](https://github.com/archestra-ai/OpenAPPA/blob/main/integrations/codex/PHASE7.md).

| Platform and Codex release | Install and sandbox HTTP | Protected shell through `appa codex` |
|---|---|---|
| macOS 26.6.2 arm64, CLI 0.159.0 | Passed in a disposable profile | Blocked by the selected host mode |
| Linux | Unverified | Unverified |
| Native Windows | Unverified | Unsupported pending Windows process and sandbox probes |

## Set up

Install `appa` and Codex CLI, sign in to Codex, then run:

```sh
appa plugin install codex
```

The installer writes hooks to the active Codex profile, registers `appa` as an HTTP MCP server, adds an `appa` permission profile, installs `/appa-guide`, and starts the separate runtime. Existing unrelated hooks, MCP servers, and permission profiles are preserved. The default policy includes the Codex battery and uses the saved Codex login for policy annotations.

In Codex, open `/hooks` and review and trust the APPA hooks. Codex skips hooks that have not been trusted, including hooks changed by an upgrade. OpenAPPA cannot inspect Codex's hook trust state noninteractively. Start protected sessions with:

```sh
appa codex --
```

Pass ordinary Codex arguments after `--`, for example `appa codex -- -C ./project`. The launcher selects the installed `appa` permission profile, enables Codex's filtered network proxy, disables known unhooked data sources, and checks runtime HTTP reachability from `codex sandbox` before starting the session. It refuses command line options that bypass these controls. Run `/appa-guide` inside the session to inspect your connected tools and adjust policy. The Codex config is under the platform APPA config directory at `codex/appa.toml`; use `--config <path>` when installing batteries for it.

To unregister APPA's Codex hooks and MCP entry while keeping policy and data, run `appa plugin remove codex`.

## Command sandbox and limits

The command wrapper needs HTTP access to the runtime from Codex's command sandbox. The installed `appa` profile extends `:workspace` and permits host `127.0.0.1` through Codex's proxy. The exception is host-wide: any sandboxed command may reach services on that host across ports. Public destinations are not allowed by this profile, and the filesystem sandbox stays on. Strict network-off, read-only, or administrator-managed profiles that forbid the exception cannot run protected shell commands; the launch handshake refuses them. The launcher does not bypass hook trust or relax a managed profile.

The pre-hook must run and accept the rewrite before the wrapper can protect a command. If a hook is disabled, untrusted, times out, or crashes, Codex may run the original command. MCP error results may bypass the post-result hook. Native Windows command containment and filtered HTTP behavior are unverified until a Windows-specific live probe passes. Terminal-only inner commands and full-screen interaction are outside the first wrapper path. Do not treat this integration as complete enforcement for those cases.

The current wrapper starts child commands with null stdin. Its `appa_stdin` policy declaration is reserved for a later implementation; later input is not forwarded.
