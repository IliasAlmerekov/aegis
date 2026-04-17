# STRUCTURE

Generated: 2026-04-17
Focus: tech+arch

## Top-level repository shape
- `src/` — product code
- `tests/` — integration and contract coverage
- `docs/` — threat model, release, config, platform, troubleshooting docs
- `scripts/` — install/uninstall/helper scripts
- `fuzz/` — parser/scanner fuzzing targets and corpora
- `perf/` — benchmark baseline policy
- `.github/workflows/` — CI and release automation

## Source modules
- `src/lib.rs` — library module exports
- `src/main.rs` — CLI entrypoint and command dispatch
- `src/interceptor/`
  - parser split into tokenizer / segmentation / nested / embedded-script helpers
  - scanner split into assessment / keywords / recursive / pipeline semantics / highlighting
- `src/planning/`
  - typed planning boundary (`prepare`, `core`, `types`)
- `src/runtime.rs`
  - runtime context, scanner binding, allowlist binding, snapshot/audit wiring
- `src/config/`
  - allowlist, model, validation
- `src/snapshot/`
  - registry + providers: git, docker, postgres, mysql, sqlite, supabase
- `src/audit/`
  - logger + audit model behavior
- `src/ui/`
  - confirmation and policy-block presentation
- `src/watch.rs`
  - NDJSON watch-mode transport

## Test structure
Representative test groups:
- `full_pipeline.rs` — end-to-end shell-wrapper behavior
- `cli_integration.rs`, `config_integration.rs`, `watch_mode.rs`
- `security_regression.rs` — bypass/security regression corpus
- `audit_integrity.rs`, `audit_concurrency.rs`
- `snapshot_integration.rs`, `docker_integration.rs`
- `installer_flow.rs`
- doc contract tests:
  - `contracts_docs.rs`
  - `release_docs.rs`
  - `platform_support_docs.rs`
  - `config_schema_docs.rs`
- performance policy test: `benchcheck_cli.rs`

## Structural observations
- The repo is unusually documentation-heavy for a pre-1.0 CLI, which is a strength
- The repo is also unusually feature-broad for a pre-1.0 CLI, which raises maintenance burden
- Structure is coherent overall, but file-size concentration indicates refactoring debt in core/security-sensitive modules

## Suggested structural priorities
1. Split `src/main.rs` into focused command handlers
2. Split `src/audit/logger.rs` into entry/integrity/query/rotation/writer units
3. Split `src/config/model.rs` into schema/loading/merge/serialization pieces
4. Split `src/ui/confirm.rs` into rendering/input/tty/highlighting flows
5. Re-tier snapshot providers into core vs extended/experimental in docs and code ownership expectations
