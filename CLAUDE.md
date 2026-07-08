# CLAUDE.md — Aegis Development Conventions

## What this project is

Aegis is a lightweight Rust CLI that acts as a `$SHELL` proxy for AI coding
agents (Claude Code, Codex). It intercepts every command an agent tries to
run, classifies it (`Safe` / `Warn` / `Danger` / `Block`), and requires human
confirmation before anything destructive executes. It is a heuristic
guardrail, not a sandbox — see
`docs/adr/adr-003-aegis-is-a-heuristic-guardrail-not-a-sandbox.md`. It must
stay fast (< 2 ms on the safe-command hot path), correct, and minimal.

The binary lives at the workspace root; the actual logic is split across
focused library crates under `crates/` (parser, scanner, policy engine,
config, snapshot backends, audit log, TUI). See `ARCHITECTURE.md` for the
current structural contract and `docs/adr/` for why it looks this way.

Read `PROJECT_STATE.md` at the start of any non-trivial task to see what
changed last session and what is currently open.

---

## Before writing any code

**Always invoke the `rust-best-practices` skill before writing or reviewing
Rust code in this repo** (`Skill({skill: "rust-best-practices"})`, backed by
`~/agents/skills/rust-best-practies/SKILL.md`). It encodes the idiomatic-Rust
guidance this project expects (ownership/borrowing, error handling, testing
style). Apply it on top of — never instead of — `CONVENTION.md`, which is
authoritative for this project's specific architecture, security invariants,
and release gates. Use the `tdd` skill for red-green-refactor work on
security-sensitive parser/scanner/policy code.

---

## Project Conventions

**Always follow `CONVENTION.md`** — it is the authoritative project-level contract for
code style, architecture, security invariants, dependency rules, testing requirements,
and release gates. When `CONVENTION.md` conflicts with any other document, use the
precedence order defined within it (security invariants → CI-enforced rules → CONVENTION.md → contributor guidance).

---

## Ubiquitous Language

**`CONTEXT.md` (repo root) is the project's domain glossary — the single source of truth
for terminology shared by humans, agents, and code.** Before naming a type, field,
config key, audit field, or before describing a concept in a PR or commit, use the exact
canonical term from `CONTEXT.md` and avoid the words listed under each term's `_Avoid_`.

- When a task introduces or sharpens a domain term, update `CONTEXT.md` in the same
  change (via the `domain-modeling` skill). Do not batch glossary updates.
- If a requirement uses a word that conflicts with the glossary (e.g. "block" — the
  `RiskLevel`, a blocklist entry, or a `PolicyRuleDecision`?), resolve the ambiguity
  against `CONTEXT.md` before writing code.
- `CONTEXT.md` holds glossary entries only — no implementation details, specs, or notes.

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

## Session Context

**At the start of every session:** read `PROJECT_STATE.md` to understand what was done
before and where the project stands. Do not skip this step on non-trivial tasks.

**After finishing a task — verify, then document.** Do not fill in these files
before the task is actually done and verified. Order matters:

1. Finish the change.
2. Verify it: `rtk cargo test --workspace`, `rtk cargo clippy -- -D warnings`,
   `rtk cargo fmt --check`, plus a benchmark run if the hot path was touched.
   Only proceed once this is green.
3. Only then, in the same change, update `PROJECT_STATE.md`:
   - Update "Last updated" date.
   - Replace the "Last session" section with a concise summary of what
     changed and what was verified.
   - Update "Milestone status" rows whose status changed.
   - Update "Open decisions / blockers" if any were resolved or new ones surfaced.
4. And `TASKS.md`: flip the relevant `[ ]` to `[x]` if the task closes a
   tracked finding.

Never mark a task done in `PROJECT_STATE.md`/`TASKS.md` before verification
actually passed.

---

## Changelog Maintenance

After every feature, fix, or breaking change **that has passed verification**,
prepend an entry under `## [Unreleased]` in `CHANGELOG.md`:

- Use Keep a Changelog categories: `Added`, `Changed`, `Fixed`, `Removed`, `Security`.
- One line per change; reference the milestone (e.g. `M5.4`) or ADR (e.g. `ADR-011`)
  when applicable.
- When cutting a release, rename `[Unreleased]` to the version and date, then add a fresh
  empty `[Unreleased]` block above it.

---

## Architecture Decision Records

When making a significant architectural decision — new crate, change to a public API,
new plugin, performance trade-off, security model change, or intentional non-goal —
write an ADR in `docs/adr/`:

- Number sequentially: check existing files (`rtk git ls-files docs/adr/`) for the next free number.
- Filename: `adr-NNN-short-slug.md`.
- Required sections: **Status** (Accepted / Proposed / Deprecated), **Context**,
  **Decision**, **Consequences**.
- Keep it short — one page max.
- Update `docs/adr/README.md` index after adding a new ADR.

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
