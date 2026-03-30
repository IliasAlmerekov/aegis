---
phase: 01-contract-integrity
plan: "01"
subsystem: testing
tags: [rust, integration-tests, snapshot, subprocess, audit]

# Dependency graph
requires: []
provides:
  - Subprocess regression coverage for SnapshotRegistry git-off flag
  - Subprocess regression coverage for SnapshotRegistry docker-off flag
  - read_stub_invocations helper for PATH-stub invocation assertions
affects: []

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "PATH-stub + invocation log: write_executable + AEGIS_TEST_*_LOG env var to assert external binaries were never called"
    - "Config injection via .aegis.toml in TempDir workspace: standard pattern for end-to-end flag coverage"

key-files:
  created: []
  modified:
    - tests/full_pipeline.rs

key-decisions:
  - "Both snapshot flags set to false in every new regression to isolate the flag under test from unrelated noise"
  - "PATH stub writes to log file via env var AEGIS_TEST_*_LOG so log path is test-controlled and collision-free"
  - "Assert stub log absent/empty (not just audit snapshots empty) to prove plugin not registered, not merely skipped by is_applicable"

patterns-established:
  - "Stub invocation assertion: read_stub_invocations(log_path) returns Vec<String>; empty means binary was never called"
  - "Snapshot-off regression: TempDir home + TempDir workspace + explicit .aegis.toml + PATH stub + AEGIS_FORCE_INTERACTIVE=1 + Danger command"

requirements-completed:
  - T1.3-GIT-OFF
  - T1.3-DOCKER-OFF

# Metrics
duration: 15min
completed: 2026-03-30
---

# Phase 01 Plan 01: Snapshot Registry Config-Flag Regression Tests Summary

**Two real-binary subprocess regressions proving SnapshotRegistry never invokes git or docker when their config flags are false, verified via PATH stubs and audit JSONL assertions**

## Performance

- **Duration:** 15 min
- **Started:** 2026-03-30T17:33:10Z
- **Completed:** 2026-03-30T17:48:00Z
- **Tasks:** 2
- **Files modified:** 1

## Accomplishments

- Added `read_stub_invocations(log_path)` helper to `tests/full_pipeline.rs` for asserting PATH-stubbed binaries were never called
- Added `snapshot_registry_git_flag_false_skips_plugin_and_audit` regression — proves git plugin never registers when `auto_snapshot_git = false`
- Added `snapshot_registry_docker_flag_false_skips_plugin_and_audit` regression — proves docker plugin never registers when `auto_snapshot_docker = false`
- Both tests pass with zero production code changes, confirming D-02 and D-03 hold

## Task Commits

Each task was committed atomically:

1. **Task 1 + 2: Add stub helper and both snapshot regressions** - `7a4e777` (test)
2. **Style fix: rustfmt on new tests** - `f79cde9` (style)

## Files Created/Modified

- `/home/iliasalmerekov/Projects/aegis/tests/full_pipeline.rs` - Added `read_stub_invocations` helper and two snapshot config-flag regression tests

## Decisions Made

- Both `auto_snapshot_git` and `auto_snapshot_docker` are set to `false` in both regression tests to keep assertions fully isolated. The test under review only controls one flag; disabling the other prevents noise from the other plugin.
- The stub logs invocations via an env-var-controlled path (`AEGIS_TEST_GIT_LOG` / `AEGIS_TEST_DOCKER_LOG`) so each test gets its own collision-free log file under the TempDir workspace.
- Assert the log is absent/empty (not just `snapshots == []`) to distinguish "plugin not registered" from "plugin registered but skipped by `is_applicable`" — the stronger claim the research identified as necessary.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Applied rustfmt to new test functions**
- **Found during:** Overall verification (`cargo fmt --check`)
- **Issue:** Two `assert_eq!` calls with message arguments were not formatted per rustfmt's multi-line style
- **Fix:** Ran `cargo fmt` to auto-apply canonical formatting
- **Files modified:** `tests/full_pipeline.rs`
- **Verification:** `cargo fmt --check` passes; both snapshot tests still green
- **Committed in:** `f79cde9`

---

**Total deviations:** 1 auto-fixed (formatting, Rule 1)
**Impact on plan:** No scope creep; formatting is a CLAUDE.md/CONVENTION.md requirement.

## Issues Encountered

None — production code was not touched because code inspection (D-02, D-03) was confirmed correct by the passing tests.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Ticket 1.3 contract gap (D-04/D-05/D-06) is now closed with executable regression coverage at the subprocess boundary.
- No production snapshot, runtime, UI, or audit wiring was modified; D-01 through D-03 hold as written.
- Phase 01 can proceed to verification or the next plan.

---
*Phase: 01-contract-integrity*
*Completed: 2026-03-30*

## Self-Check: PASSED

- `tests/full_pipeline.rs` — FOUND
- Commit `7a4e777` — FOUND (test: add stub helper + git-off + docker-off regressions)
- Commit `f79cde9` — FOUND (style: rustfmt fix)
