# OpenAPPA with Codex

Use OpenAPPA with Codex CLI to check tool calls against your policy.
For supported shell commands, APPA holds the output until the policy allows Codex to read it.

## Set up

Install `appa` and Codex CLI on macOS or Linux.
Sign in to Codex, then run:

```sh
appa plugin install codex
```

The installer connects Codex to APPA and adds `/appa-guide` to help you set up your policy.
Codex uses its own policy and background runtime, separate from Claude Code.

Open Codex and run `/hooks`.
Review and trust the APPA hooks.
Exit Codex, then start a session with:

```sh
appa codex --
```

Run `/appa-guide init` to check your tools and configure their policies.
See the [Codex guide](../../website/content/docs/codex.md) for the full setup and removal steps.
Use its [protection check](../../website/content/docs/codex.md#check-protection-in-a-test-project) to test your session.

The launcher uses your Codex shell setting, `features.code_mode_host`.
Codex CLI 0.159.2 enables the shell by default.
Before Codex starts, the launcher checks that commands in the Codex sandbox can reach APPA.
The installed `appa` permission profile allows access to every port on `127.0.0.1`.
It does not allow public hosts, and the filesystem sandbox stays active.

## Policy and tool coverage

The launcher reloads the installed policy before each new session.
`appa codex-reload` explicitly reloads it and prints the active policy key.
`appa codex-policy-key` reads that key without a reload.
Start a new session after a policy change. Existing sessions retain their information labels.
The installation receipt selects the policy for `appa describe --adapter codex` unless `--config` overrides it.

The battery supplies command contracts. The default root supplies patch, local image, plan, and local `webrun` contracts.
The web contract requires a `public` audience and marks results `suspicious`.
Other hooked tools use a wildcard classifier. Explicit contracts take precedence, and failed annotations cause refusal.
Subagent and peer routes remain explicitly blocked until lifecycle verification.

Apps, browser use, and `--search` remain available under Codex's configuration.
Hosted `WebSearch` can bypass hooks. The web contract covers the recorded local `webrun` route.
Supplied session tools do not establish the complete configured MCP inventory.
`MCP servers: none` does not prove that the Codex configuration contains no servers.

The [Codex guide](../../website/content/docs/codex.md#default-coverage-and-the-first-web-call) includes web, local-inspection, and refused-tool examples.

## Command context

Native `exec_command` options can disappear from hook events.
A native `workdir`, `shell`, or `login` option therefore does not reliably select the wrapped execution context.
Plain commands use the hook session directory and the hook's `$SHELL` with `-lc`.

Supply an explicit context in the command's first line:

```sh
# appa-codex-exec-v1 {"workdir":"/absolute/project/nested","shell":"/bin/bash","login":false}
pwd
```

APPA validates the directory, shell, and login mode before classification.
Selectors inspect the command payload after APPA removes the header.
The proxy restores the approved directory after shell startup.
Startup files can still alter the environment or execute commands.
Malformed or conflicting headers cause refusal. Environment overrides and terminal commands remain unsupported.
Without an active hook, the header is only a shell comment and supplies no execution context.
`login=false` uses `-c`. `login=true` uses `-lc`.
For a directory change alone, `cd /absolute/project/nested && pwd` retains the default shell and login mode.
See the [context reference](../../website/content/docs/codex.md#command-directory-and-shell) for header requirements.

APPA wrapped commands cannot receive later input because the wrapper closes child stdin and does not forward input.
Codex supports later input through `write_stdin` when an unwrapped execution session keeps stdin open.

## Limits

The APPA hook blocks a call if the policy check crashes, times out, or returns invalid JSON.
If you do not trust the APPA hooks through `/hooks`, Codex can skip APPA checks and run commands without APPA protection.
If the hook command fails before it replies, Codex can also run the original command without an APPA check.

If an update changes the hooks, trust the updated hooks through `/hooks` before you start a session.
Do not describe a session as protected until a manual end-to-end check passes with those trusted hooks.

The command wrapper supports Unix commands that finish without more input.
It uses `sh`, `bash`, or `zsh`, closes command input, and holds up to 100 MiB of combined output.
Interactive terminal programs, native Windows commands, and unsupported shell modes are outside this scope.

Do not use this integration to contain commands that detach and continue in the background.
Such a command can survive cancellation. APPA cannot reverse its changes.
The policy must deny these commands until APPA can contain them.

A runtime restart or a new prompt prevents APPA from releasing pending command output.
Installed Stop and SessionEnd hooks cancel pending commands and prevent APPA from releasing their output.
The installer does not register an Interrupt hook. Cancellation lacks equivalent live verification for every host path.

## Compatibility tests

To check the command wrapper without a model call, run:

```sh
cargo build -p appa
python3 integrations/codex/proxy_probe.py
```

This test uses a temporary project and Codex profile.
It checks the command wrapper and output release.
It also checks that APPA rejects unauthorized requests.
