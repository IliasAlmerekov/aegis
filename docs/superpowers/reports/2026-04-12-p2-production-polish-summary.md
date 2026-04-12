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
- Evidence: helper `add_worktree(...)` can cause an early return, which weakens confidence because the test may silently skip itself in some environments
- Decision: Defer unless a truly local/safe fix becomes obvious during implementation

## Known Limitations / Deferred Follow-Ups

- **Issue:**
- **Why deferred:**
- **Why acceptable now:**
- **Next step:**
- **Owner / destination:**

## Baseline Summary

- `rtk cargo fmt --check` —
- `rtk cargo clippy -- -D warnings` —
- `rtk cargo test` —
- `rtk cargo bench --bench scanner_bench` —
- `rtk cargo audit` —
- `rtk cargo deny check` —
