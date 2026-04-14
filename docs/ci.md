# CI and Release Guarantees

## Pinned Inputs

- Rust toolchain: `1.94.0`
- `cargo-audit`: `0.22.1`
- `cargo-deny`: `0.18.2`
- `cross`: `0.2.5`
- GitHub Actions used by `.github/workflows/ci.yml` and `.github/workflows/release.yml` are pinned by full commit SHA with readable release comments.

## Current CI Jobs

Current GitHub Actions workflows run these jobs:

- `Quality (fmt, clippy, test)`: formatting, clippy, and tests
- `Security (audit, deny)`: `cargo-audit` and `cargo-deny`
- `Release build`: release builds on Ubuntu and macOS
- `Performance baseline (scanner bench)`: `scanner_bench` plus benchmark policy evaluation
- `Release / build`: tagged release binaries for:
  - `x86_64-unknown-linux-gnu`
  - `aarch64-unknown-linux-gnu`
  - `x86_64-apple-darwin`
  - `aarch64-apple-darwin`
- `Release / release`: artifact download plus GitHub Release publication

## What CI Guarantees

- the workflow definitions do not depend on floating toolchain, tool, or action refs
- CI runs formatting, linting, tests, dependency audit, deny policy checks, release builds, and benchmark policy checks exactly as defined in the pinned workflows
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
- uses `cross 0.2.5` only for Linux `aarch64`
- builds the current four-target release matrix
- copies and renames the `aegis` binary per target asset name
- generates SHA-256 checksum sidecar files
- uploads artifacts from the build job
- publishes a GitHub Release with generated release notes and the built artifacts

This is a deterministic workflow-input contract, not a formal reproducible-build guarantee.
