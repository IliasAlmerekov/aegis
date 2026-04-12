# P2 Production Polish Design

**Date:** 2026-04-12  
**Status:** Drafted from approved brainstorming; pending written-spec review

## Goal

Bring Aegis to a state where:

> the full local verification baseline is green, and any remaining gaps that
> could weaken trust in that green status are either fixed or explicitly
> documented as intentional, bounded limitations.

This is a **confidence-oriented polish** phase, not a broad hardening phase.

## Scope

### In scope

- Review and strengthen, if needed:
  - `tests/watch_mode.rs`
  - `tests/audit_integrity.rs`
  - `tests/snapshot_integration.rs`
- Run the full local verification baseline:
  - `rtk cargo fmt --check`
  - `rtk cargo clippy -- -D warnings`
  - `rtk cargo test`
  - `rtk cargo bench --bench scanner_bench`
  - `rtk cargo audit`
  - `rtk cargo deny check`
- Triage suspicious gaps and decide:
  - **Fix now** for small/safe issues
  - **Defer** for scope-expanding issues

### Out of scope

- New product semantics
- Large runtime redesigns
- Turning P2 into a dedicated test-hardening phase

## Approach Options

### Option 1: Baseline-only

Run the six verification commands and only react to hard failures.

**Pros:** fast, minimal scope  
**Cons:** can produce a green baseline that still does not deserve full trust

### Option 2: Confidence-oriented polish (**recommended**)

First triage the suspicious test areas, fix small/safe issues, document bigger
ones, then run the full baseline as final confirmation.

**Pros:** best balance of release confidence and scope control  
**Cons:** slightly more work than baseline-only

### Option 3: Aggressive hardening

Use P2 to aggressively remove ignored tests and broaden integration coverage.

**Pros:** maximum confidence  
**Cons:** scope creep; becomes a separate phase

## Approved Direction

Use **Option 2**.

## Architecture / Execution Model

### 1. Triage-first, not fix-first

Inspect the three suspicious areas before the full baseline:

- `tests/watch_mode.rs`
- `tests/audit_integrity.rs`
- `tests/snapshot_integration.rs`

For each issue found, capture evidence and make an explicit decision:

- **Fix now**
- **Defer**

### 2. Triage checklist

Each target file is checked for:

- `#[ignore]` tests
- platform-specific skips or early returns
- env-dependent integration assumptions
- fragile timing / async behavior
- inert or weak coverage that technically exists but does not strongly verify
  the claimed behavior

### 3. Evidence record for each triaged gap

Each triaged issue must record:

- **File / test**
- **Evidence**
  - how it was found
  - what makes it suspicious
- **Decision**
  - Fix now / Defer

### 4. Fix-now policy

Fix in P2 only if the issue is:

- local
- well understood
- not changing product semantics
- not triggering a cascade of new design decisions
- solvable with a small, reliable diff

### 5. Deferred-item policy

If the issue expands scope, document it instead of fixing it in P2.

Each deferred item must include:

- **Issue**
- **Why deferred**
- **Why acceptable now**
- **Next step**
- **Owner / destination**
  - e.g. backlog item, future phase, explicit ticket

### 6. Baseline is the final proof, not the only source of truth

After triage and small/safe fixes, run the full baseline. Its role is final
confirmation, not blind discovery.

## Verification Baseline

Run exactly:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
rtk cargo bench --bench scanner_bench
rtk cargo audit
rtk cargo deny check
```

## Required Output Artifacts

### Known limitations / deferred follow-ups

Must be recorded in:

1. this P2 spec under `## Known Limitations / Deferred Follow-Ups`
2. the implementation summary under `## Residual Risks / Follow-Ups`

These entries must be concrete, not vague placeholders.

### Baseline summary

The final summary must include a short result line for each of the six baseline
commands, including:

- command
- outcome
- any relevant note

Not just “baseline green”.

## Success Criteria

P2 is successful when all are true:

1. the entire local verification baseline is green
2. suspicious gaps in the three target test areas are explicitly triaged
3. small/safe issues are fixed immediately
4. scope-expanding issues are concretely deferred, not silently skipped
5. deferred items have clear owner/destination
6. the resulting green baseline is credible enough to support release
   confidence

## Known Limitations / Deferred Follow-Ups

To be filled only with concrete items discovered during implementation.

Required format for each item:

- **Issue:** ...
- **Why deferred:** ...
- **Why acceptable now:** ...
- **Next step:** ...
- **Owner / destination:** ...

## Notes

- Recommended commit order remains:
  1. `feat: require scoped allowlist rules`
  2. `docs: align config and mode documentation`
  3. `ci: pin toolchain and security tooling versions`
- P2 assumes P0 and P1 are already complete.
