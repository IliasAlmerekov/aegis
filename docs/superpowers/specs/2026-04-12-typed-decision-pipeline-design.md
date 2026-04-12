# Typed Decision Pipeline Design

**Date:** 2026-04-12
**Status:** Proposed / approved in chat, pending written-spec review

## Objective

Introduce a canonical typed planning boundary for command interception in
Aegis, so that shell-wrapper execution, watch mode, and evaluation-only JSON
all derive their decision semantics from the same source of truth.

This work is an internal refactor for phase 1. It must preserve:

- exit codes
- audit schema
- `--output json` schema and values
- snapshot ordering and best-effort behavior
- `Block` non-bypassability
- current fail-closed semantics

## Problem Statement

Aegis already has good typed building blocks, including:

- `Assessment`
- `PolicyInput`
- `PolicyDecision`
- `RuntimeContext`

The current weakness is not lack of types, but lack of a single canonical
object representing the whole decision flow. Today the orchestration is still
split across multiple places:

- `src/main.rs` assembles shell-wrapper decision flow
- `src/watch.rs` assembles a similar but partially duplicated flow
- `--output json` projects decision data from partially assembled runtime state
- snapshot and audit stages are attached later as side effects instead of being
  driven by one shared decision plan

That makes it harder to:

- prove fail-closed behavior
- prevent semantic drift between shell, watch, and evaluation paths
- review `Block` and policy invariants
- extend the system without growing new orchestration branches

## Goals

1. Create a single canonical typed planning result for all decision surfaces:
   - `aegis -c ...`
   - `aegis watch`
   - `aegis --output json`
2. Keep the parser and scanner synchronous.
3. Keep `src/main.rs` thin.
4. Preserve current external contracts exactly for phase 1.
5. Separate pure decision planning from side effects.

## Non-Goals

This phase does **not**:

- change product semantics
- change audit log schema
- change exit codes
- change `--output json`
- unify shell execution and watch execution into one runtime executor
- redesign scanner, allowlist, snapshot registry, or audit logger internals
- change snapshot behavior from best-effort to fail-hard

## Design Summary

Phase 1 introduces a two-part planning boundary:

1. **typed preparation wrapper**
2. **pure planner core**

Together they return one canonical result:

- `PlanningOutcome::Planned(InterceptionPlan)`
- `PlanningOutcome::SetupFailure(SetupFailurePlan)`

All decision surfaces consume the same `PlanningOutcome`. They may adapt it to
their own runtime contract, but they must not re-decide policy semantics.

## Core Architectural Rule

The planning boundary is canonical for command decision semantics.

That means:

- `main.rs` must not assemble `PolicyInput` directly
- `watch.rs` must not assemble `PolicyInput` directly
- `--output json` must not have separate decision semantics
- snapshot code must not decide whether snapshots are needed
- audit code must not decide what policy result occurred

## Layered Architecture

### 1. Typed preparation wrapper

The wrapper prepares the inputs and services needed for planning. It exists so
that fail-closed setup errors become typed planning results instead of leaking
back into surface-specific code.

Suggested shape:

- `prepare_and_plan(...) -> PlanningOutcome`

Responsibilities:

- load or resolve config/runtime inputs needed for planning
- build the scanner / allowlist resolver / config view needed by planning
- detect fail-closed setup errors
- map those errors into `SetupFailurePlan`
- call the pure planner core on success

The wrapper must remain narrow. It is not allowed to grow into a new runtime
orchestrator.

### 2. Pure planner core

The planner core is a side-effect-free decision engine.

Suggested shape:

- `plan_with_services(...) -> PlanningOutcome`

Responsibilities:

- assess the command
- build `DecisionContext`
- evaluate policy
- derive approval requirement
- derive snapshot plan
- derive execution disposition
- produce `AuditFacts`
- return `InterceptionPlan`

It must be pure and fail-closed.

### 3. Surface adapters

Surface adapters receive a `PlanningOutcome` and adapt it to the existing
surface contract.

Surfaces in phase 1:

- shell-wrapper adapter
- evaluation / JSON adapter
- watch adapter

They may:

- render UI or machine output
- execute a child process
- create snapshots when the plan requires them
- append audit entries

They must **not**:

- recompute policy semantics
- reinterpret setup failures into different decision meaning
- decide snapshot requirement
- invent audit-policy semantics

### 4. Side-effect services

Side-effect services remain downstream from planning:

- approval service
- snapshot service
- execution adapters
- audit projection / append service

These services should stay dumb:

- approval service does not evaluate policy
- snapshot service does not decide whether a snapshot is required
- execution service does not recompute risk or policy
- audit service projects facts and outcomes, it does not decide them

## Typed Results

### `PlanningOutcome`

Canonical phase-1 result:

- `Planned(InterceptionPlan)`
- `SetupFailure(SetupFailurePlan)`

### `InterceptionPlan`

Phase-1 canonical decision plan:

- `assessment: Assessment`
- `decision_context: DecisionContext`
- `policy_decision: PolicyDecision`
- `approval_requirement: ApprovalRequirement`
- `snapshot_plan: SnapshotPlan`
- `execution_disposition: ExecutionDisposition`
- `audit_facts: AuditFacts`

`InterceptionPlan` must not be freely assembled by surface code. It should be
constructed only inside the planning layer so invariants stay centralized.

### `SetupFailurePlan`

Typed fail-closed setup outcome:

- `kind: SetupFailureKind`
- `fail_closed_action: FailClosedAction`
- `user_message: String`
- `audit_facts: Option<AuditFacts>`

This is separate from `InterceptionPlan` because setup-failure paths do not
always have a normal decision subject or fully valid planning context.

### `DecisionContext`

Typed proof bundle used for policy and planning:

- `mode`
- `transport`
- `ci_state`
- `cwd_state`
- `allowlist_result`
- `applicable_snapshot_plugins`

`cwd_state` is intentionally richer than a plain path. Planning must be able to
distinguish:

- valid resolved cwd
- unavailable cwd
- other cwd states that affect policy semantics

### `ApprovalRequirement`

Minimal phase-1 shape:

- `None`
- `HumanConfirmationRequired`

### `SnapshotPlan`

Minimal phase-1 shape:

- `NotRequired`
- `Required { applicable_plugins: Vec<&'static str> }`

This is a planning result, not a record of created snapshots.

### `ExecutionDisposition`

For phase 1:

- `Execute`
- `RequiresApproval`
- `Block`

This disposition describes what the surface must do next, not how it executes
the command internally.

### `FailClosedAction`

For setup failures:

- `Deny`
- `Block`
- `InternalError`

`Deny` and `Block` are policy-like fail-closed outcomes.
`InternalError` remains fail-closed but represents a non-executable setup
failure that should preserve the current external error contract.

### `AuditFacts`

`AuditFacts` must remain strictly pre-outcome.

It may include:

- command
- assessed risk
- matched patterns
- mode
- CI detection
- allowlist matched / effective
- transport
- rationale or block reason

It must **not** include:

- final user/runtime outcome such as `Approved` or `Denied`
- created snapshots

Those are runtime outcomes added later by surface-specific adapters.

## Invariants

The planning layer must enforce invariants through private constructors or
builder functions. Surface code must not be allowed to produce impossible
combinations.

Examples of invalid combinations:

- `PolicyDecision::Block` with `ApprovalRequirement::HumanConfirmationRequired`
- a planned path with an impossible `ExecutionDisposition`
- a setup-failure path without a fail-closed action
- snapshot-required policy incorrectly mapped to `SnapshotPlan::NotRequired`

Suggested approach:

- `InterceptionPlan::from_policy(...)`
- internal planning builders / constructors

## Role of `decision.rs`

In phase 1, `src/decision.rs` should remain a pure policy kernel or become an
internal planning dependency. It should not coexist with a second orchestration
layer that rebuilds the same semantics elsewhere.

The desired relationship is:

- `decision.rs` computes policy
- planning layer assembles canonical decision semantics around that kernel

## Role of `RuntimeContext`

`RuntimeContext` should remain a dependency container and helper surface.

Good responsibilities:

- scanner access
- allowlist resolver access
- config view
- snapshot registry access
- audit logger access

Bad responsibility:

- assembling decision semantics in pieces across runtime entrypoints

Phase 1 should move decision assembly into planning and keep `RuntimeContext`
from becoming a second decision-maker.

## Data Flow

### Normal planned path

1. surface calls `prepare_and_plan(...)`
2. wrapper prepares planning services
3. wrapper calls `plan_with_services(...)`
4. planner returns `PlanningOutcome::Planned(plan)`
5. surface adapter interprets the plan only through existing contract rules

### Setup-failure path

1. surface calls `prepare_and_plan(...)`
2. preparation hits a fail-closed setup error
3. wrapper returns `PlanningOutcome::SetupFailure(setup_failure)`
4. surface adapter projects that into current shell/watch/json behavior

## Side-Effect Ordering

Phase 1 preserves current ordering.

For command flows requiring approval:

1. receive `ExecutionDisposition::RequiresApproval`
2. obtain human outcome
3. if the outcome is approval and `snapshot_plan` requires snapshots, attempt
   snapshots
4. execute command
5. append audit entry from `audit_facts + runtime outcome`

Important phase-1 rule:

- snapshot creation remains best-effort
- partial snapshot failure keeps current behavior
- no snapshot is created before approval solely because of this refactor

## Wrapper Invariant

`prepare_and_plan(...)` must not perform side effects beyond preparation needed
to compute planning semantics; it must not execute commands, create snapshots,
render UI, or append audit entries.

## Files and Module Strategy

Suggested new planning area:

- `src/planning/mod.rs`
- `src/planning/types.rs`
- `src/planning/builder.rs`

Expected touched files:

- `src/main.rs`
- `src/watch.rs`
- `src/policy_output.rs`
- `src/runtime.rs`
- `src/decision.rs`
- `src/lib.rs`
- new `src/planning/...`

Files that should avoid broad refactors in phase 1:

- `src/interceptor/scanner.rs`
- `src/config/allowlist.rs`
- `src/snapshot/mod.rs`
- `src/audit/logger.rs`

## Migration Plan

### Step 1. Add planning types and constructors

Introduce the new planning module and centralize invariants there.

### Step 2. Add canonical planning APIs

Add:

- `prepare_and_plan(...)`
- `plan_with_services(...)`

Only these APIs should assemble canonical planning semantics.

### Step 3. Move shell-wrapper to planning

`main.rs` should stop assembling `PolicyInput`, approval requirement, and
snapshot gating directly. It should consume `PlanningOutcome` and adapt it.

### Step 4. Move `--output json` to planning

`policy_output.rs` should render the existing contract from `PlanningOutcome`
instead of from fragmented runtime data.

### Step 5. Move watch mode to planning

`watch.rs` should call the same planning APIs, but may retain surface-specific:

- execution adapter
- audit adapter
- frame/result adapter

### Step 6. Prove behavioral equivalence

Validate that phase 1 preserves:

- shell behavior
- watch behavior
- JSON behavior
- setup-failure behavior

## Error Handling

### Outside planning boundary

These remain outside:

- `clap` parse failures
- Tokio runtime build failures
- broken process/stdout/stderr infrastructure failures

These are process-level bootstrap problems, not command decision semantics.

### Inside planning boundary

These should become `SetupFailurePlan` when they are fail-closed:

- invalid config
- scanner / pattern setup failure
- cwd unavailable when that affects policy semantics
- allowlist context ambiguity
- similar command-setup failures that must not silently allow execution

## Test Strategy

### 1. Planner unit tests

Add direct tests for:

- `Safe` -> `Planned` + `Execute`
- `Warn` protect path -> `RequiresApproval`
- `Danger` with applicable snapshots -> `RequiresApproval` +
  `SnapshotPlan::Required`
- `Block` -> `Block`
- strict-mode non-safe without override -> `Block`
- allowlist override path -> planned execution
- setup-failure mapping -> `SetupFailure`

### 2. Invariant tests

Verify impossible combinations cannot be created through the planning API.

### 3. Shell-wrapper regressions

Verify:

- same exit codes
- same approval/block behavior
- same snapshot ordering
- same audit behavior

### 4. JSON golden tests

Add regression coverage proving:

- same fields
- same values
- same `--output json` exit codes

### 5. Watch regressions

Verify watch uses the common planner and preserves existing frame/result
contracts.

### 6. Setup-failure equivalence tests

Explicitly cover:

- config load / validation failure
- cwd-related fail-closed planning behavior
- setup-failure projection in shell
- setup-failure projection in watch
- setup-failure projection in `--output json`

## Acceptance Criteria

This design is successful for phase 1 when all of the following are true:

### Architecture

- `main.rs` does not assemble `PolicyInput`
- `watch.rs` does not assemble `PolicyInput`
- `--output json` has no separate decision semantics
- `RuntimeContext` remains a dependency container rather than a decision-maker
- planning is the single canonical decision boundary

### Behavior

- exit codes are unchanged
- audit schema is unchanged
- `--output json` is unchanged externally
- snapshot ordering is unchanged
- fail-closed behavior is preserved
- `Block` remains impossible to bypass

### Scope discipline

- phase 1 avoids large opportunistic refactors in scanner, allowlist, snapshot,
  and audit internals
- shell/watch execution remain surface-specific adapters
- side effects are not over-unified in this phase

### Verification

- planner unit tests exist
- JSON golden / regression tests exist
- setup-failure equivalence tests exist
- existing surface tests remain green with equivalent behavior

## Deferred Follow-Ups

The following are intentionally deferred beyond phase 1:

- unifying execution result models across shell and watch
- publishing `InterceptionPlan` externally in JSON
- audit schema additions based on plan internals
- broad snapshot-layer redesign
- broad runtime / service abstraction cleanup unrelated to canonical planning
