# Contributing

Thanks for helping improve Aegis.

Before opening a pull request, please read:

- [`CONVENTION.md`](CONVENTION.md) — project rules, security invariants, style, and release gates
- [`docs/architecture-decisions.md`](docs/architecture-decisions.md) — non-obvious design decisions and documented non-goals
- [`SECURITY.md`](SECURITY.md) — responsible disclosure process for security reports

For non-trivial changes, please open an issue first so we can agree on scope before implementation.

## What kinds of pull requests are welcome

Good fits for this repository:

- bug fixes with focused regression coverage
- tests that improve confidence in parser, scanner, policy, snapshot, or audit behavior
- documentation improvements that make the security model or contributor workflow clearer
- targeted performance improvements with benchmark evidence
- small UX improvements that do not weaken approval or audit guarantees

Usually not a good fit without prior discussion:

- drive-by dependency swaps
- broad refactors with no user-visible benefit
- changes that weaken fail-closed, approval, snapshot, or audit behavior
- CI / release-policy changes without an agreed issue or maintainer request
- roadmap-sized features submitted as a surprise PR

## Development environment

Minimum local setup:

- Rust stable toolchain
- Git
- a Unix-like environment supported by the project (Linux or macOS)

Optional but useful:

- `cargo-audit` for local advisory checks
- `cargo-deny` for local dependency-policy checks
- nightly Rust plus `cargo-fuzz` for fuzzing
- Docker if you want to opt in to the real Docker integration tests

Install helper tools if you want the full local verification surface:

```sh
cargo install cargo-audit cargo-deny cargo-fuzz
rustup toolchain install nightly
```

## Build the project

From the repository root:

```sh
cargo build
```

For a release build:

```sh
cargo build --release
```

## Run tests

Run the main local test suite:

```sh
cargo test
```

Run a specific integration test when iterating on one area:

```sh
cargo test --test full_pipeline
```

Docker integration tests are skipped by default. To opt in:

```sh
AEGIS_DOCKER_TESTS=1 cargo test --test docker_integration
```

These tests require a working Docker daemon and `docker` on `PATH`.

## Run formatting and linting

```sh
cargo fmt --check
cargo clippy -- -D warnings
```

## Run benchmarks

If your change touches parser/scanner hot paths or benchmark-sensitive behavior:

```sh
cargo bench --bench scanner_bench
```

See [`docs/performance-baseline.md`](docs/performance-baseline.md) for the benchmark policy and local interpretation guidance.

## Run fuzzing

The repository currently includes a parser fuzz target. Run it with nightly Rust:

```sh
cargo +nightly fuzz run parser fuzz/corpus/parser
```

Fuzzing guidance and current status are documented in [`docs/architecture-decisions.md`](docs/architecture-decisions.md).

## Local security checks

Recommended before opening a PR:

```sh
cargo audit
cargo deny check
```

The repository-managed Git hook tolerates missing local installations of `cargo-audit` and `cargo-deny`, but CI does not.

## Git hooks

Install the repository-managed Git hooks once per clone:

```sh
./scripts/setup-git-hooks.sh
```

The pre-push hook mirrors the CI quality gate as closely as possible:

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`
- `cargo audit` when `cargo-audit` is installed locally
- `cargo deny check bans licenses sources` when `cargo-deny` is installed locally

`cargo audit` remains the local advisory/CVE gate. The pre-push hook limits
`cargo-deny` to bans/licenses/sources so local pushes do not fail on transient
advisory-database parsing issues in `cargo-deny` itself.

Any failing step blocks `git push`.

## Pull request checklist

Please make sure your PR:

- has a clear summary of what changed and why
- includes tests or explains why no test change was needed
- keeps documentation in sync with behavior
- does not overstate Aegis as a sandbox or complete security boundary
- stays focused; unrelated cleanup should be split into a separate PR

## Where to ask questions

- Bugs and actionable work items: GitHub Issues
- Security reports: follow [`SECURITY.md`](SECURITY.md), not public issues
- Roadmap / idea discussion: GitHub Discussions, once enabled in repository settings
