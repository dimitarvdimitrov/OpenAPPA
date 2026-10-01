# Codex plugin

`appa plugin install codex` installs a separate Codex policy and runtime. It registers command hooks in the active Codex profile, an `appa` HTTP MCP server, and the `/appa-guide` skill. It keeps Claude Code's deployment separate.

1. Use Codex `/hooks` to review and trust the new hooks.
2. Start a session with `appa codex --`.
3. To remove installer-owned registrations, use `appa plugin remove codex`.

The default policy includes the Codex battery for command contracts. The root policy supplies patch, local image, plan, web, and reporting contracts. Command, patch, local image, and unknown-tool annotations use `builtin = "codex"`. The saved Codex CLI login supplies authentication. The classifier does not need an API key in the policy.

The `webrun` contract requires the `public` audience and labels web results `suspicious`. It covers hooked `webrun` calls. Codex hosted `WebSearch` bypasses this [hook path](https://learn.chatgpt.com/docs/hooks). The first web call can require acceptance of the lower trust before execution.

Other unknown hooked tools, including `image_genimagegen` and unknown MCP tools, use the wildcard Codex classifier. Explicit contracts take precedence over this wildcard. The classifier receives the complete canonical call and its arguments. APPA checks the annotation against the session and policy before execution. Missing, failed, malformed, or timed-out annotations cause refusal.

Subagent spawn, wait, resume, close, and peer-message routes remain blocked until a live test verifies their lifecycle contracts. The default policy also blocks `appa_stdin` because the command wrapper does not forward later input.

Protected command execution requires a supported Codex sandbox profile that permits HTTP to `127.0.0.1` through Codex's filtered proxy. That host permission covers all ports on `127.0.0.1` for sandboxed commands. Hook trust, actual sandbox connectivity, and native Windows containment must be checked on the installed Codex release. See the [Codex integration guide](../../../website/content/docs/codex.md) for supported and unverified cases.
