# Codex host-mode compatibility

The protected launch gate fails on Codex CLI 0.159.2 on macOS 26.6.2 arm64.
The tested binary SHA-256 is `16593cc2f422d5f398a8e40f550ebbaf1245392528957be342c295920a300704`.
The probes ran on 30 September 2026 with temporary projects and synthetic data.
The model probes used the saved login, isolated user configuration, and temporary hook definitions.

## Tool routes

The tool inventory contained these code-mode host tools:

```text
apply_patch, clock__curr_time, create_goal, exec_command, get_goal,
image_gen__imagegen, update_goal, view_image, web__run, write_stdin
```

The temporary profile disabled apps, browser use, and multiple agents.
It enabled `code_mode_host` and `code_mode` for route tests.
The second flag exposed the programmatic tool interface.
No external MCP server existed in this temporary profile.

| Route | Synchronous hook result | Gate result |
| --- | --- | --- |
| Shell through `exec_command` | `PreToolUse` and `PostToolUse` reported `Bash`. | The shell returned a synthetic marker. |
| Local patch through `apply_patch` | Both hooks reported `apply_patch`. | The patch created a temporary marker. |
| Local image through `view_image` | Both hooks reported `view_image`. | A valid temporary PNG produced a result. |
| Web fetch through `web__run` | Both hooks reported `webrun`. | The call read `https://example.com`. |
| Image generation through `image_gen__imagegen` | Both hooks reported `image_genimagegen`. | The call returned an image. |
| Later input through `write_stdin` | No new pre-use hook ran. The parent `Bash` call had a pre-use and post-use hook. | An unwrapped shell accepted a later synthetic command. |

The APPA proxy starts its child with null stdin. It does not forward later input.
That limit prevents this specific late-input path after a successful Bash rewrite.
It does not repair a skipped or failed pre-use hook.

With `code_mode_host=false`, a shell request failed with `code-mode host is disabled`.
No tool hook ran for that request.
The result matched the Phase 7 observation on Codex CLI 0.159.0.

## Hook failure result

A valid pre-use denial blocked synthetic Bash and patch writes.
A crashed, malformed, timed-out, or untrusted pre-use hook did not block the synthetic Bash write.
Codex ran the original command in each of those four cases.
The [official hook documentation](https://learn.chatgpt.com/docs/hooks) describes non-managed hook trust and tool coverage.

`appa codex --` inherits the user's `code_mode_host` setting after its sandbox check.
The launcher enables hooks because APPA needs them for policy checks.
Codex CLI 0.159.2 enables `code_mode_host` by default.
If the user disables it, the shell is unavailable in that session.
Each installed hook runs through `appa codex-hook` before it calls the APPA hook worker.
The supervisor gives the worker less time than Codex gives the hook.
It returns a valid Codex denial if the worker exits with an error, times out, or returns invalid JSON.
For Bash, it accepts only an explicit denial or a nonempty command wrapper.
The supervisor cannot protect a hook that Codex skips because it lacks trust.
It cannot answer if its own process fails to start or stops before it writes a response.
These paths still let Codex run an original command.
No manually trusted protected session test ran in this probe.

A disposable live Codex session also tested the installed supervisor command.
The probe used a temporary hook and bypassed trust for that temporary hook only.
The APPA worker could not reach its test endpoint, so the supervisor returned a `PreToolUse` denial.
Codex blocked the shell request, and no marker file appeared.
This probe does not establish trust for the installed hooks.

## Sandbox HTTP result

The `appa` profile extended `:workspace` and allowed `127.0.0.1` through Codex's proxy.
The sandbox HTTP handshake returned `ok` with that profile.
A network-off profile refused the connection.
A profile that denied `127.0.0.1` returned HTTP 403.
The launcher includes managed configuration in its sandbox check.
These checks do not prove support under every managed policy.

## Repeat the checks

1. Build `appa`.
2. Run the route probe in a disposable project.
3. Run the hook failure probe.
4. Run the sandbox HTTP probe.

```sh
cargo build -p appa
python3 integrations/codex/host_mode_probe.py
python3 integrations/codex/hook_failure_probe.py
python3 integrations/codex/sandbox_http_probe.py
```

The route and failure probes make authenticated model calls.
They use temporary hooks and a trust bypass only for those temporary definitions.
They do not change the user's Codex profile or prove manual hook trust.
