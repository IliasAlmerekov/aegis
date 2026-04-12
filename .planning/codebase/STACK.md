# Aegis Stack Scan

Last refreshed: 2026-04-12
Focus: tech+arch

## Core implementation stack

- Language: Rust `2024` (`Cargo.toml`)
- Package: single crate `aegis`
- Declared crate version: `1.0.0`
- Primary binaries:
  - `src/main.rs` → `aegis`
  - `src/bin/aegis_benchcheck.rs` → benchmark policy checker

## Runtime / library dependencies

From `Cargo.toml`:

- CLI / argument parsing: `clap 4.5`
- Errors: `thiserror 1`, `anyhow 1`
- Config / data formats: `serde 1`, `toml 0.8`, `serde_json 1`
- Async runtime / subprocesses: `tokio 1`, `async-trait 0.1`
- Terminal UI: `crossterm 0.28`
- Scanning: `aho-corasick 1.1`, `regex 1.11`
- Time / audit timestamps: `time 0.3`
- Audit compression / integrity: `flate2 1`, `sha2 0.10`
- Misc: `base64 0.22`

## Design choices visible in code

- Hot path remains synchronous in `src/interceptor/`.
- Scanner is two-pass:
  - fast keyword prefilter via Aho-Corasick
  - authoritative regex pass only on candidate inputs
- Built-in scanner is cached with `std::sync::LazyLock` in `src/interceptor/mod.rs`.
- Runtime dependency container exists as `RuntimeContext` in `src/runtime.rs`.
- Policy decisions are isolated in `src/decision.rs`.
- Async is used mainly for snapshot/rollback and watch-mode process handling.

## Tooling / verification stack present in repo

- Formatting: `cargo fmt --check`
- Linting: `cargo clippy -- -D warnings`
- Tests: `cargo test`
- Benchmarks: `cargo bench --bench scanner_bench`
- Supply chain: `cargo audit`, `cargo deny check`
- CI: `.github/workflows/ci.yml`
- Release automation: `.github/workflows/release.yml`

## Test / quality footprint

- Test files at repo root under `tests/`: 13 integration-style suites
- Test annotations found in `src/` + `tests/`: ~475 (`#[test]` / `#[tokio::test]`)
- Notable suites:
  - `tests/full_pipeline.rs`
  - `tests/security_regression.rs`
  - `tests/audit_concurrency.rs`
  - `tests/audit_integrity.rs`
  - `tests/snapshot_integration.rs`
  - `tests/docker_integration.rs`
  - `tests/installer_flow.rs`
  - `tests/watch_mode.rs`

## Release/public-readiness strengths

- CI is pinned to explicit tool/action versions and includes fmt/clippy/test/audit/deny/build/bench jobs.
- Release workflow builds Linux + macOS artifacts and publishes `.sha256` checksum sidecars.
- Installer/uninstaller scripts exist and have dedicated integration coverage.
- Public repo basics exist: `LICENSE`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`.
- Quick secret-pattern scan of tracked files found no obvious embedded keys/tokens.

## Release/public-readiness gaps found directly in repo

- `Cargo.toml` declares `version = "1.0.0"`, but `CONVENTION.md` and `TODO.md` still describe the project as not yet production-ready.
- Fuzzing is documented as required before v1.0 (`docs/architecture-decisions.md`), but no `fuzz/` directory is present in the repository.
- `tracing-subscriber` is a dependency, but no initialization call was found in `src/`.
- Public install flow downloads a binary directly in `scripts/install.sh`; it does not verify the published SHA-256 checksum before installation.
- The checked-in report `docs/superpowers/reports/2026-04-12-p2-production-polish-summary.md` records local PASS for fmt/clippy/test/bench/audit, but `cargo deny check` was still unresolved there because the pinned local `cargo-deny 0.19.1` panicked on advisory DB parsing.

## Bottom line from stack view

Technically, this is already a substantial, tested Rust CLI with CI, release automation, benchmarks, audit integrity, and rollback flows.  
From a release-positioning standpoint, the stack looks like a strong public beta / MVP, not a confidently finished security product.
