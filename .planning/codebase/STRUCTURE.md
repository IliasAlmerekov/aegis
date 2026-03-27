# STRUCTURE.md — Directory Layout & Organization

## Root Layout

```
/home/iliasalmerekov/Projects/aegis/
├── src/                   # All Rust source code
├── tests/                 # Integration tests
├── benches/               # Criterion benchmarks
├── config/                # Bundled runtime data (patterns.toml)
├── fuzz/                  # (planned) fuzz targets — not yet present
├── docs/                  # Architecture decisions + codex ticket artifacts
├── .claude/               # Claude Code agent config + GSD workflow
├── Cargo.toml             # Workspace manifest
├── deny.toml              # cargo-deny license + duplicate policy
├── CONVENTION.md          # Authoritative code style contract
├── CLAUDE.md              # Claude Code dev instructions
├── AGENTS.md              # Multi-agent orchestration rules
├── AEGIS.md               # (referenced) architecture spec
├── ROADMAP.md             # Phase roadmap
├── TODO.md                # Production-readiness backlog (in Russian)
└── README.md              # Project overview
```

## Source Tree (`src/`)

```
src/
├── main.rs                  # CLI entry point: clap parsing, subcommand dispatch, exec_command
├── lib.rs                   # Public crate API re-exports
├── error.rs                 # AegisError (thiserror) — typed error enum for all lib modules
├── runtime.rs               # RuntimeContext — wires scanner + config + audit + snapshots
├── interceptor/
│   ├── mod.rs               # Public API: RiskLevel enum, re-exports Scanner
│   ├── scanner.rs           # assess(cmd) → Assessment; Aho-Corasick quick pass + regex full scan
│   ├── parser.rs            # Parser::parse(cmd) → ParsedCommand; shell tokenizer + heredoc
│   └── patterns.rs          # Pattern, PatternSet, Category; TOML loading + builtin/custom merge
├── snapshot/
│   ├── mod.rs               # SnapshotPlugin trait + registry; SnapshotRecord type
│   ├── git.rs               # GitPlugin: git stash before danger commands
│   └── docker.rs            # DockerPlugin: docker commit before danger commands
├── audit/
│   ├── mod.rs               # Re-exports AuditLogger, AuditEntry, Decision
│   └── logger.rs            # Append-only JSONL audit log at ~/.aegis/audit.jsonl
├── config/
│   ├── mod.rs               # Re-exports Config, AllowlistMatch
│   ├── model.rs             # AegisConfig (serde + TOML); Mode, CiPolicy enums
│   └── allowlist.rs         # AllowlistEntry matching logic
└── ui/
    ├── mod.rs               # Re-exports show_confirmation
    └── confirm.rs           # crossterm TUI dialog: Approve / Deny / Block display
```

## Test Layout (`tests/`)

```
tests/
├── full_pipeline.rs         # End-to-end CLI binary tests (subprocess-based)
└── docker_integration.rs    # Docker snapshot integration tests
```

Inline unit tests live in `#[cfg(test)]` modules within each source file.

## Key Data Files

- `config/patterns.toml` — built-in detection patterns (7 categories, loaded at runtime via `include_str!` or file read at startup)
- `deny.toml` — cargo-deny policy: permissive licenses (MIT/Apache-2.0/ISC), no duplicates, banned crates list

## Naming Conventions

| Scope | Convention | Example |
|---|---|---|
| Files/modules | `snake_case` | `scanner.rs`, `audit_logger` |
| Types, traits, enums | `PascalCase` | `RiskLevel`, `SnapshotPlugin` |
| Functions, methods, vars | `snake_case` | `assess`, `is_applicable` |
| Constants | `SCREAMING_SNAKE_CASE` | `EXIT_DENIED`, `MAX_COMMAND_LEN` |
| Enum variants | `PascalCase` | `RiskLevel::Danger`, `Decision::Approved` |
| Pattern IDs (data) | uppercase string literals | `"FS-001"`, `"GIT-003"` |

## Config Search Order

1. `.aegis.toml` — project-local (current directory)
2. `~/.config/aegis/config.toml` — global user config
3. Built-in defaults (all fields have `#[serde(default)]`)

## Audit Log Location

`~/.aegis/audit.jsonl` — append-only JSONL, one `AuditEntry` per line.
