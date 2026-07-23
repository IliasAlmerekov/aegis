# CI Hardening Plan — top-N breadth/strictness improvements

> Agreed in the grilling session on 2026-07-17. Idea source: a reference CI
> pipeline from another project; only elements applicable to Aegis were kept.
> Implementation: two independent PRs, each through the full
> `tdd → code-review → re-review` cycle (see `~/.agents/ENGINEERING_GATES.md`).

## Scope and non-goals

**Adopting (4 elements):**

1. `clippy --all-targets` (+ `--all-features` behind the heavy gate)
2. Coverage: `cargo llvm-cov` → Codecov (informational mode)
3. Dogfood: aegis scans the shell commands of its own repository
4. Scan regression: a shared detection fixture corpus + a dedicated required job

**Into the `TASKS.md` backlog (no implementation now):**

- Corpus-based scan metric: a large labeled corpus of real-world commands +
  a detection-quality baseline modeled on `aegis_benchcheck` /
  `perf/scanner_bench_baseline.toml`. Separate task — the main cost is in
  labeling the corpus, not in CI.
- cli-version-audit: a scheduled job verifying that fresh Claude Code / Codex
  releases have not broken the settings/hooks format that
  `aegis install-hooks` patches. Their format is not our API and breaks
  without warning.
- Bench trend over time (analog of `bench compare`): a per-commit performance
  graph on top of the existing hard baseline.

**Deliberate rejections (do NOT add to the backlog):**

- ASan / memory-tests — the workspace has almost no unsafe; the only C code is
  the pinned tree-sitter grammars (fuzzed upstream); our fuzz jobs already run
  with the nightly toolchain's sanitizer.
- Windows — not a release target (release targets: 2×musl + 2×darwin).
- nextest — the suite already fits the timeouts; pure speed with no new value.
- e2e against real Codex — flakiness + external tokens; the risk is partially
  covered by the cli-version-audit task.

**Facts about the current CI established during preparation (2026-07-17):**

- `cargo clippy --workspace --all-targets -- -D warnings` — already clean.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — already clean.
- `tests/fixtures/commands.toml` from CLAUDE.md **does not exist** (aspirational);
  what actually exists is `tests/fixtures/security_bypass_corpus.toml`
  (~6 Block cases, run by `tests/security_regression.rs`) and ~140 unit tests
  in `crates/aegis-scanner/src/scanner/tests/`.
- Perf regression already exists (`aegis_benchcheck`); fuzzing already exists
  (3×100k, heavy gate).
- The CLI has no `assess` subcommand; the non-interactive interface is
  `aegis watch` (NDJSON frames on stdin → results on stdout).

---

## PR 1 — "CI plumbing" (branch `feat/ci-plumbing` off `main`)

### 1.1 Clippy `--all-targets`

- In the `quality` job replace the step:
  `cargo clippy --workspace -- -D warnings` →
  `cargo clippy --workspace --all-targets -- -D warnings`.
- New heavy-gated job `clippy-all-features` (name: `Clippy (all features)`):
  `needs: gate`, `if: needs.gate.outputs.heavy == 'true'`, ubuntu-latest,
  timeout 20 min, step:
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
  Separate cache-key-prefix (e.g. `clippy-all-features-`) — `--all-features`
  pulls in the heavy starlark dependency chain and must not pollute the
  quality cache.
- The job is deterministic → **add it to the required-checks list in
  `CLAUDE.md`** in the same PR (it is heavy-gated, like the other required
  heavy jobs).

### 1.2 Coverage (informational, every PR)

- New job `coverage` (name: `Coverage`), ubuntu-latest, timeout 20 min,
  **no** heavy gate — runs on every PR (otherwise Codecov cannot comment
  diff coverage).
- Install `cargo-llvm-cov` following the existing security-job pattern:
  pin the version in `env` (`CARGO_LLVM_COV_VERSION`; pick the current one and
  pin it), cache `~/.cargo-tools`, `cargo install --locked --root ~/.cargo-tools`.
  Also needs the `llvm-tools-preview` component in setup-rust.
- Collection: `cargo llvm-cov --workspace --lcov --output-path lcov.info`.
  Plain `cargo test` under the hood — **no nextest**. Fuzz targets live outside
  the workspace and live/Docker tests are gated by env vars — they are not in
  coverage; that is expected.
- Upload: `codecov/codecov-action` v5+, **pinned by SHA** (like every action in
  this workflow), `token: ${{ secrets.CODECOV_TOKEN }}`, `files: lcov.info`,
  `fail_ci_if_error: false` — Codecov being unavailable must not turn CI red.
- The job fails **only** if coverage failed to collect (tests failed /
  instrumentation broke), never because of percentages.
- New `codecov.yml` file at the repository root:

  ```yaml
  coverage:
    status:
      project:
        default:
          informational: true
      patch:
        default:
          informational: true
  comment:
    layout: "diff, files"
    require_changes: true
  ```

- **Not** a required check. The ratchet (failing PRs on coverage drops) is
  something the owner enables later in Codecov settings — no CI change needed.

### 1.3 Dogfood (aegis scans its own repository)

- New script `scripts/dogfood_extract.py` (PyYAML is preinstalled on GitHub
  runners; do not add new dependencies to the Rust workspace — `serde_yaml` is
  archived and would trip an advisory in `cargo audit`):
  - Parses YAML honestly (not with grep — multi-line `run: |` blocks would be
    torn apart otherwise): every `jobs.<id>.steps[].run` from
    `.github/workflows/*.yml`.
  - Plus the contents of all tracked `*.sh` files (`git ls-files '*.sh'`),
    each file = one frame.
  - Emits NDJSON frames in the format `aegis watch` expects.
    **Before implementing, read the actual frame schema** in the `Watch`
    subcommand handler (see `src/main.rs` and the watch module) and match it.
- New job `dogfood` (name: `Dogfood (aegis scans own repo)`), ubuntu-latest,
  **every PR**, timeout 15 min:
  1. `cargo build` (debug — faster; cache shared with quality).
  2. `python3 scripts/dogfood_extract.py | ./target/debug/aegis watch > results.ndjson`.
  3. Parse `results.ndjson` with `jq`:
     - any **Block** verdict → job red, print the offending commands;
     - any **Danger** → warning block in `$GITHUB_STEP_SUMMARY`, job stays
       green (`sudo apt-get`, `docker pull` etc. are legitimate in CI);
     - Safe/Warn — silent.
  - Determinism: run watch with an isolated config (e.g. `HOME=$(mktemp -d)`
    or an explicit config path) so the local/project `.aegis.toml` cannot
    influence verdicts.
- **Not** a required check until calibrated (~2 weeks of observation).
- The job plays a double role: an e2e smoke test of the real binary through the
  `watch` contract + a check that "our tool does not flag our own CI as malicious".

### 1.4 Backlog and documentation (same PR)

- `TASKS.md`: add three backlog entries (corpus-based scan metric,
  cli-version-audit, bench trend) following the file's format:
  finding/motivation, acceptance criteria, status Open, traceability → this plan.
- `CLAUDE.md`: update the required-checks list — add `Clippy (all features)`;
  add a note that `Dogfood (aegis scans own repo)` and `Coverage` are
  **deliberately** not required (calibration / informational mode).
- `CONTEXT.md`: if needed, the term "Dogfood" via `domain-modeling`
  (if the term starts living in PRs/commits/code).
- `CHANGELOG.md` `[Unreleased]` → `Added`: one line per each of the three elements.
- `PROJECT_STATE.md`: only after verification, per the CLAUDE.md convention.

### 1.5 PR 1 verification

- `rtk cargo clippy --workspace --all-targets -- -D warnings` and
  `rtk cargo clippy --workspace --all-targets --all-features -- -D warnings` — green.
- `rtk cargo test --workspace`, `rtk cargo fmt --check` — green.
- Locally: `python3 scripts/dogfood_extract.py` emits valid NDJSON for the
  current workflows; piping into the built `aegis watch` yields no Block.
- Locally run `cargo llvm-cov --workspace --lcov` — coverage collects.
- `actionlint` on the modified `ci.yml`, if available.
- After push: watch the first run of all three new jobs on the PR.

### Owner's manual actions (PR 1)

1. Create the repository in the Codecov UI, obtain the upload token.
2. Put `CODECOV_TOKEN` into the GitHub repo secrets.
3. After merge: add `Clippy (all features)` to the required checks in branch
   protection (checkbox — manually in the GitHub UI).

---

## PR 2 — "Scan regression" (branch `feat/scan-regression-corpus` off `main`)

### 2.1 The `tests/fixtures/commands.toml` corpus

- New file, `[[cases]]` format in the spirit of `security_bypass_corpus.toml`,
  but with minimal fields (only classification is checked, not the e2e CLI):

  ```toml
  [[cases]]
  name = "rm_rf_root_block"
  command = "rm -rf /"
  expected_risk = "block"
  ```

- Size: **~100–150 cases**, covering **all four** risk levels:
  - Safe: everyday commands (`ls`, `cargo build`, `git status`, `grep`, …) —
    negative cases, protection against over-blocking;
  - Warn / Danger / Block: per the existing pattern categories (FS, GIT,
    cloud, DB etc. — cross-check with `patterns.rs` and `CONTEXT.md` for
    terminology);
  - at least one positive case per pattern; for risky ones, close safe
    variants too (e.g. `rm -rf ./build` vs `rm -rf /`).
- Case sources: the existing scanner unit tests as a seed
  (`crates/aegis-scanner/src/scanner/tests/*.rs`) + the pattern table.
  **Do not delete the unit tests** — they remain the fast internal check;
  the corpus is external, data instead of code.
- Do not touch `security_bypass_corpus.toml` — it has its own role
  (bypasses, e2e fields).

### 2.2 The `tests/scan_regression.rs` test

- Integration test: parses the corpus (`toml` is already in the workspace
  dependencies), calls the scanner's public API (`assess`) for each case,
  compares the `RiskLevel` against the expectation.
- Collects **all** mismatches and fails with a single assert producing a
  readable report (case name, command, expected, actual) — not first-failure.
- Write it via `tdd` + `rust-best-practices`: red first (a case the current
  test runner does not cover), then the corpus.

### 2.3 CI job

- New job `scan-regression` (name: `Scan regression`), ubuntu-latest,
  **every PR** (cheap and deterministic — no network/Docker), timeout 15 min:
  `cargo test --test scan_regression`.
- Yes, the cases also run inside `cargo test --workspace` in quality — the
  dedicated job exists to make "detection quality" a **visible separate axis**
  in PR statuses, symmetric to the perf axis.

### 2.4 Documentation

- `CLAUDE.md`: add `Scan regression` to the required-checks list; fix the
  Testing section — `commands.toml` now exists (state the actual size instead
  of "70 test cases minimum").
- `CONTEXT.md`: the term "scan-regression corpus" via `domain-modeling`.
- `CHANGELOG.md` `[Unreleased]` → `Added`.
- `PROJECT_STATE.md` — after verification.

### 2.5 PR 2 verification

- `rtk cargo test --test scan_regression` — green, all ~100–150 cases.
- Manual mutation check: temporarily break one expected level in the corpus →
  the test fails with a readable report → revert.
- Full gate: `rtk cargo test --workspace`, clippy (both variants), fmt.
- The hot path is untouched (the corpus is tests only), a benchmark run is not
  mandatory; if pattern changes surface while adding cases — then
  `rtk cargo criterion` becomes mandatory.

### Owner's manual actions (PR 2)

1. After merge: add `Scan regression` to the required checks in branch
   protection (GitHub UI).

---

## Ordering and independence

- PR 1 and PR 2 are independent; they can be done in any order / in parallel
  on separate branches off `main`.
- The current `feat/language-aware` branch with uncommitted language-crate
  work — **do not touch**.
- Each PR: the `grill` cycle is already done → `tdd` (for PR 2's Rust code —
  with `rust-best-practices`) → `code-review` → `re-review` (≤2 rounds) →
  push/PR only after a clean cycle.
