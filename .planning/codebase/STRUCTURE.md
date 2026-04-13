# Aegis Repository Structure Scan

Last refreshed: 2026-04-12
Focus: tech+arch

## Top-level layout

### Core source

- `src/main.rs` — CLI entrypoint and orchestration
- `src/lib.rs` — public module exports
- `src/decision.rs` — policy engine
- `src/runtime.rs` — runtime dependency container
- `src/watch.rs` — NDJSON watch mode
- `src/policy_output.rs` — evaluation-only JSON rendering
- `src/rollback.rs` — rollback command support
- `src/error.rs` — shared typed errors

### Subsystems

- `src/interceptor/`
  - `parser.rs`
  - `scanner.rs`
  - `patterns.rs`
  - `nested.rs`
- `src/config/`
  - `model.rs`
  - `allowlist.rs`
  - `validate.rs`
- `src/planning/`
  - `core.rs`
  - `prepare.rs`
  - `types.rs`
- `src/snapshot/`
  - `git.rs`
  - `docker.rs`
  - `mod.rs`
- `src/audit/`
  - `logger.rs`
  - `mod.rs`
- `src/ui/`
  - `confirm.rs`
  - `mod.rs`
- `src/bin/`
  - `aegis_benchcheck.rs`

## Test / benchmark structure

Integration-style suites under `tests/`:

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
- `perf/scanner_bench_baseline.toml`

Fuzzing structure now present:

- `fuzz/Cargo.toml`
- `fuzz/fuzz_targets/parser.rs`
- `fuzz/corpus/parser/*`

## Documentation / release assets

Public-facing docs present:

- `README.md`
- `CONTRIBUTING.md`
- `CODE_OF_CONDUCT.md`
- `LICENSE`
- `CONVENTION.md`
- `docs/architecture-decisions.md`
- `docs/config-schema.md`
- `docs/platform-support.md`
- `docs/ci.md`
- `docs/performance-baseline.md`

Release/distribution files present:

- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`
- `scripts/install.sh`
- `scripts/uninstall.sh`
- `scripts/setup-git-hooks.sh`

## Structural improvements since the earlier scan

- `fuzz/` now exists in-repo instead of only in planning docs.
- `Cargo.toml` now excludes internal agent/planning directories and stray text files from crate packaging.
- Versioning is structurally aligned with an initial release (`0.1.0`).
- README structure now includes honest security-model and limitations sections.

## Structural cautions still visible

### Missing threat-model artifact

No `docs/threat-model.md` exists.

### Installer trust path still incomplete

The structure contains:

- release checksum generation
- install script

but not a checksum-verifying install flow artifact.

### Crate package is cleaner, but still broad

`cargo package --list --allow-dirty` verified successfully, but the package still includes a large internal documentation footprint under `docs/` and `docs/superpowers/`.

That is not a release blocker by itself; it is just worth knowing.

### Working tree is not pristine

At scan time, `rtk git status --short` showed concurrent local changes:

- `D file.txt`
- `D hello.txt`
- `D staged.txt`
- `?? docs/claude-source-borrowings.md`

This did not block the scan, but it means the repository is being actively edited and release preparation should account for unrelated in-flight changes.

## Structural verdict

- **Public repo:** structure is ready.
- **First GitHub release:** structure is ready.
- **Stable production/security positioning:** structure is close, but still wants a threat-model artifact and a verification-first install story.
