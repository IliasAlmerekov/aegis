# Aegis Architecture Scan

Last refreshed: 2026-04-12
Focus: tech+arch

## High-level architecture

Aegis is a single-crate Rust CLI that sits in front of the user’s real shell and evaluates commands before execution.

Current flow visible in `src/main.rs` and `src/planning/*`:

1. Parse CLI args / subcommand.
2. Build one Tokio runtime for process lifetime.
3. Prepare the runtime planner (`prepare_planner`).
4. Produce a typed `PlanningOutcome`.
5. For planned commands:
   - assess risk
   - evaluate policy
   - derive approval + snapshot requirements
   - append audit facts
   - execute / prompt / block

This is stronger than a direct “scan + if” shell wrapper: the repo now has an explicit planning boundary and typed policy outputs.

## Key architectural pieces

### Runtime composition

`src/runtime.rs` provides `RuntimeContext`, which centralizes:

- loaded config
- scanner construction
- layered allowlist
- snapshot registry
- audit logger
- current user context
- shared Tokio handle

This keeps subsystem wiring out of leaf modules.

### Interception subsystem

`src/interceptor/` contains the hot path:

- `parser.rs` — tokenization, heredocs, inline interpreters, nested shell extraction
- `nested.rs` — recursive scan target discovery
- `scanner.rs` — Aho-Corasick fast path plus regex verification
- `patterns.rs` — built-in + custom pattern loading

The parser/scanner remain synchronous, which is consistent with the performance contract in repo conventions.

### Policy subsystem

Policy logic is now separated into:

- `src/decision.rs`
- `src/planning/core.rs`
- `src/planning/types.rs`
- `src/planning/prepare.rs`

Observed architectural benefit:

- shell wrapper
- watch mode
- evaluation-only JSON

all adapt the same decision model instead of re-implementing policy ad hoc.

### Snapshot subsystem

`src/snapshot/mod.rs` provides:

- `SnapshotPlugin`
- `SnapshotRegistry`
- config-aware plugin enablement
- rollback-oriented registry construction

Built-in implementations:

- `GitPlugin`
- `DockerPlugin`

The subsystem remains honestly best-effort: snapshot failure does not silently auto-approve, but snapshot creation is not treated as a hard fidelity guarantee.

### Audit subsystem

`src/audit/logger.rs` is one of the most mature parts of the architecture:

- append-only JSONL
- RFC 3339 timestamps
- integrity chaining
- rotation
- query and summary support
- archive support
- integrity verification

This is backed by dedicated concurrency and integrity tests, which is a strong readiness signal.

## Architectural strengths for a first public release

- `main.rs` is still orchestration-heavy, but core policy semantics are no longer trapped there.
- The planning boundary (`PlanningOutcome`, `InterceptionPlan`, `SetupFailurePlan`) makes fail-closed behavior easier to reason about.
- Watch mode and JSON evaluation share typed policy semantics with shell-wrapper mode.
- Parser/scanner, policy, snapshots, audit, and UI remain separated by responsibility.
- The repo now includes real parser fuzzing infrastructure instead of only forward-looking claims.

## What improved since the previous scan

The current repo state closes several earlier architecture-readiness gaps:

- README now documents security model and limitations.
- Package version is now `0.1.0`, which matches early-release positioning better than a premature `1.0.0`.
- `fuzz/` exists and `rtk cargo +nightly fuzz build parser` succeeds.
- `cargo publish --dry-run --allow-dirty` succeeds.
- Crate packaging excludes `.claude/`, `.codex/`, `.planning/`, and stray `*.txt`.

## Residual architecture-level issues

### 1. Installer trust chain is incomplete

The release workflow publishes checksums, but the installer path still does not verify them before installation.

That means:

- release automation is ahead of installer trust UX
- architecture is good enough for a public beta
- architecture is not yet ideal for stronger secure-install claims

### 2. Threat-model artifact is still missing

`CONVENTION.md` and repo positioning still point toward a formal threat-model expectation for stronger production claims, but `docs/threat-model.md` is absent.

### 3. Documentation drift is reduced, not eliminated

The biggest README issues were fixed, but one concrete repo drift remains:

- `docs/ci.md` and `.github/workflows/ci.yml` disagree on the pinned `cargo-deny` version

### 4. Crates.io publication is not fully de-risked

The dry-run succeeded, but it warned that `aegis@0.1.0` already exists on the crates.io index.

Architecturally this is not a code problem, but it matters if release scope includes publishing to crates.io under the current name/version.

## Architecture verdict

The architecture is now good enough for:

- a public repository
- a first `0.1.0` GitHub release
- real usage as a local Linux/macOS command guardrail

The architecture is still not a comfortable basis for claiming “fully production-ready security tool” status until installer verification and threat-model documentation are completed.
