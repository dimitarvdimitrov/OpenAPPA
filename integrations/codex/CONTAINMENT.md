# Codex descendant containment

The proxy needs an inherited process boundary. Command text and output pipes cannot establish that boundary.
A child can change its session, close its pipes, and write files after its parent exits.
Process polling can miss a short-lived parent that forks and exits between samples.

## Implemented Linux restriction

The wrapper creates its shell session, then installs a seccomp filter before the shell executable starts.
The filter denies `setsid`, `setpgid`, namespace changes, `ptrace`, and `process_vm_writev`.
The filter examines the namespace flags for `clone`. It refuses pointer-based `clone3` with `ENOSYS` to permit library fallback.
It also rejects alternate syscall ABIs. The child cannot remove or weaken the inherited filter.
The [kernel reference](https://www.kernel.org/doc/html/latest/userspace-api/seccomp_filter.html) describes filter inheritance and its privilege condition.

The existing `libc` dependency supplies the calls and data structures. No helper or privileged service is required.
`no_new_privs` permits unprivileged filter installation. This choice also prevents privilege elevation through setuid executables or file capabilities.
Namespace tools and debuggers lose those capabilities within commands. Ordinary forks, threads, executable changes, and background work remain available.
The wrapper refuses execution if the kernel cannot install the filter.

This restriction prevents descendants from leaving the cleanup group through the supported syscall ABI.
It is not a namespace boundary. Wrapper death and execution through external service processes remain separate problems.
A private PID namespace provides stronger lifetime enforcement because the kernel terminates its members when namespace PID 1 exits.
A delegated cgroup also provides a kernel membership boundary, but host setup and protection of membership controls are required.

## Reuse of integrations/agentsh

The existing runner obtains descendant teardown from bubblewrap's private PID namespace.
Its `--as-pid-1` and `--die-with-parent` settings support teardown independently of sessions and output descriptors.
Agentsh supplies filesystem and syscall restrictions inside that namespace. Agentsh alone does not establish the namespace lifetime contract.
The [backend contract](../agentsh/README.md) lists its host requirements and tests.

The runner cannot serve arbitrary Codex commands without an architectural change.
It mounts declared file snapshots, clears the environment, disables the network, and imports one output file.
Codex commands use a live workspace and the existing shell environment. They also need access to the runtime through its HTTP exception.
Reuse of the complete runner changes that contract and adds Python, bubblewrap, the patched agentsh build, and kernel feature requirements.

A smaller Linux namespace launcher can reuse the runner's teardown pattern without its file-publication contract.
That change still requires bubblewrap or a native namespace supervisor. Some hosts disable unprivileged user namespaces.
The launcher must refuse execution when the required namespaces are unavailable. Process groups cannot serve as a silent fallback.
No such dependency or platform change is part of this patch.

## Native macOS evidence

A local Seatbelt profile denied direct `setsid()` and `setpgid()` calls without root privilege.
However, `posix_spawn(..., setsid=True)` still created a separate session under that profile.
The [XNU spawn implementation](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_exec.c) calls `setsid_internal` for that spawn attribute.
A syscall-only restriction does not intercept that internal call.

A nested profile also failed inside the actual Codex sandbox with `sandbox-exec: sandbox_apply: Operation not permitted`.
The protected-launcher probe established this compatibility failure. The patch does not add the nested profile.
A native solution needs control of the outer sandbox and all process-creation paths.
A restriction on all `posix_spawn` calls breaks ordinary foreground programs. Library interposition does not cover direct syscalls or protected executables.
Process-tree polling and fork notifications do not provide a kernel lifetime boundary across rapid forks and parent exit.

## Proposed larger change

A managed Linux VM is the practical macOS option for a kernel-enforced lifetime boundary with ordinary child processes.
Apple's [Virtualization framework](https://developer.apple.com/documentation/virtualization) supports Linux guests.
The existing launcher can select the managed guest automatically. Each command can run inside a private PID namespace in that guest.
The proxy can revoke the namespace supervisor after shell exit, cancellation, runtime loss, or a new prompt.
Guest teardown can provide a final boundary if the supervisor fails.

This option adds a guest image, a signed virtualization helper, and a lifecycle protocol for the launcher and proxy.
It also changes paths, workspace access, executable availability, and network behavior. Native macOS executables cannot run as Linux commands.
The design must define those compatibility changes before implementation. It must also prevent commands from access to lifecycle controls.
A Mac guest preserves native executables but adds guest licensing, setup, and resource costs. It needs its own enforced command lifetime design.

The next design decision is whether Linux guest execution is acceptable for protected macOS commands.
If native macOS executables are required, the project needs a separate outer-sandbox integration and a verified restriction on spawn attributes.
The current patch preserves the native macOS limitation. It does not claim that text checks or bounded capture remove it.
