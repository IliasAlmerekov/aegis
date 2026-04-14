# Structured Explainability and Audit Reasons Design

**Date:** 2026-04-13  
**Status:** Drafted from approved brainstorming; pending written-spec review

## Goal

Make explanation a first-class internal model across Aegis's real pipeline so
scanner facts, policy rationale, audit serialization, and UI messaging share a
common typed boundary.

Chosen scope:

- unified internal explanation contract
- scanner → policy → audit → UI consistency
- no new public `aegis explain` surface in this phase
- no semantic drift in decision behavior

This effort is about **internal explanation model consistency**, not about
adding a new user-facing diagnostics product.

## Scope

### In scope

- thin internal explanation types
- explanation ownership by layer
- unified explanation envelope assembled along the existing pipeline
- audit and UI migration to consume the unified model
- richer structured reasons in audit and UI where already justified

### Out of scope

- a new `aegis explain` command
- any second policy engine or post-hoc decision reconstruction
- semantic changes to allowlist precedence, block reasons, snapshot
  requirements, or CI/mode interpretation
- broad diagnostic architecture designed around future CLI surfaces

## Approach Options

### Option 1: Thin model-first contract, then consumer migration (**approved**)

Introduce a small internal explanation contract first, then migrate consumers:

1. model first
2. audit consumer
3. UI consumer

**Pros:** fixes boundary inconsistency at the source; preserves scanner and
policy ownership; reviewable rollout  
**Cons:** the first step is internal and may show less immediate user-visible
  impact

### Option 2: Audit-first explanation normalization

Start with the audit log and grow a model around its needs.

**Pros:** quickly improves JSONL richness  
**Cons:** risks forcing the model around serialization shape rather than real
  pipeline ownership

### Option 3: Policy-centered explanation normalization

Make policy the center of explanation assembly, then pull scanner, audit, and
UI around it.

**Pros:** straightforward around the final decision  
**Cons:** risks turning policy into the center of truth for explanation and
  weakening scanner-native explainability

## Approved Direction

Use **Option 1**:

> thin internal explanation contract first, then consumer migration: audit
> first, UI second

## Scope, Ownership, and Rollout

### Chosen scope

This is **scope B**:

- unified internal explanation contract
- scanner → policy → audit → UI pipeline consistency
- no new public diagnostic surface yet
- no semantic drift in current decisions

### Core principle

> explanation is descriptive, not authoritative

Explanation explains an already-determined decision. It must not become a
second policy engine or restate a decision through new logic after the fact.

### Ownership model

Each layer owns only its own explanation contribution:

- **scanner**
  - scan facts
  - matched patterns
  - highest-severity scan outcome
  - scan-derived context
  - scanner contributes scan facts and scan-derived severity/context, but not
    final policy interpretation

- **policy**
  - final rationale
  - deny / prompt / allow / block reason
  - allowlist effect on the decision
  - snapshot requirement trigger
  - policy may reference scanner facts, but must not restate them in a
    separately derived form unless that form is the canonical policy rationale

- **audit / UI**
  - consumers and projections only
  - consumers may omit or format explanation data, but must not synthesize new
    explanation facts

### Envelope rule

> explanation envelope is assembled along the real pipeline, not invented at
> the end

Scanner contributes facts, policy enriches with final rationale, and audit/UI
consume the normalized result.

### Explicit absence rule

> optional means explicit

If an explanation fact is absent, it must be represented as `None` or omitted
in the unified model rather than reconstructed later by a second code path.

### No duplicated truth rule

Do not hold the same explanation truth in two independent forms that can drift
apart.

### Stable ownership rule

The unified contract normalizes explanation pieces, but it does not rewrite
their meaning or transfer ownership away from the layer that produced them.

### Rollout

1. **Model first**
   - thin internal explanation types
2. **Audit consumer**
   - serialize explanation through the append-only audit path
3. **UI consumer**
   - render from the same explanation model instead of ad hoc stitching

## Concrete Model Shape and Layer Boundaries

### Intended shape

The model should be thin and internal-only.

It should be composed from normalized parts, not from one “smart” explanation
engine:

1. scan facts
2. policy rationale
3. execution-context facts
4. final explanation envelope

### 1. Scan facts

#### Owned by scanner

Scanner contributes:

- matched pattern IDs
- matched pattern details already known from assessment
- highest-severity scan outcome
- decision-source-like scan context
- scan-derived severity/context, but not final policy interpretation

If a primary trigger is exposed, it must already be deterministically
available from scanner output, not heuristically chosen later for
presentation.

#### Allowed content

- matched pattern IDs
- highest matched risk
- matched source kinds
- primary scan trigger, if already scanner-native
- highlight-relevant spans or references, if already scanner-native

#### Not allowed

Scanner must not:

- formulate final allow/deny/prompt/block reasons
- explain allowlist effect
- explain CI or mode policy outcome
- become a second policy narrator

### 2. Policy rationale

#### Owned by policy layer

Policy contributes:

- final rationale
- approval trigger reason
- denial or block reason
- whether allowlist materially changed the outcome
- whether snapshotting is required and why at policy level

Policy may reference scanner facts, but must not restate them in a separately
derived form unless that form is the canonical policy rationale.

#### Allowed content

- canonical final rationale enum/value
- block reason
- prompt reason
- allowlist-effective flag
- snapshot-required trigger
- execution disposition explanation

#### Not allowed

Policy must not:

- duplicate scanner match lists in its own explanation form
- independently summarize scanner facts when those facts already exist
- overwrite scanner-native truth with policy-generated commentary

### 3. Execution-context facts

#### Owned by orchestration/runtime boundary

These are real pipeline facts that neither scanner nor policy fully owns
alone:

- mode
- CI detected
- allowlist source/rule metadata if matched
- snapshot plugin chosen or attempted
- execution denial path facts
- other already-known runtime context relevant to explanation

These facts must be captured from actual runtime flow at the point they become
known, not reconstructed after the fact from audit-oriented helpers.

#### Allowed content

- effective mode
- CI detected flag
- matched allowlist source layer / pattern / reason / rule identity if known
- chosen snapshot plugins and why they were selected, if already known in the
  pipeline
- final decision transport/context if relevant

#### Not allowed

Execution-context facts must not:

- recompute scanner or policy meaning
- fabricate missing rule identity post hoc
- invent snapshot reasons that were not part of runtime flow

### 4. Final explanation envelope

#### Role

The envelope groups:

- scan facts
- policy rationale
- execution-context facts

It makes them available to downstream consumers in one normalized structure.

Envelope construction order should mirror pipeline order, so missing sections
are meaningful rather than accidental.

#### Allowed behavior

The envelope may:

- group fields coherently
- expose optional sections
- support concise UI projections
- support structured audit serialization

#### Not allowed

The envelope must not:

- derive new authoritative facts
- resolve conflicts between duplicated truths
- contain fallback “best guess” explanations

### Consumer boundary

Audit and UI are consumers:

> consumers may omit or format explanation data, but must not synthesize new
> explanation facts

### Allowed first-iteration shape

Allowed:

- 2–4 focused structs/enums
- optional sections where facts are genuinely absent
- thin build steps along the existing pipeline

Not allowed:

- giant omnibus explanation schema with speculative future fields
- explain API productization for future commands not in this phase
- duplicated fields that restate the same truth in multiple places

## Rollout by Stage, Testing, and Success Criteria

### Stage 1 — Thin internal explanation model

Introduce the internal explanation contract first:

- scan facts
- policy rationale
- execution-context facts
- final explanation envelope

Stage 1 must not require audit/logger or UI code to become explanation-aware
before the contract is complete.

Additional rules:

- if a primary trigger is exposed, it must be deterministically available from
  scanner output
- execution-context facts must be captured in runtime flow when they become
  known
- envelope construction order must mirror pipeline order

### Stage 2 — Audit consumer migration

Migrate audit to serialize explanation as a consumer:

- without weakening the append-only contract
- without overclaiming guarantees
- without re-deriving explanation truth in the logger

Serialized explanation should preserve source-layer boundaries where relevant,
rather than flattening scanner/policy/runtime facts into one undifferentiated
blob.

### Stage 3 — UI consumer migration

Migrate UI to render from the explanation model:

- concise by default
- richer only where current UI already justifies it
- no synthetic explanation facts

### Testing expectations

#### Model stage

Cover:

- scanner facts remain scanner-owned data
- policy rationale remains canonical for final decision reasoning
- missing optional sections stay explicit `None`/absence
- envelope order and composition follow pipeline order

#### Audit stage

Cover:

- audit serializes explanation as a consumer, not as a source of truth
- append-only behavior remains unchanged
- explanation serialization does not overstate guarantees
- allowlist / snapshot / denial reasons stay specific and regression-testable
- source-layer boundaries remain visible where relevant

#### UI stage

Cover:

- UI renders from explanation model rather than ad hoc stitching
- concise default output remains concise
- no new explanation fact is invented in rendering
- denial/prompt/block messaging remains semantically unchanged, only better
  grounded

#### Regression emphasis

For the full initiative:

- no re-derivation regressions
- no duplicated-truth drift
- no semantic drift in:
  - allowlist precedence
  - block reasons
  - prompt reasons
  - snapshot requirement logic
  - CI/mode interpretation

### Reviewability rules

This work is successful only if a reviewer can trace:

1. what scanner contributed
2. what policy added
3. what runtime/context captured
4. what audit/UI merely consumed

The diff should read as explanation ownership normalization, not a new
decision architecture.

### Success criteria

#### Architecture

- explanation has a thin internal typed contract
- ownership remains stable by layer
- audit and UI become consumers of the same model
- no new public `aegis explain` surface is introduced

#### Correctness

- explanation remains descriptive, not authoritative
- no second policy engine emerges
- no missing fact is silently synthesized later
- pipeline order is reflected in envelope construction

#### Safety

- no overstatement of guarantees
- no audit contract weakening
- no user-facing reason implies stronger certainty than the actual model
- explanation assembly on the safe path stays proportional to facts already
  computed by the existing pipeline and does not trigger new expensive
  discovery work

#### Maintainability

- future diagnostic surfaces can consume the model later without redefining it
- regression tests can assert specific reasons, not just allow/deny outcomes
- contributors can identify where to add new explanation facts without
  breaking ownership boundaries

## Follow-On

After this written spec is approved, the next step is implementation planning
for the model-first internal contract followed by audit and UI consumer
migration.
