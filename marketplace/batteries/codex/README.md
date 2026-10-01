# Codex battery

The Codex plugin includes this battery on first install. It declares the
runtime-owned `host/codex/appa_exec` child command used by the Bash wrapper.
Literal credential paths and credential-producing commands narrow the session
to `self`. Other commands use the saved-login `codex` classifier to decide
trust, audience, and attention requirements. The saved login supplies authentication.

Repository commands use a separate classifier with the same instructions as Claude Code.
The GitHub battery supplies repository visibility and author facts through `context.github`.
Writes to public or unknown destinations require `public`. Writes to private or internal
destinations require `internal`. A read preserves the audience and requires evidence about every author before
its trust can be `trusted`.

The wrapper confines complete stdout and stderr until APPA admits the result.
The `redact-secrets` sanitizer can mask a withheld result before the model sees
it. Output appears only after the command completes.

Both command classifiers can use effects declared by the deployment. If its annotation
declares effects, a complete nonzero result remains indeterminate with effect reservations
retained. Without declared effects, a complete nonzero result
can return admitted diagnostics. Result admission can still withhold output.

The wrapper starts each child with null stdin. It does not forward later input.

Put required input in the original command. Do not rely on interactive prompts.

Selectors match the command payload after APPA removes an optional `appa-codex-exec-v1` header.
They do not resolve every file path or runtime behavior. Shell expansions, aliases, and scripts rely on the
classifier. The default root classifies other hooked tools with its wildcard Annotator.
Explicit contracts take precedence. Missing or invalid annotations cause refusal.
Subagent and peer routes remain explicitly blocked until a verified lifecycle contract exists.

For a custom root policy, include the battery with:

```toml
include = ["batteries/codex/appa.toml"]

[policy]
version = 2
```

The root can place stricter command rules before the battery. The default root
also declares `apply_patch`, local utility operations, and `webrun`.
The web contract requires a `public` audience and labels results `suspicious`. Its patch classifier
requires fresh `hitl` attention for hook and MCP configuration edits, including
mixed patches. This requirement depends on the classifier's annotation.
Shared MCP batteries continue to use canonical `mcp/<server>/<tool>` names.

The execution header supplies an absolute directory, an absolute supported shell path, and a boolean login mode before command classification.
Plain commands use the hook session directory and the hook's `$SHELL` with `-lc`.
Native `exec_command` options can disappear from hook events and therefore do not select the wrapped execution context.
See the [Codex guide](../../../website/content/docs/codex.md#command-directory-and-shell) for the header format and an explicit `cd` alternative.
