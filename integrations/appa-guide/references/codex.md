# Codex

Run this reference only in a Codex session started by `appa codex --`.
On Codex CLI 0.159.0, the protected launcher's `code_mode_host=false` setting
also disables shell requests. Report command execution as unsupported until
the host-mode gate in `integrations/codex/PHASE7.md` passes; do not advise
turning that feature on in a protected session as a workaround.

## Inspect

1. Check the APPA MCP server is connected and run `appa describe --adapter codex --config <installed Codex policy> --check`. The default policy is in the platform APPA config directory under `codex/appa.toml`. Do not read secrets or unrelated files.
2. List the tool names visible in this session and pass them with `--session-tools` to `appa describe --adapter codex`. Include MCP server names as Codex reports them. Do not assume Claude Code tool spellings apply.
3. Inspect included and available batteries. The Codex battery covers host commands and known local tools; other MCP batteries are optional and require their own setup.
4. Confirm the runtime answers as adapter `codex` at `http://127.0.0.1:8766/adapter` before describing the session as protected.

## Propose and apply

Use the shared router's `init`, `adjust`, and `explain` rules. Present the complete behavior in plain English before any edit. A proposal is not permission to write. After approval, re-read the policy and inventory, edit only the approved root rules or includes, run `appa describe --adapter codex --config <policy> --check`, then reload the Codex runtime policy. A new session picks up the resulting policy.

For batteries, use `appa battery install <name> --config <policy>` only after approval. For a manual root edit, keep comments and unrelated rules. Never edit a battery in place.

Explain a denial with the tool's actual Codex spelling, the information label or authority that caused it, and any exact remedy offer the runtime returned. Call `appa/execute_remedy_plan` only with an offer id in the immediately preceding blocked result.

## Coverage limits

Codex can skip a hook that is disabled, untrusted, changed but not retrusted, timed out, or crashed. The startup check does not make later hooks infallible. The command wrapper withholds output only after the pre-hook successfully replaces the command. Native tool validation or host-managed operations may happen before a hook. MCP errors may bypass the post-result hook. Do not claim those paths were checked.

The command wrapper needs loopback HTTP access from Codex's command sandbox. The installed `appa` permission profile extends `:workspace`, allows `127.0.0.1`, and routes it through Codex's proxy. `appa codex` checks this HTTP path with `codex sandbox` before launch and refuses when it cannot reach the runtime. That host rule lets any sandboxed command reach services on that host, across ports. Strict network-off, read-only, and managed profiles that disallow the rule cannot run protected shell commands in this release. Do not disable the filesystem sandbox, allow public hosts, or bypass hook trust to make the check pass. Native Windows protected-command support remains unverified until a Windows sandbox and console-leak probe passes.
