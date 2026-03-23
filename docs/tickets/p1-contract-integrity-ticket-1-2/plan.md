# Plan

- ticket_id: P1-T1.2-runtime-context
- owner: lead_orchestrator
- status: PLANNED

## Milestones

1. Introduce a single runtime context and move config-aware dependency
   initialization into it.
2. Rewire shell-wrapper flow to pass the context through assessment, policy,
   snapshot, and audit paths.
3. Add regression coverage for config-aware snapshot registry construction and
   runtime-context behavior.
4. Run tester/reviewer/security_reviewer loop until all stages are `PASSED`.

## Task Graph

1. Add `RuntimeContext` + focused runtime helpers (`scanner`, `policy_engine`,
   `snapshot_registry`, `audit_logger`, snapshot runtime).
2. Add explicit constructors for config-aware dependencies where needed
   (especially snapshot registry and audit logger wiring).
3. Switch `main` runtime path to construct `RuntimeContext` once and thread it
   through decision/audit helpers.
4. Add/adjust unit + integration tests.
5. Run verification and review stages; fix any findings.

## Task Details

### Task 1 — RuntimeContext

- Owner: coder
- Scope:
  - create a dedicated runtime module
  - define `RuntimeContext`
  - centralize config load fallback + subsystem construction
- Dependencies: none
- Verification:
  - unit tests for context-built snapshot registry / scanner fallback behavior
- Rollback:
  - remove runtime module and restore prior direct helper construction

### Task 2 — Runtime flow rewiring

- Owner: coder
- Scope:
  - make `run_shell_wrapper` build one context
  - pass context into assessment, decision, snapshot, and audit append paths
  - remove local `SnapshotRegistry::default()` / runtime-path scanner creation
- Dependencies: Task 1
- Verification:
  - compile/test full shell-wrapper path
- Rollback:
  - restore helper signatures and prior direct constructors

### Task 3 — Regression coverage

- Owner: tester
- Scope:
  - add tests for config-aware snapshot plugin registration
  - add tests for runtime path consistency as needed
- Dependencies: Tasks 1-2
- Verification:
  - `rtk cargo test`
  - targeted unit/integration cases
- Rollback:
  - remove only new tests if they prove invalid, not runtime fixes

## Verification Plan

- `rtk cargo fmt --check`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo test`
- `rtk cargo test --test full_pipeline`

No parser/scanner hot-path algorithm change is planned, so benchmark run is
optional unless reviewer/tester finds a perf-sensitive regression.

## Rollback Plan

If the refactor introduces regressions:

1. revert runtime-path wiring to the pre-context flow,
2. keep only isolated constructor helpers that are correctness-neutral,
3. preserve any new tests that document the intended config/runtime contract.

## Confirmation

Human instruction for this turn explicitly requested continuing the
lead-orchestrated flow for Ticket 1.2 through tester/reviewer/security stages.

## Next Owner

coder
