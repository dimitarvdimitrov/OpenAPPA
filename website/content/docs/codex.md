---
title: Codex
category: Works with
order: 7
description: Set up OpenAPPA for Codex CLI and configure your tools with /appa-guide.
---

OpenAPPA runs alongside Codex CLI and checks tool calls against your policy.
For supported shell commands, it also holds the output until the policy allows Codex to read it.
Codex uses a separate policy and background runtime from Claude Code.

## Install

Install `appa` and Codex CLI on macOS or Linux.
Sign in to Codex with your usual account.
Run:

```sh
appa plugin install codex
```

The installer connects Codex to APPA through hooks and the `appa` MCP server.
It adds `/appa-guide`, an `appa` permission profile, and a starting policy with the Codex battery.
The default policy uses your saved Codex login. It does not need an API key.
Your other hooks, MCP servers, and permission profiles stay in place.

## Trust the hooks and start Codex

1. Open Codex.
2. Run `/hooks`.
3. Review and trust the APPA hooks.
4. Exit Codex.

Hooks let Codex ask APPA whether a tool call is allowed.
Codex can skip hooks that you do not trust. If an update changes the hooks, you must trust them again.

Start a session with:

```sh
appa codex --
```

The launcher uses your Codex shell setting, `features.code_mode_host`.
Codex CLI 0.159.2 enables the shell by default.
Before Codex starts, the launcher checks that commands in the Codex sandbox can reach APPA.
If this check fails, the launcher reports the error and stops.

## Set up your tools with `/appa-guide`

Run `/appa-guide init` inside your Codex session.

The guide finds your Codex policy and checks the tools and MCP servers in the session.
It explains which tools have rules and proposes [batteries](/batteries) or rules for the rest.
You review each proposal before the guide changes your policy.

Start a new `appa codex --` session to use an updated policy.
Use `/appa-guide explain` to understand a blocked call and the available ways to continue.

## Limits to know

The APPA hook blocks a call if the policy check crashes, times out, or returns invalid JSON.
If Codex skips an untrusted hook, or the hook command itself fails before its reply, Codex can run the original command.

The launcher cannot check whether you trust the hooks.
Do not describe a session as protected until an end-to-end check passes with hooks that you manually trust through `/hooks`.
The [setup and demonstration](https://github.com/archestra-ai/OpenAPPA/blob/main/integrations/codex/DEMO.md) explains the check.

The installed `appa` permission profile allows access to every port on `127.0.0.1`.
It does not allow public hosts, and the filesystem sandbox stays active.
Strict or managed profiles that block this connection cannot start through the launcher.

Commands cannot receive more input after they start. Interactive terminal programs are unsupported.
Native Windows commands do not have a verified protected path.
Some native tool checks happen before a hook. MCP tool errors can skip the hook that checks the result.

The [tests on Codex CLI 0.159.2](https://github.com/archestra-ai/OpenAPPA/blob/main/integrations/codex/HOST_MODE_GATE.md) did not check a session with hooks manually trusted through `/hooks`.

## Uninstall

Remove the hooks, MCP server, permission profile, and guide that the installer added:

```sh
appa plugin remove codex
```

Your Codex policy and data stay in place.

To stop the background runtime, run:

```sh
appa runtime --adapter codex stop
```
