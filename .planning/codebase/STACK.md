# Technology Stack

**Analysis Date:** 2026-03-27

## Languages

**Primary:**

- Rust 2024 Edition - Core application and all production code

**Secondary:**

- TOML - Configuration files (`aegis.toml`, `Cargo.toml`, `deny.toml`)
- YAML - CI/CD workflows (`.github/workflows/`)
- Shell - Installation script

## Runtime

**Environment:**

- Rust toolchain: stable (latest available)
- MSRV: track latest stable
- Target platforms: Linux x86_64, Linux aarch64, macOS x86_64, macOS aarch64

**Package Manager:**

- Cargo
- Lockfile: `Cargo.lock` (present and committed)

## Frameworks & Core Libraries

**CLI:**

- `clap 4.5` with derive API - Command-line argument parsing (`src/main.rs`)

**Async Runtime:**

- `tokio 1.x` (features: `process`, `fs`, `rt`) - Async subprocess execution in snapshot plugins (`src/snapshot/git.rs`, `src/snapshot/docker.rs`)

**Terminal UI:**

- `crossterm 0.28` - TUI confirmation dialog with colored output (`src/ui/confirm.rs`)

**Pattern Matching:**

- `aho-corasick 1.1` - Fast multi-pattern scan for quick risk assessment (`src/interceptor/scanner.rs`)
- `regex 1.11` - Full regex pattern evaluation for detailed risk assessment (`src/interceptor/patterns.rs`)

**Configuration & Serialization:**

- `serde 1.x` with derive - Serialization framework for config and audit
- `toml 0.8` - TOML config parsing (`src/config/model.rs`)
- `serde_json 1.x` - JSON serialization for audit logs (`src/audit/logger.rs`)

**Error Handling:**

- `thiserror 1.x` - Typed error definitions in library code (`src/error.rs`)
- `anyhow 1.x` - Error propagation in CLI code (`src/main.rs`)

**Async Traits:**

- `async-trait 0.1` - Object-safe async trait methods (`src/snapshot/mod.rs`)

**Logging & Observability:**

- `tracing 0.1` - Structured logging framework
- `tracing-subscriber 0.3` (features: `fmt`, `env-filter`) - Log subscriber and filtering

**Time/Date Handling:**

- `time 0.3` (features: `formatting`, `parsing`) - RFC 3339 timestamp formatting in audit logs (`src/audit/logger.rs`)

**Compression:**

- `flate2 1.x` - gzip compression for audit log rotation (`src/audit/logger.rs`)

## Development & Testing

**Testing:**

- `criterion 0.5` (default-features: false, feature: `cargo_bench_support`) - Benchmarking framework (`benches/scanner_bench.rs`)

**Temporary Files (testing only):**

- `tempfile 3.x` - Fixture creation in integration tests

**Code Quality:**

- `cargo fmt` (built-in) - Code formatting
- `cargo clippy` - Linting (enforced: all warnings must pass)
- `cargo-audit` - CVE scanning (CI-enforced)
- `cargo-deny` - License and dependency policy checking (CI-enforced)

## Key Dependencies

**Critical:**

- `tokio` - Enables async subprocess communication (snapshots must not block the main thread)
- `regex` - Detailed pattern matching for risk assessment; compiled once per session via `std::sync::LazyLock`
- `aho-corasick` - Fast first-pass scan; must complete in < 2ms for safe commands
- `crossterm` - TUI dialog rendering; core user-facing component

**Infrastructure:**

- `tracing` + `tracing-subscriber` - Structured logging in all modules
- `serde` + `toml` - Configuration loading and validation
- `thiserror` - Type-safe error propagation in library code

## Configuration

**Environment:**

- Config search order (highest priority first):
  1. `.aegis.toml` in current directory (project-level)
  2. `~/.config/aegis/config.toml` (global)
  3. Built-in defaults

- No required environment variables for runtime operation
- Optional env var: `AEGIS_FORCE_INTERACTIVE=1` for testing (forces interactive mode even when stdin is a pipe)

**Build:**

- `Cargo.toml` - Primary manifest
- `deny.toml` - Security and license policy enforcement
- Release profile: maximum optimization (opt-level=3, LTO=thin, symbol stripping enabled, codegen-units=1)

**Cargo Features:**

- No feature flags defined (monolithic binary)

## Platform Requirements

**Development:**

- Rust stable toolchain with rustfmt and clippy components
- Linux or macOS host (Windows development via WSL2)
- Access to git and docker CLIs (for integration testing only)

**Production:**

- Deployment target: Linux (x86_64, aarch64) and macOS (x86_64, aarch64)
- Runtime requirements: None (statically-linked binary)
- Requires: `git` CLI in PATH (if git snapshots enabled), `docker` CLI in PATH (if docker snapshots enabled)

**CI/CD:**

- GitHub Actions (`.github/workflows/ci.yml`, `.github/workflows/release.yml`)
- Runs on: Ubuntu Latest, macOS-13, macOS-14
- Cross-compilation: `cross` crate for Linux aarch64 builds on Linux x86_64 hosts

## Dependency Security

**Prohibited Dependencies:**

- `once_cell` - Banned via `deny.toml` (superseded by `std::sync::LazyLock` stable since Rust 1.80)
  - Exception: allowed through audited third-party transitive deps (criterion, tempfile, tracing) until ecosystem catches up

**License Policy:**

- Allowed: MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, ISC, BSD-2-Clause, BSD-3-Clause, Unicode-DFS-2016, Unicode-3.0, Zlib
- Confidence threshold: 0.8
- Banned registries: Only crates.io allowed; no git dependencies or non-standard registries

**CVE Scanning:**

- `cargo audit` runs on every CI push
- Zero-tolerance: any CVE in direct or transitive dependencies blocks the build

---

_Stack analysis: 2026-03-27_
