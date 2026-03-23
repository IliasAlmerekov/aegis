# Research

- ticket_id: P1-T1.2-runtime-context
- owner: lead_orchestrator
- status: COMPLETED

## Objective

Introduce a single config-aware `RuntimeContext` so command assessment, decisioning,
snapshot creation, and audit append all use the same initialized runtime dependencies.

## Relevant Modules

- `src/main.rs`
- `src/interceptor/mod.rs`
- `src/interceptor/scanner.rs`
- `src/config/model.rs`
- `src/config/allowlist.rs`
- `src/snapshot/mod.rs`
- `src/audit/logger.rs`
- `tests/full_pipeline.rs`

## Current Runtime Flow

1. `run_shell_wrapper` now builds one `RuntimeContext`, then uses it for
   assessment, allowlist lookup, decision flow, snapshot creation, and audit
   append in `src/main.rs:176-209`.
2. `RuntimeContext::load/new` centralize config fallback, scanner build,
   allowlist compilation, snapshot registry/runtime init, and audit logger
   wiring in `src/runtime.rs:33-70`.
3. `RuntimeContext::assess`, `allowlist_match`, `create_snapshots`, and
   `append_audit_entry` are the config-aware runtime entry points used by the
   shell-wrapper path (`src/runtime.rs:77-140`).
4. Decision logic in `src/main.rs:317-392` is now dependency-injected through
   `&RuntimeContext` instead of locally creating scanner/snapshot/audit helpers.

## Data Contracts

- Config fields relevant to runtime consistency:
  - `custom_patterns`
  - `allowlist`
  - `auto_snapshot_git`
  - `auto_snapshot_docker`
  - `ci_policy`
  - `audit`
  Defined in `src/config/model.rs:83-137`.
- `SnapshotRegistry::from_config` now reflects `auto_snapshot_git` /
  `auto_snapshot_docker` when constructing plugin lists
  (`src/snapshot/mod.rs:54-68`).
- `interceptor::scanner_for` preserves the cached builtin/custom scanner
  contract while letting runtime code bind one scanner into the context
  (`src/interceptor/mod.rs:81-120`).
- `RuntimeContext` keeps one `AuditLogger` with config-driven rotation wiring
  for the append pipeline (`src/runtime.rs:117-148`).

## Existing Tests and Gaps

- `tests/full_pipeline.rs` already covers allowlist, CI policy, audit append,
  audit rotation, and custom-pattern runtime behavior.
- `src/main.rs` unit tests cover CI policy and fallback semantics.
- `src/runtime.rs` now adds unit coverage for context-bound custom scanner build,
  fail-closed invalid scanner fallback, and shared config-driven dependencies.
- `src/snapshot/mod.rs` now adds unit coverage for config-aware plugin
  registration.
- Remaining gap: no dedicated benchmark was run because this ticket did not
  alter parser/scanner matching algorithms.

## ADR / Convention Constraints

- Keep `src/main.rs` thin (`CONVENTION.md`, Architecture Rules).
- Preserve fail-closed scanner fallback and `Block` semantics
  (`CONVENTION.md`, Security Invariants).
- Keep parser/scanner hot path synchronous
  (`CONVENTION.md`, Architecture Rules / Performance Rules).
- No dependency changes, no weakening of snapshot/audit guarantees.

## Risks and Unknowns

- `src/main.rs`, `src/interceptor/mod.rs`, and `src/snapshot/mod.rs` remain
  security-sensitive paths and require correctness/security review.
- Snapshot behavior is still best-effort; this ticket centralizes initialization
  but does not change rollback fidelity guarantees.

## Source References

- `TODO.md:64-107`
- `src/main.rs:176-209`
- `src/main.rs:317-392`
- `src/runtime.rs:13-148`
- `src/interceptor/mod.rs:68-120`
- `src/snapshot/mod.rs:54-90`
- `src/audit/logger.rs:181-279`
- `src/config/model.rs:83-137`
