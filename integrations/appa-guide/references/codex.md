# Codex

Use this reference in a Codex session started with `appa codex --` after plugin installation.
The shared skill carries the `init`, `adjust`, and `explain` rules.

## Inspect

1. Check that the session has the APPA MCP connection.
2. Run `appa describe --adapter codex`.
3. Use the complete path on its `Config:` line as `<policy>` below.
4. List the tool names that this session can call, including MCP tools.
5. Run `appa describe --adapter codex --config <policy> --session-tools <name>,<name>,... --check` with those names.
6. Read the policy, its included batteries, and the READMEs for suggested batteries.
7. Check that `${APPA_RUNTIME_URL:-http://127.0.0.1:8766}/adapter` answers `codex`.

The `Config:` path from `appa describe --adapter codex` comes from the installation receipt when one exists.
Without a receipt, the default path is `codex/appa.toml` under the platform APPA config directory.
An explicit `--config` overrides that selection.
The Codex battery supplies command contracts.
The default Codex root policy supplies local tool contracts.
The installed syntax reference is `references/contracts.md` beside this skill.
Other MCP batteries need separate setup.

Use the tool names that Codex reports. Do not substitute Claude Code names.
Do not read secrets or unrelated files.
If a command is unsupported or fails, report the exact command and error.
Do not treat the connection check as proof that every call passes through APPA.

## Propose and apply

Present each proposal in plain English before you ask for approval.
Wait for approval before you change the policy.

After approval:

1. Read the policy and tool list again.
2. Install approved batteries with `appa battery install <name> --config <policy>`.
3. Edit only the approved root rules or includes.
4. Run `appa describe --adapter codex --config <policy> --check`.
5. Run `appa codex-reload`.
6. Record the active policy key that the command prints.
7. Start a new session with `appa codex --`.
8. Run `appa codex-policy-key`.
9. Check that its key matches the recorded key before the denial check.

The launcher reconciles the installed policy before it starts Codex.
It checks the runtime deployment and the active policy key.
`appa codex-policy-key` reads the active key without a reload.
A changed policy does not reset the labels of an existing session.

Keep comments and unrelated rules. Do not edit a battery in place.

## Explain a blocked call

Name the blocked tool as Codex reports it.
Explain which rule or approval blocks the call and what must change before it can proceed.
Include the exact remedy offer if APPA returns one.
Call `appa/execute_remedy_plan` only with an offer id from the immediately preceding blocked result.

## Limits to report

The launcher inherits Codex's shell setting, `features.code_mode_host`.
Codex CLI 0.159.2 enables the shell by default.
The launcher checks that commands in the Codex sandbox can reach APPA before it starts Codex.
The installed `appa` profile allows access to every port on `127.0.0.1`.
A profile that blocks this connection fails the launch check.

Each APPA hook runs a policy check.
The hook supervisor blocks the call if that check crashes, times out, or returns invalid JSON.
If Codex skips an untrusted hook, or the supervisor fails before its reply, Codex can run the original command.
The launcher cannot check whether the user trusts the hooks.

Some native tool checks happen before a hook.
MCP tool errors can skip the hook that checks the result.
The command wrapper cannot receive more input after a command starts or support interactive terminal programs.
Native Windows commands do not have a verified protected path.

Ask the user to review and trust the APPA hooks through `/hooks`.
Do not describe a session as protected until a manually trusted end-to-end check passes.
Do not disable the filesystem sandbox, allow public hosts, or bypass hook trust to pass the launch check.

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
