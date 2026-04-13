# Registry-Based Extension Points Design

**Date:** 2026-04-13  
**Status:** Drafted from approved brainstorming; pending written-spec review

## Goal

Clarify and normalize Aegis's existing extension and source-of-truth
boundaries without introducing a broad new registry architecture.

Chosen scope:

- normalize existing source-of-truth APIs
- clarify ownership, provenance, and effective/runtime views
- keep changes thin and local
- avoid naming-driven redesign

This effort is about **clearer boundaries and more reviewable composition**,
not about introducing a new cross-cutting abstraction.

## Scope

### In scope

- thin docs and API alignment across patterns, snapshots, and allowlist layers
- targeted cleanup that resolves concrete ambiguities in ownership, provenance,
  or materialization
- clearer canonical facade entry points
- preserving existing semantics while making extension boundaries easier to
  reason about

### Out of scope

- general `Registry` trait or framework
- mass renaming to `*Registry`
- semantic changes to pattern merge order or duplicate handling
- semantic changes to snapshot policy behavior
- semantic changes to config-layer precedence or allowlist matching behavior
- any untrusted runtime extension or code loading model

## Approach Options

### Option 1: Thin alignment first, then targeted cleanups (**approved**)

Start with a thin docs/API-first pass across all three zones, then perform
targeted internal cleanup in this order:

1. `patterns.rs`
2. `snapshot/`
3. `allowlist.rs`

**Pros:** consistent vocabulary; low risk; good reviewability  
**Cons:** the first pass is intentionally small and may not look dramatic

### Option 2: Patterns-first with opportunistic alignment

Start with `patterns.rs` and only align other zones as needed during cleanup.

**Pros:** concrete improvement lands quickly  
**Cons:** risks setting a local style before a shared vocabulary is agreed

### Option 3: Explicit registry-vocabulary refactor

Introduce a more formal registry vocabulary and reshape types around it where
possible.

**Pros:** stronger symmetry across zones  
**Cons:** dangerously close to naming-driven redesign; too broad for this phase

## Approved Direction

Use **Option 1**:

> thin alignment first, then targeted cleanups  
> first docs/API-first pass across all three zones, then patterns → snapshot →
> allowlist

Core rule:

> local clarity first, cross-cutting consistency second, no
> registry-by-renaming

## Scope, Vocabulary, and Rollout

### Chosen scope

This is **scope A**:

- normalize existing source-of-truth APIs
- clarify ownership and provenance
- keep changes thin and local
- avoid broad new abstractions

### Explicit non-goals

- no general `Registry` trait or framework
- no naming-driven redesign
- no mass renaming to `*Registry` for symmetry
- no semantic changes to pattern loading, snapshot applicability, or layered
  config precedence

### Shared vocabulary

This initiative uses four descriptive roles:

- **source of truth** — where the canonical set or canonical definition lives
- **provenance** — where an element or rule came from
- **effective view** — what is active after merge or precedence resolution
- **runtime/materialized view** — what has been shaped into execution-ready
  form

Vocabulary is descriptive first, not prescriptive.

These terms help explain the current code; they do not require identical type
shapes across `patterns`, `snapshot`, and `allowlist`.

### Zone-specific intent

- `patterns.rs`
  - likely source-of-truth plus effective-set boundary
  - closest thing to “registry-like”, but not required to be renamed
- `snapshot/`
  - already contains a real `SnapshotRegistry`
  - focus is on clarifying registration and materialization boundaries
- `allowlist.rs`
  - primary role is layered provenance plus effective rule evaluation
  - not required to look like a registry in the snapshot sense

### Rollout

#### Pass 0 — thin docs/API alignment across all three zones

Allowed:

- doc comments
- naming clarification
- thin facade or helper methods
- minimal type or API cleanup without semantic drift

Pass 0 must not introduce new cross-zone invariants unless those invariants
already exist implicitly in the current code.

No public API churn beyond thin clarification or facade cleanup is expected in
Pass 0.

#### Then targeted cleanup order

1. `patterns.rs`
2. `snapshot/`
3. `allowlist.rs`

## Concrete Shape Per Zone

### `patterns.rs`

#### Intended shape

`patterns.rs` should clearly answer:

1. where the canonical pattern source lives
2. where built-in and custom patterns are merged
3. which structure is the effective runtime set for scanner consumption

#### Practical direction

- `PatternSet` can likely remain the main effective-set type
- docs and APIs should make it explicit that this is the canonical merged
  pattern view for scanning
- `PatternSource` remains explicit provenance

Scanner-facing behavior must continue to depend on the effective merged set,
not on source-specific branching outside `patterns.rs`.

#### Allowed cleanups

- stronger docs for `Pattern`, `PatternSource`, and `PatternSet`
- thin helper or facade methods that expose:
  - built-in source
  - merged source set
  - effective runtime set
- small naming improvements for internal helpers that remove ambiguity

#### Not allowed

- renaming to `PatternRegistry` just for symmetry
- new registry framework
- semantic changes to merge order
- semantic changes to duplicate-id rejection

#### Cleanup success criterion

This cleanup must resolve a concrete ambiguity in source, provenance, or
effective-view role.

Tests should pin the canonical scanner-facing entry point that consumes the
effective merged pattern set.

### `snapshot/`

#### Intended shape

`snapshot/` already contains a real `SnapshotRegistry`.

The goal is to clarify the boundary between:

- available providers
- configured or materialized provider set
- runtime use of that set

#### Practical direction

Clarify:

- what the registry stores
- when a provider is “available”
- when a provider has been materialized for the current runtime config
- how rollback-oriented registry construction differs from execution-time
  registry construction

“Available providers” should mean providers known to the binary or runtime,
not providers already approved for the current command path.

#### Allowed cleanups

- stronger docs around `SnapshotRegistry`, `from_config`, and `for_rollback`
- thin helpers that separate:
  - registered built-ins
  - config-filtered runtime set
  - rollback-capable set
- light internal cleanup around provider resolution and materialization

#### Not allowed

- dynamic plugin architecture
- untrusted runtime extension loading
- weakening snapshot-policy behavior
- any implicit “no provider found => allow silently” behavior

#### Cleanup success criterion

This cleanup must remove a concrete ambiguity in registration,
materialization, or provider resolution.

### `allowlist.rs`

#### Intended shape

`allowlist.rs` is not required to look like a registry.

Its primary role is:

- layered provenance
- precedence-aware effective rule view
- runtime evaluation against command context

#### Practical direction

Clarify:

- where provenance layer is stored
- where the effective layered view is formed
- where compiled or runtime matching happens
- which facade entry points are canonical for:
  - compile
  - match
  - advisory analysis

Advisory analysis must remain clearly non-authoritative relative to
compiled/runtime matching behavior.

#### Allowed cleanups

- stronger docs for `LayeredAllowlistRule`, `Allowlist`, and `AllowlistMatch`
- thin helper or facade methods that separate:
  - layered input
  - compiled effective rule set
  - runtime match result
- small internal cleanup if it makes precedence reasoning clearer

#### Not allowed

- weakening config-layer precedence
- changing scope requirements
- forcing allowlist into a registry abstraction just for symmetry
- any miss behavior that could imply allow-by-default

#### Cleanup success criterion

This cleanup must remove a concrete ambiguity in provenance, effective view,
or runtime matching boundary.

Tests should preserve the distinction between advisory analysis APIs and
authoritative runtime match APIs.

## Testing, Reviewability, and Success Criteria

### Cross-zone testing expectations

This initiative must not rely on “it was only docs/API cleanup.”

Each zone needs evidence that clarity improved without semantic drift.

#### Patterns

Preserve and confirm:

- built-in and custom merge behavior
- duplicate-id rejection
- provenance visibility through `PatternSource`
- scanner-facing behavior continues to depend on the effective merged set, not
  on source-specific branching outside `patterns.rs`

#### Snapshot

Preserve and confirm:

- available providers = providers known to the binary or runtime
- configured/materialized set = providers enabled for the current runtime config
- execution-time applicability remains distinct from availability
- rollback-capable set remains explicit and not conflated with execution-time
  configured set

#### Allowlist

Preserve and confirm:

- config-layer precedence
- effective layered rule behavior
- runtime match behavior
- advisory analysis remains clearly non-authoritative relative to compiled or
  runtime matching behavior

### Reviewability rules

This work is successful only if:

- Pass 0 is visibly thin
- no new cross-zone architecture appears by implication
- each targeted cleanup has a concrete local purpose
- a reviewer can answer, per zone:
  - what is the source of truth?
  - where is provenance represented?
  - what is the effective/runtime view?
  - which API is the canonical facade?

### Allowed verification style

- docs/API tests where relevant
- existing-behavior-preserved tests
- local regression tests for clarified boundaries
- no speculative future-proofing tests for abstractions that do not exist

### Rollout verification by stage

#### Pass 0 — docs/API-first alignment

Verify:

- terminology is consistent
- public docs/comments do not overclaim
- main facade entry points are easier to identify
- no semantic behavior changed
- no public API churn beyond thin clarification/facade cleanup

#### Patterns cleanup

Verify:

- merge order unchanged
- effective-set semantics unchanged
- scanner still consumes the merged effective set as the canonical boundary

#### Snapshot cleanup

Verify:

- provider availability vs config materialization vs applicability are clearer
- no change to snapshot policy semantics
- no implicit allow behavior from provider resolution ambiguity

#### Allowlist cleanup

Verify:

- layered precedence unchanged
- compiled matching unchanged
- advisory analysis stays clearly secondary to actual matching behavior
- tests preserve the distinction between advisory analysis APIs and
  authoritative runtime match APIs

### Success criteria

#### Architecture clarity

- each zone has a clearer statement of:
  - source of truth
  - provenance
  - effective/runtime view
  - canonical facade
- vocabulary is consistent across zones without forcing identical type
  structures

#### Scope discipline

- no new broad registry abstraction
- no rename-to-registry campaign
- no semantic redesign in patterns, snapshot policy, or layered config behavior

#### Safety

- no registry miss or lookup ambiguity introduces implicit allow behavior
- no weakening of config-layer precedence
- no untrusted runtime code loading or plugin expansion model

#### Maintainability

- future contributors can find the relevant boundary faster
- extension points are clearer without becoming broader than needed
- the code explains current composition rules better than before

## Follow-On

After this written spec is approved, the next step is implementation planning
for the thin alignment pass followed by patterns, snapshot, and allowlist
cleanup in that order.
