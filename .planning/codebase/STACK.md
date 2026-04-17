# STACK

Generated: 2026-04-17
Focus: tech+arch

## Core language and packaging
- Rust `2024` edition (`Cargo.toml`)
- Single-crate repository (`aegis`), not a Cargo workspace
- Current package version: `0.2.0`
- License: `MIT`

## Runtime shape
- CLI binary with library modules under `src/`
- Tokio runtime for subprocess/snapshot orchestration
- Synchronous hot path for parsing and scanning
- Shell-wrapper / proxy model: `aegis -c ...`, `aegis watch`, `aegis audit`, `aegis rollback`, `aegis config`

## Main crates in use
- CLI: `clap 4.5`
- Errors: `thiserror`, `anyhow`
- Config/data: `serde`, `toml`, `serde_json`
- Logging: `tracing`, `tracing-subscriber`
- Async/subprocess: `tokio`, `async-trait`
- Terminal UI: `crossterm`
- Detection engine: `aho-corasick`, `regex`
- Time/compression/integrity: `time`, `flate2`, `sha2`, `base64`
- Bench/testing: `criterion`, `tempfile`

## Quality and release tooling
- CI workflow runs:
  - `cargo fmt --check`
  - `cargo clippy -- -D warnings`
  - `cargo test`
  - `cargo audit`
  - `cargo deny check`
  - `cargo bench --bench scanner_bench`
  - bounded parser/scanner fuzzing via `cargo-fuzz`
- Release workflow builds 4 targets:
  - `x86_64-unknown-linux-gnu`
  - `aarch64-unknown-linux-gnu`
  - `x86_64-apple-darwin`
  - `aarch64-apple-darwin`
- Release artifacts include `.sha256` sidecars

## Test and verification footprint
- Rust source: ~23.8k LOC under `src/`
- Rust tests: ~6.8k LOC under `tests/`
- Fuzz targets present for parser and scanner
- Benchmark baseline policy stored in `perf/scanner_bench_baseline.toml`

## Platform posture
- Supported: Linux, macOS
- Best-effort: WSL2 terminal usage
- Not supported: native Windows (`PowerShell`, `cmd.exe`)

## Security posture in docs
- Explicitly positioned as a heuristic guardrail, not a sandbox
- Audit log integrity supports optional chained SHA-256 mode
- Snapshot system is documented as best-effort, not guaranteed recovery
