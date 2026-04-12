# Aegis Architecture Scan

Last refreshed: 2026-04-12
Focus: tech+arch

## High-level architecture

Aegis is a single-crate Rust CLI that sits in front of the user’s real shell and evaluates commands before execution.

Core flow in `src/main.rs`:

1. Parse CLI args / subcommand.
2. Build one Tokio runtime for process lifetime.
3. Load one `RuntimeContext`.
4. Assess command risk with the interceptor pipeline.
5. Evaluate policy with `decision.rs`.
6. If required, create snapshots.
7. Prompt / block / auto-approve.
8. Append audit entry.
9. Execute real shell command or return reserved Aegis exit code.

## Runtime dependency graph

`RuntimeContext` (`src/runtime.rs`) is the key composition point:

- validates config runtime requirements
- builds scanner from built-in + custom patterns
- compiles structured allowlist
- creates snapshot registry from config
- captures current user
- configures audit logger
- exposes one persistent Tokio handle

This is a good architectural shift away from scattered defaults/singletons.

## Interception pipeline

### Parser / normalization

`src/interceptor/parser.rs` handles:

- tokenization with quotes/escaping
- logical command segmentation
- nested shell unwrapping (`bash -c`, `sh -c`, env-prefixed shell calls)
- heredocs
- inline interpreters (`python -c`, `node -e`, etc.)
- command substitution / subshell extraction
- top-level pipeline extraction

### Scanner

`src/interceptor/scanner.rs` implements:

- Aho-Corasick quick prefilter
- regex full-scan for actual matches
- recursive nested-payload scanning
- semantic pipeline checks (`PIPE-*`)
- uncertain WARN fallback for:
  - oversized command input
  - oversized inline scripts
  - recursive-depth overflow

### Pattern model

`src/interceptor/patterns.rs` merges:

1. built-in embedded TOML patterns
2. user `custom_patterns`

It enforces:

- duplicate-id rejection
- empty-field rejection
- pattern source tracking (`builtin` / `custom`)

## Policy architecture

`src/decision.rs` is a side-effect-free policy kernel.

Inputs:

- scanner assessment
- mode (`Protect`, `Audit`, `Strict`)
- CI detection
- allowlist match state
- snapshot policy / override flags
- transport (`Shell`, `Watch`, `Evaluation`)

Outputs:

- `PolicyAction` (`AutoApprove`, `Prompt`, `Block`)
- rationale
- snapshot requirement
- whether allowlist materially changed the outcome

This is clean and release-friendly: policy is no longer encoded only in UI/main branching.

## Snapshot architecture

`src/snapshot/mod.rs` provides:

- `SnapshotPlugin` trait
- `SnapshotRegistry`
- config-aware plugin enablement
- rollback registry independent from snapshot-creation flags

Built-in plugins:

- `GitPlugin`
- `DockerPlugin`

Important accuracy note:

- snapshot creation is best-effort
- plugin failure logs warnings and does not abort overall decision flow
- rollback exists and is exposed publicly via `aegis rollback <snapshot-id>`

## Audit architecture

`src/audit/logger.rs` is one of the more mature subsystems:

- append-only JSONL log
- RFC 3339 timestamps
- per-process sequence number
- optional rotation
- optional gzip archives
- optional tamper-evident SHA-256 chain
- query / summary / integrity verification
- companion lock file for multi-process safety

Integration evidence is strong here:

- dedicated concurrency tests
- dedicated integrity tests
- watch-mode audit context support

## Additional product surfaces

### Watch mode

`src/watch.rs` adds a long-lived automation surface:

- NDJSON in
- NDJSON out
- per-frame execution
- `/dev/tty` confirmation path
- watch-specific audit metadata

### Evaluation-only JSON mode

`src/policy_output.rs` exposes a machine-readable planning/evaluation contract without execution.

## Architecture strengths for first public release

- Clear module split by concern.
- Thin-enough `main.rs` orchestration relative to earlier ADR goals.
- RuntimeContext centralizes config-bound dependencies.
- Policy is explicit and testable.
- Audit subsystem has strong operational depth.
- Scanner/parser/security regression coverage is substantial.
- CI and release automation are already part of the architecture, not an afterthought.

## Architecture-level risks / blockers for “stable security tool” positioning

- Documentation architecture is not yet aligned with runtime architecture:
  - README still states outdated prompt defaults.
  - README links missing `AEGIS.md`.
  - ADR-010 says README documents the security model, but that section is absent.
- Fuzzing is architecturally declared as required before v1.0, but repository contents do not include fuzz targets.
- Installer architecture does not yet match release architecture:
  - release workflow publishes checksums
  - installer does not verify them
- Versioning/messaging mismatch:
  - code/package says `1.0.0`
  - repository conventions/TODO still say important production gates remain open
- `tracing_subscriber` is present in dependencies but not wired in runtime, leaving observability partially unfinished.

## Bottom line from architecture view

Architecture quality is good enough for a public MVP and arguably for a first tagged release.  
It is not yet honest to present the project as a fully mature or production-grade security boundary until documentation, fuzzing, install verification, and release-positioning drift are fixed.
