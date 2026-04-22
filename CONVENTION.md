# Aegis Conventions

This document is the project-level contract for code, architecture, security behavior,
tooling, and release readiness in Aegis.

It consolidates the current enforced rules from:

- `.claude/CLAUDE.md`
- `docs/adr/README.md`
- `.github/workflows/ci.yml`
- `CONTRIBUTING.md`
- `ROADMAP.md`

If these documents ever disagree, treat this precedence as authoritative:

1. Security invariants and ADRs
2. CI-enforced rules
3. This file
4. Contributor guidance

## 1. Project Scope

Aegis is a Rust CLI that acts as a `$SHELL` proxy and intercepts shell commands
before they reach the real shell.

Its job is to:

- parse and classify shell commands
- require human approval for suspicious or dangerous commands
- hard-block catastrophic commands
- create best-effort snapshots for dangerous commands when configured
- append every decision to an audit log

Aegis is:

- a heuristic command guardrail
- not a sandbox
- not a complete security boundary

The project must not claim stronger guarantees than the implementation actually provides.

## 2. Security Invariants

These rules are non-negotiable.

- The deny path must never silently fall through to allow.
- Classification and policy failures must be fail-closed.
- In Aegis, fail-closed means:
  - explicit deny or block, or
  - explicit human approval required before execution.
- Silent auto-approval on scanner, config, confirmation, or policy failure is forbidden.
- `Block`-level commands must never be bypassed by allowlist, CI mode, or refactors.
- Approved commands run with the user's real permissions; code and docs must preserve that model.
- The audit log is a security artifact and must remain append-only.
- Snapshot behavior must be described honestly as best-effort unless fidelity is proven.
- Any change that weakens command interception, confirmation, allowlist safety, CI safety,
  snapshot guarantees, or audit integrity is a security-sensitive change.

## 3. Architecture Rules

The repository is a single Rust crate named `aegis`. It is not a Cargo workspace.

Current module responsibilities:

- `src/main.rs`: CLI parsing and orchestration only
- `src/error.rs`: shared typed errors
- `src/interceptor/parser.rs`: shell parsing and command segmentation
- `src/interceptor/scanner.rs`: command classification
- `src/interceptor/patterns.rs`: pattern loading and pattern types
- `src/config/`: layered config model and allowlist logic
- `src/snapshot/`: snapshot plugin trait and Git/Docker implementations
- `src/ui/confirm.rs`: interactive confirmation flow
- `src/audit/logger.rs`: append-only audit log and rotation

Architectural constraints:

- `src/main.rs` must remain thin; business logic belongs in focused modules.
- `src/interceptor/` is the hot path and must stay synchronous.
- Async is allowed for subprocess and snapshot operations, not for parser/scanner logic.
- Quick scan must remain Aho-Corasick based.
- Full regex evaluation must remain on the slower second pass only.
- `RiskLevel` ordering is semantic and must not be changed.
- `Pattern` continues to use `Cow<'static, str>` to support both built-in and user-defined patterns.
- The project must preserve the current exit-code contract.

## 4. Rust and Code Style

- Rust edition: `2024`
- Current MSRV policy: track latest stable during active development
- Production requirement: declare an explicit MSRV and enforce it in CI before claiming production readiness
- Follow standard Rust naming:
  - types / traits / enums: `PascalCase`
  - functions / methods / variables / modules: `snake_case`
  - constants: `SCREAMING_SNAKE_CASE`
  - enum variants: `PascalCase`
  - pattern IDs in data: uppercase strings like `"FS-001"`
- Prefer ASCII unless there is a strong reason not to.
- Keep comments concise and explanatory, not redundant.
- All new public items must have `///` doc comments.
- Avoid broad re-export layers unless they materially improve the public API.

## 5. Error Handling Rules

- Library code uses typed errors via `thiserror` and `AegisError`.
- `anyhow` is allowed only in CLI glue or top-level application wiring when it simplifies propagation.
- `unwrap()` and `expect()` are forbidden in non-test production paths except for explicit,
  documented startup-time panics where panic is the intended contract.
- Errors must not be silently discarded in production logic.
- Do not convert actionable errors into `None` or a silent fallback unless that behavior is
  intentional, documented, and tested.

## 6. Dependency Rules

Approved dependency categories currently include:

- `clap`
- `crossterm`
- `aho-corasick`
- `regex`
- `serde`
- `toml`
- `thiserror`
- `anyhow`
- `tokio`
- `async-trait`
- `tracing`
- `tracing-subscriber`
- `criterion`

Dependency rules:

- Prefer the standard library where it is sufficient.
- Do not add new dependencies without clear justification.
- `once_cell` is banned; use `std::sync::LazyLock`.
- Avoid dependencies that add unnecessary portability or build complexity.
- Supply-chain health is part of project correctness, not an optional extra.

## 7. Configuration and Audit Contracts

Configuration rules:

- User config is TOML.
- Effective config is layered from:
  - project `.aegis.toml`
  - global `~/.config/aegis/config.toml`
  - built-in defaults
- New config fields must preserve backward compatibility.
- Config changes must be documented and tested for merge semantics.

Audit rules:

- Audit output remains append-only JSONL at `~/.aegis/audit.jsonl`.
- New entries use RFC 3339 / ISO 8601 timestamps with timezone.
- Audit querying must remain compatible with rotated archives.
- Machine-readable audit output is part of the public contract.
- Backward compatibility for older log entries must be maintained where practical.

## 8. Performance Rules

- Safe-command overhead is performance-sensitive and should stay under the project target.
- Hot-path changes in parser/scanner must minimize allocation and cloning.
- Avoid unnecessary heap work in classification code.
- Do not introduce async overhead into the interception hot path.
- Performance-sensitive changes should be benchmarked against `benches/scanner_bench.rs`.

## 9. Testing and Verification

Minimum expectations:

- Unit tests live near the code in `#[cfg(test)]` modules where appropriate.
- Integration tests live under `tests/`.
- Parser changes must add edge-case coverage.
- Classification changes must add positive and negative cases.
- Policy and confirmation changes must add fail-closed regression coverage.
- CI/non-interactive behavior changes must add no-TTY coverage.
- Snapshot changes must add lifecycle and rollback tests.
- Security-sensitive behavior must be regression-tested before merge.
- Hot-path behavior must be checked for performance regressions when parser/scanner logic changes.

Quality gates:

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`
- `cargo audit`
- `cargo deny check`

Pre-push hooks should mirror CI as closely as possible.

Production-level verification requirements:

- parser and scanner fuzz targets must exist and be maintained
- complex shell parsing behavior should receive regression tests and fuzz coverage
- performance-sensitive changes should be measured against an explicit budget
- release candidates should be validated on the supported platform matrix
- critical public contracts must have backward-compatibility tests where practical

## 10. CI and Local Tooling

CI currently enforces formatting, linting, tests, security audit, dependency policy, and release build.

Local development rules:

- `rtk` is an agent-side execution convention used to reduce context-window noise during AI-assisted sessions.
- `rtk` is not part of the runtime product contract and not a requirement for end users or normal project builds.
- Do not rely on local-only shortcuts that bypass CI checks.
- Keep CI, docs, and local contributor instructions aligned.

## 11. Documentation Rules

- Docs must match actual behavior.
- Remove or soften claims that are not supported by tests, benchmarks, or implementation.
- Security docs must state limitations explicitly.
- User-facing docs must not overstate snapshot fidelity, threat coverage, or maturity.
- Changes to public behavior require corresponding doc updates.

## 12. Current Release Gates

The project should not market itself as a mature security product unless these remain true:

- fail-open behavior is removed
- security model is documented honestly
- `Block` behavior is implemented exactly as documented
- layered config semantics are real and tested
- snapshot claims match implementation
- critical failure modes have regression coverage
- supply-chain checks pass in CI

Additional production-readiness gates:

- an explicit MSRV is declared and enforced
- a supported platform matrix is documented and tested
- public compatibility promises are documented for config, audit log, and exit codes
- fuzzing exists for parser/scanner or there is an explicit, documented replacement strategy
- release artifacts are checksumed and verifiable
- release automation is exercised end-to-end
- threat model and limitations documents exist and are current

## 13. Future Mandatory Changes from the Roadmap

These are not all complete today, but they are part of the intended project contract
and should guide all new work so we do not build in the wrong direction.

### Release and supply-chain readiness

Before a release is treated as trustworthy:

- validate the release workflow end-to-end with a real tag
- verify installer downloads with SHA256 or equivalent checksum validation
- provide a verification-first install path, not only `curl | sh`
- publish reproducible release notes with artifacts, checksums, targets, and changelog
- add crate publishing validation such as `cargo publish --dry-run`
- publish or generate SBOM / provenance metadata if the release process supports it
- prefer artifact signing or attestations once release automation is stable

### Product clarity

Before stronger adoption messaging:

- position Aegis clearly as an MVP / local guardrail / human approval layer
- add a dedicated limitations section
- add an architecture diagram that matches real code paths
- add a threat model document with assets, attacker model, assumptions, and known bypasses

### Deferred features

The following features are intentionally deferred and must not be treated as near-term defaults:

- Windows support, until shell interception is redesigned for that platform
- rollback CLI, until snapshot fidelity is trustworthy
- remote audit sinks, until the local audit contract is stable
- web dashboard, until core security semantics are stable
- policy DSL, until current policy semantics are proven

### Fuzzing and parser hardening

Before a strong v1 security posture:

- parser and scanner fuzz targets should exist and be maintained
- complex shell input handling should be treated as high-risk input parsing
- fuzzing should be treated as a release gate for security-sensitive parsing changes

### Compatibility and support policy

Before production claims:

- document supported OS targets and tested versions
- document expected shell execution assumptions and known unsupported environments
- document compatibility guarantees for:
  - config schema
  - audit log schema
  - exit codes
- define deprecation policy for public behavior changes

### Operational readiness

Before production claims:

- define what constitutes a security regression
- define who can approve a release-blocking override
- define rollback / hotfix expectations for bad releases
- define how documentation drift is caught before release

## 14. Change Review Heuristics

Treat a change as high-risk if it touches any of:

- `src/main.rs`
- `src/interceptor/`
- `src/config/allowlist.rs`
- `src/config/model.rs`
- `src/ui/confirm.rs`
- `src/audit/logger.rs`
- `src/snapshot/`
- `Cargo.toml`
- `Cargo.lock`
- CI workflows
- installer or release automation

High-risk changes must be reviewed for:

- fail-closed behavior
- allowlist bypasses
- `Block` semantics
- audit integrity
- backward compatibility
- documentation drift
- supply-chain impact

## 15. Production Readiness Criteria

The project can be described as production-ready only when all of the following are true:

- all current security invariants in this document are implemented and tested
- all release-blocking items tracked in the roadmap are complete or explicitly retired
- supported platforms and environment assumptions are documented
- config, audit log, and exit-code compatibility policy is documented
- release workflow is reproducible and verified
- installer verification exists
- dependency and license checks pass in CI
- parser/scanner fuzzing is active or replaced by an explicitly approved equivalent strategy
- threat model and limitations documentation are published
- documentation and implementation are in sync at release time
