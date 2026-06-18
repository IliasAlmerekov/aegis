# M5.1 Close 800-LoC Quality Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use `rust-best-practices` before implementing this plan. Use `/implement <task description>` for code changes, following the repo red-tester → green-tester → reviewer loop. Every shell command must go through `rtk`.

## Goal

Close `TASKS.md` **M5.1** completely.

The original M5.1 text says to split `crates/aegis-sandbox/src/lib.rs` from 2071 LoC. That part is already done: current `aegis-sandbox` source files are under 800 LoC. The remaining `Done when` contract is broader:

> no file in the workspace exceeds 800 LoC; tests still pass.

Therefore this task is a code-organization and test-organization refactor across the remaining oversized Rust files, with no behavior changes.

## Current Baseline

As of 2026-06-18, excluding `.git`, `target`, and `.worktrees`, the Rust files over 800 LoC are:

| Lines | File |
| ---: | --- |
| 915 | `crates/aegis-config/src/model.rs` |
| 892 | `crates/aegis-snapshot/src/lib.rs` |
| 1275 | `crates/aegis-snapshot/src/supabase/runtime.rs` |
| 3340 | `tests/full_pipeline.rs` |
| 1645 | `tests/installer_flow.rs` |

`crates/aegis-sandbox/src/lib.rs` is currently 225 LoC, and the platform split is already present:

| Lines | File |
| ---: | --- |
| 225 | `crates/aegis-sandbox/src/lib.rs` |
| 746 | `crates/aegis-sandbox/src/linux.rs` |
| 640 | `crates/aegis-sandbox/src/macos.rs` |
| 235 | `crates/aegis-sandbox/src/support.rs` |
| 18 | `crates/aegis-sandbox/src/unsupported.rs` |
| 418 | `crates/aegis-sandbox/src/windows.rs` |

## Non-Goals

- Do not change runtime behavior.
- Do not change public APIs unless the compiler requires visibility adjustments for moved code.
- Do not add dependencies.
- Do not modify `Cargo.toml`, `Cargo.lock`, `deny.toml`, or CI workflows without explicit human sign-off.
- Do not combine this with M4.1 native-Windows scope changes.
- Do not rewrite security-sensitive command interception, snapshot ordering, audit semantics, or policy behavior.

## Safety Constraints

- Preserve fail-closed behavior.
- Preserve append-only audit behavior.
- Preserve snapshot/rollback semantics.
- Preserve existing CLI output contracts and exit-code contracts.
- Keep parser/scanner hot paths untouched unless a reviewer explicitly asks for follow-up work.
- Keep `src/main.rs` thin; this task should not need to touch it.
- No `unsafe {}` additions.
- No `.unwrap()` / `.expect()` additions in non-test production paths.

## Recommended Approach

Use narrow, mechanical module extraction. Prefer moving cohesive blocks intact over rewriting logic. After each extraction, run the smallest relevant test target, then run full gates at the end.

The recommended sequence is:

1. Add a file-size regression test first.
2. Split `aegis-config/src/model.rs`.
3. Split `aegis-snapshot/src/lib.rs`.
4. Split `aegis-snapshot/src/supabase/runtime.rs`.
5. Split oversized integration test files.
6. Update `TASKS.md` to mark M5.1 done only after verification passes.

This order keeps the quality gate measurable from the start and tackles production library modules before integration-test organization.

## Acceptance Criteria

- [ ] No Rust source file in the active workspace exceeds 800 LoC, excluding generated/build directories and `.worktrees`.
- [ ] A regression test enforces the 800-LoC budget so M5.1 cannot silently regress.
- [ ] `TASKS.md` M5.1 reflects the actual completed work instead of the stale `aegis-sandbox/src/lib.rs (2071 LoC)` wording.
- [ ] Public APIs remain source-compatible.
- [ ] No behavior changes are introduced.
- [ ] Required gates pass or known baseline failures are documented separately.

## Task 1 — Add a File-Size Budget Regression Test

**Purpose:** make the `Done when` objective executable.

**Files:**

- Add: `tests/file_size_budget.rs`

**Implementation outline:**

- Walk `CARGO_MANIFEST_DIR`.
- Skip:
  - `.git/`
  - `target/`
  - `.worktrees/`
  - any hidden/cache directories that are not part of the active workspace contract.
- Check only `*.rs`.
- Count lines with standard library I/O.
- Fail if any file has more than 800 lines.
- Include all offending paths in one assertion message so the next worker sees the full list.

**Rust best-practices notes:**

- Use `Result<(), Box<dyn std::error::Error>>` or a small local helper for the test; this is test code, so ergonomics are acceptable.
- Avoid adding a dependency such as `walkdir`; use recursive `std::fs::read_dir`.
- Use descriptive test naming, e.g. `rust_source_files_should_stay_under_800_lines`.

**Verification:**

```bash
rtk cargo test --test file_size_budget
```

Expected before refactor: fails and lists the five oversized files.

## Task 2 — Split `crates/aegis-config/src/model.rs`

**Current problem:** `model.rs` mixes template text, public config types, partial merge structures, serde helpers, migration code, and tests.

**Target layout:**

- Keep: `crates/aegis-config/src/model.rs`
  - public module wiring
  - public re-exports from existing `enums` and `rules`
  - public top-level config structs if moving them would create noisy public-path changes
  - `AegisConfig` public impl entry points
- Add: `crates/aegis-config/src/model/template.rs`
  - `INIT_TEMPLATE`
  - init-template helper functions, if any
- Add: `crates/aegis-config/src/model/partial.rs`
  - `PartialConfig`
  - `PartialSandboxSettings`
  - `PartialAuditConfig`
  - `PartialPruneConfig`
  - merge helpers
- Add: `crates/aegis-config/src/model/serde_helpers.rs`
  - `deserialize_allowlist_rules`
  - `deserialize_config_version`
  - `deserialize_optional_config_version`
  - related small parser helpers
- Add: `crates/aegis-config/src/model/migration.rs`
  - `migrate_deprecated_allowlist_in_file`
  - migration-specific helpers
- Add: `crates/aegis-config/src/model/tests.rs` or smaller `#[cfg(test)]` submodules
  - move existing `mod tests` out of `model.rs`

**Implementation notes:**

- Keep public type names and serde shape unchanged.
- If submodules need private access, use `pub(super)` / `pub(crate)` narrowly.
- Do not widen visibility to `pub` just to make the compiler happy.
- Keep config validation fail-closed.
- Keep the existing `#[serde(default, deny_unknown_fields)]` contracts intact.

**Verification after this task:**

```bash
rtk cargo test -p aegis-config
rtk cargo test --test file_size_budget
```

`file_size_budget` may still fail for the remaining oversized files; that is expected until later tasks.

## Task 3 — Split `crates/aegis-snapshot/src/lib.rs`

**Current problem:** `lib.rs` is both the public facade and the implementation home for registry materialization, retention policy, clock abstractions, pruning helpers, and tests.

**Target layout:**

- Keep: `crates/aegis-snapshot/src/lib.rs`
  - crate docs
  - `#![deny(missing_docs)]`
  - provider module declarations
  - public exports
  - minimal public facade glue
- Add: `crates/aegis-snapshot/src/registry.rs`
  - `SnapshotRegistry`
  - `SnapshotRegistryConfig`
  - `registry_config_from_parts`
  - provider materialization helpers
  - `available_provider_names` if it fits better here; re-export from `lib.rs`
- Add: `crates/aegis-snapshot/src/retention.rs`
  - `PrunableRecord`
  - `RetentionPolicy`
  - `MinimalAuditSnapshot`
  - pruning candidate selection helpers
- Add: `crates/aegis-snapshot/src/clock.rs`
  - `Clock`
  - `SystemClock`
  - `FixedClock`
- Add: `crates/aegis-snapshot/src/testing.rs`
  - existing registry build counter test hook
- Add: `crates/aegis-snapshot/src/paths.rs`
  - `resolve_snapshots_dir`
  - `home_dir`

**Implementation notes:**

- Preserve public re-exports so downstream code still imports the same names from `aegis_snapshot`.
- Because `#![deny(missing_docs)]` is enabled, every new public item must keep or receive doc comments.
- Avoid broad `pub use` if `pub(crate)` is sufficient.
- Keep `SnapshotPlugin` in `lib.rs` unless moving it clearly improves the facade without increasing API churn.

**Verification after this task:**

```bash
rtk cargo test -p aegis-snapshot
rtk cargo test --test file_size_budget
```

## Task 4 — Split `crates/aegis-snapshot/src/supabase/runtime.rs`

**Current problem:** `runtime.rs` contains manifest writing, manifest validation, snapshot execution, rollback checks, delete behavior, and a large test module.

**Target layout option A (recommended): convert `runtime.rs` into `runtime/mod.rs` plus submodules.**

- Move: `crates/aegis-snapshot/src/supabase/runtime.rs` → `crates/aegis-snapshot/src/supabase/runtime/mod.rs`
- Add: `crates/aegis-snapshot/src/supabase/runtime/manifest_io.rs`
  - `write_manifest_atomically`
  - manifest read/write helpers
  - parent directory sync helpers if currently local to runtime
- Add: `crates/aegis-snapshot/src/supabase/runtime/manifest_state.rs`
  - `phase1_complete`
  - manifest schema validation
  - strict eligibility recomputation
  - `SupabaseCapabilities::phase1`
  - test fixtures if shared by multiple runtime test modules
- Add: `crates/aegis-snapshot/src/supabase/runtime/snapshot.rs`
  - snapshot command construction/execution
  - artifact path/checksum/size handling
- Add: `crates/aegis-snapshot/src/supabase/runtime/rollback.rs`
  - rollback support and config-target matching
- Add: `crates/aegis-snapshot/src/supabase/runtime/delete.rs`
  - delete/idempotency behavior
- Add: `crates/aegis-snapshot/src/supabase/runtime/tests.rs`
  - or multiple test files grouped by unit of work

**Target layout option B:** keep `runtime.rs` as the coordinator and place submodules under `runtime/`.

Use option B if the move to `runtime/mod.rs` creates unnecessary churn. The final line count still must be below 800.

**Implementation notes:**

- `runtime.rs` currently uses `use super::*`; during extraction prefer explicit imports inside each submodule.
- Keep non-public manifest structs private to the `supabase` module unless tests require `pub(super)`.
- Do not change snapshot ID encoding.
- Preserve all rollback eligibility and target-match checks.
- Preserve manifest atomic-write semantics:
  - write temp file
  - sync temp file
  - rename
  - sync parent directory
- Preserve test-only manifest write failure injection.

**Verification after this task:**

```bash
rtk cargo test -p aegis-snapshot supabase
rtk cargo test -p aegis-snapshot
rtk cargo test --test file_size_budget
```

## Task 5 — Split `tests/full_pipeline.rs`

**Current problem:** one large integration test file covers many unrelated end-to-end behaviors: shell wrapper passthrough, JSON planning, allowlist policy, block behavior, snapshots, audit projection, disabled toggle behavior, and other contracts.

**Target layout:**

- Keep shared helpers in a support module:
  - Add: `tests/support/mod.rs`
  - Move helpers such as:
    - `aegis_bin`
    - `base_command`
    - `direct_shell_command`
    - `read_audit_entries`
    - `write_executable`
    - `write_disabled_toggle`
    - `read_stub_invocations`
- Split tests into focused files:
  - `tests/full_pipeline_shell.rs`
  - `tests/full_pipeline_json.rs`
  - `tests/full_pipeline_policy.rs`
  - `tests/full_pipeline_allowlist.rs`
  - `tests/full_pipeline_snapshot.rs`
  - `tests/full_pipeline_toggle.rs`
  - `tests/full_pipeline_audit.rs`

Exact filenames may differ; prefer names that match the behavior under test.

**Implementation notes:**

- Rust integration test files are separate crates. Shared helpers must be in `tests/support/mod.rs` and imported with `mod support;`.
- Keep environment setup identical.
- Do not merge assertions or change test semantics while moving code.
- Move one logical group at a time and run the matching integration test file.

**Verification during this task:**

```bash
rtk cargo test --test full_pipeline_shell
rtk cargo test --test full_pipeline_json
rtk cargo test --test full_pipeline_policy
rtk cargo test --test full_pipeline_allowlist
rtk cargo test --test full_pipeline_snapshot
rtk cargo test --test full_pipeline_toggle
rtk cargo test --test full_pipeline_audit
```

Then:

```bash
rtk cargo test --test file_size_budget
```

## Task 6 — Split `tests/installer_flow.rs`

**Current problem:** one integration test file covers installer stubs, checksum verification, real binary release fixtures, TTY behavior, and live release behavior.

**Target layout:**

- Extend shared helper module or add installer-specific support:
  - `tests/support/installer.rs`
  - helpers:
    - `script_path`
    - `write_executable`
    - `find_command_on_path`
    - `write_command_shim`
    - `write_host_command_shims`
    - `write_fake_release_binary`
    - `aegis_test_binary`
    - `copy_release_binary`
    - `sha256_hex`
    - `host_asset_name`
    - `write_release_checksum`
    - stub writers
    - runner helpers
- Split tests into focused files:
  - `tests/installer_checksum.rs`
  - `tests/installer_platform.rs`
  - `tests/installer_tty.rs`
  - `tests/installer_live_release.rs`

**Implementation notes:**

- Preserve `AEGIS_TEST_LIVE_INSTALL=1` gating for live-network release tests.
- Preserve Unix/macOS-specific `cfg` gates.
- Preserve host command shim behavior.
- Do not make live tests run by default.
- Avoid adding dependencies for temp fixtures or HTTP; keep the existing std/process approach.

**Verification during this task:**

```bash
rtk cargo test --test installer_checksum
rtk cargo test --test installer_platform
rtk cargo test --test installer_tty
rtk cargo test --test installer_live_release
rtk cargo test --test file_size_budget
```

## Task 7 — Update `TASKS.md`

**Files:**

- Modify: `TASKS.md`

**Required update:**

- Mark M5.1 as `[x]`.
- Replace stale wording about only `aegis-sandbox/src/lib.rs (2071 LoC)` with an accurate summary:
  - sandbox split already completed
  - remaining oversized config/snapshot/integration-test files split
  - regression test added to enforce the 800-LoC budget
- Keep future M5.2–M5.4 unchanged.

**Verification:**

```bash
rtk grep -n "M5.1" TASKS.md
```

## Final Verification Gates

Run after all code moves are complete:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
rtk cargo audit
rtk cargo deny check
```

If `cargo deny check` fails on known baseline Starlark-chain advisories, report it separately from M5.1 only if this refactor did not change the dependency surface.

Because this task does not change parser/scanner hot paths, `scanner_bench` is not required unless implementation unexpectedly touches parser/scanner code.

## Review Checklist

- [ ] Diff is mostly moves/extractions, not rewrites.
- [ ] File-size budget test fails on the old oversized state and passes after refactor.
- [ ] No public API drift unless intentionally documented.
- [ ] New module boundaries are cohesive and named by responsibility.
- [ ] Visibility is as narrow as possible.
- [ ] No new dependencies.
- [ ] No non-test production `.unwrap()` / `.expect()`.
- [ ] No new `unsafe`.
- [ ] All moved public items keep doc comments.
- [ ] Existing security-sensitive tests still pass.

## Suggested Commit Split

1. `test: enforce rust file size budget`
2. `refactor: split aegis config model`
3. `refactor: split snapshot registry and retention`
4. `refactor: split supabase snapshot runtime`
5. `test: split full pipeline integration tests`
6. `test: split installer flow integration tests`
7. `docs: mark m5.1 quality gate complete`

If the user asks for fewer commits, combine adjacent refactor/test splits while keeping `TASKS.md` in the final commit.
