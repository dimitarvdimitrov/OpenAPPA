# Codex proxy compatibility

The [host compatibility gate](HOST_MODE_GATE.md) fails on Codex CLI 0.159.2.
`appa codex --` refuses protected session starts.
`code_mode_host=false` removes shell access.
If the feature is enabled, Codex can run an original command after a failed or untrusted pre-use hook.

## Probe

Build `appa` with `cargo build -p appa`.
Run `python3 integrations/codex/proxy_probe.py`.

The probe uses a temporary project and a temporary Codex profile. It makes no model call.
It checks the rewritten wrapper, the admitted result, early output, and a forged job handle.

## Launcher

`appa plugin install codex` adds an `appa` permission profile to the active Codex config.
`appa codex --` selects the profile and checks runtime HTTP access from `codex sandbox` before the compatibility refusal.
The profile permits all sandboxed commands to reach all ports on `127.0.0.1`.
Codex requires a manual `/hooks` review because the launcher cannot verify hook trust through a supported noninteractive interface.

## Scope

The proxy supports finite Unix commands without later input. It starts the selected `sh`, `bash`, or `zsh` shell without a controlling terminal.
It closes child stdin and buffers at most 1 MiB of combined output. It checks job status while the child runs.

Native Windows commands, later input, PTY programs, prompts, full-screen tools, daemonized children, and unsupported shell modes remain outside this proxy scope.
The protected launcher requires a Codex sandbox profile with the disclosed `127.0.0.1` HTTP exception.
The hook cannot guarantee coverage if Codex skips or disables it.

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

## Later input on Codex CLI 0.159.2

Tests ran on macOS 26.6.2 arm64 on 30 September 2026.
A direct `codex sandbox` command received input through a FIFO without echo.
A disposable model-controlled Codex session then ran the rewritten APPA wrapper.
Its non-TTY command received EOF before later input arrived.
Codex returned `write_stdin failed: stdin is closed for this session; rerun exec_command with tty=true to keep stdin open`.
The terminal retry echoed the synthetic `HELLO` input.

This release did not provide a non-echoing pipe for later input in the tested command session.
The wrapper keeps child stdin closed. Later input is unsupported.

## Lifetime and output

A runtime restart invalidates live job handles. Persistent ownership prevents replay.
The runtime withholds a post-hook after restart because it cannot verify the old turn.
Stop, Interrupt, and a new prompt close live jobs and block late post-hook acknowledgement.

On cancellation, the wrapper kills the shell process group and ordinary descendants.
A deliberately daemonized child can escape that group and continue after the wrapper settles indeterminately.
Codex CLI 0.159.0 also left such a child alive after its parent exited.
Do not use this proxy to contain daemonized commands. The policy must deny them until process-tree containment exists.
The wrapper withholds output from an escaped child that retains its pipes. It cannot reverse that child's side effects.
