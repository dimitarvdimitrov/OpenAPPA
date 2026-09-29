# Codex proxy compatibility probe

See [the setup demo](DEMO.md) for installation and [Phase 7 findings](PHASE7.md)
for the tested scope and remaining work. The protected launcher currently
disables `code_mode_host`; on Codex CLI 0.159.0 this also disables the shell
tool, so the full command workflow is not yet supported through `appa codex`.

`proxy_probe.py` exercises a runtime-owned Bash job through the installed Codex CLI's command sandbox. Build `appa` first with `cargo build -p appa`, then run `python3 integrations/codex/proxy_probe.py`. The probe creates a temporary project and Codex profile, makes no model call, and reports whether a rewritten wrapper reaches the runtime and releases only the admitted result after completion.

`live_e2e_probe.py` makes one authenticated, synthetic model call through
Codex's hooks and the sandboxed wrapper. It enables `code_mode_host` and
bypasses hook trust only for its disposable fixture; a passing result is not
evidence that the protected launcher has passed its host-mode gate.

The [validation report](PHASE7.md) lists the other disposable probes for the
default classifier, hook failure behavior, sandbox guards, and hook lifecycle.

The current command proxy is a **Unix, finite, noninteractive** implementation. It launches the selected `sh`, `bash`, or `zsh` login shell without a controlling terminal, buffers at most 1 MiB of combined output, and checks that the job remains active while it runs. Native Windows command execution, forwarded stdin, terminal jobs, and shell modes that cannot be reproduced are unsupported and refused. The proxy must be used with a protected launcher and a Codex sandbox profile that permits the disclosed `127.0.0.1` HTTP exception; the hook alone cannot guarantee coverage if Codex skips or disables it.

`appa plugin install codex` adds an `appa` permission profile to the active Codex config. `appa codex --` selects it, enables Codex's filtered network proxy, and runs an HTTP handshake from `codex sandbox` before launching. The profile's `127.0.0.1` permission applies to all sandboxed commands and all ports on that host. Codex's hook trust still needs a manual `/hooks` review; the launcher cannot verify it through a supported noninteractive interface.

A runtime restart invalidates live job handles. Settled ownership is retained to prevent replay, but a post-hook arriving after restart is withheld because the old turn's liveness cannot be proved. Stop, Interrupt, and a new prompt also close live jobs and prevent late post-hook acknowledgement.

On cancellation, the wrapper kills the shell's process group and its ordinary descendants. A command that deliberately daemonizes by creating another session can escape that group and continue after the wrapper settles indeterminately. The installed Codex 0.159 sandbox also leaves such a descendant running when its parent exits. Do not treat this proxy as containment for daemonizing commands; policy should deny them until a process-tree containment mechanism is available. Output from an escaped descendant that retains the wrapper's pipes is withheld, but its side effects cannot be rolled back.
