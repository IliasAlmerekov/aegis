# Lazy Initialization for Non-Critical Subsystems Design

**Date:** 2026-04-13  
**Status:** Drafted from approved brainstorming; pending written-spec review

## Goal

Reduce unnecessary startup and non-critical-path work in Aegis without
changing the core security model or decision pipeline.

Chosen scope:

- conservative, subsystem-specific laziness only
- preserve current external semantics
- keep core decision-path wiring eager
- classify deferred failures explicitly

This effort is about **safer readiness optimization**, not runtime lifecycle
redesign.

## Scope

### In scope

- lazy snapshot registry and plugin materialization
- lazy audit internals only
  - rotation helpers
  - archive scanning and maintenance helpers
  - query-side helper state, where relevant
- explicit late-init failure classification for deferred paths

### Out of scope

- `Config::load()`
- `Allowlist::new(...)`
- `scanner_for(...)`
- `detect_effective_user()`
- deeper laziness inside scanner or pattern classification
- reusable lazy subsystem framework
- broad runtime lifecycle redesign

## Approach Options

### Option 1: Conservative defer-points only (**approved**)

Apply laziness only to the safest defer-points:

- snapshot registry and plugin instantiation
- audit internals, while preserving an eager logger contract

**Pros:** minimal semantic risk; reviewable; rollback-friendly  
**Cons:** startup savings may be moderate rather than dramatic

### Option 2: Conservative defer-points plus richer late-init contract

Do Option 1, but also add broader typed activation-state structure around
deferred subsystem helpers.

**Pros:** more structured observability for late-init failures  
**Cons:** larger design surface; greater risk of over-architecting

### Option 3: General lazy-subsystems framework

Introduce reusable lazy wrappers or generic deferred-init state machines for
multiple runtime subsystems.

**Pros:** potentially reusable for future work  
**Cons:** premature abstraction; too broad for this phase; weaker reviewability

## Approved Direction

Use **Option 1** with limited discipline from Option 2:

> conservative, subsystem-specific laziness only; explicit deferred failure
> behavior; no broad runtime redesign

Guardrails:

1. no reusable lazy framework in this phase
2. preserve current external semantics
3. classify late-init failures explicitly
4. no hidden fallback to “just continue”

## Architecture Overview

### Boundary model

Lazy work may optimize readiness, but must not move core security decisions
onto a later, less observable path.

Allowed laziness applies only to subsystems that:

1. are not needed on most safe-command paths
2. are not part of the core classification contract
3. can defer internal work without weakening security semantics

### Eager vs lazy boundary

#### Eager runtime contract remains intact for:

- config load and validation
- allowlist construction
- scanner construction
- effective-user detection
- audit config parsing and logger base contract

#### Lazy work is allowed for:

- snapshot registry and plugin materialization on snapshot-eligible paths
- audit helper activation that is not required to preserve the append-only
  logging contract itself

## Stage 1: Snapshot laziness

### Objective

Delay snapshot registry and plugin materialization until a command has already
reached a path where snapshotting is relevant.

### Core rule

Deferred snapshot init must not occur before a command has already been
classified into a path that is eligible for snapshotting.

This means laziness starts **after** classification; it does not participate
in risk determination.

### Additional rule

Lazy snapshot materialization may depend on runtime command context already
available at snapshot time, but must not require reclassification.

There must be no second decision pipeline.

### Allowed pattern

Allowed:

- eager retention of snapshot policy values in runtime config
- on-demand registry or plugin creation only when execution reaches a
  snapshot-eligible path
- local helper methods such as `ensure_snapshot_registry()`

Not allowed:

- constructing snapshot machinery on every command path “just in case”
- probing snapshot applicability before classification outcome exists
- hiding deferred init failure behind “not applicable”

### Failure semantics

Late snapshot init failure must:

- be classified explicitly as **snapshot-path failure**
- remain visible to the operator
- avoid generic, context-free failure handling
- avoid changing prior classification semantics

Lazy snapshot behavior must not overstate snapshot guarantees; best-effort
snapshotting remains best-effort.

## Stage 2: Audit internals laziness

### Objective

Keep the audit logger contract eager while deferring non-critical helper work
until first append or query that actually needs it.

### Core rule

Helper activation failure must not invalidate or silently skip the append-only
logging contract itself.

Lazy helper failure is not permission to lose the base append path.

### Additional rule

If append-time helper activation fails, the primary append attempt must still
follow the existing logger contract unless the current implementation already
treats that condition as a hard failure.

This avoids accidental stronger-or-weaker behavior drift.

### Allowed pattern

Eager:

- parse and validate audit config
- establish logger path and base runtime contract

Lazy:

- rotation scanning and archive maintenance helpers
- archive discovery or query-side helper state
- other non-critical helper activation tied to append or query time

Not allowed:

- fully lazy logger creation
- moving the append contract itself to first use
- silently suppressing helper activation failure when it changes observability

## State discipline

Any memoized field must have deterministic, single-meaning state transitions.

For example, state must not overload a single `None` or equivalent to mean:

- not started
- failed
- unavailable

If those distinctions matter, they must be represented explicitly.

## Implementation Style Constraints

Allowed patterns in this phase:

- `LazyLock`
- local memoized fields such as `Option<T>` or a small explicit state enum
- narrow helper methods like:
  - `ensure_snapshot_registry()`
  - `ensure_rotation_helper()`

Not allowed in this phase:

- reusable lazy framework
- generic subsystem state machine layer
- cross-cutting abstraction created mainly for future reuse

## Testing and Verification

### Snapshot stage

- tests that safe and non-snapshot paths do not materialize snapshot machinery
- tests that snapshot-eligible paths materialize it exactly when needed
- tests that late snapshot init failure is surfaced as snapshot-path failure
- tests that no reclassification or policy drift is introduced

### Audit stage

- tests that the logger contract remains available eagerly
- tests that helpers are not activated before the first relevant append or query
- tests that helper activation failure is surfaced explicitly
- tests that append continues to follow the existing contract unless current
  behavior already hard-fails

### Regression emphasis

For both stages:

- existing-behavior-preserved tests
- lazy-boundary tests
- failure-classification tests
- no-hidden-fallback regressions

### Performance note

Each stage should record what eager work was removed from the non-critical or
startup path, and whether hot-path benchmarking was rerun or considered
unnecessary.

## Reviewability Rules

This refactor is acceptable only if:

- each defer-point is local and easy to reason about
- late-init behavior is explicitly described
- the diff reads as targeted on-demand helper activation, not runtime redesign
- snapshot and audit changes remain separable in rollout and rollback

## Rollback Strategy

Rollback remains staged:

- **Stage 1 rollback:** revert snapshot laziness only
- **Stage 2 rollback:** revert audit-helper laziness while keeping the eager
  audit contract intact

This keeps the initiative locally reversible and easy to review.

## Success Criteria

### Architecture

- laziness is limited to non-critical subsystem work
- core decision pipeline remains eager and unchanged
- no reusable lazy framework is introduced

### Correctness

- deferred init never changes classification semantics
- deferred init never creates hidden fallback behavior
- late-init failures remain explicit and separately classified
- audit append-only contract remains intact
- snapshot activation never starts before a snapshot-eligible path exists

### Performance intent

- unnecessary startup or non-critical work is reduced
- safe-command paths do not gain new semantic complexity
- deferred work is moved off paths that do not need it

### Scope control

- no changes to scanner or pattern laziness
- no changes to config, allowlist, or effective-user startup contract
- no broad refactor of unrelated runtime wiring

## Follow-On

After this written spec is approved, the next step is implementation planning
for the conservative snapshot-first, audit-second rollout.
