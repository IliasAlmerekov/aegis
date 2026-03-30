# Phase 01: contract-integrity / Ticket 1.3 - Research

**Researched:** 2026-03-27
**Domain:** SnapshotRegistry config-to-runtime integration coverage
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Keep `impl Default for SnapshotRegistry` as-is. It delegates to `from_config(&Config::default())` which is already safe. No callers in production paths use it — `RuntimeContext::new` always calls `from_config` explicitly.

- **D-02:** `from_config` and `RuntimeContext` are already correct — no changes to `src/snapshot/mod.rs` logic or `src/runtime.rs`.
- **D-03:** UI (`src/ui/confirm.rs`) and audit (`src/audit/logger.rs`) already correctly reflect only the snapshots that were actually created — no changes needed.

### Missing gap — integration tests

- **D-04:** Add integration tests in `tests/full_pipeline.rs` covering:
  - `auto_snapshot_git = false` in `.aegis.toml` → git stash is NOT called (no snapshot record in audit)
  - `auto_snapshot_docker = false` in `.aegis.toml` → docker commit is NOT called (no snapshot record in audit)
- **D-05:** Tests must run against a real built binary (same pattern as other tests in `full_pipeline.rs`), not mocks.
- **D-06:** The git snapshot test must set `current_dir` to a temp dir that is NOT a git repo, OR a temp git repo — so that `GitPlugin::is_applicable` returns the expected value. Use a non-git temp dir with `auto_snapshot_git=false` to confirm the plugin was never registered (not just skipped by `is_applicable`).

### Claude's Discretion

- Exact test helper structure (fixture setup, assertion style) — follow the patterns already established in `full_pipeline.rs`.

### Deferred Ideas (OUT OF SCOPE)

- Exposing a `rollback` CLI command (Ticket from ROADMAP.md v2) — out of scope for 1.3.
- Additional snapshot plugin types (S3, PostgreSQL) — v2 ROADMAP, not this ticket.
</user_constraints>

## Summary

Code inspection confirms the core implementation already honors the snapshot flags end-to-end in production wiring. `SnapshotRegistry::from_config` conditionally registers Git and Docker plugins, `RuntimeContext::new` always uses `SnapshotRegistry::from_config(&config)`, and `main.rs` only shows/audits the `snapshots` returned by `context.create_snapshots(...)`. I found existing unit coverage for registry composition and runtime sharing, but no integration coverage in `tests/full_pipeline.rs` for `auto_snapshot_git` or `auto_snapshot_docker`.

The real planning gap is not implementation logic; it is proof at the subprocess boundary that a config file written into a temp workspace actually suppresses plugin registration for the real binary. To test that rigorously, the phase should use the existing `full_pipeline.rs` harness, write explicit `.aegis.toml` files, run a Danger-level command through the compiled binary, and stub `git` / `docker` in `PATH` so the test can assert those binaries were never invoked at all.

**Primary recommendation:** Plan this as a test-focused phase in `tests/full_pipeline.rs`; do not change `src/snapshot/mod.rs`, `src/runtime.rs`, `src/ui/confirm.rs`, or `src/audit/logger.rs` unless test execution disproves the current code inspection.

## Project Constraints (from CLAUDE.md)

- Read `.claude/AGENTS.md` before non-trivial work.
- Follow `CONVENTION.md`; security invariants and CI-enforced rules take precedence.
- Route every shell command through `rtk`; never run raw commands.
- Use short conventional commits; never add `Co-Authored-By`.
- Keep `src/main.rs` thin; business logic belongs in focused modules.
- Do not add new dependencies without clear justification; prefer stdlib when sufficient.
- Do not add `once_cell`; use `std::sync::LazyLock`.
- Do not introduce `unsafe {}`.
- Do not use `unwrap()` / `expect()` in non-test production paths unless explicitly justified by startup contract.
- All new `pub` items require `///` doc comments.
- Preserve the exit-code contract.
- Preserve append-only audit semantics and layered config compatibility.
- Keep `src/interceptor/` synchronous; do not introduce async into parser/scanner hot path.
- Do not run `cargo build`, `cargo test`, `cargo bench`, `cargo audit`, or `cargo deny` autonomously before the human-approved plan checkpoint.
- Treat changes under `src/main.rs`, `src/interceptor/`, `src/ui/confirm.rs`, `src/config/allowlist.rs`, `src/config/model.rs`, `src/snapshot/`, and `src/audit/logger.rs` as security-sensitive.

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| Rust integration tests (`cargo test`) | std/Cargo | Binary-level regression coverage | Already the repo contract for end-to-end behavior |
| `tempfile` | 3 | Isolated temp home/workspace dirs | Existing integration-test pattern |
| `std::process::Command` | std | Spawn the compiled `aegis` binary and stub CLIs | `tests/full_pipeline.rs` already uses it |
| `serde_json` | 1 | Parse audit JSONL assertions | Existing helper already uses it |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tokio` (dev feature macros/rt-multi-thread) | 1 | Async unit/integration support elsewhere | Not needed for this ticket’s minimal `full_pipeline` tests |
| `tempfile`-backed stub executables | repo pattern | PATH-based call interception | Use to prove `git` / `docker` were never invoked |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Real built binary in `tests/full_pipeline.rs` | Unit tests around `SnapshotRegistry` only | Misses config loading, runtime construction, and audit/UI integration |
| PATH stubs that log invocations | Mocking `SnapshotPlugin` | Violates D-05 and weakens end-to-end confidence |
| Audit-only assertion | Stub CLI log + audit assertion | Audit-only can miss `is_applicable` calls that still hit `git` / `docker` |

**Installation:** none — use the existing Rust test stack from `Cargo.toml`.

## Architecture Patterns

### Recommended Project Structure
```text
tests/
└── full_pipeline.rs   # Add ticket 1.3 subprocess regressions here
```

### Pattern 1: Config flows from workspace `.aegis.toml` into snapshot registration
**What:** The binary loads layered config from the current directory, then constructs `RuntimeContext`, which constructs `SnapshotRegistry::from_config(&config)`.
**When to use:** Any end-to-end test that must prove runtime behavior changes because of config.
**Source:** `src/config/model.rs:163-215`, `src/runtime.rs:63-67`, `src/snapshot/mod.rs:52-65`

### Pattern 2: Snapshots are created only on Danger commands
**What:** `main.rs` only calls `context.create_snapshots(...)` on Danger risk, both for allowlisted and interactive Danger commands.
**When to use:** New tests must use a Danger-level command; a Safe or Warn command will never exercise snapshot code.
**Source:** `src/main.rs:360-385`

### Pattern 3: UI and audit mirror only actual snapshot results
**What:** The confirmation UI renders snapshots only when the passed slice is non-empty, and audit entries serialize exactly the provided snapshot vector.
**When to use:** Assert `snapshots == []` in audit; do not expect missing field omission.
**Source:** `src/ui/confirm.rs:214-224`, `src/audit/logger.rs:187-203`, `src/audit/logger.rs:236-241`

### Pattern 4: `full_pipeline.rs` already has the exact harness needed
**What:** `base_command(home)`, `read_audit_entries(home)`, and `write_executable(path, body)` already support real-binary, temp-home, audit, and stubbed-PATH testing.
**When to use:** Reuse these helpers rather than inventing a new test harness.
**Source:** `tests/full_pipeline.rs:12-52`

### Read-First Files for Planner/Executor
1. `tests/full_pipeline.rs`
2. `src/snapshot/mod.rs`
3. `src/runtime.rs`
4. `src/main.rs`
5. `src/config/model.rs`
6. `src/audit/logger.rs`
7. `src/ui/confirm.rs`

### Anti-Patterns to Avoid
- **Testing only `snapshot_id` absence:** A plugin can still run `is_applicable` and touch `git` / `docker` without producing a snapshot.
- **Using a Warn command:** It will never enter snapshot creation.
- **Running inside the repo root:** Git applicability may change because the repo itself is a Git checkout.
- **Adding code changes first:** Current code inspection does not justify touching production files yet.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| End-to-end config proof | New custom harness crate | `tests/full_pipeline.rs` helpers | Already matches repo convention and real binary flow |
| CLI spying | In-process plugin mocks | PATH stub executables via `write_executable(...)` | Proves the real binary never invoked external tools |
| Audit parsing | Ad hoc string matching | `read_audit_entries(home)` + JSON assertions | Stable and already used in existing tests |
| Config injection | Manual config structs in test code | Write `.aegis.toml` into temp workspace | Exercises actual layered config loader |

**Key insight:** The deceptive complexity here is distinguishing “plugin was not registered” from “plugin was registered but skipped.” Only real-binary execution plus stubbed external binaries cleanly proves the stronger claim.

## Common Pitfalls

### Pitfall 1: Treating “no snapshot record” as proof of no plugin activity
**What goes wrong:** The test passes even if `git rev-parse` or `docker ps` still ran.
**Why it happens:** `snapshot_all()` calls `is_applicable()` before `snapshot()`.
**How to avoid:** Stub `git` / `docker` in `PATH` and assert the log file stays absent/empty.
**Warning signs:** Audit `snapshots` is empty, but stub logs show CLI invocations.

### Pitfall 2: Running the test from the project repo
**What goes wrong:** Git applicability becomes true because the Aegis repo itself is a Git repo.
**Why it happens:** `GitPlugin::is_applicable()` uses `git rev-parse --git-dir` in `cwd`.
**How to avoid:** Use a temp workspace that is not a Git repo.
**Warning signs:** Unexpected `git` invocations or snapshots in a supposedly isolated test.

### Pitfall 3: Forgetting to disable unrelated snapshot plugins
**What goes wrong:** Another plugin adds noise to the audit or PATH log.
**Why it happens:** Git defaults to `true`, Docker defaults to `false`.
**How to avoid:** In docker-off coverage, explicitly set `auto_snapshot_git = false` too, so the assertion isolates Docker behavior.
**Warning signs:** `git` gets called during a docker-focused negative test.

### Pitfall 4: Using a non-Danger command
**What goes wrong:** No snapshot code runs, so the test proves nothing about registry behavior.
**Why it happens:** `main.rs` only snapshots Danger commands.
**How to avoid:** Use a known Danger command and force the dialog with `AEGIS_FORCE_INTERACTIVE=1`.
**Warning signs:** Command exits through Safe/Warn path; no snapshot hook opportunity exists.

## Code Examples

Verified repo patterns:

### Real-binary test harness
```rust
fn base_command(home: &Path) -> Command {
    let mut command = Command::new(aegis_bin());
    command.env("AEGIS_REAL_SHELL", "/bin/sh");
    command.env("AEGIS_CI", "0");
    command.env("HOME", home);
    command
}
```
**Source:** `tests/full_pipeline.rs:16-24`

### Snapshot registration is config-gated
```rust
pub fn from_config(config: &Config) -> Self {
    let mut plugins: Vec<Box<dyn SnapshotPlugin>> = Vec::new();

    if config.auto_snapshot_git {
        plugins.push(Box::new(GitPlugin));
    }

    if config.auto_snapshot_docker {
        plugins.push(Box::new(DockerPlugin::new()));
    }

    Self { plugins }
}
```
**Source:** `src/snapshot/mod.rs:52-65`

### Audit receives exactly the produced snapshots
```rust
let entry = AuditEntry::new(
    assessment.command.raw.clone(),
    assessment.risk,
    assessment.matched.iter().map(Into::into).collect(),
    decision,
    snapshots.iter().map(Into::into).collect(),
    allowlist_match.map(|m| m.pattern.clone()),
);
```
**Source:** `src/runtime.rs:127-133`

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Unit-only proof that `SnapshotRegistry::from_config` filters plugins | Add subprocess coverage in `tests/full_pipeline.rs` that exercises config loading and runtime construction | This ticket / 2026-03-27 planning context | Catches integration regressions unit tests cannot |
| Audit-only negative assertion | Audit assertion plus PATH-stub assertion | Recommended for Ticket 1.3 | Distinguishes “not registered” from “registered but skipped” |

**Deprecated/outdated:**
- “Check that `git stash` / `docker commit` did not run” as the only assertion: too weak, because `is_applicable()` may still invoke `git` / `docker`.

## Contradictions / Corrections to Existing CONTEXT.md

1. **D-06 is directionally right but not sufficient as written.**
   - A non-git temp dir alone does **not** prove the Git plugin was never registered.
   - If the plugin is registered, `GitPlugin::is_applicable()` still executes `git rev-parse --git-dir`.
   - **Planner correction:** pair the non-git temp dir with a stub `git` binary and assert it was never invoked.

2. **D-04’s docker wording is too narrow.**
   - “docker commit is NOT called” is weaker than the actual contract needed.
   - If Docker were registered, `DockerPlugin::is_applicable()` would still call `docker ps -q`.
   - **Planner correction:** assert no `docker` invocation at all, not just no `docker commit`.

3. **“No snapshot record in audit” needs schema-accurate wording.**
   - The audit entry still exists and still includes `snapshots`; the expected value is an empty array `[]`.
   - **Planner correction:** assert `entry["snapshots"] == []`, not field absence.

## Open Questions

1. **Should this ticket also add a positive git-on integration test?**
   - What we know: the negative coverage is the required gap; unit tests already prove registry composition.
   - What's unclear: whether the team wants one stronger end-to-end “git on” proof in the same ticket.
   - Recommendation: keep the minimal plan test-only and negative-path focused; add positive git-on only if reviewers want stronger regression coverage after the minimal tests land.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| cargo | Running targeted Rust tests after planning | ✓ | 1.94.0 | — |
| rustc | Building test binary | ✓ | 1.94.0 | — |
| git | Optional positive git-on test; also useful for local debugging | ✓ | 2.43.0 | Negative tests can use PATH stub |
| docker | Optional positive docker coverage only | ✓ | 29.3.1 | Minimal plan skips live Docker |
| `/bin/sh` | `base_command(home)` harness | ✓ | available | — |

**Missing dependencies with no fallback:** None.

**Missing dependencies with fallback:** None.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust `cargo test` integration tests |
| Config file | none |
| Quick run command | `rtk cargo test --test full_pipeline snapshot_` |
| Full suite command | `rtk cargo test` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| T1.3-GIT-OFF | `.aegis.toml` with `auto_snapshot_git = false` prevents any git snapshot plugin activity; audit `snapshots` is `[]` | integration | `rtk cargo test --test full_pipeline snapshot_registry_git_flag_false_skips_plugin_and_audit -x` | ✅ `tests/full_pipeline.rs` exists; case missing |
| T1.3-DOCKER-OFF | `.aegis.toml` with `auto_snapshot_docker = false` prevents any docker snapshot plugin activity; audit `snapshots` is `[]` | integration | `rtk cargo test --test full_pipeline snapshot_registry_docker_flag_false_skips_plugin_and_audit -x` | ✅ `tests/full_pipeline.rs` exists; case missing |

### Sampling Rate
- **Per task commit:** targeted `rtk cargo test --test full_pipeline <new_test_name>`
- **Per wave merge:** `rtk cargo test --test full_pipeline`
- **Phase gate:** full `rtk cargo test`, then repo baseline checks once implementation is approved to run

### Wave 0 Gaps
- [ ] Add two new integration test cases in `tests/full_pipeline.rs`
- [ ] Add a tiny helper or inline pattern for reading stub CLI invocation logs
- [ ] Decide whether to keep both tests fully isolated by setting the non-target snapshot flag to `false` as well

## Minimal Executable Plan

1. **Add git-off subprocess regression in `tests/full_pipeline.rs`.**
   - Temp `home`, temp non-git `workspace`, stub `git` in `PATH`, explicit `.aegis.toml` with `auto_snapshot_git = false` (and preferably `auto_snapshot_docker = false` for isolation), Danger command, deny via `AEGIS_FORCE_INTERACTIVE=1`, assert no git invocation and audit `snapshots == []`.

2. **Add docker-off subprocess regression in `tests/full_pipeline.rs`.**
   - Same pattern, but stub `docker`, explicit `auto_snapshot_docker = false`, and disable git too for isolation, then assert no docker invocation and audit `snapshots == []`.

3. **Verify only after plan approval.**
   - Run the two targeted tests first, then broader `full_pipeline`, then project baseline checks if the approved plan calls for them.

## Sources

### Primary (HIGH confidence)
- `.planning/phases/01-contract-integrity/1.3-CONTEXT.md` — scope, locked decisions, claimed gap
- `.planning/codebase/ARCHITECTURE.md` — runtime flow, snapshot/audit/config layering
- `.planning/codebase/TESTING.md` — integration-test conventions and exit-code contract
- `.claude/CLAUDE.md` — repository constraints and command/test restrictions
- `.claude/AGENTS.md` — orchestration and plan-before-run constraints
- `CONVENTION.md` — security invariants, architecture, testing, and compatibility rules
- `Cargo.toml` — exact test-stack crate versions
- `src/snapshot/mod.rs` — config-gated plugin registration and existing unit tests
- `src/runtime.rs` — runtime construction and audit bridging
- `src/main.rs` — Danger-only snapshot call path
- `src/config/model.rs` — effective config defaults and merge behavior
- `src/ui/confirm.rs` — snapshot rendering semantics
- `src/audit/logger.rs` — audit snapshot serialization semantics
- `tests/full_pipeline.rs` — existing real-binary integration harness and missing coverage

### Secondary (MEDIUM confidence)
- None.

### Tertiary (LOW confidence)
- None.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - derived from `Cargo.toml` and existing repo test patterns
- Architecture: HIGH - verified directly in current source files
- Pitfalls: HIGH - derived from exact `is_applicable()` and audit/UI code paths

**Research date:** 2026-03-27
**Valid until:** Until the cited files change materially; otherwise 30 days
