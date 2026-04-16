# Platform support

## Support matrix

| Platform | Status | Shell / process model | Notes |
| --- | --- | --- | --- |
| Linux | Supported | POSIX-style shell execution via `bash` / `zsh` / `/bin/sh` fallback | Primary target for install, shell wrapping, and test coverage. |
| macOS | Supported | POSIX-style shell execution via `bash` / `zsh` / `/bin/sh` fallback | Supported with the same Unix-like shell assumptions as Linux. |
| Windows host via WSL2 terminal | Best-effort / not separately validated | Linux userspace and POSIX-style shell execution inside WSL2 | Treated as a Linux environment for terminal usage, but not yet backed by dedicated WSL CI/smoke validation. |
| Windows | Not supported | `PowerShell` and `cmd.exe` are out of scope | Deferred until Aegis has a dedicated Windows interception design. |

## Current strategy

Aegis officially supports **Unix-like systems only** today.

That includes Linux and macOS directly, and can include **WSL2 terminal usage**
when Aegis runs inside the Linux environment provided by WSL2, but that path
is not separately validated yet.

That means the supported runtime boundary is:

- POSIX-style shell invocation
- `SHELL`-based wrapper setup
- `AEGIS_REAL_SHELL` recursion protection
- Unix-like path and process semantics

## WSL2 guidance

If you use Windows, the best-effort path is to run Aegis **inside a WSL2 Linux
terminal**, where it uses the same Unix-like shell and process assumptions as
on Linux.

Current WSL2 position:

- Windows host via WSL2 terminal: best-effort Linux-like environment
- native Windows shells (`PowerShell`, `cmd.exe`): unsupported
- WSL2 support is not yet backed by dedicated CI or explicit smoke coverage

## Unsupported Windows strategy

Windows is intentionally out of scope for the current release line.

The project does **not** currently support:

- `PowerShell` command semantics
- `cmd.exe` quoting / escaping semantics
- Windows-specific path handling
- Windows process / shell-wrapper behavior

The installer rejects Windows explicitly instead of pretending support exists.

## Why Windows is deferred

A safe Windows implementation needs a separate design for:

- `PowerShell` parsing and execution semantics
- `cmd.exe` process model and quoting rules
- path normalization across drive letters and backslashes
- recursion-safe shell-wrapper installation on Windows

Until that exists, the support policy remains explicit Unix-only.
