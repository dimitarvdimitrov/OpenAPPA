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
See the [setup and demonstration](DEMO.md) for the full flow and removal steps.

The launcher uses your Codex shell setting, `features.code_mode_host`.
Codex CLI 0.159.2 enables the shell by default.
Before Codex starts, the launcher checks that commands in the Codex sandbox can reach APPA.
The installed `appa` permission profile allows access to every port on `127.0.0.1`.
It does not allow public hosts, and the filesystem sandbox stays active.

## Limits

The APPA hook blocks a call if the policy check crashes, times out, or returns invalid JSON.
If Codex skips an untrusted hook, or the hook command itself fails before its reply, Codex can run the original command.
If an update changes the hooks, you must trust them again through `/hooks`.
Do not describe a session as protected until a manual end-to-end check passes with those trusted hooks.

The command wrapper supports Unix commands that finish without more input.
It uses `sh`, `bash`, or `zsh`, closes command input, and holds up to 1 MiB of combined output.
Interactive terminal programs, native Windows commands, and unsupported shell modes are outside this scope.

Do not use this integration to contain commands that detach and continue in the background.
Such a command can survive cancellation. APPA cannot reverse its changes.
The policy must deny these commands until APPA can contain them.

A runtime restart or a new prompt ends access to previous jobs and their output.
Stop and Interrupt also end those jobs. Output from a previous job is not released after a runtime restart.

## Compatibility tests

The [test results](HOST_MODE_GATE.md) record Codex CLI 0.159.2 behavior on macOS 26.6.2 arm64.
Those tests used temporary projects and hooks. They did not check a session with hooks manually trusted through `/hooks`.

The command-input tests found no supported path for more input after a command starts.
The APPA wrapper keeps that input closed.

## Execution context

Codex supplies the session directory and the command to Bash hooks.
The documented hook input does not include the native `workdir`, `shell`, or `login` options.
APPA cannot recover these omitted options from the hook.
Plain commands use the hook's session directory and the hook process's `$SHELL` with `-lc`.

Native execution options alone therefore do not select the APPA child's execution context.
The [official hook reference](https://learn.chatgpt.com/docs/hooks) describes these input fields.

The first command line can supply an explicit APPA execution header:

```sh
# appa-codex-exec-v1 {"workdir":"/absolute/project/nested","shell":"/bin/bash","login":false}
pwd
cat selected.txt
```

This header is the authoritative execution context for the APPA child.
All three fields are required. The directory must exist and use an absolute path.
The shell must use an absolute path to an existing `sh`, `bash`, or `zsh` file.

`login=false` selects `-c`. `login=true` selects `-lc` and permits normal shell startup files.
APPA restores the declared directory after shell startup, before the command.
Startup files can still alter the environment or execute their own commands.

APPA removes the header before policy classification and supplies the declared directory to context providers.
Command selectors and annotators receive the actual command, shell, login mode, and directory.
Malformed headers, duplicate fields, repeated headers, unknown fields, and conflicting supplied hook options cause refusal.
Environment overrides and terminal commands remain unsupported.
Without an active hook, the header is only a shell comment and supplies no execution context.

For a plain command, an explicit `cd /absolute/project/nested` can select a directory within the command itself.
The header also supplies the correct initial directory to policy context providers.


To check the command wrapper without a model call, run:

```sh
cargo build -p appa
python3 integrations/codex/proxy_probe.py
```

This test uses a temporary project and Codex profile.
It checks the command wrapper, output release, and rejection of a forged job handle.
