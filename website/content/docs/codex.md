---
title: Codex
category: Works with
order: 7
description: Install OpenAPPA hooks and a separate policy for Codex CLI sessions.
---

The Codex integration connects synchronous hooks to an OpenAPPA runtime on `127.0.0.1:8766`.
It uses a separate policy and data directory from Claude Code.
A successful `PreToolUse` rewrite starts an APPA command wrapper.
The wrapper holds command output until the runtime admits it.

## Set up

Install `appa` and Codex CLI.
Sign in to Codex.
Run:

```sh
appa plugin install codex
```

The installer adds hooks, an HTTP MCP server, an `appa` permission profile, and `/appa-guide` to the active Codex profile.
It starts a separate runtime for the Codex policy.
It preserves unrelated hooks, MCP servers, and permission profiles.
The default policy includes the Codex battery and uses the saved Codex login for annotations.

Codex skips hooks that lack trust. A changed hook needs another review in `/hooks`.
The launcher inherits the user's `code_mode_host` setting.
It enables hooks for APPA policy checks.
Codex CLI 0.159.2 enables shell access by default.
The hook supervisor returns a Codex block if the APPA worker fails or exceeds its deadline.
Codex can still run an original command if it skips the hook or the supervisor itself fails.

See the [host-mode probe](https://github.com/dimitarvdimitrov/OpenAPPA/blob/mitko/codex-install-policy/integrations/codex/HOST_MODE_GATE.md) for probe results.

The command below checks the installed profile and sandbox HTTP path.
It then starts Codex:

```sh
appa codex --
```

The Codex policy is under the platform APPA config directory at `codex/appa.toml`.

Use `--config <path>` when you install batteries for that policy.

The launcher checks the runtime HTTP path from `codex sandbox` and refuses unsafe command line options.
It cannot verify hook trust through a supported noninteractive interface.

Run `appa plugin remove codex` to remove APPA's Codex hooks and MCP entry.

The command preserves the policy and data.

## Command sandbox and limits

The command wrapper needs HTTP access to the runtime from Codex's command sandbox.
The installed `appa` profile extends `:workspace` and permits `127.0.0.1` through Codex's proxy.
The exception covers every port on that host.
The profile does not allow public hosts, and the filesystem sandbox remains active.
The HTTP check refuses profiles that block the loopback rule.
The launcher includes managed configuration in that check.

The pre-use hook must replace the command before the wrapper can protect it.
If Codex skips the hook or the supervisor fails, Codex can run the original command.
MCP error results can bypass the post-use hook.
The wrapper does not forward later stdin.
Native Windows commands and terminal jobs lack a validated protected path.
