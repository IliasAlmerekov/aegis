# Aegis — Production Readiness Roadmap

This document is a catalogue of problems that currently prevent Aegis from
being considered a production-ready, architecturally strong project.  
It describes **only the problems** — no solutions or implementation advice.

---

## 1. Blocking I/O inside async functions

**`postgres.rs`, `mysql.rs`** — `output_with_busy_retry` is declared `async fn`
but calls `std::thread::sleep` inside the retry loop. This occupies a Tokio
worker thread for the entire sleep duration, reducing the runtime's ability to
schedule other async work and violating the contract of cooperative async
concurrency.

**`docker.rs`** — `run_docker_output_blocking` calls `std::process::Command`
(sync, blocking) and `std::thread::sleep` inside a `loop`. This function is
called from `is_applicable`, which is itself called during `snapshot_all` — an
async function. The result is a blocking operation on a Tokio thread.

**`postgres.rs` — `is_applicable`** — spawns `std::process::Command::new("which")`
synchronously to probe for the `pg_dump` binary. This is blocking I/O on the
async executor thread.

**`docker.rs` — `is_applicable`** — calls `run_docker_output_blocking` which
spawns a blocking `std::process::Command` and may loop with `thread::sleep`.
Called from `snapshot_all` in an async context.

---

## 2. `SnapshotPlugin::is_applicable` is a synchronous method performing blocking I/O

The `SnapshotPlugin` trait exposes `is_applicable` as a plain (non-async) `fn`.
Several implementations (`DockerPlugin`, `PostgresPlugin`, `MysqlPlugin`) do
process spawning and blocking waits inside this method, but the trait signature
provides no signal to callers that implementations may block. Callers in the
async path (`SnapshotRegistry::snapshot_all`) invoke it without any
off-thread dispatch.

---

## 3. Dead code suppression on production modules

**`src/error.rs:1`** — `#[allow(dead_code)]` is placed on the `AegisError` enum.
This means at least some error variants are unused in the production code paths,
but the dead-code warning is suppressed rather than the unused variants removed.

**`src/interceptor/parser/mod.rs:2`** — `#![allow(dead_code)]` is placed at the
crate-inner module level, suppressing dead-code warnings across the entire
parser module. For a hot-path security-critical module, dead code indicates
either unused abstractions or feature scaffolding that was never completed.

---

## 4. Missing MSRV (`rust-version`) in `Cargo.toml`

The `Cargo.toml` does not declare a `rust-version` field. `CONVENTION.md`
explicitly states that "declaring an explicit MSRV and enforcing it in CI"
is a prerequisite for production readiness. Without this, there is no
guarantee that the project compiles on the Rust versions used by downstream
consumers, and CI does not catch MSRV regressions.

---

## 5. Windows is not tested in CI

The CI workflow (`ci.yml`) runs only on `ubuntu-latest` and `macos-latest`.
The codebase contains `#[cfg(windows)]` / `#[cfg(not(windows))]` branches
(notably in `runtime.rs` for user detection), meaning Windows-specific code
paths exist but are never exercised in CI. Breakage on Windows is undetectable
from the current pipeline.

---

## 6. Incomplete own release-readiness checklist

`docs/release-readiness.md` lists a **Minimum Launch Checklist** where every
item is unchecked (`[ ]`):

- README, docs, and release notes consistency
- Convenience installer and troubleshooting documented for first-time users
- Release workflow exercised on a real tag
- Release artifacts with checksum sidecars
- Install and uninstall guidance currency
- Supported platforms stated clearly
- Threat-model and limitation language visible

The project self-identifies these as launch blockers. None is marked complete.

---

## 7. Audit log write failures are silently swallowed in non-verbose mode

`RuntimeContext::append_audit_entry` and `append_watch_audit_entry` emit a
warning about audit log write failures only when `verbose` is `true`. The audit
log is described in `CONVENTION.md` as a **security artifact** that must remain
append-only. Silent failure to append an entry means a decision may go
unrecorded with no signal to the operator.

---

## 8. Audit integrity is `Off` by default

`AuditIntegrityMode::Off` is the default for all new configs. SHA-256 hash
chaining — the only tamper-detection mechanism — is therefore opt-in. For a
tool whose primary value proposition includes an append-only security audit
trail, the most secure mode is not active unless the user explicitly configures
it.

---

## 9. `append_watch_audit_entry` has 11 parameters

`RuntimeContext::append_watch_audit_entry` takes 11 arguments and suppresses
the `clippy::too_many_arguments` lint with `#[allow(clippy::too_many_arguments)]`.
The suppression is in production library code rather than a test helper. This
indicates a missing parameter-aggregation type and makes the function's contract
difficult to read, audit, or call correctly.

---

## 10. `custom_pattern_cache_key` is an undocumented fragile string protocol

The cache key for custom scanner instances (`interceptor/mod.rs`) is built by
concatenating all pattern fields with ASCII control characters `\u{1f}` (unit
separator) and `\u{1e}` (record separator). There is no type representing this
key, no validation that the encoded content does not contain the separator
characters, and no documentation of the encoding. A pattern value containing
`\u{1f}` would silently produce an incorrect cache key.

---

## 11. `detect_effective_user_from_id_command` has a hardcoded binary path

`runtime.rs` resolves the current OS user by invoking `/usr/bin/id`. On some
Linux distributions, `id` is located at `/bin/id` or resolved via the shell's
`PATH`. On those systems, user-scoped allowlist rules will silently fail to
match because the user identity cannot be resolved.

---

## 12. `SnapshotPlugin` trait mismatch with architecture specification

`CLAUDE.md` specifies the `SnapshotPlugin` trait as:

```rust
#[async_trait]
pub trait SnapshotPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn is_applicable(&self, cwd: &Path) -> bool;
    async fn snapshot(&self, cwd: &Path, cmd: &str) -> Result<String>;
    async fn rollback(&self, snapshot_id: &str) -> Result<()>;
}
```

The trait is implemented as specified, but `is_applicable` is synchronous while
real implementations perform process spawning and blocking I/O. The architectural
constraint — that `is_applicable` is a cheap, non-blocking eligibility check —
is not enforced by the type system and is violated by multiple implementations.

---

## 13. Module structure divergence from `CONVENTION.md` and `ARCHITECTURE.md`

`CONVENTION.md` prescribes the following module layout:

```
src/
  main.rs
  error.rs
  interceptor/{mod,scanner,parser,patterns}.rs
  snapshot/{mod,git,docker}.rs
  ui/confirm.rs
  audit/logger.rs
  config/model.rs
```

The actual codebase has grown substantially beyond this:

- `src/planning/` — not in spec
- `src/runtime.rs`, `src/runtime_gate.rs` — not in spec
- `src/watch.rs` — not in spec
- `src/decision.rs` — not in spec
- `src/explanation.rs` — not in spec
- `src/toggle.rs` — not in spec
- `src/install.rs`, `src/rollback.rs` — not in spec
- `src/cli_commands.rs`, `src/cli_dispatch.rs`, `src/shell_compat.rs`,
  `src/shell_flow.rs`, `src/shell_wrapper.rs`, `src/policy_output.rs` — not in spec
- `src/snapshot/` has grown to include `postgres`, `mysql`, `sqlite`,
  `supabase` — only `git` and `docker` are mentioned in the spec

The architecture documentation has not been updated to reflect the actual module
boundary layout, creating drift between the stated contract and the
implementation.

---

## 14. `AuditEntry` struct has 18+ fields, many optional

`AuditEntry` is a flat struct with over 18 fields, the majority of which are
`Option<T>` with `skip_serializing_if`. The structure does not distinguish
between fields that are always present, fields specific to watch mode, and
fields for audit integrity. This flat layout makes it difficult to reason about
invariants (e.g. "watch-mode entries always have `source` and `cwd`"), perform
exhaustive pattern matching, or guarantee consistency between related optional
fields.

---

## 15. No cross-compilation or non-x86 architecture testing in CI

The release pipeline produces binaries for multiple architectures, but the CI
pipeline (`ci.yml`) does not include any cross-compilation or test jobs for
ARM (`aarch64`), musl libc, or other non-default targets. Regressions on these
targets are undetectable before a release.

---

## 16. `AegisConfig` vs `Config` public type alias adds API ambiguity

The config module re-exports `AegisConfig` under the alias `pub type Config =
AegisConfig`. The actual struct is named `AegisConfig` internally but all
public-facing code uses `Config`. This creates a disconnect between the
canonical name in source code, documentation, error messages, and the external
API.

---

## 17. `PartialConfig` duplicates every field of `AegisConfig` without abstraction

The layered config merge uses a `PartialConfig` struct that mirrors every scalar
field of `AegisConfig` as `Option<T>`. Any new field added to `AegisConfig`
requires a parallel addition in `PartialConfig` and a corresponding merge arm in
`merge_layer`. There is no compile-time enforcement that these stay in sync, and
the `merge_layer` function is already very long due to this per-field expansion.

---

## 18. `runtime.rs` leaks a Tokio runtime in tests via `std::mem::forget`

The `test_handle` helper in `runtime.rs` creates a `tokio::runtime::Runtime`
and intentionally leaks it with `std::mem::forget`. Each test call accumulates
a permanently leaked runtime for the process lifetime. While this is test-only
code, it establishes a pattern that can mask resource handling bugs and inflates
memory use across the test suite.

---

## 19. No `#[must_use]` on `Result`-returning public functions where applicable

Several public-facing functions that return `Result<T>` (e.g. `AuditLogger::append`,
`PatternSet::load`, `Allowlist::from_layered_rules`) lack `#[must_use]`
annotations. Callers that accidentally discard the result will not receive a
compiler warning, which is especially problematic for the audit logger where a
silently ignored error means a security event goes unrecorded.

---

## 20. `config_version` validation blocks loading older configs without migration path

`deserialize_config_version` rejects any `config_version` other than the current
value (`1`) at deserialization time with an error. There is no migration or
upgrade path for future config versions. If `config_version` is ever incremented,
all existing user config files will become instantly unreadable with no automated
recovery mechanism.

---

## 21. Fuzz targets run only 2000 iterations in CI

The CI fuzz job runs both `parser` and `scanner` fuzz targets with `-runs=2000`.
This is a coverage-theater level of fuzzing — 2000 iterations provides
effectively no probabilistic security guarantee for a security-critical input
parser. The fuzz corpus is maintained, but its coverage under CI constraints is
negligible.

---

## 22. `src/lib.rs` has no `#![warn(missing_docs)]` lint gate

The library crate does not enforce documentation coverage with
`#![warn(missing_docs)]` or `#![deny(missing_docs)]`. Many public items in
`src/snapshot/`, `src/planning/`, and `src/watch.rs` lack `///` documentation,
despite `CONVENTION.md` requiring that "all new public items must have `///`
doc comments."

---

## 23. `HOME` fallback in `default_snapshots_dir` defaults to `"."`

`snapshot/mod.rs::default_snapshots_dir` uses `env::var_os("HOME").unwrap_or_else(|| ".".into())`.
When `HOME` is unset (CI environments, containers, some service accounts),
the snapshots directory resolves to `./.aegis/snapshots` — relative to the
current working directory. Snapshot files accumulate in the project directory
rather than a stable per-user location.

---

## 24. No integration test coverage for snapshot rollback end-to-end

`tests/snapshot_integration.rs` exists, but actual database (Postgres, MySQL,
SQLite) and Docker snapshot-then-rollback flows cannot run in CI without the
respective daemons. The test file presumably skips or mocks these flows. There
is no documented strategy or CI job that validates the rollback path against
real database or container state.
