# Parser Fuzzing Design

**Date:** 2026-04-12  
**Status:** Drafted from approved brainstorming; pending written-spec review

## Goal

Add a real fuzzing foundation to the repository by implementing a **phase-1,
parser-first** rollout:

- introduce `cargo-fuzz` infrastructure
- add a `parser` fuzz target
- add a small seed corpus for parser-focused shell-input cases
- synchronize documentation so it no longer implies parser/scanner fuzzing is
  already fully implemented

This phase is intentionally narrow. It is designed to close the gap between the
documented release posture and the actual repository state without expanding
into a larger parser/scanner hardening effort.

## Scope

### In scope

- add `fuzz/` infrastructure compatible with `cargo-fuzz`
- add `fuzz/fuzz_targets/parser.rs`
- add parser seed corpus files under `fuzz/corpus/parser/`
- update docs to reflect the exact fuzzing status after this phase

### Out of scope

- adding `fuzz/fuzz_targets/scanner.rs` in this phase
- CI automation or scheduled fuzzing campaigns
- broad runtime refactors in `src/interceptor/parser.rs`
- exposing extra parser internals only to improve fuzz coverage

## Approach Options

### Option 1: Parser-first phased rollout (**recommended**)

Implement parser fuzzing now, but structure the repo so scanner fuzzing is the
next natural step.

**Pros:** minimal safe scope; easier crash triage; fixes the docs/reality gap  
**Cons:** scanner fuzzing remains incomplete after this phase

### Option 2: Parser + scanner in one phase

Add both fuzz targets immediately.

**Pros:** gets closer to the desired end-state faster  
**Cons:** noisier rollout; harder triage; larger scope

### Option 3: Docs-only correction

Remove the claims and defer all fuzzing work.

**Pros:** fastest change  
**Cons:** does not improve the actual release posture

## Approved Direction

Use **Option 1**.

Implement real parser fuzzing now, and explicitly document that scanner fuzzing
is still not implemented in this phase.

## Repository Integration

### File layout

```text
fuzz/
  Cargo.toml
  fuzz_targets/
    parser.rs
  corpus/
    parser/
      empty.txt
      whitespace.txt
      quotes.txt
      unterminated-quote.txt
      heredoc.txt
      unterminated-heredoc.txt
      inline-python.txt
      nested-shell.txt
```

No `artifacts/` directory is committed; `cargo-fuzz` may create it locally when
needed.

### Target design

`fuzz/fuzz_targets/parser.rs` fuzzes the **string-facing parser API**, not a raw
byte contract.

Phase 1 should prefer the narrowest natural public entry point:

- `Parser::parse(&str)`

If existing public parser helpers are naturally usable without weakening
encapsulation, they may be included, but phase 1 must not broaden visibility
just to increase fuzz coverage.

### Input handling

The fuzz target converts input bytes with `String::from_utf8_lossy(...)` before
calling the parser.

This is intentional:

- it exercises the public string-oriented parser surface
- it keeps the harness simple and robust
- it avoids inventing a stronger raw-byte API contract than the parser exposes

### Seed corpus

The initial corpus should cover representative shell-input classes, including:

- empty input
- whitespace-only input
- quoted commands
- unterminated quotes
- heredoc
- unterminated heredoc
- inline interpreter snippets
- nested shell invocations such as `bash -c ...`

The corpus is a bootstrap set, not a completeness claim.

## Runtime-Code Guardrails

Phase 1 should **not** change `src/interceptor/parser.rs` or
`src/interceptor/scanner.rs` unless one of the following is true:

1. the fuzz target cannot be built without a minimal supporting change, or
2. the new fuzz target immediately reveals a concrete crash/panic worth fixing
   in the same phase

This keeps the rollout focused on infrastructure and verified crash discovery,
not speculative parser redesign.

## Documentation Synchronization

At minimum, update:

- `docs/architecture-decisions.md`

The resulting docs must state the exact status:

- **parser fuzz target implemented**
- **scanner fuzz target not yet implemented in this phase**

Any other explicit claims that parser/scanner fuzz targets already exist should
be corrected in the same pass.

## Verification Plan

This phase uses a reproducible smoke-run verification model.

### Required commands

```bash
rtk cargo +nightly fuzz build parser
rtk cargo +nightly fuzz run parser fuzz/corpus/parser
```

The fuzz run should be treated as a **short smoke run**, not a long campaign.

### Verification expectations

- the `parser` target builds successfully
- the target runs from the provided seed corpus
- no non-standard local setup is required beyond normal `cargo-fuzz` usage
- documentation matches the real post-change state of the repo

## Success Criteria

This phase is successful when all are true:

1. `cargo +nightly fuzz build parser` succeeds
2. the parser fuzz target can be smoke-run from the seed corpus
3. the rollout does not require unnecessary parser/scanner runtime changes
4. the repository now contains real parser fuzzing infrastructure
5. docs no longer imply that parser + scanner fuzzing is already fully present

## Follow-On Phase

The next phase of the same initiative should add:

- `fuzz/fuzz_targets/scanner.rs`
- scanner-specific corpus
- scanner-focused crash triage and any required hardening

That follow-up should reuse the phase-1 infrastructure rather than replace it.
