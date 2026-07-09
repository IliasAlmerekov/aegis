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

Aegis is a Cargo workspace: the `aegis` binary crate at the repository root
acts as a shell-proxy guardrail and depends on focused library crates under
`crates/` (Phase 4 of `ROADMAP.md` — complete). All 9 crates are extracted:
`aegis-types` (shared data vocabulary), `aegis-parser` (shell tokenizer +
`PrefixPattern` matcher), `aegis-scanner` (`Scanner`, `PatternSet`, built-in
`patterns.toml`), `aegis-policy` (the pure `PolicyEngine`), `aegis-config`
(config model, loader, validation, schema, and `amend`), `aegis-explanation`
(`CommandExplanation` and related types), `aegis-tui` (crossterm confirmation
dialog), `aegis-snapshot` (six snapshot backends), and `aegis-audit`
(`AuditLogger`, append-only JSONL with optional hash-chain integrity). DAG
boundaries are enforced by `tests/architecture_boundaries.rs`.

The current runtime architecture is split across a small set of focused
modules and crates:

- `src/main.rs` — CLI argument parsing and top-level orchestration only
- `crates/aegis-parser/` — shell tokenizer, segmentation, and `PrefixPattern` matching
- `src/interceptor/parser/` — thin re-export shim over the `aegis-parser` crate
- `crates/aegis-scanner/` — synchronous risk classification, `PatternSet`, built-in patterns
- `src/interceptor/scanner.rs`, `src/interceptor/patterns.rs` — thin re-export shims over `aegis-scanner`
- `crates/aegis-policy/` — pure policy evaluation (`Assessment` + context → decision)
- `src/decision/` — thin re-export shim over the `aegis-policy` crate
- `src/runtime_gate.rs` — Rust-side CI detection contract
- `src/toggle.rs` — global on/off toggle state rooted at `~/.aegis/disabled`
- `src/watch/` — NDJSON watch-mode control loop
- `crates/aegis-snapshot/` — six snapshot backends (Git, Docker, Postgres, MySQL, SQLite, Supabase); `src/snapshot/` is a re-export shim
- `crates/aegis-audit/` — append-only JSONL audit writer and hash-chain integrity; `src/audit/` is a re-export shim
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
| ADR-011 | Agent hooks rewrite transparently in Rust; setup-shell escapes paths | [`adr-011-hooks-rewrite-transparently-in-rust-and-setup-shell-escapes.md`](adr-011-hooks-rewrite-transparently-in-rust-and-setup-shell-escapes.md) |
| ADR-012 | Claude Code hook uses an absolute shim, at parity with Codex | [`adr-012-claude-hook-uses-absolute-shim.md`](adr-012-claude-hook-uses-absolute-shim.md) |
| ADR-013 | Project config uses a security ratchet | [`adr-013-project-config-security-ratchet.md`](adr-013-project-config-security-ratchet.md) |
| ADR-014 | Launcher and absolute-path normalization for token-prefix detection | [`adr-014-launcher-and-absolute-path-normalization-for-token-prefix-detection.md`](adr-014-launcher-and-absolute-path-normalization-for-token-prefix-detection.md) |
| ADR-015 | Destructive SQL is detected by match-anywhere regex, not token-prefix rules | [`adr-015-destructive-sql-detected-by-regex-not-token-prefix.md`](adr-015-destructive-sql-detected-by-regex-not-token-prefix.md) |
| ADR-016 | Effect-opaque execution uses recovery backstops without raising risk | [`adr-016-effect-opaque-execution-uses-recovery-backstops.md`](adr-016-effect-opaque-execution-uses-recovery-backstops.md) |

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
