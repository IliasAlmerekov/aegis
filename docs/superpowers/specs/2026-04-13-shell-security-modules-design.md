# Shell Security Modules Design

**Date:** 2026-04-13  
**Status:** Drafted from approved brainstorming; pending written-spec review

## Goal

Refactor the shell-security hot path into smaller, focused modules without
changing Aegis's policy model.

Chosen scope:

- **Scope B**: modularization first
- preserve behavior by default
- allow only narrow, explicitly documented fail-closed or boundary fixes
- keep `interceptor/` as the primary home of security meaning

This effort is about **reviewable decomposition**, not a redesign of risk
evaluation.

## Scope

### In scope

- split `src/interceptor/parser.rs` into smaller parser-focused modules
- split `src/interceptor/scanner.rs` into smaller scanner-focused modules after
  parser stabilization
- preserve the current parser/scanner facade unless a narrow corrective change
  is justified
- add focused regression coverage for moved boundaries
- carry benchmark notes for parser/scanner hot-path changes

### Out of scope

- redesigning the risk model
- changing `RiskLevel` meaning or ordering
- changing allowlist precedence
- changing approval semantics
- broad refactors of `src/config/allowlist.rs` or `src/ui/confirm.rs`
- moving security-semantic logic out of `src/interceptor/` unless it is a rare,
  genuinely shared, non-semantic helper

## Approach Options

### Option 1: One refactor package for parser + scanner

Do both splits in one effort.

**Pros:** fewer follow-up phases  
**Cons:** larger blast radius; harder review; harder rollback

### Option 2: Parser-first staged rollout (**approved**)

Split parser first, stabilize its facade, then split scanner.

**Pros:** smaller review units; cleaner scanner contracts; simpler rollback  
**Cons:** may leave short-lived adapter seams or follow-up cleanup

### Option 3: Scanner-first staged rollout

Split scanner first, then reshape parser as needed.

**Pros:** attacks the largest file first  
**Cons:** higher risk of awkward boundaries because scanner depends on parser
shape

## Approved Direction

Use **Option 2**.

Implementation strategy:

- **interceptor-first**
- **parser first, scanner second**
- **mechanical split by default**
- **semantic change only by exception**

Guiding rule:

> If the security meaning of code cannot be understood without reading
> `interceptor/`, that code should remain in `interceptor/`.

## Architecture Overview

### Boundary model

`parser/` owns shell-structure extraction only:

- tokenization
- segmentation
- extraction of nested command bodies
- extraction of embedded command bodies

`scanner/` owns semantic classification only:

- classification orchestration
- match aggregation
- risk determination
- semantic interpretation over parser-produced structure

These layers do **not** own:

- allowlist precedence
- approval UI policy
- snapshot policy execution
- audit writing

## Stage 1: Parser split

### Objective

Split `src/interceptor/parser.rs` into small, security-shaped modules while
preserving the external parser facade and scanner-facing behavior.

### Target structure

```text
src/interceptor/parser/
  mod.rs
  tokenizer.rs
  segmentation.rs
  nested_shells.rs
  embedded_scripts.rs
```

### Module responsibilities

#### `mod.rs`

- public facade
- owns `ParsedCommand`, `PipelineChain`, `PipelineSegment`, and `Parser`
- re-exports the current external parser entry points
- coordinates internal modules without holding heavy logic
- keeps internal-only helper types from leaking out unnecessarily

#### `tokenizer.rs`

- owns `split_tokens`
- shell-aware token splitting
- quoting, escaping, and separator token handling
- no scanner policy knowledge

#### `segmentation.rs`

- owns shell-shape segmentation helpers, including:
  - `logical_segments`
  - `split_top_level_segments`
  - `split_top_level_command_groups`
  - `split_pipeline_segments`
  - `normalize_segment`
  - `finalize_segment`
- owns `top_level_pipelines`
- separates shell-shape cutting from security interpretation

#### `nested_shells.rs`

- owns extraction of nested shell commands
- covers `bash -c`, `sh -c`, env-prefixed shell invocations, and equivalent
  existing parser behavior
- preserves current recursive unwrap behavior unless a narrow corrective fix is
  justified

#### `embedded_scripts.rs`

- owns the full class of embedded command bodies, including:
  - heredoc extraction
  - inline interpreter scripts
  - `eval` payload extraction
  - process-substitution body extraction

### Stage 1 data flow

Public flow remains the same:

1. caller invokes `Parser::parse(raw_cmd)`
2. `parser/mod.rs` delegates to internal modules
3. parser returns the same public shapes and facade entry points relied on by
   scanner

Stage 1 changes internal placement of logic, not the producer contract.

### Stability rule

The following remain stable externally unless a narrow corrective change is
explicitly justified:

- `Parser::parse`
- `split_tokens`
- `top_level_pipelines`
- existing extraction helpers already used by scanner

### Allowed corrective fixes in Stage 1

A corrective fix is allowed only if all are true:

1. it is an obvious boundary bug or fail-closed improvement
2. it is local to parser responsibility
3. it does not redefine overall risk or policy semantics
4. it adds a regression test
5. it is listed explicitly in design notes, summary, or commit explanation
6. it includes an explicit before/after behavior statement

Allowed examples:

- fix extraction where parser incorrectly drops an embedded body that should
  already be part of current parser intent
- fix a segmentation defect that breaks the shell-shape contract consumed by
  scanner
- fix parser behavior that makes downstream analysis obviously less fail-closed

Not allowed in Stage 1:

- adding future-shaped parser abstractions that are not needed for the split
- changing scanner-facing semantics for elegance
- adding new security meaning into parser
- redesigning parser contracts around hypothetical Stage 2 needs

### Stage 1 verification

- parser edge-case regression coverage
- existing-behavior-preserved tests
- moved-module boundary tests
- corrective regression tests for any fix
- each stage must record whether hot-path benchmarking was rerun, and if not,
  why rerunning it was considered unnecessary
- benchmark note stating that semantics and performance are intended to be
  preserved by default

## Stage 2: Scanner split

### Objective

Split `src/interceptor/scanner.rs` after parser stabilization without changing
the policy model or weakening the fail-closed stance.

### Target structure

```text
src/interceptor/scanner/
  mod.rs
  assessment.rs
  pipeline_semantics.rs
  highlighting.rs
  keywords.rs
  recursive.rs
```

### Module responsibilities

#### `mod.rs`

- public facade for `Scanner`, `Assessment`, `MatchResult`, `HighlightRange`,
  and `DecisionSource`
- thin coordination only
- no broad policy sprawl

#### `assessment.rs`

- owns orchestration of `assess`
- owns the quick-scan gate
- owns full-scan fanout
- owns dedup/merge of match results
- owns final max-risk aggregation
- constructs the final `Assessment`

#### `pipeline_semantics.rs`

- owns semantic interpretation over parser-produced pipeline structure
- owns rules tied to top-level pipeline adjacency and neighboring stage
  relationships
- no UI or allowlist policy logic

#### `highlighting.rs`

- owns collection, merge, sort, and normalization of highlight spans
- supports confirmation/UI consumers only
- errors here must not affect classification outcome

#### `keywords.rs`

- owns keyword extraction for the Aho-Corasick prefilter
- owns uncovered-pattern handling
- supports fast-path safety
- this file is **false-negative-sensitive** and requires especially strict
  regression checking

#### `recursive.rs`

- owns glue around recursive and nested scan targets
- coordinates with current nested recursive scan machinery
- makes the contract between parser extraction and scanner traversal explicit

### Stability rule

Stage 2 preserves:

- `RiskLevel` meaning
- the quick-scan plus regex second-pass model
- fail-closed posture
- existing `Block` / `Danger` / `Warn` / `Safe` interpretation
- parser as producer and scanner as semantic consumer

### Allowed corrective fixes in Stage 2

A corrective fix is allowed only if all are true:

1. it is an obvious classification or boundary defect
2. it has a fail-closed or correctness rationale
3. it includes an explicit before/after behavior statement
4. it adds focused regression coverage
5. it is clearly documented as a corrective fix rather than a policy redesign

### Stage 2 verification

- classification regressions
- semantic pipeline regressions
- moved-module boundary tests
- corrective regression tests where relevant
- each stage must record whether hot-path benchmarking was rerun, and if not,
  why rerunning it was considered unnecessary
- benchmark note for hot-path-sensitive areas, especially:
  - quick scan
  - keyword extraction coverage
  - recursive scan orchestration overhead

Stage 2 starts only after Stage 1 leaves a stable enough parser facade and no
ambiguous parser ownership seams.

## Reviewability Rules

This refactor is only successful if the diff remains provably reviewable:

- parser first, scanner second
- each move should be mechanically traceable
- orchestration must not spread back across modules after the split
- `keywords.rs` is treated as a false-negative-sensitive seam
- `highlighting.rs` remains presentation-support logic only

## Rollback Strategy

Rollback must remain staged:

- **Stage 1 rollback:** revert parser split only
- **Stage 2 rollback:** revert scanner split on top of the stabilized parser
  facade

This is a primary reason for the parser-first rollout.

## Corrective Fix Log

Every corrective fix in this initiative must carry an explicit log entry in the
design note, summary, or commit explanation that includes:

- a before/after behavior statement
- the fail-closed or boundary rationale
- a reference to the regression test covering the fix

## Success Criteria

### Architecture

- shell-security logic is split into small, purpose-shaped modules
- `interceptor/` remains the main home of security meaning
- external semantic contracts do not drift without explicit corrective
  justification

### Stability

- `Parser::parse`, `split_tokens`, `top_level_pipelines`, and required
  extraction entry points remain a stable facade
- scanner preserves the existing policy model:
  - same `RiskLevel` meaning
  - same fast-path plus regex second-pass approach
  - same fail-closed posture

### Correctness

- existing-behavior-preserved tests exist
- moved-module boundary tests exist
- corrective regression tests exist where fixes were made
- scanner-stage classification and semantic pipeline regressions exist

### Performance

- no intentional hot-path regression
- parser/scanner changes include benchmark notes
- extra attention is paid to:
  - quick-scan path
  - keyword extraction coverage
  - recursive scan overhead

### Scope control

- `allowlist.rs` and `confirm.rs` are not substantially redesigned as part of
  this initiative
- incidental churn such as import, path, or facade adjustments is acceptable,
  but no semantic redesign is in scope
- external extraction from `interceptor/` remains a rare exception for
  genuinely shared, non-semantic helpers only

## Follow-On

After this spec is approved in written form, the next step is to create an
implementation plan for the staged parser-first rollout.
