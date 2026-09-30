# appa-guide host parity

This document defines parity between appa-guide on Claude Code, Codex,
and kagent. The shared router in `SKILL.md` owns these invariants. Host
references implement them through different tools.

Codex follows the local policy proposal and approval flow in the Claude Code column.
It uses `appa describe --adapter codex` and a separate runtime.
This contract defines intended parity. Static text checks do not establish equivalent host behavior.

Codex has these qualifications:

- Discovery covers supplied session tools. It cannot establish the complete set of configured MCP servers or identify every absent connection.
- The adapter translates callable aliases into canonical policy identities. Coverage and proposed rules must use those identities.
- The installation receipt selects the policy unless `--config` overrides it. A guide must also check the runtime deployment and active policy key.
- The installer supplies `references/contracts.md` and the command classifier procedure. Their presence does not establish successful policy updates in every host environment.
- Hook trust, skipped hooks, sandbox access, and MCP error results limit protection. Unsupported paths require an explicit report.

Parity requires the same policy meaning, operator decisions, and reported outcome for equivalent tools and configuration.
Behavioral evidence must establish each host's actual policy admission and result handling.

## Required invariants

| ID | Requirement | Claude Code mapping | kagent mapping |
|---|---|---|---|
| P01 | Load one canonical router and exactly one complete host reference. | Read `references/claude-code.md`. | Read `references/kagent.md`. |
| P02 | Complete host inventory before battery matching or policy proposals. | Pass the session's tool names to `appa describe --session-tools`, which adds the MCP servers configured on the machine, and read the live config. | Inspect Agents, resolved workloads, RemoteMCPServers, runtime Services, and live config. |
| P03 | Treat the serving root config as truth. Never confuse available files, templates, or release manifests with serving policy. | Run `appa describe`; stop when a running runtime names another config path. | Use `appa_get_runtime_state`; runtime policy storage and reload stay behind typed vouched MCP operations. |
| P04 | Distinguish available, matched, included, and refreshable batteries. Suggest only authoritative matches not already included. | Use the battery matches `appa describe --session-tools` prints for the deployment's installed version. | Call `appa_match_batteries` with observed wire names; use its `matches` and `included` fields unchanged. |
| P05 | Generate IFC-first defaults. Static `self` and `internal` audiences need no source. Trusted internal work stays autonomous. | Apply the Claude Code uncovered-tool rules. | Apply the same rules to kagent wire names. |
| P06 | Inspection and proposal drafting are read-only. Present a complete proposal before asking approval. | Ask for approval only after the complete config proposal. | Ask in chat only after the complete per-runtime proposal. |
| P07 | Approval applies only to the exact pending proposal. Never invent an offer id or ask the operator for one. | Use the host permission channel for the approved write. | Invoke the mutation, then use only the exact offer id returned by its blocked result to open the confirmation card. On an explicit approval turn, each plugin bridges one unambiguous pending review if the model tries to stop. |
| P08 | Revalidate immediately before mutation. Apply only approved behavior, verify serving state, then reload. | Re-read config and tool inventory before write and verify the local runtime afterward. | Pass the observed policy key to one typed runtime management tool, which validates, publishes, reloads, and rolls back atomically. Agent CR changes remain complete-manifest applies. |
| P09 | A no-change result performs no write or reload and uses no approval language. | Finish with the unchanged outcome. | Finish with the unchanged outcome and no confirmation card. |
| P10 | User-facing replies report outcomes, not inspection mechanics. Unavailable components and blocked coverage remain explicit. | Use the concise proposal format in the Claude reference. | Use the same core format plus kagent-specific Agent, server, and runtime exceptions. |
| P11 | Sensitive inspection is least privilege. Do not read credentials or unrelated resources to construct policy. | Read only the live config, relevant includes, matched batteries, and installed tool guide. | Never read Secrets or Helm values; inspect only allowlisted Kubernetes kinds and release manifests. |
| P12 | Unsupported host behavior is explicit and cannot be described as protected. | Stop when the local plugin/runtime deployment cannot be verified. | Verify the resolved image and Ready condition. Refuse unsupported memory prefetch, unsafe Go delegation, and missing `-full` images. |
| P13 | Multiple runtimes never collapse into one implicit proposal or success claim. | Stop when the discovered process uses another config path. | Make, approve, apply, and verify one named proposal per runtime. |
| P14 | Battery inclusion preserves maintained defaults. Exact aliases come from the include; only suffix-only host translations become root rules. | Include the unchanged battery and add only approved root overrides. | Include the unchanged battery; copy a declaration only when kagent's wire name requires translation. |
| P15 | A battery's credential lives in the runtime environment, never in policy; the root maps `self` and `internal` onto the battery's audience source. | Name `APPA_PROVIDER_<PROVIDER>_TOKEN` for the local runtime's environment and add the `[policy.audience]` mapping the battery README shows. | Carry the variable in the chart's `env` values and add the same `[policy.audience]` mapping to the root policy. |

## Allowed host differences

| ID | Difference | Required handling |
|---|---|---|
| X01 | Discovery transport | Claude Code reads local process and session configuration. kagent reads Kubernetes and Helm resources. Codex reports supplied session tools and the limits of configured-server discovery. |
| X02 | Mutation transport | Claude Code edits local files. kagent calls vouched runtime management MCP tools; Agent lifecycle uses complete CR manifests. Both require approval and post-write verification. |
| X03 | Human-review channel | Claude Code uses its host permission channel. kagent uses the Approve/Reject card reached through an exact remedy offer. Neither host may infer approval. |
| X04 | Runtime topology | Claude Code normally reaches a local runtime. kagent reaches one or more Services. Each serving runtime must be independently identified and verified. |
| X05 | Harness limitations | kagent may lack a gateable boundary for a feature. The guide must refuse or label that feature unsupported; it may never weaken the policy silently. |

The rule “No other host difference may weaken an invariant” applies to the intended contract.

Before a new exception applies, document it here and add a regression.

## Regression evidence

- `appa-runtime/tests/guide_parity.rs` checks invariant identifiers and static host mappings. It does not establish equivalent behavior.
- `appa-runtime/tests/guide_skill.rs` locks canonical packaging, proposal,
  approval, policy, and chart instructions.
- `appa-runtime/src/mcp.rs` tests deterministic battery matching and included
  state.
- `appa-runtime/src/config.rs` tests included-battery identity.
- `integrations/kagent/appa-kagent-adk/tests` and
  `integrations/kagent/appa-kagent-adk-go` lock runtime-owned tool exposure and
  reserved-name refusal.
- `integrations/kagent/tests` executes policy behavior through the real runtime
  and plugin with a scripted model.
- `integrations/kagent/e2e/ui/test_guide_ui.py` executes the kagent proposal,
  approval, rejection, protection, refresh, and cleanup paths in Chromium.

## Completion rule

A parity change requires checks for the shared invariant, every affected host mapping, static regressions, behavioral regressions, and the relevant live workflow.
A platform limitation requires an explicit exception.
The affected host fails closed or reports it as
unsupported.
Codex manual hook trust and cancellation lack equivalent live evidence in this audit.
