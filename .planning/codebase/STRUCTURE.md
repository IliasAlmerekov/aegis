# Aegis Repository Structure Scan

Last refreshed: 2026-04-12
Focus: tech+arch

## Top-level layout

### Source

- `src/main.rs` — CLI entrypoint and orchestration
- `src/lib.rs` — public module exports
- `src/decision.rs` — pure policy engine
- `src/runtime.rs` — runtime dependency container
- `src/policy_output.rs` — evaluation-only JSON projection
- `src/rollback.rs` — snapshot rollback lookup + audit logging
- `src/watch.rs` — NDJSON watch mode
- `src/error.rs` — shared typed errors

### Subsystems

- `src/interceptor/`
  - `mod.rs`
  - `parser.rs`
  - `scanner.rs`
  - `patterns.rs`
  - `nested.rs`
- `src/config/`
  - `model.rs`
  - `allowlist.rs`
  - `validate.rs`
- `src/snapshot/`
  - `mod.rs`
  - `git.rs`
  - `docker.rs`
- `src/audit/`
  - `logger.rs`
  - `mod.rs`
- `src/ui/`
  - `confirm.rs`
  - `mod.rs`
- `src/bin/`
  - `aegis_benchcheck.rs`

## Test layout

Integration-style suites in `tests/`:

- `full_pipeline.rs`
- `cli_integration.rs`
- `config_integration.rs`
- `snapshot_integration.rs`
- `docker_integration.rs`
- `security_regression.rs`
- `audit_concurrency.rs`
- `audit_integrity.rs`
- `watch_mode.rs`
- `installer_flow.rs`
- `platform_support_docs.rs`
- `config_schema_docs.rs`
- `benchcheck_cli.rs`

Fixtures:

- `tests/fixtures/security_bypass_corpus.toml`

Benchmarks:

- `benches/scanner_bench.rs`
- policy baseline file: `perf/scanner_bench_baseline.toml`

## Documentation layout

Core docs found:

- `README.md`
- `CONVENTION.md`
- `docs/architecture-decisions.md`
- `docs/config-schema.md`
- `docs/platform-support.md`
- `docs/ci.md`
- `docs/performance-baseline.md`

Process / planning artifacts also exist:

- `docs/tickets/...`
- `docs/superpowers/plans/...`
- `docs/superpowers/specs/...`
- `docs/superpowers/reports/...`

## Release / distribution assets

- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`
- `scripts/install.sh`
- `scripts/uninstall.sh`
- `scripts/setup-git-hooks.sh`
- `LICENSE`
- `CONTRIBUTING.md`
- `CODE_OF_CONDUCT.md`

## Structural positives

- Source tree matches the intended domain split: interceptor, config, snapshots, audit, UI.
- Test tree is broad and organized by operational concern, not only by unit granularity.
- Release/distribution assets already exist in-repo.
- Docs for config, CI, performance, and platform support are present.

## Structural gaps relevant to release/public readiness

- README references `AEGIS.md`, but that file is absent.
- No `fuzz/` directory exists despite ADR/TODO references to fuzz targets.
- No dedicated threat-model or limitations document was found.
- No `CHANGELOG.md`, `SECURITY.md`, or public release-notes artifact exists in the repo tree.
- Version/maturity signaling is structurally inconsistent:
  - package version is already `1.0.0`
  - roadmap/TODO still describes major production-readiness work as incomplete

## Practical release interpretation from structure alone

- **Public repository:** structure is already good enough.
- **First tagged release:** structure is good enough for a preview/beta release.
- **Stable “security tool” release:** structure still wants a few missing public-facing docs/artifacts before that claim is comfortable.
