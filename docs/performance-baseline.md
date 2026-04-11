# Performance baseline policy

This document defines the repeatable benchmark strategy for Aegis scanner
performance checks.

## What is checked

The CI performance job runs:

```bash
cargo bench --bench scanner_bench
cargo run --bin aegis_benchcheck -- --baseline perf/scanner_bench_baseline.toml --criterion-root target/criterion
```

Criterion produces per-benchmark `estimates.json` files under `target/criterion/`.
`aegis_benchcheck` reads the checked-in policy file and compares each benchmark's
observed **mean** against the corresponding baseline budget.

## Checked-in baseline

The machine-readable policy lives at:

- `perf/scanner_bench_baseline.toml`

It currently covers:

- `1000_safe_commands`
- `100_dangerous_commands`
- `heredoc_worst_case`

The initial values were rounded from a local benchmark capture on **2026-04-11**
and then given extra headroom so the policy is stable on shared CI runners.

## Threshold policy

- default allowed regression: **+25%**
- `heredoc_worst_case`: **+30%**

This is intentionally conservative for the first CI-integrated version. The goal
is to catch meaningful slowdowns without creating noisy failures from normal
runner variance.

If a benchmark exceeds its threshold, `aegis_benchcheck` exits non-zero and
prints a line like:

```text
FAIL 1000_safe_commands observed 3.500 ms baseline 2.800 ms delta +25.0% threshold +25.0%
```

That output is the primary interpretation surface in CI logs.

## Scheduled job

The CI workflow also exposes a scheduled performance run. Its purpose is to:

- re-check the baseline regularly even without feature work
- leave a benchmark artifact trail in GitHub Actions
- surface drift before release prep

## How to update the baseline

Update the checked-in policy only when:

1. the slowdown is understood and accepted, or
2. the benchmark itself changed in a way that invalidates the old baseline.

Recommended update process:

1. run `rtk cargo bench --bench scanner_bench`
2. inspect `target/criterion/*/new/estimates.json`
3. adjust `perf/scanner_bench_baseline.toml`
4. explain the reason in the PR description or ticket summary

Do **not** update the baseline just to silence an unexplained regression.
