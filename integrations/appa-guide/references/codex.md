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
7. Confirm that `http://127.0.0.1:8766/adapter` answers `codex` before you describe a session as protected.

The default policy is in the platform APPA config directory under `codex/appa.toml`.
The Codex battery covers host commands and known local tools.
Other MCP batteries need separate setup.

Do not read secrets or unrelated files.
Do not use Claude Code tool spellings for Codex.

## Propose and apply

Use the shared router's `init`, `adjust`, and `explain` rules.
Present the complete behavior in plain English before an edit.

A proposal does not authorize a write.

After approval, re-read the policy and inventory.
Edit only the approved root rules or includes.
Run `appa describe --adapter codex --config <policy> --check`.
Reload the Codex runtime policy.
A new session uses the new policy.

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
