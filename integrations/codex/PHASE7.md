# Phase 7 verification and remaining phases

Observed on 29 September 2026 with Codex CLI 0.159.0 on macOS 26.6.2 arm64,
using the Phase 6 tip `75abc295`. All live runs used disposable projects and
synthetic markers. No native Windows host was available.

| Check | Result | Evidence |
|---|---|---|
| Adapter, policy, package, and focused runtime tests | Pass after updating one stale lifecycle assertion | `cargo test -p appa-adapter-codex -p appa-runtime-api -p appa-package -p appa-policy`; focused `appa` tests |
| Codex hook rewrite and result shape | Pass | `probe.py` observed one Bash pre/post pair and the rewritten marker in the model-visible result |
| Sandboxed HTTP wrapper and output delay | Pass | `proxy_probe.py` reached the runtime, showed no early output, and emitted only the admitted complete marker |
| Disposable marketplace install, launcher handshake, remove | Pass | `appa plugin install codex --json`, `appa codex -- --version`, `appa plugin remove codex --json`; Codex hooks, profile, guide and policy were created; runtime stopped after the run |
| Combined Codex model → APPA hook → sandboxed wrapper → model | Pass only as an opt-in compatibility probe | `live_e2e_probe.py` used a temporary static policy, loopback proxy profile, and disposable hook-trust bypass |
| Ordinary shell work through `appa codex` | **Blocked by the selected host mode** | The launcher sets `features.code_mode_host=false`; an isolated Codex exec with that same setting returned `code-mode host is disabled` for a shell request. Enabling it made the synthetic shell path work, but the coverage of code-mode operations has not passed a bypass gate. An authenticated, manually trusted `appa codex` command session has not yet been tested. |
| Native Windows, later stdin, subagent returns, MCP-error withholding | Unverified or unsupported | No Windows handoff/test, wrapper child stdin is `Stdio::null()`, shipped policy disables subagents, and the MCP-error gap is documented. |

## Phase-sized work before claiming the planned experience

1. **Host mode compatibility gate.** Probe the actual tool surface with
   `code_mode_host` enabled, including direct code-mode filesystem/network
   operations and their hook coverage. Find a supported Codex configuration
   that retains gateable Bash while disabling unhooked data-producing paths.
   Then update the launcher and add a live protected-launch test without a
   trust bypass. If no such configuration exists, scope the separately planned
   APPA MCP exec/stdin route and its execution boundary. Until then, the
   launcher must not claim working protected shell commands.
2. **Input and process containment.** Implement the planned `appa_stdin`
   control path before forwarding nonempty input. The current Unix child uses
   `Stdio::null()`, and the `appa_stdin` battery declaration is dormant. Add
   cancellation and daemonized-descendant containment tests before claiming
   interactive or persistent process support.
3. **Native Windows command path.** Implement and test shell rendering,
   redirected pipes without an inherited console, a child Job Object, sandbox
   HTTP proxy behavior, and the disposable `windows-validate.ps1` handoff.
   The current non-Unix path is a generic `cmd.exe /C` with null stdin; it
   does not satisfy the plan's Windows command requirements.
4. **Remaining live coverage.** Add opt-in authenticated tests for the
   default Codex classifier, policy denials/remedies, patch and MCP results,
   resume, and disabled/crashed/untrusted hooks. Verify a subagent return
   contract before enabling that route. Keep the known MCP error result gap
   explicit; no post-hook means no result withholding guarantee.

The [setup demo](DEMO.md) describes the tested install path and its current
boundary. The compatibility probes deliberately do not replace manual hook
trust or prove that a normal protected launch is complete.
