# Coder Handoff

- owner: coder
- status: PASSED

## Changed Files

- `src/runtime.rs`
- `src/main.rs`
- `src/interceptor/mod.rs`
- `src/snapshot/mod.rs`
- `src/lib.rs`

## Decisions

- Added `RuntimeContext` to centralize config fallback, scanner binding,
  allowlist compilation, snapshot registry/runtime setup, and audit logger
  construction.
- Rewired `run_shell_wrapper` / `decide_command` to pass `&RuntimeContext`
  instead of creating scanner/snapshot/audit dependencies inside helper
  functions.
- Added `SnapshotRegistry::from_config(&Config)` so snapshot plugins now honor
  `auto_snapshot_git` / `auto_snapshot_docker`.
- Exposed `interceptor::scanner_for(...)` so runtime wiring can keep the cached
  scanner contract while still binding one scanner instance into the context.

## Known Risks

- Touches security-sensitive runtime paths (`src/main.rs`, `src/interceptor/mod.rs`,
  `src/snapshot/mod.rs`); requires explicit reviewer/security review.
- Snapshot runtime availability remains best-effort by design; failure still
  degrades to "no snapshots" without auto-approval.

## Next Owner

tester
