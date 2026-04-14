# Aegis

> A shell proxy that intercepts AI agent commands and requires human confirmation before destructive operations.

[![CI](https://github.com/IliasAlmerekov/aegis/actions/workflows/ci.yml/badge.svg)](https://github.com/IliasAlmerekov/aegis/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-linux%20%7C%20macos-lightgrey)](#install)

Safe commands pass through instantly (< 2 ms). Dangerous ones — `rm -rf`, `terraform destroy`, `DROP TABLE`, and 60+ more — trigger a confirmation prompt before anything runs.

---

## Install

### Recommended: verification-first install

Download the binary and matching checksum for your target, verify them, then install the verified binary:

```bash
curl -fsSLO https://github.com/IliasAlmerekov/aegis/releases/latest/download/aegis-linux-x86_64
curl -fsSLO https://github.com/IliasAlmerekov/aegis/releases/latest/download/aegis-linux-x86_64.sha256

# Linux
sha256sum -c aegis-linux-x86_64.sha256

# macOS
expected="$(awk '{print $1}' aegis-linux-x86_64.sha256)"
actual="$(shasum -a 256 aegis-linux-x86_64 | awk '{print $1}')"
[ "$expected" = "$actual" ]

# Installing into /usr/local/bin may require sudo.
install -m 0755 aegis-linux-x86_64 /usr/local/bin/aegis
```

Replace `aegis-linux-x86_64` with the release asset for your platform.
Manual install only places the binary. You still need to configure your shell to use Aegis; see [Track all agent commands (global setup)](#track-all-agent-commands-global-setup) for details. The quick installer handles that managed shell setup for you.

### Quick install

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh
```

The installer downloads the selected binary and its matching `.sha256`, verifies the checksum before installation, and fails closed on:
- missing checksum
- checksum mismatch
- missing supported checksum verifier tool

### Source install

From source:

```bash
cargo install --git https://github.com/IliasAlmerekov/aegis aegis
```

### Uninstall

For quick or script-managed installs:

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/uninstall.sh | sh
```

For manual verification-first binary installs:

- remove the `aegis` binary you copied into your PATH
- manually undo any shell configuration you added yourself

For source installs:

```bash
cargo uninstall aegis
```

Then manually undo any shell or agent configuration you added yourself, if applicable.

---

## How it works

Aegis sets itself as your `$SHELL`. Every command — from Claude Code, Codex, a script, or your terminal — passes through Aegis first:

```
agent → $SHELL (aegis) → assess
                           ├── Safe   → exec immediately
                           ├── Warn   → confirm (default Yes)
                           ├── Danger → confirm (default No)
                           └── Block  → refuse, exit 3
```

---

## Security model

Aegis is a **heuristic command guardrail** for common destructive shell commands.

- It is **not a sandbox**
- It is **not a complete security boundary**
- Approved commands run with **your normal user permissions**
- Detection is based on the **raw command text**, not full shell execution tracing
- Snapshots and rollback are **best-effort** when configured

Aegis is designed to reduce accidental damage from direct, recognisable commands. It should not be treated as protection against a determined bypass.

---

## Limitations

Aegis may not catch:

- obfuscated or encoded shell input
- runtime-assembled commands such as `eval "$(some_function)"`
- indirect execution where one command writes a script and a later command runs it
- alias/function expansion that changes what a command does after parsing

It also does not:

- restrict filesystem, network, or syscall access after you approve a command
- guarantee lossless snapshots or perfect rollback fidelity
- support Windows; current support is Linux and macOS only

---

## Track all agent commands (global setup)

The installer automatically sets `$SHELL` to the Aegis binary and adds a managed block to your `~/.bashrc` / `~/.zshrc`. Open a new terminal and Aegis is active.

**Claude Code** — set the shell path explicitly:

1. Open Claude Code settings
2. Set the shell field to `$(which aegis)`

**Other AI agents** that respect `$SHELL` (Codex CLI, etc.) pick it up automatically.

To use Aegis only for a single project, add a `.aegis.toml` to the project root with the desired policy:

```toml
mode = "Strict"  # block non-safe commands in this directory only
```

---

## Quick verification

```bash
# Should show a confirmation prompt — type n to deny
aegis -c 'rm -rf /tmp/aegis-test'

# Should pass through instantly with no prompt
aegis -c 'echo hello'
```

---

## Key commands

```bash
# Policy evaluation (no execution, no audit entry)
aegis -c '<cmd>' --output json

# Config
aegis config init   # write .aegis.toml in the current directory
aegis config show   # print merged active config

# Audit log
aegis audit --last 20         # last 20 entries
aegis audit --risk Danger     # filter by risk level
aegis audit --format json     # export as JSON

# Snapshots
aegis rollback '<snapshot-id>'  # restore a snapshot taken before a Danger command
```

---

## Custom patterns

Add your own rules to `~/.config/aegis/config.toml` or `.aegis.toml`:

```toml
[[custom_patterns]]
id          = "USR-001"
category    = "Cloud"
risk        = "Danger"
pattern     = "my-nuke-script\\.sh"
description = "Internal teardown script"
safe_alt    = "my-nuke-script.sh --dry-run"
```

Patterns are Rust regex strings. Use `(?i)` for case-insensitive matching.

---

## Built-in pattern categories

60 patterns across 7 categories: **Filesystem**, **Git**, **Database**, **Cloud**, **Docker**, **Process**, **Package**.

---

## Docs

- [Config schema](docs/config-schema.md) — modes, allowlists, snapshot policy, `--output json`
- [Platform support](docs/platform-support.md) — Linux and macOS only; Windows is out of scope
- [CI and release](docs/ci.md) — workflow guarantees and pinned tool versions

---

## Contributing

Bug reports and pull requests are welcome. Open an issue before starting large changes.  
See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## License

MIT — see [LICENSE](LICENSE).
