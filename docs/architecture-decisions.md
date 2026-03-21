# Architecture Decisions

This document records non-obvious design decisions in Aegis — the _why_ behind choices that aren't self-evident from the code.

---

## ADR-001: `std::sync::LazyLock` instead of `once_cell`

**Decision:** Use `std::sync::LazyLock` for lazy-initialized statics (compiled regex patterns, Aho-Corasick automaton).

**Rationale:** `once_cell` was the standard approach before Rust 1.80. Since 1.80, `LazyLock` and `OnceLock` are stable in stdlib. The `once_cell` crate itself recommends migrating to stdlib in its README. Using stdlib eliminates an external dependency with zero trade-off.

```rust
// Correct
use std::sync::LazyLock;
static PATTERN_DB_001: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)DROP\s+(TABLE|DATABASE)").unwrap());
```

**Status:** Enforced. `once_cell` is banned in `deny.toml`.

---

## ADR-002: `async-trait` for `SnapshotPlugin`

**Decision:** Use `#[async_trait]` macro on `SnapshotPlugin` trait rather than hand-rolling `BoxFuture`.

**Rationale:** `async fn` in a trait returns `impl Future`, which is a generic type. Generic types are not object-safe — they cannot be put in a vtable, so `dyn SnapshotPlugin` would not compile. Two solutions exist:

- `async-trait` crate: wraps the return type in `Pin<Box<dyn Future>>` automatically. Readable, minimal boilerplate. Small heap allocation per call.
- Manual `BoxFuture`: zero-overhead but verbose, harder to read and maintain.

The allocation overhead is irrelevant for snapshot operations (called at most once per Danger command, measured in hundreds of milliseconds anyway). Readability wins.

```rust
#[async_trait]
pub trait SnapshotPlugin: Send + Sync {
    async fn snapshot(&self, cwd: &Path, cmd: &str) -> Result<String>;
    async fn rollback(&self, snapshot_id: &str) -> Result<()>;
}
```

**Note:** The hot path (scanner, parser) has no async at all — latency constraints there are met with synchronous code.

**Status:** Enforced.

---

## ADR-003: `Cow<'static, str>` in `Pattern`

**Decision:** The unified `Pattern` type uses `Cow<'static, str>` for string fields instead of `&'static str`.

**Rationale:** Built-in patterns are compiled into the binary as `&'static str` — zero runtime cost. User-defined patterns loaded from `aegis.toml` are heap-allocated `String` values — they cannot be `&'static`.

A single `Pattern` type that covers both cases requires `Cow<'static, str>`:

- `Cow::Borrowed(&'static str)` for built-ins — no allocation, no copy.
- `Cow::Owned(String)` for user patterns — owns the string.

This avoids two separate match arms everywhere patterns are used.

```rust
// Built-in: Cow::Borrowed, zero-copy
pub static PATTERN_FS_001: Pattern = Pattern {
    id: Cow::Borrowed("FS-001"),
    pattern: Cow::Borrowed(r"rm\s+-[^\s]*r[^\s]*\s+/"),
    ..
};

// User-defined: Cow::Owned, from deserialized String
Pattern {
    id: Cow::Owned(user_pattern.id),
    pattern: Cow::Owned(user_pattern.pattern),
    ..
}
```

**Status:** Enforced.

---

## ADR-004: `#[non_exhaustive]` on `RiskLevel`

**Decision:** `RiskLevel` is marked `#[non_exhaustive]`.

**Rationale:** v2 roadmap includes a `Policy DSL` that may introduce new risk levels (e.g., `Audit` — log without blocking). Without `#[non_exhaustive]`, any downstream crate (or our own integration tests) that does an exhaustive `match` on `RiskLevel` would break to compile error when a new variant is added.

With `#[non_exhaustive]`, external match arms must include a wildcard, and the compiler enforces this. Internal matches (same crate) remain exhaustive as usual.

```rust
#[non_exhaustive]
pub enum RiskLevel { Safe, Warn, Danger, Block }
```

**Status:** Enforced.

---

## ADR-005: Two-pass scan (Aho-Corasick + Regex)

**Decision:** Command classification uses two passes: a fast Aho-Corasick keyword scan first, full regex scan only on potential matches.

**Rationale:** The vast majority of commands in normal development are safe (`cargo build`, `git status`, `ls`, etc.). Running 50+ regex patterns against every command would add measurable latency even though regex is fast.

Aho-Corasick over a set of keywords (`rm`, `drop`, `terraform`, `kubectl`, etc.) runs in O(n) over the command string with a single pass, independent of the number of keywords. If no keyword matches, the command is immediately classified Safe and executed — target < 2ms.

Only when a keyword matches do we run the full regex suite against the command to determine exact risk level and which patterns fired.

**Status:** Enforced. Do not add regex to the first pass.

---

## ADR-006: Synchronous scanner, async subprocess calls

**Decision:** `scanner.rs` and `parser.rs` are fully synchronous. Only subprocess calls (git stash, docker commit, shell exec) are async via tokio.

**Rationale:** The scanner is on the critical latency path. Async overhead (future allocation, scheduler yield, wakeup) would add noise to < 2ms measurements. There is no I/O in the scanner — it operates purely on an in-memory string.

Subprocess calls (snapshot, exec) involve real I/O and can take 100ms+. These are naturally async and benefit from non-blocking execution.

**Status:** Enforced. Do not introduce `async` into `interceptor/`.

---

## ADR-007: Append-only audit log (JSONL)

**Decision:** Audit log is append-only JSONL at `~/.aegis/audit.jsonl`. It is never rewritten or rotated by Aegis itself.

**Rationale:** The audit log is a security artifact. An append-only log cannot be silently corrupted by a bug that rewrites the file. JSONL (one JSON object per line) allows streaming reads, `grep`, `jq` filtering, and line-by-line processing without parsing the entire file.

Log rotation, if needed, is the user's responsibility (`logrotate` or similar). Aegis does not implement it.

The format (field names, types) is a public contract from v1 and must not change in breaking ways.

**Status:** Enforced.

---

## ADR-008: Security tooling in CI (cargo-audit + cargo-deny)

**Decision:** `cargo audit` and `cargo deny check` are required CI steps that block merge on failure.

**Rationale:** Aegis intercepts shell commands on behalf of users who trust it with their infrastructure. A supply-chain compromise or a dependency with a known CVE would undermine the tool's core purpose. For a security tool, dependency hygiene is not optional.

- `cargo-audit`: checks against the RustSec Advisory Database for known CVEs in the dependency tree.
- `cargo-deny`: enforces license policy (MIT/Apache-2.0/ISC only), bans specific crates (e.g., `once_cell`), and prevents duplicate major versions of core crates.

**Status:** Enforced via CI. See `.github/workflows/ci.yml`.

---

## ADR-009: Fuzz testing for parser

**Decision:** `parser.rs` has dedicated fuzz targets using `cargo-fuzz` (libFuzzer).

**Rationale:** The parser handles untrusted shell command strings that may contain heredoc bodies, inline Python/Node scripts, nested quotes, and escape sequences. This is the highest-complexity, highest-risk code in the project — the exact profile where fuzzing reliably finds bugs that hand-written tests miss.

Fuzz targets are in `fuzz/fuzz_targets/`. Run with `cargo +nightly fuzz run fuzz_scanner`.

**Status:** Required before v1.0 release.
