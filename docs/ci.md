# CI and Release Guarantees

## Pinned Inputs

- Rust toolchain: `1.94.0`
- `cargo-audit`: `0.22.1`
- `cargo-deny`: `0.19.0`
- `cross`: `0.2.5`
- GitHub Actions used by `.github/workflows/ci.yml` and `.github/workflows/release.yml` are pinned by full commit SHA with readable release comments.

## Current CI Jobs

Current GitHub Actions workflows run these jobs:

- `Quality (fmt, clippy, test)`: formatting, clippy, and tests
- `Live installer validation`: downloads the latest GitHub Release asset for the host platform, verifies the SHA-256 sidecar, installs to a temporary `BINDIR`, and asserts `aegis --version` succeeds. Runs on `ubuntu-latest` and `macos-latest`; gated in the test suite by `AEGIS_TEST_LIVE_INSTALL=1` so default `cargo test` stays network-free.
- `Security (audit, deny)`: `cargo-audit` and `cargo-deny`
- `Release build`: release builds on Ubuntu and macOS
- `Performance baseline (scanner bench)`: `scanner_bench` plus benchmark policy evaluation
- `Fuzzing (parser + scanner)`: `cargo +nightly fuzz run parser` and `cargo +nightly fuzz run scanner` with bounded `-runs`
- `Release / build`: tagged release binaries for:
  - `x86_64-unknown-linux-musl`
  - `aarch64-unknown-linux-musl`
  - `x86_64-apple-darwin`
  - `aarch64-apple-darwin`
- `Release / release`: artifact download plus GitHub Release publication

## Homebrew tap validation

Homebrew validation is currently a release-operator smoke test rather than a
default CI job. The formula lives at `packaging/homebrew/Formula/aegis.rb` and is
regenerated from a release tag by `scripts/update-homebrew-formula.sh`. The
required `brew tap` / `brew install` / `brew test` commands on macOS and Linux
are listed in `docs/release-readiness.md`. A gated live test
(`AEGIS_TEST_LIVE_HOMEBREW=1`) lives in `tests/homebrew_live.rs` and keeps
default `cargo test` network-free; a CI job that runs it on
`ubuntu-latest`/`macos-latest` will be added only after explicit workflow
sign-off.

## npm package validation

npm package validation is a release-operator smoke test until explicit CI
workflow sign-off is granted. Network-free contract tests live in
`tests/npm_package.rs` and assert the manifest, installer fail-closed
behavior, checksums, updater, and docs without touching the network. The gated
live test in `tests/npm_live.rs` runs only when `AEGIS_TEST_LIVE_NPM=1` is set,
keeping default `cargo test` network-free. The npm wrapper downloads a pinned
GitHub Release binary during `postinstall`, verifies SHA256, and does not edit
shell startup files or agent config.

## What CI Guarantees

- the workflow definitions do not depend on floating toolchain, tool, or action refs
- CI runs formatting, linting, tests, dependency audit, deny policy checks, release builds, and benchmark policy checks exactly as defined in the pinned workflows
- CI additionally verifies parser and scanner fuzzing with bounded corpus-backed runs.
- release artifacts are checksumed and uploaded by the pinned release workflow

## What CI Does Not Guarantee

- byte-for-byte reproducible binaries across all environments
- independence from hosted-runner image changes, the crates ecosystem, or external infrastructure
- stronger runtime security semantics than the Aegis code actually implements
- that GitHub Actions CI and Aegis runtime CI handling are the same feature

## Runtime `ci_policy` vs GitHub Actions CI

These are different contracts:

- GitHub Actions CI is the repository automation defined in `.github/workflows/*.yml`
- runtime `ci_policy` is an Aegis config input that changes how the Aegis binary behaves when it detects CI

Current runtime behavior is documented in `docs/config-schema.md`, but at a high level:

- `ci_policy` is part of the runtime policy engine, not the workflow definition
- in `Protect`, `ci_policy = Block` blocks non-safe commands instead of prompting
- `Strict` is not weakened by CI detection
- `Audit` remains non-blocking

## Release Workflow Contract

The current release workflow is triggered by tags matching `v*` and:

- installs Rust `1.94.0`
- uses `cross 0.2.5` for both Linux musl targets (`x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`) so the release matrix does not depend on runner-specific musl linker setup
- builds the current four-target release matrix
- verifies Linux musl artifacts are statically linked (`ldd` reports `not a dynamic executable`) before checksum generation
- copies and renames the `aegis` binary per target asset name
- generates SHA-256 checksum sidecar files
- uploads artifacts from the build job
- publishes a GitHub Release with generated release notes and the built artifacts

This is a deterministic workflow-input contract, not a formal reproducible-build guarantee.

M3.5 live release validation is covered by the gated
`tests/release_assets_live.rs` integration test. It is disabled during default
`rtk cargo test`; release operators run it with
`AEGIS_TEST_LIVE_RELEASE=1 AEGIS_TEST_RELEASE_TAG=vX.Y.Z` after a tag has been
published to prove that GitHub Release assets and `.sha256` sidecars are both
present and mutually consistent.

For users who want to validate a downloaded release asset before installing it,
see [Release readiness](release-readiness.md). That document also splits launch
blockers from longer-term security hardening items.
