# Codex

Start a Codex session with `appa codex --` after you install the plugin.
Review and trust the installed hooks in `/hooks`.
Do not describe a regular Codex session as protected by this policy.

## Inspect

1. Check the APPA MCP connection.
2. Run `appa describe --adapter codex --config <installed Codex policy> --check`.
3. List the tool names visible in this session.
4. Pass the names with `--session-tools` to `appa describe --adapter codex`.
5. Include MCP server names as Codex reports them.
6. Inspect the included and available batteries.
7. Check that `http://127.0.0.1:8766/adapter` answers `codex` before you describe a session as protected.

The `Config:` path from `appa describe --adapter codex` comes from the installation receipt when one exists.
Without a receipt, the default path is `codex/appa.toml` under the platform APPA config directory.
An explicit `--config` overrides that selection.
The Codex battery supplies command contracts.
The default Codex root policy supplies local tool contracts.
The installed syntax reference is `references/contracts.md` beside this skill.
Other MCP batteries need separate setup.

Do not read secrets or unrelated files.
Do not use Claude Code tool spellings for Codex.

## Propose and apply

Use the shared router's `init`, `adjust`, and `explain` rules.
Present the complete behavior in plain English before an edit.

A proposal does not authorize a write.

After approval:

1. Re-read the policy and inventory.
2. Edit only the approved root rules or includes.
3. Run `appa describe --adapter codex --config <policy> --check`.
4. Run `appa codex-reload`.
5. Record the active policy key that the command prints.
6. Start a new session with `appa codex --`.
7. Run `appa codex-policy-key`.
8. Check that its key matches the recorded key before the denial check.

The launcher reconciles the installed policy before it starts Codex.
It checks the runtime deployment and the active policy key.
`appa codex-policy-key` reads the active key without a reload.
A changed policy does not reset the labels of an existing session.

After approval, use `appa battery install <name> --config <policy>` for batteries.
For a manual root edit, keep comments and unrelated rules.
Do not edit a battery in place.

Explain a denial with the tool's Codex name and the information label or authority that caused it.
Include any exact remedy offer from the runtime.
Call `appa/execute_remedy_plan` only with an offer id from the immediately preceding blocked result.

## Coverage limits

Codex can skip a hook that fails or lacks trust.
The startup check cannot guarantee that a later hook will run.
The wrapper withholds output only after a successful pre-use rewrite.
Native tool checks can occur before a hook.
MCP errors can bypass the post-use hook.

Do not claim that these paths passed a check.

The command wrapper needs loopback HTTP access from Codex's command sandbox.
The installed `appa` profile extends `:workspace` and allows `127.0.0.1` through Codex's proxy.
That host rule covers all ports on the allowed host.
The sandbox HTTP check refuses profiles that block the rule.
The launcher inherits Codex's shell setting and checks runtime HTTP access before it starts Codex.
The hook supervisor denies a tool call if its APPA worker fails or exceeds its deadline.
Codex can skip an untrusted hook or run an original command if the supervisor fails.
Native Windows commands need a separate sandbox and console probe.

Do not disable the filesystem sandbox, allow public hosts, or bypass hook trust to make the HTTP check pass.

## Develop the command classifier hint

Battery guidance connects recognizable CLI commands with each battery's source and sink intent.
This guidance does not reproduce typed MCP contracts, audience sources, or specialized Annotators.

During `init`, develop the hint for the root `codex.command-requirements` Annotator:

1. Read each approved battery's `appa.toml` and README from the deployment's battery store.
2. Translate its source trust and destination audience requirements into concise command guidance.
3. Use dynamic facts only when an installed context provider supplies them for that command.
4. State uncertainty conservatively when the command and supplied context cannot establish the boundary.
5. Preserve the existing root hint as an operator customization.
6. Keep the complete hint within 512 characters.
7. If guidance conflicts or exceeds that limit, propose the smallest explicit revision.
8. Include the exact proposed hint and its practical effect in the proposal.
9. After approval, copy the complete `codex.command-requirements` declaration into the root when necessary.
10. Change only its `hint`.
11. Preserve its implementation, inputs, and mandate.
12. Leave included battery files unchanged.
13. Run the policy check, reload, and procedure for a new session above.

Keep `codex.repository-requirements` separate from the generic command Annotator.
The GitHub battery supplies its facts through `context.github`.
Do not claim that a generated hint reproduces those facts or the specialized contract.

Configured MCP discovery remains incomplete for Codex.
The inventory covers the supplied session tools and cannot identify every absent connection.
`MCP servers: none` does not establish that the Codex configuration contains no servers.
