# Set up OpenAPPA with Codex

Use this flow to install OpenAPPA, set up your policy, and try it in a test project.

## Install

1. Install `appa` and Codex CLI on macOS or Linux.
2. Sign in to Codex with your usual account.
3. Run `appa plugin install codex`.

The installer connects Codex to APPA and adds `/appa-guide` to help you set up your policy.
Codex uses its own policy and background runtime, separate from Claude Code.

## Trust the hooks

1. Open Codex.
2. Run `/hooks`.
3. Review and trust the APPA hooks.
4. Exit Codex.

Hooks let Codex ask APPA whether a tool call is allowed.
Codex can skip hooks that you do not trust. If an update changes the hooks, you must trust them again.

## Start a session

1. Open a test project directory.
2. Run `appa codex -- -C .`.
3. Ask Codex to run `printf 'appa-shell-check\n'` with its shell tool.
4. Run `/appa-guide init`.

Before Codex starts, the launcher checks that commands in the Codex sandbox can reach APPA.
If a step fails, record the command and its exact error. Stop before the next step.

The launcher uses your Codex shell setting, `features.code_mode_host`.
Codex CLI 0.159.2 enables the shell by default. If you disable it, the shell tool is unavailable.

## Set up your policy

The guide finds your Codex policy and checks the tools and MCP servers in the session.
It proposes rules or batteries, which are ready-made policies for supported tools.
You review each proposal before the guide changes your policy.

Start a new `appa codex --` session after a policy change.
Use `/appa-guide explain` to understand a blocked call and the available ways to continue.

## Check protection in the test project

1. Check that `/hooks` lists the APPA hooks as trusted.
2. Ask the guide to check that APPA checked the shell command and its output.
3. Ask the guide to propose a temporary rule that blocks a write to a test file.
4. Review the proposal before you approve it.
5. Start a new `appa codex -- -C .` session after the guide applies the rule.
6. Ask Codex to write that test file.
7. Check that Codex reports the denial and that the file does not exist.
8. Ask the guide to remove the temporary rule after your approval.
9. Record the CLI release from `codex --version`, platform, Codex profile, commands, and results.

A printed marker alone does not prove that APPA checked the command.
Do not describe the session as protected until this check passes with hooks that you manually trust through `/hooks`.
If the guide cannot establish that APPA checked a call, record that step as incomplete.

The [tests on Codex CLI 0.159.2](HOST_MODE_GATE.md) used temporary hooks and bypassed their trust check.
They did not test a session with hooks manually trusted through `/hooks`.

## Limits to know

The APPA hook blocks a call if the policy check crashes, times out, or returns invalid JSON.
If Codex skips an untrusted hook, or the hook command itself fails before its reply, Codex can run the original command.

The installed permission profile allows access to every port on `127.0.0.1`.
It does not allow public hosts, and the filesystem sandbox stays active.
Profiles that block this connection cannot start through the launcher.

Commands cannot receive more input after they start. Interactive terminal programs are unsupported.
Native Windows commands do not have a verified protected path.

## Remove

1. Exit Codex.
2. Run `appa plugin remove codex`.

Removal takes back the hooks, MCP server, permission profile, and guide that the installer added.
It keeps your Codex policy and data.

To stop the background runtime, run `appa runtime --adapter codex stop`.
