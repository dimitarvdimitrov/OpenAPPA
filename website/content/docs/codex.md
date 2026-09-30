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
If you do not trust the APPA hooks through `/hooks`, Codex can skip APPA checks and run commands without APPA protection.

If an update changes the hooks, trust the updated hooks through `/hooks` before you start a session.

Start a session with:

```sh
appa codex --
```

Interactive sessions show APPA's mark, `trust:`, and `audience:` in a separate live footer.
The terminal wrapper reserves two rows and uses the alternate screen, including with `--no-alt-screen`.
The labels reflect the current trajectory after an accepted `SessionStart` hook.
If the runtime does not answer, the footer shows only the mark.
Noninteractive commands and redirected input or output use Codex directly.
[Codex issue #17827](https://github.com/openai/codex/issues/17827) requests native custom statusline support that can replace this wrapper.

The launcher uses your Codex shell setting, `features.code_mode_host`.
Codex CLI 0.159.2 enables the shell by default.
For this launch, it sets `approval_policy = "on-request"` and `approvals_reviewer = "user"`.
It also sets `execute_remedy_plan` to `approval_mode = "approve"` for the `appa` MCP server.
This setting lets remedies reach APPA without a separate Codex tool approval prompt.
APPA still asks you to approve any remedy that requires human review.
Your global Codex settings stay unchanged. Explicit conflicting approval options cause a launch error.
Before Codex starts, the launcher checks that commands in the Codex sandbox can reach APPA.
If this check fails, the launcher reports the error and stops.

## Set up your tools with `/appa-guide`

Run `/appa-guide init` inside your Codex session.

The guide finds your Codex policy and checks the tools and MCP servers in the session.
It explains which tools have rules and proposes [batteries](/batteries) or rules for the rest.
You review each proposal before the guide changes your policy.

The launcher reloads the installed policy before each new session.
For an explicit reload, run `appa codex-reload` and record its active policy key.
After the reload, start a new `appa codex --` session.
`appa codex-policy-key` reads the active key without a reload.
Existing sessions retain their information labels.

The installation receipt selects the policy path for `appa describe --adapter codex`.
An explicit `--config` overrides that selection.
Codex discovery covers the supplied session tools. It does not identify every configured MCP connection.
`MCP servers: none` does not prove that the Codex configuration contains no servers.
Use `/appa-guide explain` to understand a blocked call and the available ways to continue.

## Default coverage and the first web call

The Codex battery supplies command contracts.
The default root supplies contracts for patches, local images, plans, and the recorded local `webrun` route.
Other hooked tools use a wildcard classifier with the complete canonical call and arguments.
Explicit contracts take precedence. Classification does not automatically permit a call.
Failed, missing, malformed, or timed-out annotations cause refusal.
Subagent and peer routes remain explicitly blocked until lifecycle verification.

The local `web__run` callable maps to `host/codex/webrun`.
Its contract requires a `public` audience and labels the result `suspicious`.
The first web call can require acceptance of that trust restriction before execution.
Hosted `WebSearch` can bypass hooks, so this contract does not establish its protection.
Apps, browser use, and `--search` remain available under Codex's configuration, but availability does not prove APPA coverage.

1. Start with a public query through the local `web__run` route.
2. If APPA blocks the call, run `/appa-guide explain`.
3. Review the offered acceptance remedy and its trust restriction.
4. Use only the offer id from that blocked result with `execute_remedy_plan`.

Local inspection can also restrict a session's audience.
For example, `ls -la .` or `cat appa.toml` can require acceptance or sanitization before APPA releases the result.
An accepted audience restriction remains in that session and can prevent a later public web query.

1. If inspection stops, run `/appa-guide explain`.
2. Review the acceptance or sanitization plan that APPA returns.
3. Execute only the exact plan that you select from the current remedy offer.

For a refused-tool proposal, ask `/appa-guide adjust` about a refused MCP tool such as `mcp__example__search`.
The guide checks the inventory and proposes a maintained battery or a root contract for `mcp/example/search`.
Its proposal states the source trust, destination audience, and necessary approval.
You review the complete proposal before the guide writes it, reloads the policy, and starts a new session.
A missing classifier answer remains a refusal until classification succeeds or an approved explicit contract covers the call.

## Command directory and shell

Codex can omit native `exec_command` options from hook events.
A native `workdir`, `shell`, or `login` option therefore does not reliably select APPA's wrapped execution context.
Plain wrapped commands use the hook session directory and the hook's `$SHELL` with `-lc`.
A login shell can read startup files and change environment values.
APPA restores the approved directory after shell startup.
Startup files can still alter the environment or execute commands.

For an explicit context, put this header on the first line of the command:

```sh
# appa-codex-exec-v1 {"workdir":"/absolute/project/nested","shell":"/bin/bash","login":false}
pwd
```

The header requires an existing absolute directory, an existing absolute `sh`, `bash`, or `zsh` path, and a boolean `login` value.
APPA validates the header before classification and removes it from the command payload that selectors inspect.
The job preserves the approved directory, shell, and login mode.
Malformed or conflicting headers cause refusal. Environment overrides and terminal commands remain unsupported.
Without an active hook, the header is only a shell comment and supplies no execution context.
`login=false` uses `-c`. `login=true` uses `-lc`.

For a directory change alone, use an explicit command:

```sh
cd /absolute/project/nested && pwd
```

This alternative retains the default shell and login mode.
The wrapper closes child stdin and does not forward later input.
Codex supports later input through `write_stdin` when an unwrapped execution session keeps stdin open.

## Check protection in a test project

1. Open a test project directory.
2. Start a session with `appa codex -- -C .`.
3. Check that `/hooks` lists the APPA hooks as trusted.
4. Ask Codex to run `printf 'appa-shell-check\n'` with its shell tool.
5. Ask the guide to check that APPA checked the shell command and its output.
6. Ask the guide to propose a temporary rule that blocks a write to a test file.
7. Review the proposal before you approve it.
8. Run `appa codex-reload` after the guide applies the rule.
9. Record the active policy key that the command prints.
10. Start a new `appa codex -- -C .` session.
11. Run `appa codex-policy-key`.
12. Check that its key matches the recorded key.
13. Ask Codex to write that test file.
14. Check that Codex reports the denial and that the file does not exist.
15. Ask the guide to remove the temporary rule after your approval.
16. Reload the policy and start a new session.
17. Record the CLI release from `codex --version`, platform, Codex profile, commands, and results.

A printed marker alone does not prove that APPA checked the command.

Do not describe the session as protected until this check passes.
Use hooks that you manually trust through `/hooks`.
If the guide cannot establish that APPA checked a call, record that step as incomplete.

## Limits to know

The APPA hook blocks a call if the policy check crashes, times out, or returns invalid JSON.
If you do not trust the APPA hooks through `/hooks`, Codex can skip APPA checks and run commands without APPA protection.
If the hook command fails before it replies, Codex can also run the original command without an APPA check.

The launcher cannot check whether you trust the hooks.

The installed `appa` permission profile allows access to every port on `127.0.0.1`.
It does not allow public hosts, and the filesystem sandbox stays active.
Strict or managed profiles that block this connection cannot start through the launcher.

APPA wrapped commands cannot receive later input because the wrapper closes child stdin and does not forward input.
Interactive terminal programs are unsupported. The wrapper holds up to 100 MiB of combined output.
Native Windows commands do not have a verified protected path.
Some native tool checks happen before a hook. MCP tool errors can skip the hook that checks the result.

## Recover an edited APPA hook

An edited or absent APPA hook group prevents launch, reinstallation, or removal.
The diagnostic identifies the event, hooks path, and installation receipt path.
Unrelated hook groups remain in place.

1. Back up the hooks file and installation receipt that the diagnostic names.
2. Compare the affected event with `receipt.hooks[event]`.
3. Preserve custom values in the backup.
4. If unrelated hooks share the APPA group, move them into a separate group with their original matcher.
5. Restore only the affected APPA group from the receipt.
6. Retry the original command.
7. After reinstallation, review and trust the updated hooks through `/hooks`.

Do not delete the receipt as a recovery shortcut.
Reinstallation can duplicate APPA hooks without that ownership record.

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
