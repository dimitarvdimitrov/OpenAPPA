# Codex plugin

`appa plugin install codex` installs a separate Codex policy and runtime. It registers command hooks in the active Codex profile, an `appa` HTTP MCP server, and the `/appa-guide` skill. It keeps Claude Code's deployment separate. Start a session with `appa codex --` after using Codex `/hooks` to review and trust the new hooks. Remove installer-owned registrations with `appa plugin remove codex`.

The default policy includes the Codex battery. Its command and unknown-tool annotations use `builtin = "codex"`, which requires a working saved Codex CLI login. The CLI consult uses that login; it does not need an API key in policy. The default currently leaves subagent spawn and peer-message routes undeclared pending live lifecycle verification.

Protected command execution requires a supported Codex sandbox profile that permits HTTP to `127.0.0.1` through Codex's filtered proxy. That host permission covers all ports on `127.0.0.1` for sandboxed commands. Hook trust, actual sandbox connectivity, and native Windows containment must be checked on the installed Codex release. See the [Codex integration guide](../../../website/content/docs/codex.md) for supported and unverified cases.
