---
name: bench-scanner
description: Run criterion benchmarks for the scanner hot path and check for latency regressions against the 2ms budget.
allowed_tools: ["Bash", "Read", "Grep", "Glob"]
---

# /bench-scanner

Use this workflow after any change to `src/interceptor/scanner.rs` or `src/interceptor/parser.rs` to verify the 2ms hot-path budget is preserved.

## Goal

Confirm safe-path latency is under 2ms (p99) and detect regressions before merge.

## Common Files

- `benches/scanner_bench.rs` — criterion benchmark definitions
- `src/interceptor/scanner.rs` — assess() hot path
- `src/interceptor/parser.rs` — tokenizer

## Suggested Sequence

1. **Run benchmarks** — `rtk cargo criterion 2>&1 | tail -40`
2. **Check safe-path p99** — must be < 2ms. Look for `assess/safe` group in output.
3. **Check warn-path** — document any increase, warn if > 5ms.
4. **Check danger-path** — full regex scan, target < 10ms.
5. **Compare to baseline** — criterion saves HTML reports in `target/criterion/`. If available, note delta from previous run.
6. **Flag regressions** — if safe-path p99 > 2ms, STOP and investigate before proceeding.

## Output Format

After running, produce a short report:

```
BENCH REPORT
============
safe-path   p50: X µs   p99: X µs   [PASS/FAIL — budget: 2ms]
warn-path   p50: X µs   p99: X µs
danger-path p50: X µs   p99: X µs

Regression: [NONE / <describe>]
```

## Notes

- Criterion requires a warm run — first invocation may be slower due to compilation.
- Run on a quiet machine; avoid running alongside heavy background processes.
- Alloca­tions in the hot path are the primary cause of latency spikes — check with `cargo bench -- --profile-time=5` if regression is found.
