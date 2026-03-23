---
name: eval-harness
description: Eval-driven development for Aegis pattern quality — define pass/fail criteria before implementing patterns, measure pass@k reliability for the scanner.
origin: adapted from ECC
---

# Eval Harness (Aegis Scanner)

Treat each detection pattern as a hypothesis. Define expected behaviour first, implement second, verify third.

## When to Activate

- Adding a new detection pattern
- Modifying an existing pattern's regex or Aho-Corasick keyword
- Changing `assess()` logic in `scanner.rs`
- Measuring scanner reliability across a pattern category
- Benchmarking false positive / false negative rates before a release

## Philosophy

Eval-Driven Development for security patterns:
- **Define evals before writing code** — forces precise thinking about match boundaries
- **False negatives are critical failures** — a missed dangerous command is a security hole
- **False positives erode trust** — if Aegis blocks safe commands, users disable it
- **pass@k over the fixture suite** — measures reliability, not just "it compiles"

## Eval Types

### Pattern Capability Eval

Tests that a new or modified pattern does what it claims:

```toml
# tests/fixtures/commands.toml — add before implementing the pattern

[[cases]]
id = "eval-FS-007-pos-1"
command = "rm -rf /"
expected_risk = "Danger"
expected_pattern_id = "FS-007"
note = "canonical rm -rf root — must fire"

[[cases]]
id = "eval-FS-007-pos-2"
command = "rm -rf /var/log/app"
expected_risk = "Danger"
expected_pattern_id = "FS-007"
note = "rm -rf on system path"

[[cases]]
id = "eval-FS-007-neg-1"
command = "rm ./tmp/build.log"
expected_risk = "Safe"
note = "safe rm — must NOT fire FS-007"

[[cases]]
id = "eval-FS-007-neg-2"
command = "rm -f ./dist/output.js"
expected_risk = "Safe"
note = "rm -f on local file — must NOT fire"
```

Minimum per pattern: **2 positive + 2 negative** cases. Aim for 4+4 for complex patterns.

### Regression Eval

Before modifying an existing pattern, record current pass rate:

```markdown
[REGRESSION EVAL: FS-007 keyword change]
Baseline: commit abc1234
Fixture cases: 8 total (4 pos, 4 neg)
Pre-change: 8/8 PASS
Post-change: [run and record]
```

### Category Eval

Periodically measure false positive / false negative rates across a whole category:

```bash
rtk cargo test -- fixtures 2>&1 | grep -E "PASS|FAIL|ignored"
```

Target: **100% on positive cases** (zero false negatives), **< 1% false positive rate** on negative cases.

## Metrics

### pass@k

Run `cargo test` k times (or across k variant commands):

- **pass@1**: Does the pattern fire correctly on first attempt with canonical input?
- **pass@k**: Does the pattern correctly handle k variant phrasings?
- Target for positive cases: `pass@3 = 100%` (three variant phrasings all caught)
- Target for negative cases: `pass@5 = 100%` (five safe variants all clear)

### False Negative Rate

```
FNR = missed_dangerous / total_dangerous_cases
```

Target: **FNR = 0** for Danger and Block patterns. Non-zero FNR is a showstopper.

### False Positive Rate

```
FPR = incorrectly_flagged / total_safe_cases
```

Target: **FPR < 0.5%** across the full fixture set.

## Workflow

### Step 1: Define Evals (before coding)

Add fixture cases to `tests/fixtures/commands.toml`. Run `cargo test` — they should **fail** (pattern doesn't exist yet).

### Step 2: Implement

Add `BuiltinPattern` to `patterns.rs`. Run `cargo test` — evals should now **pass**.

### Step 3: Evaluate

```bash
rtk cargo test -- interceptor 2>&1 | tail -20
```

Confirm all new eval cases pass, no existing cases regressed.

### Step 4: Report

```markdown
EVAL REPORT: FS-007
===================
Positive cases:  4/4 PASS  (pass@1: 100%)
Negative cases:  4/4 PASS  (FPR: 0%)
Regression:      8/8 PASS  (no regressions)

FNR: 0.0%   FPR: 0.0%

Status: READY
```

## Eval Storage

Store eval definitions and run history alongside fixtures:

```
tests/
  fixtures/
    commands.toml          # all fixture cases (ground truth)
.claude/
  evals/
    FS-007.md              # eval definition for pattern FS-007
    scanner-category.md    # category-level eval results
```

## Best Practices

1. **Evals before code** — if you can't write the test, you don't understand the pattern
2. **Name cases clearly** — `eval-<ID>-pos-<N>` and `eval-<ID>-neg-<N>`
3. **Include edge cases** — heredoc, pipes, quoted args, env var expansion
4. **Never delete negative cases** — they prevent future false positives
5. **Human review for Block-level patterns** — `pass^3` (three consecutive full suite runs) before shipping Block-risk patterns
6. **Track FNR trend** — any increase in FNR across a release is a regression
