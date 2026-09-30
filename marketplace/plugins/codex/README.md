# Codex plugin

`appa plugin install codex` installs a separate Codex policy and runtime.
It registers hooks, an `appa` HTTP MCP server, a permission profile, and the `/appa-guide` skill.
Claude Code uses a separate deployment.

After installation, review and trust the APPA hooks through Codex `/hooks`.
Start a session with `appa codex --`.
Remove installer-owned registrations with `appa plugin remove codex`.

The default policy includes the Codex battery for command contracts.
The root policy supplies patch, local image, plan, web, and reporting contracts.
`host/codex/webrun` requires a `public` audience and labels web results `suspicious`.
The first web call can require acceptance of that restriction before execution through an APPA remedy.
Hosted `WebSearch` can bypass hooks. This contract covers the recorded local `webrun` route.

Other hooked tools use the `codex.undeclared-tool` wildcard Annotator with `builtin = "codex"`.
It receives the complete canonical tool name and arguments.
Explicit contracts take precedence over the wildcard.
Classification determines trust, audience, and attention requirements. APPA then checks those requirements before it permits the call.
Failed, missing, malformed, or timed-out annotations cause refusal.
The wildcard cannot declare effects. Subagent and peer routes remain explicitly blocked until lifecycle verification.
The default also blocks `appa_stdin` because the wrapper does not forward later input.

The command, patch, local image, and wildcard Annotators require a saved Codex CLI login.
No API key is necessary in the policy.
The launcher reconciles the installed policy before each new session.
`appa codex-reload` explicitly reloads it and prints the active policy key.
`appa codex-policy-key` reads that key without a reload.

Protected commands need a supported Codex sandbox profile with HTTP access to `127.0.0.1` through Codex's filtered proxy.
The installed host rule covers every port on `127.0.0.1`.
Hook trust and sandbox connectivity depend on the installed Codex release.
Native Windows containment lacks equivalent verification.
See the [Codex guide](../../../website/content/docs/codex.md) for execution context, setup, remedies, and coverage limits.
