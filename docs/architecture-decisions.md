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

**Decision:** Audit log uses append-only JSONL segments rooted at `~/.aegis/audit.jsonl`. The active segment is append-only; when configured, Aegis rotates it by size into immutable numbered archives, optionally gzip-compressed. New entries store RFC 3339 timestamps with timezone plus a per-process sequence number; legacy Unix-second timestamps remain readable.

**Rationale:** The audit log is a security artifact. An append-only log cannot be silently corrupted by a bug that rewrites the file. JSONL (one JSON object per line) allows streaming reads, `grep`, `jq` filtering, and line-by-line processing without parsing the entire file.

Rotation is size-based and keeps a bounded number of archives. Querying must read both the active segment and rotated archives so operators can still inspect history after rotation.

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

## ADR-010: Security model — heuristic guardrail, not a sandbox

**Decision:** Aegis is explicitly documented as a heuristic command guardrail. It is not a sandbox, not a complete security boundary, and makes no claim to catch obfuscated, indirect, or runtime-assembled commands.

**Rationale:** Without a written security model, users may develop incorrect expectations — believing Aegis provides stronger guarantees than it actually does. A tool that overpromises and underdelivers is more dangerous than one with clearly stated limits, because users make trust decisions based on it.

The honest model:

1. **Heuristic matching.** Aegis applies pattern matching to the raw command string. This catches the common case: an AI agent issuing a recognisable destructive command directly. It cannot catch commands that are assembled, encoded, or deferred at runtime — doing so would require a full shell interpreter and OS-level tracing.

2. **No sandbox.** Approved commands run with the user's full permissions. Aegis does not restrict file descriptors, network access, syscalls, or namespaces. It is not `seccomp`, `pledge`, or a container.

3. **Explicit non-goals.** The following are out of scope by design and documented as such in `README.md`:
   - Obfuscated shell (`$'\x72\x6d'` for `rm`)
   - Indirect execution (write a script, then execute it in a later command)
   - Script-generated commands (`eval "$(fn_that_returns_rm_rf)"`)
   - Alias/function expansion (`alias ls='rm -rf /'`)
   - Encoded payloads (`base64 -d | bash` variants beyond `PKG-004`)
   - Subshell injection in otherwise-safe commands

**What this means in practice:** Aegis protects against accidental and well-intentioned-but-mistaken destructive commands — the failure mode of current AI agents operating honestly. It is not designed to stop an adversarially-controlled agent that is actively trying to evade detection. For stronger guarantees, users should combine Aegis with OS-level controls (containers, restricted accounts, network segmentation).

**Status:** Documented in `README.md § Security model`. Not enforced by code — it is a statement about scope, not an implementation constraint.

---

## ADR-011: Typed planning boundary for interception policy

**Decision:** Interception policy is exposed through a typed planning boundary:
`prepare_planner` maps fail-closed setup errors into typed setup-failure plans,
and `plan_with_context` produces a canonical `InterceptionPlan` for shell-wrapper,
watch mode, and evaluation-only JSON.

**Rationale:** This keeps policy semantics in one place while leaving UI,
snapshots, execution, and audit append as downstream adapters. Surfaces should
adapt a typed plan; they should not rebuild policy inputs or decision meaning
independently.

**Status:** Enforced by the `src/planning/` module and consumed by shell, watch,
and JSON evaluation flows.

---

## ADR-009: Fuzz testing for parser and scanner

**Decision:** Aegis uses `cargo-fuzz` (libFuzzer) for security-sensitive shell-input fuzzing. In phase 1, the repository implements a dedicated parser fuzz target; scanner fuzzing remains a required follow-on phase.

**Rationale:** The parser handles untrusted shell command strings that may contain heredoc bodies, inline Python/Node scripts, nested quotes, and escape sequences. This is the highest-complexity, highest-risk code in the project — the exact profile where fuzzing reliably finds bugs that hand-written tests miss. The scanner is the next surface in the same rollout, but it is intentionally deferred so parser failures are easier to triage first.

The phase-1 parser target lives at `fuzz/fuzz_targets/parser.rs`. Run it with `cargo +nightly fuzz run parser fuzz/corpus/parser`.

**Status:** Parser fuzz target implemented in this phase; scanner fuzz target not yet implemented in this phase. Fuzzing remains required before v1.0 release.
