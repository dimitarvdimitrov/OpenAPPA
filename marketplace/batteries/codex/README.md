# Codex battery

The Codex plugin includes this battery on first install. It declares the
runtime-owned `host/codex/appa_exec` child command used by the Bash wrapper.
Literal credential paths and credential-producing commands narrow the session
to `self`. Other commands use the saved-login `codex` classifier to decide
trust, audience, and attention requirements. No API key is needed.

The wrapper confines complete stdout and stderr until APPA admits the result.
The `redact-secrets` sanitizer can mask a withheld result before the model sees
it. Output appears only after the command completes. A nonzero effectful
command remains indeterminate with reservations retained, and its diagnostics
are withheld. Nonzero commands under the shipped effect-free contract can
return admitted diagnostics.

The policy reserves a `host/codex/appa_stdin` declaration for later input
checks. The current wrapper starts children with null stdin and does not
forward nonempty input. Rerun a command with finite input supplied up front;
do not rely on interactive prompts.

Selectors match the command text, not every command's resolved file path or
runtime behavior. Shell expansions, aliases, and scripts rely on the
classifier. The Codex default leaves unknown hooked tools and subagent
operations undeclared until their host behavior has passed a live gate.

For a custom root policy, include the battery with:

```toml
include = ["batteries/codex/appa.toml"]

[policy]
version = 2
```

The root can place stricter command rules before the battery. The default root
also declares `apply_patch` and local utility operations. Shared MCP batteries
continue to use canonical `mcp/<server>/<tool>` names.
