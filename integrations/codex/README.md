# Codex proxy compatibility

## Probe

Build `appa` with `cargo build -p appa`.
Run `python3 integrations/codex/proxy_probe.py`.

The probe uses a temporary project and a temporary Codex profile. It makes no model call.
It checks the rewritten wrapper, the admitted result, early output, and a forged command handle.

## Scope

The proxy supports finite Unix commands without later input. It starts the selected `sh`, `bash`, or `zsh` shell without a controlling terminal.
It closes child stdin and buffers at most 100 MiB of combined command output. It checks command status while the child runs.

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

A runtime restart invalidates live command handles. Persistent ownership prevents replay.
The runtime withholds a post-hook after restart because it cannot verify the old turn.
Stop, Interrupt, and a new prompt close live commands and block late post-hook acknowledgement.

The wrapper kills the shell process group after normal shell exit or cancellation.
Ordinary background children can finish before the shell exits. For example, `command & wait` remains supported.
The wrapper also cleans up the group if capture or execution fails.

The proxy rejects these recognized literal launches before execution:

- `setsid` and `disown`.
- `nohup command &`, with supported redirections.
- `systemd-run` and `launchctl submit`, `bootstrap`, or `kickstart`.

The refusal says: “Detached background processes are unsupported. Run this command in the foreground.”
Comments and quoted examples do not trigger this check. Foreground `nohup` commands remain supported.
Scripts, aliases, computed names, complex shell syntax, and here-documents can bypass the text check.
The text check is a usability aid. It does not prove containment.

On Linux aarch64 and x86_64, an inherited seccomp filter prevents changes to sessions, process groups, and namespaces.
The filter applies before shell startup and survives scripts, forks, and executable changes.
It does not depend on stdout or stderr. Children with redirected or closed output pipes remain in the cleanup group.
Direct syscall attempts fail with `EPERM`. `clone3` returns `ENOSYS` so libraries can use the checked `clone` fallback.
The command cannot start if filter installation fails. Other Linux architectures refuse execution.
No new package or root privilege is required. The implementation uses the existing `libc` dependency.

Linux commands also receive `no_new_privs`. Setuid executables and file capabilities cannot grant additional privileges.
Alternate syscall ABIs, namespace tools, `ptrace`, and cross-process memory writes are unsupported in this command path.
These restrictions preserve the cleanup group. They do not replace Codex's filesystem or network sandbox.

Native macOS still lacks an enforced descendant boundary. A script can create a session and survive cleanup.
A child with inherited pipes causes bounded capture and an indeterminate result. A child with closed pipes can escape without that signal.
Either child can write files after shell exit or cancellation. Automatic text checks do not remove this limitation.
Codex CLI 0.159.0 also left such a child alive after its parent exited.
The [containment design](CONTAINMENT.md) describes the tested macOS blockers and the proposed platform change.
Users do not need custom deny rules for the automatic checks. Those checks cannot supply complete macOS containment.

The Linux boundary covers descendants while the wrapper remains alive. It does not cover commands that delegate execution to an external service.
Forced wrapper termination, host failure, external schedulers, and existing service processes remain outside this cleanup guarantee.
A runtime restart, Stop, Interrupt, or a new prompt revokes authorization. The wrapper polls authorization and then kills its group.
The polling interval is 250 milliseconds. A failed authorization request can take up to three seconds.
Cleanup cannot reverse writes that occur before teardown. A timeout on output capture does not prove process termination.

Tests check process survival and delayed file writes for inherited, redirected, and closed output pipes.
Linux tests cover hidden script launches through `setsid()` and `setpgid()`, normal shell exit, and cancellation.
Lifecycle tests cover Stop, Interrupt, runtime replacement at the same URL, and a new prompt.
Native macOS escape tests retain evidence of the limitation until a stronger boundary removes it.
