---
name: verification-loop
description: Comprehensive pre-PR verification for Aegis — build, clippy, tests, benchmarks, and security audit. Run after any significant change.
origin: adapted from ECC
---

# Verification Loop (Rust / Aegis)

Run this skill after completing a feature, fix, or refactor — and always before opening a PR.

## When to Activate

- After adding or modifying a pattern in `patterns.rs`
- After changes to `scanner.rs` or `parser.rs`
- After any dependency update
- Before creating a PR
- After refactoring any module

## Verification Phases

### Phase 1: Build

```bash
rtk cargo build 2>&1 | tail -20
```

If build fails → STOP. Fix before continuing.

### Phase 2: Clippy

```bash
rtk cargo clippy -- -D warnings 2>&1 | head -40
```

Zero warnings required. All clippy warnings are errors in this project.

### Phase 3: Tests

```bash
rtk cargo test 2>&1 | tail -30
```

Report:
- Total / passed / failed
- Any ignored tests (flag for review)

If any test fails → STOP.

### Phase 4: Benchmarks (when scanner/parser changed)

```bash
rtk cargo criterion 2>&1 | tail -40
```

Check:
- `assess/safe` p99 < **2ms** (hard budget)
- No regression vs previous run

Skip this phase if the change is docs-only or config-only.

### Phase 5: Security Audit

```bash
rtk cargo audit
rtk cargo deny check
```

Zero CVEs and zero policy violations required.

### Phase 6: Diff Review

```bash
rtk git diff --stat HEAD
```

Scan each changed file for:
- Unintended changes
- Missing error handling on `?` chains
- Any `unwrap()` / `expect()` added outside tests
- Any new dependency not in the approved list (CLAUDE.md)

## Output Format

After all phases, produce:

```
VERIFICATION REPORT — Aegis
============================

Build:      [PASS/FAIL]
Clippy:     [PASS/FAIL]  (N warnings)
Tests:      [PASS/FAIL]  (N/M passed)
Benchmarks: [PASS/SKIP/FAIL]  (safe p99: X µs)
Audit:      [PASS/FAIL]  (CVEs: N, deny violations: N)
Diff:       N files changed

Overall:    [READY / NOT READY] for PR

Issues:
1. ...
```

## Notes

- Never open a PR with a FAIL in any phase except Benchmarks/SKIP.
- Benchmark phase is SKIP only when no scanner/parser code was touched.
- If `cargo audit` fails due to a transitive dep with no fix yet, document it explicitly in the PR — it is never silently ignored.
