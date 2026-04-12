# P2 Production Polish Summary

## Triage Evidence

### tests/watch_mode.rs
- File / test: `watch_mode_audit_entry_sets_transport_watch`
- Evidence: test was previously ignored because `AuditLogger` lacked `AEGIS_AUDIT_PATH` support
- Decision: Fix now
- Result: fixed by honoring `AEGIS_AUDIT_PATH` in `src/audit/logger.rs` and re-enabling the test

### tests/audit_integrity.rs
- File / test: entire file
- Evidence: coverage appears active; no ignored tests are present, but this file still needs baseline confirmation rather than immediate code changes
- Decision: Keep under verification, no immediate fix

### tests/snapshot_integration.rs
- File / test: `git_snapshot_and_rollback_work_from_git_worktree`
- Evidence: helper `add_worktree(...)` can cause an early return, so the test may not exercise the intended path in every environment
- Decision: Defer

## Known Limitations / Deferred Follow-Ups

- **Issue:** `tests/snapshot_integration.rs::git_snapshot_and_rollback_work_from_git_worktree` can return early when `git worktree add` is unavailable or unsupported in the test environment, which weakens confidence in that specific scenario.
- **Why deferred:** making this fully deterministic may require broader environment/test-fixture decisions rather than a local confidence fix.
- **Why acceptable now:** this affects one environment-sensitive integration scenario only; core snapshot, rollback, and audit confidence still come from the active snapshot integration coverage plus the full baseline.
- **Next step:** decide whether to redesign the test fixture to make worktree support deterministic or move this case into an explicitly environment-gated integration suite.
- **Owner / destination:** follow-up phase / backlog item for snapshot-integration hardening.

## Baseline Summary

- `rtk cargo fmt --check` —
- `rtk cargo clippy -- -D warnings` —
- `rtk cargo test` —
- `rtk cargo bench --bench scanner_bench` —
- `rtk cargo audit` —
- `rtk cargo deny check` —
