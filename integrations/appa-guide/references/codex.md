# Codex

Use this reference in a Codex session started with `appa codex --` after plugin installation.
The shared skill carries the `init`, `adjust`, and `explain` rules.
Do not describe a regular Codex session as protected by this policy.

## Inspect

1. Check that the session has the APPA MCP connection.
2. Run `appa describe --adapter codex`.
3. Use the complete path on its `Config:` line as `<policy>` below.
4. List the tool names that this session can call, including MCP tools.
5. Run `appa describe --adapter codex --config <policy> --session-tools <name>,<name>,... --check` with those names.
6. Read the policy, its included batteries, and the READMEs for suggested batteries.
7. Check that `${APPA_RUNTIME_URL:-http://127.0.0.1:8766}/adapter` answers `codex`.

The default policy is under `codex/appa.toml` in APPA's config directory.
The installation receipt selects the `Config:` path when it exists. An explicit `--config` overrides that selection.
Check that the runtime serves that policy before a proposal.
The Codex battery supplies command contracts.
The default root supplies patch, local image, plan, and local web contracts.
The installed `references/contracts.md` supplies the policy syntax reference.

Discovery covers the supplied session tools. It does not establish the complete configured MCP server set or every absent connection.
`MCP servers: none` does not establish that the Codex configuration contains no servers.

Use the callable names that Codex reports with `--session-tools`.
Use the translated canonical policy identities in proposed rules.
Do not substitute Claude Code names.

| Callable name | Hook name | Policy identity |
| --- | --- | --- |
| `exec_command` | `Bash` | `host/codex/appa_exec` |
| `web__run` | `webrun` | `host/codex/webrun` |
| `image_gen__imagegen` | `image_genimagegen` | `host/codex/image_genimagegen` |
| `mcp__example__search` | `mcp__example__search` | `mcp/example/search` |

Do not read secrets or unrelated files.
If a command is unsupported or fails, report the exact command and error.
Do not treat the connection check as proof that every call passes through APPA.

## Propose and apply

Present each proposal in plain English before you ask for approval.
Wait for approval before you change the policy.

After approval:

1. Read the policy and tool list again.
2. Install approved batteries with `appa battery install <name> --config <policy>`.
3. Edit only the approved rules and includes in the root policy.
4. Run `appa describe --adapter codex --config <policy> --check`.
5. Run `appa codex-reload`.
6. Record the active policy key that the command prints.
7. Start a new `appa codex --` session.
8. Run `appa codex-policy-key`.
9. Check that its key matches the recorded key before a denial demonstration.

Keep comments and unrelated rules. Do not edit a battery in place.

The launcher reconciles the installed policy before each new session.
It checks the runtime deployment and active policy key.
`appa codex-policy-key` reads the active key without a reload.
A reload does not reset the information labels of an existing session.

## Explain a blocked call

Name the blocked tool as Codex reports it.
Explain which rule or approval blocks the call and what must change before it can proceed.
Include the exact remedy offer if APPA returns one.
Call `appa/execute_remedy_plan` only with an offer id from the immediately preceding blocked result.

## Default coverage and remedies

The root web contract covers the recorded local `webrun` hook.
It requires a `public` audience and marks results `suspicious`.
The first web call can require acceptance of that trust restriction before execution.
Hosted `WebSearch` can bypass hooks, so this contract does not establish its protection.
Apps, browser use, and `--search` follow Codex's configuration. Availability does not establish APPA coverage.

Other hooked tools use `codex.undeclared-tool`, a wildcard Annotator with `builtin = "codex"`.
It receives the complete canonical call and arguments. Explicit contracts take precedence.
APPA checks its annotation before admission. Failed, missing, malformed, or timed-out annotations cause refusal.
The wildcard cannot declare effects. Subagent and peer routes remain explicitly blocked until lifecycle verification.

For a refused `mcp__example__search` call, propose a maintained battery or a root contract for `mcp/example/search`.
State its source trust, destination audience, and necessary approval.
Present the complete proposal before a write. Apply the approved proposal with the reload procedure above.

Local commands such as `ls -la .` or `cat appa.toml` can require acceptance of an audience restriction or sanitization.
An accepted restriction remains in the session and can prevent a later public query.

If APPA offers a remedy:

1. Explain the exact acceptance, sanitization, or approval requirement.
2. Let the user select the offered plan.
3. Use `appa/execute_remedy_plan` only with the offer id from that blocked result.

For the first web check, use a public query through local `web__run`.
If APPA blocks it, explain its current remedy offer before execution.
Do not claim that a hosted search followed this route.

## Command directory and shell

Codex can omit native `exec_command` options from hook events.
A native `workdir`, `shell`, or `login` option therefore does not reliably select APPA's wrapped execution context.
Plain commands use the hook session directory and the hook's `$SHELL` with `-lc`.

Put an explicit execution header on the first line of the command:

```sh
# appa-codex-exec-v1 {"workdir":"/absolute/project/nested","shell":"/bin/bash","login":false}
pwd
```

The header requires an existing absolute directory, an existing absolute `sh`, `bash`, or `zsh` path, and a boolean `login` value.
APPA validates it before classification and removes it from the command payload that selectors inspect.
The job preserves the approved directory, shell, and login mode.
The proxy restores the approved directory after shell startup.
Startup files can still alter the environment or execute commands.
Malformed or conflicting headers cause refusal. Environment overrides and terminal commands remain unsupported.
Without an active hook, the header is only a shell comment and supplies no execution context.
`login=false` uses `-c`. `login=true` uses `-lc`.
For a directory change alone, `cd /absolute/project/nested && pwd` retains the default shell and login mode.

## Limits to report

The launcher inherits Codex's shell setting, `features.code_mode_host`.
Codex CLI 0.159.2 enables the shell by default.
The launcher checks that commands in the Codex sandbox can reach APPA before it starts Codex.
The installed `appa` profile allows access to every port on `127.0.0.1`.
A profile that blocks this connection fails the launch check.

Each APPA hook runs a policy check.
The hook supervisor blocks the call if that check crashes, times out, or returns invalid JSON.
If the user does not trust the APPA hooks through `/hooks`, Codex can skip APPA checks and run commands without APPA protection.
If the supervisor fails before it replies, Codex can also run the original command without an APPA check.
The launcher cannot check whether the user trusts the hooks.

Some native tool checks happen before a hook.
MCP tool errors can skip the hook that checks the result.
APPA wrapped commands cannot receive later input because the wrapper closes child stdin and does not forward input.
Codex supports later input through `write_stdin` when an unwrapped execution session keeps stdin open.
Interactive terminal programs remain unsupported.
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
