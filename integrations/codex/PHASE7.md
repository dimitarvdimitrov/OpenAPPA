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
| Ordinary protected shell through `appa codex` | Blocked by the selected host mode | The launcher sets `features.code_mode_host=false`. An isolated Codex 0.159.0 exec with that setting returned `code-mode host is disabled` for a shell request. With it enabled, the synthetic proxy flow worked. Probe all code-mode filesystem/network operations and their hook coverage before changing the launcher, or design the separately planned APPA MCP command route. Then test a manually trusted protected launch. |
| Hook trust and later hook failure | Known host gap | The launcher cannot verify trust noninteractively, and its sandbox handshake does not stop a later crash or timeout. The live failure matrix above demonstrates original-command execution. Coverage docs must keep that limitation explicit. |
| Result sanitizer and effectful outcomes | Partly tested | Fake HTTP and deterministic tests prove output replacement and nonzero effectful withholding. A live Codex run with a synthetic secret in **both** stdout and stderr, nonzero diagnostics, output overflow, runtime loss at admission, duplicate post, and concurrent actor isolation has not passed. Direct and proxy-routed HTTP were tested separately, but not their full result matrix. |
| Later stdin and terminal jobs | Unsupported | The child still uses `Stdio::null()`; `host/codex/appa_stdin` is dormant. Implement input authorization, split-input checks, pipe/echo handling and process containment before claiming interaction. |
| Actual Codex MCP error, remedy and resume | Unverified | Direct hook fixtures cover ordinary MCP result and error-post shapes, plus compact/reopen. They cannot prove that Codex emits the post-hook on `is_error`, preserves remedy actor binding, or delivers checked text after a real resume/subagent return. Subagents remain disabled in shipped policy. |
| Planned automated acceptance suite | Incomplete | The planned `codex_model`, `codex_hooks`, `codex_lifecycle`, `codex_windows`, and `real_codex` test targets do not exist. The disposable Python probes cover selected live behavior but are not CI gates. The stdin, sanitizer, error, concurrency, resume, and cross-platform cases above need deterministic fixtures and opt-in live tests. |
| Native Windows command path | Unsupported and untested | No Windows validation host or `windows-validate.ps1` handoff exists. The current non-Unix child path is generic `cmd.exe /C` with null stdin, without the planned detached console, strict handles or Job Object containment. A macOS cross-target check was blocked by missing `x86_64-w64-mingw32-gcc`; that is no evidence of Windows behavior. |
| Linux and strict/managed profiles | Unverified | The live probes ran only on this macOS build and its filtered-network profile. Linux, network-off, managed restrictions and Windows elevated/unelevated modes need their own observed matrix. |
| Published release artifacts and full guide flow | Unverified | Local package and guide tests passed. A published binary/bundle and a real `/appa-guide` policy proposal in a manually trusted Codex session were not exercised. |

## Phase-sized follow-up

1. **Host-mode coverage and protected launch.** Map the enabled code-mode
   operations against synchronous hooks. Either retain a shell route with
   demonstrated mediation or implement the planned APPA MCP command route.
   Complete a manually trusted `appa codex` edit/test and denial demo before
   claiming a protected session.
2. **Input, process containment, and result matrix.** Implement gated stdin
   and containment for escaped descendants, then exercise the remaining
   sanitizer, failure, retry, concurrency, and resume cases above. The current
   finite Unix route can be documented as partial during this work.
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
