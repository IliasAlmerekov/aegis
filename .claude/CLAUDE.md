# CLAUDE.md — Aegis Development Conventions

## Agent Configuration

Before starting any non-trivial task, read `.claude/AGENTS.md` — it defines the lead
orchestrator identity, sub-agent registry, orchestration rules, human checkpoints, and
global constraints that apply to all agents working in this repository.

---

## Project Conventions

**Always follow `CONVENTION.md`** — it is the authoritative project-level contract for
code style, architecture, security invariants, dependency rules, testing requirements,
and release gates. When `CONVENTION.md` conflicts with any other document, use the
precedence order defined within it (security invariants → CI-enforced rules → CONVENTION.md → contributor guidance).

---

## Shell Commands

**Always prefix all shell commands with `rtk`** to reduce noise in the context window:

```bash
rtk cargo build
rtk cargo test
rtk git status
rtk cargo clippy
```

Never run bare `cargo`, `git`, `rustc`, or other CLI tools — always `rtk <command>`.

Denied Aegis decisions must be respected; do not propose out-of-band bypass
instructions or shell-escape workarounds for blocked risky commands.

---

## Commit Style

Use short conventional commits. **Never** add `Co-Authored-By` trailers.

---

## Project Overview

Aegis is a lightweight Rust CLI that acts as a `$SHELL` proxy, intercepting AI agent commands and requiring human confirmation before destructive operations. It must be fast (< 2ms for safe paths), correct, and minimal.

---

## Rust Edition & Toolchain

- Edition: **2024** (as set in `Cargo.toml`)
- MSRV: track latest stable
- Format with `rustfmt` (default settings, no overrides unless justified)
- Lint with `clippy` — all warnings must be resolved before merge

---

## Crate Conventions

### Approved dependencies (from architecture spec)

| Purpose                 | Crate                            |
| ----------------------- | -------------------------------- |
| CLI                     | `clap 4.5` with derive API       |
| Terminal UI             | `crossterm 0.28`                 |
| Fast multi-pattern scan | `aho-corasick 1.1`               |
| Full regex scan         | `regex 1.11`                     |
| Config                  | `serde` + `toml 0.8`             |
| Error types (lib)       | `thiserror 1`                    |
| Error propagation (bin) | `anyhow 1`                       |
| Async subprocess        | `tokio` (features: process, fs)  |
| Async trait methods     | `async-trait 0.1`                |
| Structured logging      | `tracing` + `tracing-subscriber` |
| Benchmarks              | `criterion 0.5`                  |

Do not add new dependencies without a clear justification. Prefer stdlib when sufficient.

**Explicitly prohibited dependencies:**

- `once_cell` — superseded by `std::sync::LazyLock` (stable since Rust 1.80). Do not add it.

---

## Module Structure

Follow the architecture defined in `AEGIS.md` exactly:

```
src/
  main.rs              # CLI entry point only — no business logic
  error.rs             # AegisError via thiserror
  interceptor/
    mod.rs             # public API: Scanner
    scanner.rs         # assess(cmd) -> RiskLevel
    parser.rs          # tokenizer, heredoc, inline scripts
    patterns.rs        # Pattern struct, Category, loading
  snapshot/
    mod.rs             # trait SnapshotPlugin + Registry
    git.rs             # GitPlugin
    docker.rs          # DockerPlugin
  ui/
    confirm.rs         # crossterm TUI dialog
  audit/
    logger.rs          # append-only JSONL
  config/
    model.rs           # AegisConfig (serde + TOML)
```

`main.rs` must stay thin: parse args, wire up components, call into modules.

---

## Error Handling

- In library code (`interceptor/`, `snapshot/`, `audit/`, `config/`): use typed errors via `thiserror`. Every error variant must be explicit — no `anyhow` in lib modules.
- In `main.rs` and CLI glue code: use `anyhow` for easy propagation.
- Never use `.unwrap()` or `.expect()` in production paths. Use `?` or handle explicitly.
- `.expect()` is acceptable only in tests and in startup initialization where a panic is the correct behavior (e.g., "config file is malformed on startup").

---

## Naming Conventions

Follow standard Rust naming:

- Types, traits, enums: `PascalCase` — `RiskLevel`, `SnapshotPlugin`, `AuditEntry`
- Functions, methods, variables, modules: `snake_case` — `assess`, `is_applicable`, `audit_logger`
- Constants: `SCREAMING_SNAKE_CASE` — `MAX_COMMAND_LEN`
- Enum variants: `PascalCase` — `RiskLevel::Danger`, `Decision::Approved`
- Pattern IDs in data: uppercase string literals — `"FS-001"`, `"GIT-003"`

---

## Key Types — Do Not Deviate

These types are defined by the architecture. Implement them as specified:

```rust
// #[non_exhaustive] ensures external match arms require a wildcard —
// forward-compatible when new risk levels are added in v2.
#[non_exhaustive]
pub enum RiskLevel { Safe, Warn, Danger, Block }

// Built-in patterns compiled into the binary — use &'static str (zero-copy).
pub struct BuiltinPattern {
    pub id:          &'static str,
    pub category:    Category,
    pub risk:        RiskLevel,
    pub pattern:     &'static str,
    pub description: &'static str,
    pub safe_alt:    Option<&'static str>,
}

// User-defined patterns from aegis.toml — use owned String (runtime strings
// cannot be &'static).
#[derive(Deserialize)]
pub struct UserPattern {
    pub id:      String,
    pub pattern: String,
    pub risk:    RiskLevel,
}

// Unified runtime type — Cow<'static, str> avoids cloning built-in strings
// while still accepting owned strings from user config.
pub struct Pattern {
    pub id:          Cow<'static, str>,
    pub category:    Category,
    pub risk:        RiskLevel,
    pub pattern:     Cow<'static, str>,
    pub description: Cow<'static, str>,
    pub safe_alt:    Option<Cow<'static, str>>,
}

pub struct Assessment {
    pub risk:    RiskLevel,
    pub matched: Vec<Arc<Pattern>>,
    pub command: ParsedCommand,
}

// async fn in dyn Trait is not object-safe (returns impl Future, which is
// generic). Use async-trait to generate a BoxFuture vtable-compatible wrapper.
#[async_trait]
pub trait SnapshotPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn is_applicable(&self, cwd: &Path) -> bool;
    async fn snapshot(&self, cwd: &Path, cmd: &str) -> Result<String>;
    async fn rollback(&self, snapshot_id: &str) -> Result<()>;
}
```

---

## Performance Rules

The hot path (safe commands) must stay under 2ms. Keep this in mind:

- The Aho-Corasick scan is the fast first pass — keep it cheap, no allocations.
- Regex patterns are compiled once at first use via `std::sync::LazyLock` (stdlib, no external crate needed):

```rust
// Correct — LazyLock is stable since Rust 1.80
use std::sync::LazyLock;
static PATTERN_DB_001: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)DROP\s+(TABLE|DATABASE)").unwrap());
```

- Do not use `once_cell` — it is superseded by stdlib.
- Avoid cloning strings in the scanner hot path — use `&str` and slices.
- Benchmark any change to `scanner.rs` or `parser.rs` with `rtk cargo criterion`.

---

## Testing

### Unit tests

- Place in `#[cfg(test)]` modules within each source file.
- Test each `Pattern` match/non-match in `patterns.rs`.
- Test `parser.rs` edge cases: heredoc, inline scripts, escaped quotes, pipes.

### Integration tests

- Live in `tests/integration/`.
- `end_to_end.rs`: full pipeline from raw command string to `Assessment`.
- `shell_wrapper.rs`: verify correct stdin/stdout/stderr/exit code passthrough.
- Test fixtures in `tests/fixtures/commands.toml` (70 test cases minimum for v1).

### Benchmarks

- Live in `benches/scanner_bench.rs`.
- Measure `assess()` on safe, warn, and danger commands separately.
- Run with: `rtk cargo criterion`

### Coverage target

- All `RiskLevel` variants must have both positive and negative test cases.
- Every `Pattern` must have at least one test asserting it fires correctly.

### Fuzz testing

The command parser (`parser.rs`) handles heredoc, inline scripts, and shell escaping — this is security-critical input parsing. Fuzzing is **required**, not optional.

```toml
# fuzz/Cargo.toml
[dependencies]
libfuzzer-sys = "0.4"

[[bin]]
name = "fuzz_scanner"
path = "fuzz_targets/scanner.rs"
```

```bash
rtk cargo +nightly fuzz run fuzz_scanner
```

Add fuzz targets for: `parser::parse`, `scanner::assess`, heredoc unwrapping.

---

## Security Auditing

Aegis is a security tool — its own dependency chain must be clean.

### cargo-audit (CVE scanning)

```bash
rtk cargo audit
```

Run on every CI push. A build with known CVEs in dependencies does not ship.

### cargo-deny (license + duplicate policy)

```bash
rtk cargo deny check
```

Enforces: no duplicate versions of core crates, only permissive licenses (MIT/Apache-2.0/ISC), no banned crates.

Both tools must pass in CI before merge. See `.github/workflows/ci.yml`.

---

## Configuration Format

User config is TOML (`aegis.toml` global, `.aegis.toml` per-project). When adding config fields:

- All fields must be optional with sensible defaults via `#[serde(default)]`.
- Document each field with an inline comment in the example config.
- Never break backwards compatibility with existing config files.

---

## Audit Log Format

Audit log is append-only JSONL at `~/.aegis/audit.jsonl`. Each line is one `AuditEntry` serialized as JSON. Never rewrite the file — only append. The format is part of the public contract from v1.

---

## Commit Style

Use conventional commits:

```
feat: add PostgreSQL snapshot plugin
fix: handle heredoc with embedded single quotes
perf: eliminate allocation in Aho-Corasick hot path
test: add 15 fixture cases for cloud patterns
docs: update pattern table in AEGIS.md
```

Scope is optional. Keep the subject line under 72 characters. Body explains _why_, not _what_.

---

## What Not to Do

- Do not add business logic to `main.rs`.
- Do not use `regex` in the quick scan (first pass) — only Aho-Corasick.
- Do not block the main thread during subprocess calls — use `tokio`.
- Do not write to stdout in library modules — use `tracing` events.
- Do not add dependencies that bring in C build steps (keep the binary portable).
- Do not change the `RiskLevel` order — it is semantically ordered by severity.
- Do not use `once_cell` — use `std::sync::LazyLock` (stable since Rust 1.80).
- Do not write `async fn` in a trait without `#[async_trait]` — it will not compile as `dyn Trait`.
- Do not use `&'static str` for user-provided config values — use `Cow<'static, str>` or `String`.
