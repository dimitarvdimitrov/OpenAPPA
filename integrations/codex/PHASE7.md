# Phase 7 validation report

Tested on 29 September 2026 with Codex CLI 0.159.0 on macOS 26.6.2 arm64.
All live writes used disposable projects and synthetic markers. An opt-in
compatibility probe used the saved Codex login; it did not copy credentials.
The current protected launcher still has the host-mode blocker described
below, so this report does **not** certify a fully protected Codex session.

## What passed

| Area | Result | Evidence and limit |
|---|---|---|
| Rust adapter, runtime, policy and installer tests | Pass | Codex adapter tests; 21 Codex runtime unit tests; `codex_exec`, `codex_install`, `codex_policy`, `hook_client`, and existing guide/marketplace tests. These include nonzero effectful withholding, restart and late-result handling, ordinary grandchild cancellation, and preservation of unrelated profile entries. |
| Guide Python tests | Pass | 12 tests with `uv run --with pyyaml python3 -m unittest discover -s scripts -p 'test_appa_guide_runtime.py'`. The system Python lacked PyYAML. |
| Live hook rewrite | Pass | `probe.py`: one Bash pre/post pair, matching call ID, rewritten command, and rewritten marker in the model-visible result. |
| Live pre-hook denial | Pass when the hook runs | `hook_failure_probe.py`: valid denials stopped a synthetic shell file write and an `apply_patch` file creation. |
| Live host failure behavior | Bypass observed | The same probe wrote the synthetic file after pre-hook crash, malformed output, timeout, or untrusted hook skip. Codex executed the original shell command in each case. |
| Real patch result shape | Observed and fixed | Codex supplied a plain string to `PostToolUse` for `apply_patch`. APPA now encodes it as a JSON string on the runtime wire. `lifecycle_probe.py` confirms successful patch and MCP string result pairs reach the runtime. |
| Saved-login default classifier | Pass for two synthetic calls | `classifier_probe.py` received valid answers from `codex.command-requirements` and `codex.patch-requirements`; it made no tool calls. This does not validate every policy judgment or approval flow. |
| Sandboxed proxy | Pass for finite output | `proxy_probe.py`: three early polls saw no output; completion released only the admitted marker; a forged job handle was refused. `codex_exec` tests cover direct HTTP and selected failure outcomes. |
| Sandbox profile | Pass on this macOS release | `sandbox_guards_probe.py`: in-workspace write and listed `127.0.0.1` HTTP succeeded; sibling write and unlisted `localhost` were denied; public `https://example.com/` received a proxy 403. The loopback exception still applies to every port on the allowed host. |
| Direct hook lifecycle | Pass in a synthetic runtime fixture | `lifecycle_probe.py`: MCP success, patch success, compact/reopen, unknown-tool and peer-message denials, and withholding of an MCP error **when** a post-hook arrives. This is not an actual Codex resume or MCP-error-host probe. |
| Combined model → hook → proxy → model | Pass only in a disposable compatibility probe | `live_e2e_probe.py` used a temporary static policy, explicit loopback profile, enabled code-mode host, and a fixture-only hook-trust bypass. The model saw exactly the admitted marker. |
| Installation and removal | Pass in a disposable profile | `appa plugin install codex --json`, `appa codex -- --version`, and `appa plugin remove codex --json` created the Codex hooks, profile, guide, policy and runtime, passed the sandbox handshake, then removed registration while preserving policy/data. |
| Source install and local bundle after catalog repair | Pass | A second disposable source install succeeded. `appa bundle` succeeded and included the Codex plugin manifest, battery, and policy in a 142-entry archive. The adapter is linked into `appa`; a separate adapter executable is not expected. Published release artifacts remain untested. |

## Corrections made during verification

- The earlier Phase 7 documentation edit changed the Codex battery without
  regenerating its marketplace digest. A real source install failed with
  `package codex does not match its catalog identity`. The catalog has been
  regenerated; the catalog check, source installation, and local bundle now
  pass.
- The Codex adapter treated a plain string `PostToolUse` result as raw JSON.
  APPA therefore withheld successful patch and MCP results before the fix.
  It now preserves the host string as a JSON string value; the pure wire test
  and direct runtime lifecycle probe pass.
- A stale lifecycle test expected Codex runtime commands to be unavailable.
  It now checks endpoint routing in an isolated environment.

## What is blocked or still missing

| Area | State | Work needed to claim support |
|---|---|---|
| Ordinary protected shell through `appa codex` | Blocked by the selected host mode | The launcher sets `features.code_mode_host=false`. An isolated Codex 0.159.0 exec with that setting returned `code-mode host is disabled` for a shell request. With it enabled, the synthetic proxy flow worked. Probe all code-mode filesystem/network operations and their hook coverage before changing the launcher, or scope an APPA MCP command route as a new fallback phase. Then test a manually trusted protected launch. |
| Hook trust and later hook failure | Known host gap | The launcher cannot verify trust noninteractively, and its sandbox handshake does not stop a later crash or timeout. The live failure matrix above demonstrates original-command execution. Coverage docs must keep that limitation explicit. |
| Result sanitizer and effectful outcomes | Partly tested | Fake HTTP and deterministic tests prove output replacement and nonzero effectful withholding. A live Codex run with a synthetic secret in **both** stdout and stderr, nonzero diagnostics, output overflow, runtime loss at admission, duplicate post, and concurrent actor isolation has not passed. Direct and proxy-routed HTTP were tested separately, but not their full result matrix. |
| Later stdin and terminal jobs | Unsupported | The child still uses `Stdio::null()`; `host/codex/appa_stdin` is dormant. Implement input authorization, split-input checks, pipe/echo handling and process containment before claiming interaction. |
| Actual Codex MCP error, remedy and resume | Unverified | Direct hook fixtures cover ordinary MCP result and error-post shapes, plus compact/reopen. They cannot prove that Codex emits the post-hook on `is_error`, preserves remedy actor binding, or delivers checked text after a real resume/subagent return. Subagents remain disabled in shipped policy. |
| Planned automated acceptance suite | Incomplete | The planned `codex_model`, `codex_hooks`, `codex_lifecycle`, `codex_windows`, and `real_codex` test targets do not exist. The disposable Python probes cover selected live behavior but are not CI gates. The stdin, sanitizer, error, concurrency, resume, and cross-platform cases above need deterministic fixtures and opt-in live tests. |
| Native Windows command path | Unsupported and untested | No Windows validation host or `windows-validate.ps1` handoff exists. The current non-Unix child path is generic `cmd.exe /C` with null stdin, without the planned detached console, strict handles or Job Object containment. A macOS cross-target check was blocked by missing `x86_64-w64-mingw32-gcc`; that is no evidence of Windows behavior. |
| Linux and strict/managed profiles | Unverified | The live probes ran only on this macOS build and its filtered-network profile. Linux, network-off, managed restrictions and Windows elevated/unelevated modes need their own observed matrix. |
| Published release artifacts and full guide flow | Unverified | Local package and guide tests passed. A published binary/bundle and a real `/appa-guide` policy proposal in a manually trusted Codex session were not exercised. |

## Comparison with CLAPPA

The missing rows mix product work, host limits, and validation. CLAPPA uses Claude Code's native Bash tool. It does not own the shell process, its output stream, or later stdin bytes. The original Codex plan requires a stronger command boundary and selects Windows 11 as an initial target. Those requirements extend beyond demonstrated CLAPPA parity.

| Report area | CLAPPA status | Codex gap and size |
|---|---|---|
| Ordinary protected shell | Native Bash uses pre- and post-use hooks. APPA has no shell proxy ([hook setup](../../appa-runtime/src/init/settings.rs), [adapter](../../appa-adapter-claude-code/src/parse.rs)). | **Large launch blocker.** A protected shell claim requires a usable host mode and checked coverage of other operations. |
| Hook trust and failure | APPA blocks a pending call if its client returns code 2. A skipped or timed-out host hook supplies no APPA decision ([client](../../appa-runtime/src/hook_client.rs)). | **Shared host limit.** Codex startup checks cannot prevent a later hook failure. The [failure note](WORKING_NOTES.md) records the tested cases. |
| Result sanitizer and uncertain effects | The shared runtime tracks uncertain outcomes. CLAPPA replaces normal results when the post-hook arrives ([adapter](../../appa-adapter-claude-code/src/redact.rs)). Failed output lacks full coverage. | **Mainly validation.** Codex has deterministic tests. Live stdout, stderr, overflow, and failure cases remain open. |
| Later stdin and terminal jobs | CLAPPA does not check each stdin chunk or own a terminal. | **Large optional feature.** The Codex plan requests finite pipe input because later stdin has no new pre-hook. The initial scope can remain finite commands without later input. |
| MCP error, remedy, resume, and subagents | Remedy and resume use shared runtime code. CLAPPA checks subagent returns ([tests](../../appa-runtime/tests/claude_code_subagent.rs)). | **Mixed.** Remedy and resume need live Codex tests. An MCP error without a post-hook is an accepted host limit. Safe Codex subagents need a separate phase if enabled. |
| Automated acceptance suite | Claude has Rust tests and a deterministic CLI gate ([CI](../../.github/workflows/ci.yml), [gate](../../marketplace/plugins/claude-code/live-gate-check.py)). | **Medium test work.** The required cases matter more than the exact test filenames. |
| Native Windows command path | Claude has Windows binary and package checks. Those checks do not prove a protected Windows shell session ([build workflow](../../.github/workflows/build-appa-runtime-binaries.yml)). | **Large Codex-specific phase** if Windows 11 remains an initial target. |
| Linux and strict or managed profiles | Claude's Linux tests do not test Codex permission profiles or its loopback exception. | **Platform validation.** Linux needs a live sandbox check. Strict and managed profiles that forbid loopback need refusal diagnostics, not support. |
| Published artifacts and guide flow | Release packaging exists. No live Claude `/appa-guide` proposal test appears in the repository ([release workflow](../../.github/workflows/release.yml)). | **Release validation.** Codex source installation and a local bundle passed. Published artifacts need inspection. The guide flow needs a live test after the shell route works. |

For a first macOS flow with finite commands, the host-mode gate and a manually trusted launch are the main blockers. Live output and denial checks must support the claims for that flow. Stdin, subagents, and native Windows can be separate phases if the initial scope changes. The original plan includes Windows 11 in its initial target.

## Phase-sized follow-up

1. **Host-mode coverage and protected launch.** Map the enabled code-mode
   operations against synchronous hooks. Either retain a shell route with
   demonstrated mediation or scope a new APPA MCP command route if the proxy
   route cannot pass its gate.
   Complete a manually trusted `appa codex` edit/test and denial demo before
   claiming a protected session.
2. **Input and process containment.** Gated stdin and containment for escaped
   descendants require a separate phase if finite later input remains in
   scope. The sanitizer, failure, retry, concurrency, and resume matrix is
   separate validation. The current finite Unix route remains partial.
3. **Native Windows path and handoff.** Implement the command wrapper and
   `windows-validate.ps1`, then run the colleague-assisted live matrix on a
   matching Windows 11 host. Release claims must name the tested architecture
   and sandbox mode.
4. **Cross-platform and release checks.** Run Linux and managed/strict profile
   matrices, live MCP-error/remedy and guide workflows, the shipped artifact
   inspection, and supported Claude/Codex workflow comparisons.

## Reproduce the available probes

The automated checks passed with these commands:

```sh
cargo test -p appa-adapter-codex -p appa-runtime-api -p appa-package -p appa-policy
cargo test -p appa --lib codex
cargo test -p appa --test codex_exec --test codex_install --test codex_policy --test hook_client --test settings_hooks --test init_cli --test marketplace_cli --test guide_parity --test guide_skill --test claude_code_subagent
uv run --with pyyaml python3 -m unittest discover -s scripts -p 'test_appa_guide_runtime.py'
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

The disposable probes ran with:

```sh
cargo build -p appa
python3 integrations/codex/probe.py
python3 integrations/codex/proxy_probe.py
python3 integrations/codex/sandbox_guards_probe.py
python3 integrations/codex/lifecycle_probe.py
python3 integrations/codex/classifier_probe.py       # saved-login model calls
python3 integrations/codex/hook_failure_probe.py     # saved-login model calls
python3 integrations/codex/live_e2e_probe.py         # saved-login model call
bash scripts/appa-marketplace.sh --check
```

The [setup demo](DEMO.md) describes installation and its current boundary.
The compatibility probes use disposable trust bypass only to test the hook
contract; they do not replace manual trust in a normal session.
