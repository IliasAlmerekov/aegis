# Aegis Stack Scan

Last refreshed: 2026-04-12
Focus: tech+arch

## Core stack

- Language: Rust `2024`
- Package: single crate `aegis`
- Declared version: `0.1.0`
- Primary binaries:
  - `src/main.rs` → `aegis`
  - `src/bin/aegis_benchcheck.rs` → benchmark-policy checker

## Runtime dependencies

From `Cargo.toml`:

- CLI: `clap 4.5`
- Errors: `thiserror 1`, `anyhow 1`
- Config / serialization: `serde 1`, `toml 0.8`, `serde_json 1`
- Async / subprocess orchestration: `tokio 1`, `async-trait 0.1`
- Terminal UI: `crossterm 0.28`
- Scanner: `aho-corasick 1.1`, `regex 1.11`
- Audit / timestamps / integrity: `time 0.3`, `flate2 1`, `sha2 0.10`
- Misc: `base64 0.22`

## Supporting repo tooling

- Benchmarks: `criterion 0.5`
- Dependency policy: `deny.toml`
- Release workflows:
  - `.github/workflows/ci.yml`
  - `.github/workflows/release.yml`
- Fuzzing package present:
  - `fuzz/Cargo.toml`
  - `fuzz/fuzz_targets/parser.rs`
  - `fuzz/corpus/parser/*`

## Observable implementation choices

- `src/interceptor/` remains synchronous and hot-path oriented.
- Scanner is still two-pass:
  - Aho-Corasick keyword prefilter
  - regex verification only for candidates
- Policy is separated from UI/execution in:
  - `src/decision.rs`
  - `src/planning/*`
- Runtime dependency wiring is centralized in `src/runtime.rs`.
- Audit integrity and rotation are first-class runtime concerns, not bolt-ons.

## Verification snapshot from this rescan

Executed locally against the current repo state:

- `rtk cargo fmt --check` ✅
- `rtk cargo clippy -- -D warnings` ✅
- `rtk cargo test` ✅ `490 passed`
- `rtk cargo bench --bench scanner_bench` ✅
- `rtk cargo run --quiet --bin aegis_benchcheck -- --baseline perf/scanner_bench_baseline.toml --criterion-root target/criterion` ✅
- `rtk cargo audit` ✅
- `rtk cargo deny check bans licenses sources` ✅
  - passes with warnings about unmatched allowed licenses and duplicate `windows-sys`
- `rtk cargo +nightly fuzz build parser` ✅
- `rtk cargo publish --dry-run --allow-dirty` ✅
  - dry-run verified packaging/build path
  - warning: `aegis@0.1.0` already exists on crates.io index

## Packaging state

Current crate metadata improved materially versus the earlier scan:

- `Cargo.toml` now excludes:
  - `.claude/**`
  - `.codex/**`
  - `.planning/**`
  - `*.txt`
- `cargo package --list --allow-dirty` no longer includes the previously flagged stray text files.
- Package still includes a broad set of internal project docs under `docs/` and `docs/superpowers/`, but the crate is publishable and the dry-run verify step succeeded.

## Release/public readiness from stack view

### Strong signals

- Versioning now matches an early public release (`0.1.0`), not a premature `1.0.0`.
- Parser fuzzing infrastructure now exists and the parser fuzz target builds.
- Local verification baseline passes.
- Bench regression policy passes.
- Publish dry-run succeeds.

### Remaining stack-level cautions

- `scripts/install.sh` still downloads and installs the binary directly without verifying the published `.sha256` sidecar.
- README still presents `curl | sh` as the primary install path.
- `docs/ci.md` says `cargo-deny` is pinned to `0.19.1`, while `.github/workflows/ci.yml` currently sets `CARGO_DENY_VERSION: 0.18.2`.
- If crates.io publication matters, the dry-run warning about `aegis@0.1.0` already existing on the index needs explicit release-owner confirmation.

## Bottom line

The stack is now good enough for a first public `0.1.0` release and for real Linux/macOS usage as a local guardrail.
It still falls short of a fully mature production-security positioning because installer verification and some release/documentation details remain unfinished.
