# Platform support

## Support matrix

| Platform | Status | Shell / process model | Notes |
| --- | --- | --- | --- |
| Linux | Supported | POSIX-style shell execution via `bash` / `zsh` / `/bin/sh` fallback | Primary target for install, shell wrapping, and test coverage. |
| macOS | Supported | POSIX-style shell execution via `bash` / `zsh` / `/bin/sh` fallback | Supported with the same Unix-like shell assumptions as Linux. |
| Windows | Not supported | `PowerShell` and `cmd.exe` are out of scope | Deferred until Aegis has a dedicated Windows interception design. |

## Current strategy

Aegis officially supports **Unix-like systems only** today.

That means the supported runtime boundary is:

- POSIX-style shell invocation
- `SHELL`-based wrapper setup
- `AEGIS_REAL_SHELL` recursion protection
- Unix-like path and process semantics

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
