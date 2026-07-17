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

## Iteration 0 — language-aware analysis (ADR-022)

Iteration 0 of the language-aware analysis plan
(`docs/plans/2026-07-16-language-aware-analysis.md`) GREEN list requires six
measurements: clean-build requirements, release binary growth, parse latency,
peak worker RSS, startup cost, and all-target build parity. This section records
each one — measured where it is meaningful in Iteration 0, explicitly deferred
with rationale where it is not — so the budget state is documented rather than
silently missing. Measurements are reproducible by running the cited bench
locally; the date and bench command are the evidence, not a transient capture.

### 1. Clean-build requirements — documented

`aegis-language` pulls in the pinned Tree-sitter runtime plus four crates.io
grammars, each a `build.rs` that compiles bundled C source. That is the
clean-build requirement: a C toolchain (cc) on every supported target. The
4-target cross-compile matrix (see §6) proves the C source builds clean on
musl x86_64/aarch64 and darwin x86_64/aarch64. No numeric clean-build *time* is
recorded: it is runner-dependent and noisy, and the requirement (C toolchain +
the four grammar `build.rs` artifacts) is the actionable fact.

### 2. Release binary growth — measured (zero)

`aegis-language` is a workspace member but is **not a dependency of the shipping
`aegis` binary** in Iteration 0 — nothing in `src/` depends on it. Release
binary growth is therefore **0 bytes**: the shipping binary is byte-for-byte
unchanged by this crate. A growth budget (and the size delta from statically
linking Tree-sitter) becomes meaningful when the crate is linked into the root
binary in a later iteration; until then it is exactly zero.

### 3. Parse latency — measured

- Bench: `cargo bench --bench parse_latency_bench -p aegis-language` (wired
  into the `Performance baseline (scanner bench)` CI job). A *measurement*
  bench — parses one representative inline-source snippet per foundation
  grammar so the recorded latency reflects the slow-path cost an inline
  interpreter target would pay, not a degenerate single-statement parse.
- Measured 2026-07-17 (local, release), mean per parse of a small realistic
  snippet (imports + function + loop + conditional):

  | grammar    | mean parse latency |
  |------------|--------------------|
  | Python     | ~43 µs             |
  | JavaScript | ~25 µs             |
  | TypeScript | ~27 µs             |
  | Bash       | ~18 µs             |

- The no-source path is separate and far cheaper: `no_source_bench` measures
  ~1.03 µs per iteration over a 10-command no-source corpus (~103 ns per
  no-source command), and asserts `Outcome::NotStarted` inside `b.iter` so a
  regression that starts the worker panics the bench.
- Budget: parse latency is a slow-path cost, off the safe-command hot path. No
  numeric gate is enforced yet; the `aegis_benchcheck` policy file can add a
  budget row once the worker is wired in and a regression slope is worth
  gating.

### 4. Peak worker RSS — deferred

The Iteration 0 worker experiment is in-process and runs only on inline-source
commands; it does not run on the no-source hot path and is not a separate
process. A meaningful peak-RSS budget is defined by the bounded **ephemeral
worker process** (length-bounded framing, crash/hang isolation, typed
degradation), which lands in Iteration 3. Recording a number now would measure
the throwaway in-process helper, not the production worker. Deferred to
Iteration 3.

### 5. Startup cost — deferred

"Startup cost" in ADR-022 is the cost of starting the ephemeral worker process
(fork + protocol handshake). There is no worker process in Iteration 0 —
`worker::analyze` is an in-process helper — so there is no startup cost to
measure. Deferred to Iteration 3, where the bounded worker process exists.

### 6. All-target build parity — exercised

The `cross-matrix` CI job compiles `aegis-language`'s tests (`--tests`, which
pulls in `grammar_smoke` referencing all four grammars) for each of the four
release targets: musl x86_64/aarch64 and darwin x86_64/aarch64. A grammar that
fails to link on a target fails the job. Parity is therefore exercised as a
build/link gate on all four targets. (Cross targets cannot *execute* the
tests, so runtime parse-parity stays host-only — proven by `grammar_smoke` in
the quality job.)

### 7. Accepted resource budgets — final Iteration 0 defaults

The plan requires Iteration 0 to *replace the hypothesis budget table with chosen
final defaults within ADR-022 ceilings*. The provisional table in the plan
(`docs/plans/2026-07-16-language-aware-analysis.md`) is now superseded by the
accepted defaults below. Each row states its evidence class:

- **measured** — backed by a bench in this document;
- **ceiling-adopted** — accepted as the pre-1.0 default because it *is* an
  ADR-022 hard bound (there is nothing to tune below a bound; a bound needs no
  empirical slope);
- **tune-on-wiring** — accepted as the default now, but its empirical slope is
  re-confirmed by a bench in the iteration that first exercises the governing
  machinery (source reader = Iter 4, recursive queue = Iter 5, worker = Iter 3),
  and must stay within the ceiling.

| Budget                          | Accepted default | Evidence class  | Basis |
|---------------------------------|-----------------:|-----------------|-------|
| No-source worker start          | none (must not start) | **measured** | `no_source_bench` asserts `Outcome::NotStarted`; ~103 ns/command (§3) |
| Existing inline source          | 16 KiB           | ceiling-adopted | preserves the current scanner inline-script limit |
| Script-file default             | 256 KiB          | tune-on-wiring (Iter 4) | global config may tune within the 1 MiB ceiling |
| Script-file hard ceiling        | 1 MiB            | ceiling-adopted | non-configurable (ADR-022) |
| Script files per command        | 8                | ceiling-adopted | project may only tighten |
| Aggregate source per command    | 1 MiB            | ceiling-adopted | project may only tighten |
| Recursive analysis depth        | 8                | ceiling-adopted | hard ceiling for pre-1.0 |
| Total language-analysis time    | 100 ms           | **measured** headroom | per-grammar parse latency is 18–43 µs (§3), so 100 ms is a conservative wall with ~3 orders of magnitude of headroom; re-confirmed as a gated budget when the worker is wired (Iter 3) |

Latency and binary-size budgets are recorded above (§2, §3); the peak-memory
budget is the one deferred to Iteration 3 (§4), where the ephemeral worker
process — the only thing whose RSS is meaningful — first exists. No default here
exceeds an ADR-022 ceiling.

### REVIEW GATE status (ADR-022 §8 + plan Iteration 0)

The Iteration 0 REVIEW GATE requires, before merging a production dependency:
`cargo audit`, `cargo deny check`, all four release builds, license review, and
the grammar security corpus. `aegis-language` is **not linked into the shipping
binary yet**, so the "don't merge a production dependency" clause does not fire
for the shipping `aegis` binary; the gate must be fully green before a later
iteration links the crate. Current status:

| Gate item             | Status                                                                 |
|----------------------|------------------------------------------------------------------------|
| `cargo audit`        | ✓ run 2026-07-17 — 6 advisories, all pre-existing in the opt-in `starlark-policy` feature chain; **none** in tree-sitter or criterion |
| `cargo deny check`   | ✓ run 2026-07-17 — advisories/bans/licenses/sources ok                  |
| Four release builds  | CI-gated via `cross-matrix` (`--tests` per target; link-presence)      |
| License review       | ✓ all four grammars MIT (recorded in `BUILTIN_MANIFEST`; enforced by `deny.toml` permissive-licenses) |
| Grammar security corpus | **OPEN** — not yet built; required before `aegis-language` is linked into the shipping binary |
