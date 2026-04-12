# Aegis

> A shell proxy that intercepts AI agent commands and requires human confirmation before destructive operations.

[![CI](https://github.com/IliasAlmerekov/aegis/actions/workflows/ci.yml/badge.svg)](https://github.com/IliasAlmerekov/aegis/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-linux%20%7C%20macos-lightgrey)](#install)

Safe commands pass through instantly (< 2 ms). Dangerous ones — `rm -rf`, `terraform destroy`, `DROP TABLE`, and 60+ more — trigger a confirmation prompt before anything runs.

---

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/install.sh | sh
```

Or from source:

```bash
cargo install --git https://github.com/IliasAlmerekov/aegis aegis
```

Uninstall:

```bash
curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/uninstall.sh | sh
```

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
Full pattern table: [AEGIS.md](AEGIS.md).

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
