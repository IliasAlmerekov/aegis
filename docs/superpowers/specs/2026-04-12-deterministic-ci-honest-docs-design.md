# Deterministic CI and Honest Docs Design

**Date:** 2026-04-12
**Status:** Proposed / approved in chat, pending written-spec review

## Objective

Make the Aegis CI/release pipeline meaningfully deterministic and bring the
public documentation back in sync with the current implementation, without
changing runtime product semantics.

In practice, this P1 should deliver:

- pinned Rust toolchain versions in CI and release workflows
- pinned versions for workflow-installed CLI tools
- GitHub Actions pinned by commit SHA instead of floating tags
- a clearly documented CI/release contract
- README and docs that describe the current behavior honestly

This is explicitly a reproducibility-and-clarity pass, not a policy rewrite.

## Scope

### In scope

1. **Deterministic CI / release**
   - pin Rust toolchain to an exact version
   - pin `cargo-audit`, `cargo-deny`, and `cross` to exact versions
   - pin GitHub Actions by commit SHA with readable version comments
   - make workflow intent and guarantees easier to understand

2. **Honest documentation**
   - update `README.md` to be a short, user-facing summary of actual behavior
   - expand `docs/config-schema.md` into the precise config / policy reference
   - add `docs/ci.md` for CI and release guarantees
   - keep `docs/platform-support.md` focused on platform support boundaries

### Out of scope

This P1 does **not** include:

- new policy logic
- allowlist semantic changes
- confirmation semantic changes
- mode behavior changes
- snapshot behavior changes
- JSON contract changes
- changes to tested runtime behavior just to make docs read better

If truthful documentation reveals a real runtime bug that cannot be described
honestly without changing behavior, that bug should be split into a follow-up
instead of being hidden inside this P1.

## Problem Statement

Two separate issues currently create avoidable production-readiness drift:

1. **Workflow determinism drift**
   - CI currently uses floating inputs such as `stable` toolchains and
     non-SHA-pinned actions
   - workflow-installed tools are not version-fixed
   - that makes it harder to say exactly what CI executed for a given commit

2. **Documentation drift**
   - some README statements lag behind the implementation
   - details such as prompt semantics, mode behavior, snapshot policy, and
     JSON output structure are easier to recover from code than from docs
   - CI behavior and runtime `ci_policy` are not cleanly separated in docs

The result is that the project cannot honestly claim “deterministic CI behavior
and well-explained policy surface” even though the codebase is already close.

## Non-Goals

This work does **not**:

- change what Aegis executes, blocks, prompts for, or snapshots
- change the meaning of `Protect`, `Audit`, or `Strict`
- change the meaning of `allowlist_override_level`
- change the semantics of `ci_policy`
- change prompt acceptance beyond documenting the existing `y` / `yes` rule
- change the shape or version of `--output json`
- promise stronger reproducibility than the workflows actually provide

## Design Decisions

### 1. Deterministic CI means pinned workflow and tooling inputs

For this P1, “deterministic CI” means:

- GitHub Actions are pinned by commit SHA
- Rust toolchains are pinned to an exact version
- workflow-installed tools are pinned to exact versions
- the pinned inputs are easy to inspect directly in the workflow files

It does **not** mean:

- byte-for-byte reproducible binaries across all environments
- independence from the wider package ecosystem or hosted runners
- a formal reproducible-build guarantee

The correct claim after this work is:

> Aegis CI and release workflows do not depend on unpinned workflow/tooling
> targets.

That is strong, honest, and directly supported by the workflow sources.

### 2. Workflow readability matters as much as pinning

Version pinning alone is not enough if the workflow becomes hard to audit.

The workflows should therefore:

- use top-level `env` values where that reduces repeated version strings
- keep job and step names explicit
- annotate SHA-pinned actions with human-readable comments like `# v4.2.2`
- preserve the current pipeline intent rather than introducing unrelated
  restructuring

The goal is reproducibility with explainability, not just reproducibility by
obscurity.

### 3. Runtime `ci_policy` and GitHub Actions CI must be documented separately

There are two different concepts that currently risk being conflated:

1. **The GitHub Actions pipeline**
   - what checks run on pushes, pull requests, schedules, and releases
   - what versions and actions those workflows use

2. **Aegis runtime CI behavior**
   - what Aegis itself does when it detects it is running in CI
   - how `ci_policy` affects `Protect`
   - why `Strict` does not weaken in CI

These belong in different docs:

- `docs/ci.md` documents pipeline behavior and release guarantees
- `docs/config-schema.md` documents runtime `ci_policy`
- `README.md` only gives the short operational summary

### 4. README should stay high-level and user-facing

README is not the authoritative spec surface.

After this change, README should answer:

- what Aegis is
- how it behaves at a high level
- what the modes mean in practice
- what default confirmation behavior looks like
- what snapshot policy means at a high level
- where to read the exact contracts

README should avoid becoming the place where policy edge cases are fully
specified.

### 5. `docs/config-schema.md` becomes the precise config and policy reference

`docs/config-schema.md` should be expanded from a schema-evolution note into a
precise reference for the runtime config and policy surface.

At minimum it should document:

- config versioning
- layered merge order
- `mode`
- `allowlist_override_level`
- `snapshot_policy`
- `auto_snapshot_git`
- `auto_snapshot_docker`
- `ci_policy`
- allowlist migration and scope requirements
- the current `--output json` schema contract

This is the correct place for exact statements such as:

- confirmation accepts only `y` / `yes`
- empty input and non-interactive prompt-required flows deny
- `Block` is never bypassed in `Protect` or `Strict`
- `Audit` is intentionally non-blocking

### 6. Snapshot policy must be documented as policy, not inferred from flags

The current code has both:

- `snapshot_policy = None | Selective | Full`
- per-plugin flags such as `auto_snapshot_git` and `auto_snapshot_docker`

The docs should describe the relationship directly:

- `None` means no snapshots are requested
- `Selective` means plugin flags control which snapshot plugins may run
- `Full` means all applicable snapshot plugins are requested regardless of
  per-plugin flags

This should be documented as the existing behavior, not as a proposed future
model.

### 7. JSON output documentation must mirror the existing contract exactly

The current `--output json` contract is part of the public automation surface.

This P1 should document:

- schema version `1`
- exact top-level fields
- optional fields such as `block_reason`,
  `allowlist_match.pattern`, and `allowlist_match.reason`
- stable array-shaped fields such as:
  - `matched_patterns`
  - `snapshots_created`
  - `snapshot_plan.applicable_plugins`
- the meaning of `decision`, `exit_code`, `ci_state`, `execution`, and
  `decision_source`

The goal is to make the docs truthful without changing serialization behavior.

### 8. Platform support docs should stay narrow

`docs/platform-support.md` should remain the source of truth for:

- supported platforms
- unsupported platforms
- shell/process model assumptions
- support boundaries

It should not absorb policy semantics or CI details that belong elsewhere.

### 9. No runtime semantics changes are allowed in pursuit of nicer docs

This is the core scope guard for the entire P1.

The order of truth is:

1. code and tests
2. docs updated to match code
3. follow-up ticket if docs reveal a genuine runtime defect

That prevents this documentation pass from quietly mutating the product.

## File-by-File Design

### `.github/workflows/ci.yml`

Planned changes:

- pin every `uses:` reference to a commit SHA
- annotate each action pin with a human-readable version comment
- replace `stable` toolchain setup with an exact Rust version
- pin `cargo-audit` and `cargo-deny` installs to exact versions
- use shared workflow `env` values for version constants where that improves
  readability
- keep the current checks intact:
  - fmt
  - clippy
  - test
  - audit
  - deny
  - release build
  - scanner benchmark policy evaluation

### `.github/workflows/release.yml`

Planned changes:

- pin every `uses:` reference to a commit SHA
- annotate each action pin with a readable version comment
- replace `stable` toolchain setup with the exact shared Rust version
- pin `cross` to an exact version
- keep the existing release target matrix unless a doc fix requires more
  precise wording about current support
- preserve current checksum and release upload behavior

### `README.md`

Planned changes:

- keep it concise and user-facing
- correct prompt semantics to “only `y` / `yes` approve; default is deny”
- align high-level mode semantics with the code
- summarize snapshot policy at a high level
- briefly explain runtime `ci_policy`
- briefly explain evaluation-only `--output json`
- link to `docs/config-schema.md`, `docs/platform-support.md`, and `docs/ci.md`
  for exact details

### `docs/config-schema.md`

Planned changes:

- keep schema evolution content
- add the precise current config/policy reference
- document layered merge behavior
- document allowlist normalization and runtime-effective scope requirements
- document the exact current mode semantics
- document snapshot policy and plugin flags together
- document runtime `ci_policy`
- add a JSON output contract section for schema version `1`

### `docs/ci.md`

Planned changes:

- create a dedicated CI/release contract document
- describe the pinned toolchain, tools, and actions
- list current CI jobs and their guarantees
- explain what CI guarantees and what it does not guarantee
- explain the difference between workflow determinism and runtime `ci_policy`
- document current release workflow constraints and artifact expectations

### `docs/platform-support.md`

Planned changes:

- retain only platform support matrix and runtime boundary information
- remove or avoid policy/config/CI detail duplication

## Runtime Semantics to Document Precisely

### Prompt semantics

The current runtime behavior should be documented exactly:

- interactive confirmation approves only on `y` or `yes`
- any other input denies
- empty input denies
- read failure denies
- non-interactive prompt-requiring flows deny

This is an existing behavior description, not a new UI rule.

### Mode semantics

The documentation should reflect the current runtime semantics:

- `Protect`
  - `Safe` auto-approves
  - `Warn` prompts unless an allowlist override makes it effective
  - `Danger` prompts and may request snapshots unless an allowlist override
    makes it effective
  - `Block` blocks
- `Audit`
  - remains intentionally non-blocking
  - does not prompt or block at runtime
- `Strict`
  - `Safe` auto-approves
  - non-safe commands block unless an allowlist override permits them
  - `Block` remains blocked

### CI behavior

The docs should explain current runtime behavior cleanly:

- `ci_policy` is a runtime policy input
- in `Protect`, `ci_policy = Block` blocks non-safe commands instead of prompting
- `Strict` is not weakened by CI detection
- `Audit` remains non-blocking
- this is separate from GitHub Actions workflow behavior

### Snapshot policy

The docs should describe snapshot behavior without embellishment:

- snapshot requests matter only for `Danger` flows
- `snapshot_policy` controls whether snapshots are requested at all
- per-plugin flags matter only in `Selective`
- `Full` requests all applicable plugins
- `None` requests none

## Testing and Validation Expectations

This P1 is primarily workflow and documentation work, but it still needs
verification appropriate to the touched surfaces.

Expected validation includes:

- workflow files remain syntactically correct
- pinned versions are visible and internally consistent
- documentation statements are checked against current code paths
- no runtime semantics changes are introduced accidentally

Where helpful, the implementation may add or update doc-adjacent tests or help
text assertions, but only if they confirm the existing behavior rather than
change it.

## Risks and Mitigations

### Risk: accidental semantic drift while “cleaning up” docs

Mitigation:

- treat code and tests as the source of truth
- avoid changing runtime behavior in the same patch
- split any discovered bug into a follow-up ticket

### Risk: claiming stronger reproducibility than the workflows provide

Mitigation:

- document pinned inputs precisely
- avoid claiming formal reproducible builds
- describe guarantees as “no unpinned workflow/tooling targets”

### Risk: README becomes over-detailed again

Mitigation:

- keep exact semantics in `docs/config-schema.md` and `docs/ci.md`
- use README as summary plus pointers

## Acceptance Criteria

This design is successful when all of the following are true:

1. **CI and release workflows**
   - do not rely on unpinned workflow/tooling targets
   - pin Rust to an exact version
   - pin `cargo-audit`, `cargo-deny`, and `cross` to exact versions where used
   - pin GitHub Actions by SHA

2. **Documentation**
   - `README.md`, `docs/config-schema.md`, `docs/platform-support.md`, and
     `docs/ci.md` do not contradict the current implementation
   - prompt semantics, mode semantics, snapshot policy, allowlist semantics,
     and JSON schema are documented honestly

3. **Scope discipline**
   - runtime semantics are unchanged
   - tested runtime output is unchanged except for purely descriptive updates
     that do not alter product behavior
   - any runtime mismatch requiring a semantic fix is split into a follow-up

## Follow-Ups Explicitly Deferred

The following are intentionally outside this P1:

- any runtime policy bugfix that would change user-visible semantics
- any redesign of CI job coverage
- any broadened platform-support promise
- any JSON schema version bump or output contract rewrite
- any reproducible-build or signed-artifact program beyond the current release
  pipeline contract
