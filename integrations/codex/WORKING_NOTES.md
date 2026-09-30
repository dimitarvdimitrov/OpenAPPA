# Codex integration working notes

This document records observed behavior and known limits before release documentation. Each entry names its test scope and evidence.

## Hook failure modes: Codex and Claude Code

The Codex results come from a live Codex CLI 0.159.0 probe on macOS. The Claude Code results come from the hook reference and local APPA code. No live Claude Code failure probe exists yet.

| Case | Claude Code with CLAPPA | Codex with APPA |
|---|---|---|
| Valid pre-hook denial | Claude Code blocks the pending tool call. | The live probe stopped a Bash file write and an `apply_patch` file creation. |
| APPA runtime does not answer or sends an invalid reply | If `appa hook` returns code 2 before its host timeout, Claude Code blocks the pending tool call. | The APPA client returns code 2 for a pre-hook failure. Live Codex behavior for this exact case remains unverified. |
| Hook process crashes, times out, or prints malformed JSON | A timeout does not block a tool call. A nonblocking exit code or invalid hook output leaves the call to normal permission checks. | The live probe saw the original Bash command run in all three cases. The command wrote a synthetic file. |
| Host skips the hook | Interactive workspace trust or `disableAllHooks` can prevent execution. APPA then supplies no decision. | The live probe saw Codex skip an untrusted hook. The original Bash command wrote a synthetic file. |

CLAPPA gives `appa hook` 120 seconds for its internal decision. It sets the Claude Code hook timeout to 130 seconds. This gap lets APPA return code 2 before Claude Code cancels the hook. It does not guarantee a block if the hook process crashes or Claude Code skips it.

Normal permission checks can still deny a call after a hook failure. A skipped hook does not guarantee that the call executes.

The Codex crash probe used a synthetic hook. It did not test a real APPA runtime outage in Codex. That result does not establish APPA behavior during a Codex runtime outage.

### Evidence

- [Codex live failure probe](hook_failure_probe.py) and [Phase 7 report](PHASE7.md) record the Codex cases.
- [APPA hook client](../../appa-runtime/src/hook_client.rs) returns code 2 for a pending call if its runtime does not answer.
- [CLAPPA hook settings](../../appa-runtime/src/init/settings.rs) set the client and host timeouts.
- [Claude Code hook reference](https://code.claude.com/docs/en/hooks) defines exit-code, timeout, and workspace-trust behavior.
