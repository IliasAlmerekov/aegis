# Aegis ADR index

This directory contains the Architecture Decision Records (ADRs) for Aegis.

- [`ARCHITECTURE.md`](../../ARCHITECTURE.md) defines the current structural
  contracts — what exists, how modules fit together, and which invariants are
  expected to hold now.
- The ADRs in this directory explain **why** those contracts exist, which
  trade-offs the project accepted, and which non-goals are intentional.

These records are companions to:

- [`README.md`](../../README.md) — user-facing behavior and install flow
- [`docs/threat-model.md`](../threat-model.md) — security posture, mitigations,
  and residual risks
- [`CONVENTION.md`](../../CONVENTION.md) — project-level contract for
  architecture, style, and release gates

## Current architecture snapshot

Aegis is a single-crate Rust CLI centered on the `aegis` binary that acts as a
shell-proxy guardrail.

The current runtime architecture is split across a small set of focused
modules:

- `src/main.rs` — CLI argument parsing and top-level orchestration only
- `src/interceptor/parser.rs` — shell parsing and segmentation
- `src/interceptor/scanner.rs` — synchronous risk classification
- `src/interceptor/patterns.rs` — built-in and user pattern loading
- `src/runtime_gate.rs` — Rust-side CI detection contract
- `src/toggle.rs` — global on/off toggle state rooted at `~/.aegis/disabled`
- `src/watch.rs` — NDJSON watch-mode control loop
- `src/snapshot/` — best-effort Git / Docker / database snapshot providers
- `src/audit/logger.rs` — append-only JSONL audit writer and integrity handling
- `scripts/hooks/` — Claude / Codex hook payloads and the shared shell-side
  toggle helper template
- `scripts/install.sh` / `scripts/uninstall.sh` — convenience installer and
  managed cleanup

At a high level, the current product contract is:

1. intercept commands before execution
2. classify them as `Safe`, `Warn`, `Danger`, or `Block`
3. require confirmation or hard-block according to policy
4. record the decision in the audit log
5. optionally snapshot before dangerous execution when configured

The global toggle and hook integrations extend that flow, but do not replace
the core classification / approval pipeline when enforcement is active.

## ADR index

| ADR | Decision | File |
|-----|----------|------|
| ADR-001 | Keep the CLI entrypoint thin | [`adr-001-keep-cli-entrypoint-thin.md`](adr-001-keep-cli-entrypoint-thin.md) |
| ADR-002 | The interception hot path stays synchronous | [`adr-002-the-interception-hot-path-stays-synchronous.md`](adr-002-the-interception-hot-path-stays-synchronous.md) |
| ADR-003 | Aegis is a heuristic guardrail, not a sandbox | [`adr-003-aegis-is-a-heuristic-guardrail-not-a-sandbox.md`](adr-003-aegis-is-a-heuristic-guardrail-not-a-sandbox.md) |
| ADR-004 | Snapshots are best-effort; audit is append-only | [`adr-004-snapshots-are-best-effort-audit-is-append-only.md`](adr-004-snapshots-are-best-effort-audit-is-append-only.md) |
| ADR-005 | Global toggle at command boundaries | [`adr-005-global-toggle-at-command-boundaries.md`](adr-005-global-toggle-at-command-boundaries.md) |
| ADR-006 | CI detection has an explicit override contract | [`adr-006-ci-detection-has-an-explicit-override-contract.md`](adr-006-ci-detection-has-an-explicit-override-contract.md) |
| ADR-007 | Shell hooks share one managed helper, but must not fail open | [`adr-007-shell-hooks-share-one-managed-helper-but-must-not-fail-open.md`](adr-007-shell-hooks-share-one-managed-helper-but-must-not-fail-open.md) |
| ADR-008 | Installer is global-first and rejects removed mode controls | [`adr-008-installer-is-global-first-and-rejects-removed-mode-controls.md`](adr-008-installer-is-global-first-and-rejects-removed-mode-controls.md) |
| ADR-010 | Full shell evaluation and deferred execution remain non-goals | [`adr-010-full-shell-evaluation-and-deferred-execution-remain-non-goals.md`](adr-010-full-shell-evaluation-and-deferred-execution-remain-non-goals.md) |

`ADR-009` is intentionally absent from the active set; numbering is preserved
as-is so historical references do not drift.

## Verification guidance

Current fuzz targets live under:

- `fuzz/fuzz_targets/parser.rs`
- `fuzz/fuzz_targets/scanner.rs`

They are the preferred stress path for parser / scanner edge cases, alongside:

- focused regression tests in `tests/`
- `cargo bench --bench scanner_bench` for hot-path changes

When a change affects parsing, scanning, or command-boundary behavior, the
expected verification posture is:

- regression tests for positive and negative cases
- clippy / fmt / full test suite where relevant
- benchmark evidence for hot-path-sensitive changes
- fuzz target maintenance rather than documentation-only confidence

## Shared references

- [`README.md`](../../README.md)
- [`docs/threat-model.md`](../threat-model.md)
- [`docs/config-schema.md`](../config-schema.md)
- [`docs/release-readiness.md`](../release-readiness.md)
- `tests/toggle_cli.rs`
- `tests/agent_hooks.rs`
- `tests/installer_flow.rs`
- `tests/full_pipeline.rs`
