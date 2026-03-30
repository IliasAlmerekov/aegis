# CONVENTIONS.md — Code Style & Patterns

## Language & Edition

- Rust Edition **2024**
- Format: `rustfmt` with default settings (no overrides unless justified)
- Lint: `clippy` — all warnings resolved before merge

## Error Handling

### Library modules (`interceptor/`, `snapshot/`, `audit/`, `config/`)

- Use typed errors via `thiserror`; every variant must be explicit
- No `anyhow` in lib modules
- Use `?` for propagation; never `.unwrap()` in production paths
- `.expect()` only in tests and startup initialization where a panic is correct

```rust
// Correct — typed error in lib
#[derive(thiserror::Error, Debug)]
pub enum AegisError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
```

### CLI glue (`main.rs`)

- Use `anyhow` for easy propagation

## Lazy Statics / Global State

Use `std::sync::LazyLock` (stable since Rust 1.80). **Never** use `once_cell`.

```rust
use std::sync::LazyLock;
static PATTERN_DB_001: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)DROP\s+(TABLE|DATABASE)").unwrap());
```

## Async Code

- Use `tokio` for async subprocess and fs operations (no blocking on main thread)
- Async trait methods require `#[async_trait]` — raw `async fn` in `dyn Trait` is not object-safe

```rust
#[async_trait]
pub trait SnapshotPlugin: Send + Sync {
    async fn snapshot(&self, cwd: &Path, cmd: &str) -> Result<String>;
}
```

## String Types

- Built-in patterns (compile-time): `&'static str` — zero-copy
- User-provided config values: `Cow<'static, str>` or `String` — never `&'static str`
- Unified runtime `Pattern` struct uses `Cow<'static, str>` for both without cloning builtins

```rust
pub struct Pattern {
    pub id:      Cow<'static, str>,
    pub pattern: Cow<'static, str>,
    // ...
}
```

## Performance — Hot Path Rules

The safe-path assessment must stay under **2ms**:

- Aho-Corasick first pass: no allocations, keyword-only scan
- Regex patterns: compiled once at `Scanner::new()`, reused on every call
- Do NOT use `regex` in the quick scan — Aho-Corasick only
- Avoid cloning strings in `scanner.rs` and `parser.rs`

## Key Types — Do Not Deviate

```rust
#[non_exhaustive]
pub enum RiskLevel { Safe, Warn, Danger, Block }

pub struct Assessment {
    pub risk:    RiskLevel,
    pub matched: Vec<MatchResult>,
    pub command: ParsedCommand,
}
```

`#[non_exhaustive]` on `RiskLevel` ensures external callers need a wildcard arm — forward-compatible when new variants are added.

## Dependency Policy

- Add no new dependencies without clear justification
- Prefer stdlib when sufficient
- Prohibited: `once_cell` (superseded by `std::sync::LazyLock`)
- No C build-step deps (keep binary portable)
- No business logic in `main.rs` — it is a thin dispatch layer
- No `stdout` writes in library modules — use `tracing` events

## Documentation & Comments

- Public items get doc-comments explaining the _why_, not just the _what_
- Inline comments for non-obvious logic (see `decide_command` in `main.rs` for the CI fast-path block)
- `// ── Section Name ──` banners used to group related logic within long files

## Commit Style

Conventional commits, subject under 72 chars, no Co-Authored-By trailers:

```
feat: add PostgreSQL snapshot plugin
fix: handle heredoc with embedded single quotes
perf: eliminate allocation in Aho-Corasick hot path
test: add 15 fixture cases for cloud patterns
```

## Module Architecture Rule

`main.rs` must stay thin: parse args → wire components → call into modules. No business logic there.
