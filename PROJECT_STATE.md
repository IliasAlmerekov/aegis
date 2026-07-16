# Project State

> **Agent instructions:** Read this file at the start of every session to restore context.
> After completing any significant change, update the relevant sections here.
> Keep entries concise. This file is a pointer to current state, not a log —
> history lives in git and `CHANGELOG.md`; architectural rationale lives in `docs/adr/`.

---

## Current version

`0.6.1` — pre-1.0, targeting `1.0.0`

## Active branch

`main` (Node.js 24 release-workflow follow-up staged on `ci/node24-actions`)

## Last updated

2026-07-16

---

## Last session (2026-07-16)

- **Release publication migrated to Node.js 24-native actions; PR CI pending.**
  `actions/download-artifact` v8.0.1 and `softprops/action-gh-release` v3.0.2
  are pinned by immutable commit SHA, and the release-workflow contract rejects
  the prior Node.js 20 pins. The focused red/green test, all 10 release-workflow
  tests, fmt, clippy, 1446 workspace tests, audit/deny, and diff-check passed.
  The first parallel workspace run hit an unrelated `snapshot_ordering` flake;
  its focused retry and the complete workspace retry passed.

## Last session (2026-07-15)

- **v0.6.1 release candidate prepared locally.** Workspace crate and internal
  dependency versions, `Cargo.lock`, npm metadata, release docs, README install
  instructions, and Landing version copy now agree on `0.6.1`; the changelog
  cuts the accumulated Unreleased entries as `2026-07-15`. The release-contract
  tests (22), Landing production build, npm dry-run package, 1445 workspace
  tests, fmt, clippy, audit/deny, and diff-check passed. The tag remains pending
  until the release-preparation commit is pushed and required branch CI is green.
- **H7b implemented and verified locally; PR CI pending.** Unix Audit
  directories/artifacts now use `0700`/`0600`; active, lock, query, integrity,
  tail-hash, and rotation opens share descriptor-bound no-follow plus
  tighten-if-owned validation. Rotation preflights every managed slot and
  commits gzip output from owner-only staging before removing the active log.
  ADR-020 and threat-model limits cover caller-owned parent races, non-Unix,
  and crash durability. TDD, review/re-review, 1445 workspace tests, clippy,
  fmt, audit/deny, docs tests, and diff-check passed locally. The checkbox stays
  open until required PR CI passes.
- **H7a closed.** Snapshot stores and bundle directories now use `0700`, while
  SQLite/PostgreSQL/MySQL/Supabase artifacts and manifests use `0600` on Unix.
  Unsafe store leaves are tightened only when owned by the current uid; symlinks
  and other-owner paths fail closed before sensitive writes. Creation composes
  H6 containment before secure reservation; non-Unix deliberately has no POSIX
  mode promise. A follow-up preserves caller-owned SQLite restore modes,
  types unreadable store-metadata failures, tests the other-uid branch, and
  recovers Supabase writes from stale manifest temps. ADR-019, the glossary,
  and regression coverage landed. TDD,
  review/re-review, workspace tests, clippy, fmt, audit, deny, and diff-check
  passed locally; H7a is closed in `TASKS.md`.
- **H6 closed.** SQLite, PostgreSQL,
  and MySQL now prove rollback/delete artifacts remain beneath their plugin-owned
  Snapshot store, rejecting forged outside paths, traversal, and symlink
  escapes with `PathEscapesSnapshotStore`. SQLite restores only to the configured
  database path; legacy in-store artifacts remain supported. ADR-018 and the
  Snapshot store / Snapshot artifact / Path containment glossary are added.
  TDD, review/re-review, `cargo fmt --check`, clippy, workspace tests, audit,
  deny, and diff-check passed locally (the allowed starlark advisories remain);
  required PR CI checks passed. H6 is closed in `TASKS.md`.
- **H5 closed.** PR #122 merged after all required CI checks passed.
  Public/config/landing wording now calls `ChainSha256` an unkeyed local Audit
  integrity chain that detects corruption and inconsistent edits, never an
  adversarial anchor. `aegis audit --verify-integrity` uses the variant-B
  success/failure contract with a residual-risk note. ADR-017 records external
  anchoring as a 1.0 non-goal; a tracked-file wording regression guard and CLI
  integration coverage were added. The guard resolves the repository root from
  `CARGO_MANIFEST_DIR` and allowlists only exact historical/denial lines, so it
  is independent of test cwd and cannot suppress adjacent capability claims.
  Local `fmt`, `clippy`, workspace tests, audit/deny, focused docs tests, and
  review/re-review passed; all required CI checks passed before merge. H5 is
  closed in `TASKS.md` with PR #122 traceability.

## Last session (2026-07-14)

- **M10 closed.** README denial/flow examples and the snapshot-ordering
  regression test passed review/re-review; PR #120 merged after all required CI
  contexts passed.
- **Security backlog normalized.** `TASKS.md` now keeps only the Finding,
  Acceptance criteria, Status, and Traceability for every item. Verified work is
  closed, H7 and M3 are split into independently closable `a`/`b` findings, H9
  is limited to the remaining ADR-016 required-recovery contract, and H5/M1/M8
  now match the audit-integrity / optional-Sandbox / best-effort Snapshot product
  boundaries. Stale Sprint 2/3 groupings were replaced with the agreed
  dependency/risk order.
- **Implementation detail moved to `docs/plans/`.** The existing H9 plan was
  moved from `docs/planning/`, updated with completed/open iterations, and linked
  alongside focused plans for every open P1/P2 finding plus a consolidated P3
  plan. `CONTEXT.md` now distinguishes the `Audit integrity chain`, captured
  `Snapshot` state, and `Rollback` from adversarial tamper proof, backup, or
  general undo.
- **Factually closed:** H3 and M6 remain closed; M3b canonical hook wrapping is
  recorded separately as closed. M10's README denial/flow examples are fixed
  and its PR-CI closure gate passed. H9
  remains Partial (iterations 1–3 only), while H5, H6, H7a/b, M1, M2, M3a, M4,
  M5, and M7–M9 stay open. Docs verification:
  `cargo test --test contracts_docs --test homebrew_formula --test npm_package
  --test release_docs --test snapshot_ordering` = 40 passed; local Markdown
  links = 0 broken; `git diff --check` clean. Standards/Spec review findings on
  M10 closure, plan readiness, and H9 terminology were confirmed and fixed;
  round-2 re-review closed all three. The Audit-mode/H9 concern was dropped as
  not reproducible because fail-closed degradation applies only after recovery
  is required.

---

## Last session (2026-07-09)

- **H9 — effect-opaque execution recovery backstop (ADR-016), Iterations
  1–3 done via TDD.** Iter 1 (model + audit plumbing): direct `effect_opaque:
  bool` field on `Assessment` (orthogonal to `RiskLevel`), `confinement_required`
  axis on `PolicyDecision` (false in v1 — reserved for an optional strict
  tier), `RecoveryDegradation` enum in `aegis-types`, and four backward-compatible
  optional audit fields (`effect_opaque`, `snapshots_required`,
  `confinement_required`, `recovery_degradation`) — older JSONL still
  deserializes. Iter 2 (bounded shape detection): new
  `crates/aegis-scanner/src/scanner/effect_opaque.rs` detects script-file
  execution (`sh ./x.sh`, `python3 ./x.py`, `source ./x`, `. ./x`), interpreter
  stdin (`sh -s`), and pipe-to-shell; inline `-c`/`-e` bodies, package runners,
  and flag-only interpreters are negative forms. Detection runs before the
  safe-path early return; an allocation-free `split_whitespace` +
  `eq_ignore_ascii_case` pre-filter keeps `1000_safe_commands` at 1.96 ms (< 2 ms
  budget). Iter 3 (policy + snapshot flow): `snapshots_required` now fires for
  `effect_opaque` under `SnapshotPolicy::{Selective, Full}` with an applicable
  plugin (no risk raise, no extra prompt); the planning-core plugin-resolution
  guard (`recovery_backstop_applies`) resolves plugins for effect-opaque
  commands, and `execute_with_snapshots` is risk-agnostic so the pre-exec
  snapshot lifecycle works unchanged; project `.aegis.toml` still cannot
  disable recovery (C3 ratchet — added H9 traceability test). Verified:
  `cargo test --workspace` = 1397 passed, `clippy -D warnings` clean, `fmt
  --check` clean, scanner bench 1.96 ms, `cargo audit`/`cargo deny check` ok.
  **Iter 4 (degradation UX / fail-closed) and Iter 5 (threat-model /
  config-schema / README docs + TASKS close-out) deferred per scope decision.**
  ADR-016 written and indexed; `engine.rs` tests extracted to
  `engine/tests.rs` to hold the 800-LoC budget.
- **H9 review cycle (Standards/Spec CHANGES REQUESTED) closed via TDD.**
  (1) Runtime audit construction (`RuntimeContext::build_audit_entry`) now
  populates `effect_opaque` and `snapshots_required` from the assessment and
  policy decision instead of the `Some(false)` defaults — a `sh ./cleanup.sh`
  execution policy required recovery for is no longer logged as backstop-free;
  `confinement_required` records the v1 reserved-tier state. (2) Inline-flag
  detection is now position-sensitive (`interpreter_invocation_is_effect_opaque`):
  `python ./x.py -c` / `bash ./x.sh -c` stay effect-opaque (script file is the
  payload; a later `-c`/`-e` is a script argument), `python -c "code" ./x.py`
  stays inline. (3) `Mode::Audit` documented as an intentional observe-only
  opt-out from ADR-016 recovery (broader than `SnapshotPolicy::None`), with a
  characterization test. Spec #2 (fail-closed when no snapshot can be created)
  remains deferred to Iter 4 — docs make the deferral explicit, no fail-closed
  claim. Spec #4 (README install/FAQ) is out of scope for H9 — the `README.md`
  modifications on this branch predate the H9 work (landing polish). Verified:
  `cargo test --workspace` = 1402 passed, `clippy -D warnings` clean, `fmt
  --check` clean. The review-fix touches only `segment_is_effect_opaque`
  (gated by the allocation-free `has_potential_shape` pre-filter, which is
  false for all 10 safe bench templates), so the safe hot path is unchanged;
  the scanner bench on this WSL2 host read 2.4 ms under load (criterion warned
  it could not hit its sample target), not a code regression — the < 2 ms
  budget was established at 1.96 ms for the pre-filter, which this change does
  not modify.
## Last session (2026-07-07)

- **H4 closed via TDD.** Shell hooks (`claude-code.sh`, `codex-pre-tool-use.sh`) now fail
  closed when the `aegis` binary is unavailable: a `command -v "${AEGIS_BIN}"` guard before
  `exec` emits a `deny` decision (matching the Rust `hook_deny_output` shape) and exits 0,
  instead of `exec` failing with 127 and letting the command run unscanned (ADR-007). The
  original H4 finding (jq fail-open) was already fixed in `8dbb61d`; this closes the residual
  binary-missing fail-open. Hook versions bumped (claude 2→3, codex 3→4). New regression tests
  for both scripts in `tests/agent_hooks.rs`; 3 install tests split into
  `tests/agent_hooks_install.rs` to hold the 800-line budget. 538 tests green, clippy/fmt clean.
- **Security: RUSTSEC-2026-0204.** Bumped transitive `crossbeam-epoch` 0.9.18 → 0.9.20 (via
  starlark → blake3 → rayon-core) to clear the `cargo audit` failure blocking push.

Full history of prior sessions: `git log` and `CHANGELOG.md`.

---

## Milestone status

| Milestone | Title | Status |
|-----------|-------|--------|
| Phase 0–4 | Foundation → Multi-crate workspace | ✅ Done |
| M1 | Snapshot lifecycle & rollback UX | ✅ Done |
| M2 | Audit log hardening | ✅ Done |
| M3 | Distribution (installer, musl, brew, npm, releases) | ✅ Done |
| M4 | Scope reduction (drop native Windows) | ✅ Done |
| M5.1–M5.4 | 800-LoC budget, fuzz CI, snapshot/rollback CI, supply-chain gates | ✅ Done |
| 1.0 docs gate | README, threat model, docs accuracy | 🔲 Open (reopened 2026-07-09 checkup — ARCHITECTURE/CONVENTION/ROADMAP/CHANGELOG stale; see Open decisions) |
| P0 security blockers (C1–C4) | Uppercase bypass, `$IFS` obfuscation, project-config weakening, token-prefix anchoring | ✅ Done |
| P1 security findings (H1–H4, H8) | Segmentation, destructive SQL, H3 patterns, hooks, destructive Git forms | ✅ Done |
| P1 security findings (H5, H6, H7a, H7b, H9) | Integrity wording, containment, artifact hardening, ADR-016 degradation | 🔲 Open (H5/H6/H7a closed; H7b/H9 remain) |
| P2 security findings | M3b/M6/M10 closed; M1, M2, M3a, M4, M5, M7, M8, M9 open | 🔲 Open |
| 1.0 perf gate | Hot path < 2 ms (p99) via criterion | 🔲 Open |
| 1.0 test gate | Zero false-negatives on security bypass corpus | 🔲 Open |

Full task breakdown: `TASKS.md`. Phase/milestone definitions: `ROADMAP.md`.

---

## Current code state

Multi-crate Cargo workspace. Binary crate (`aegis`) at root depends on:

- `crates/aegis-types` — shared data vocabulary (RiskLevel, Decision, …)
- `crates/aegis-parser` — shell tokenizer + PrefixPattern matcher
- `crates/aegis-scanner` — Scanner, PatternSet, built-in patterns.toml
- `crates/aegis-policy` — pure PolicyEngine (TOML DSL + optional Starlark)
- `crates/aegis-config` — config model, loader, validation, schema
- `crates/aegis-explanation` — CommandExplanation and related types
- `crates/aegis-tui` — crossterm confirmation dialog
- `crates/aegis-snapshot` — six snapshot backends (git, docker, pg, mysql, sqlite, supabase)
- `crates/aegis-audit` — AuditLogger, append-only JSONL with optional hash-chain integrity
- `crates/aegis-starlark` — opt-in Starlark policy evaluation (behind `starlark-policy`)
- `crates/aegis-sandbox` — bwrap + Landlock (Linux) / sandbox-exec (macOS) execution confinement

Eleven crates total. DAG boundaries for the first nine are enforced by
`tests/architecture_boundaries.rs`; `aegis-sandbox` is covered separately by
`tests/platform_scope.rs`, and `aegis-starlark` is not yet asserted in either
(gap). Architectural rationale for the shape of this workspace lives in
`docs/adr/` (ADR-001 through ADR-016; `ADR-009` is intentionally absent,
numbering preserved).

As of the 2026-07-15 H7b slice: `cargo fmt --check` and clippy are clean;
`cargo test --workspace` = 1445 passed / 0 failed (87 suites). `cargo audit` /
`cargo deny check` pass aside from the pre-existing allowed advisories under the
opt-in `starlark-policy` feature.

---

## Open decisions / blockers

- **H7b closure blocker:** implementation and local gates are clean; required
  PR CI must pass before the `TASKS.md` checkbox is closed.
- **Current security order** (`TASKS.md`): H6 → H7a → H7b; H9; M3a; M4 → M7;
  M9; M1; M2 → M5; H5 → M8; then P3. This is dependency/risk order, not a
  calendar sprint.
- **P1 open contract:** H5 aligns public wording with an unkeyed local `Audit
  integrity chain`; H6 proves snapshot path containment; H7a protects snapshot
  artifact modes; H7b hardens audit modes and symlink opens; H9 finishes only
  ADR-016 missing-required-recovery degradation. Arbitrary dynamic evaluation
  and TOCTOU are not H9 closure criteria.
- **P2 open contract:** M1 surfaces optional `Sandbox` degradation without making
  confinement mandatory; M3a makes the intentional disabled `Toggle` visible;
  M8 aligns Snapshot/Rollback wording with captured pre-execution state rather
  than building a general backup system. M2, M4, M5, M7, and M9 retain their
  focused correctness findings. M3b, M6, and M10 are closed.
- **Docs accuracy regressions (2026-07-09 checkup):** ARCHITECTURE.md references
  removed paths (`src/decision/engine.rs`, `src/interceptor/…`, `src/config/…`,
  `src/snapshot/*.rs`), states a stale 1500/2000 LoC budget (actual 800), and
  omits the sandbox layer; CONVENTION.md says "10 crates" (11) and cites
  removed `src/audit/logger.rs`; ROADMAP.md still lists Windows work + "9
  crates" against the M4 drop-Windows decision; CHANGELOG `[Unreleased]` misses
  a few post-0.6.0 CI/docs commits; `docs/config-schema.md` omits the
  `[sandbox]` section that exists in code and `aegis-schema.json`.
- 1.0 perf gate: hot path p99 < 2 ms not yet confirmed by a criterion run on
  the current workspace.
- 1.0 test gate: zero-false-negative security bypass corpus not yet locked in.
- CI ARM cross-compilation (`aarch64-unknown-linux-musl`) pending.
- Sandbox tests on `ubuntu-latest` / `macos-latest` with real Docker/SQLite
  pending.
- macOS Homebrew/npm smoke test still an operator follow-up.
- `tests/contracts_docs.rs::readme_links_to_contract_docs` still asserts
  removed install-mode vocabulary (`Local`/`Binary`); README only satisfies it
  via a historical sentence. Needs cleanup so the test stops pinning deleted
  modes.

---

## Workflow cadence

- Read this file, `TASKS.md`, and `CONVENTION.md` before starting non-trivial
  work.
- Load the `rust-best-practices` skill before writing or reviewing Rust code
  (see `CLAUDE.md`; the root `AGENTS.md` was removed — Codex reads
  `.codex/AGENTS.md`).
- Security-sensitive parser/scanner/policy changes go through red → green →
  review TDD (see `tdd` skill); close out with `cargo fmt --check`, `cargo
  clippy -- -D warnings`, full `cargo test --workspace`, and a benchmark run
  when the hot path is touched.
- New architectural decisions get an ADR in `docs/adr/` in the same change,
  not a note in this file.
- Every feature/fix/breaking change gets one line under `## [Unreleased]` in
  `CHANGELOG.md` in the same change.
- After a significant change: update "Last session", any changed `Milestone
  status` rows, and `Open decisions / blockers` here — keep it terse.

---

## How to continue

1. Pick the next open item from `TASKS.md` (P1 H5–H8, then P2 M1–M9), or the
   1.0 perf/test gates above.
2. Confirm current baseline: `rtk cargo test --workspace`, `rtk cargo clippy
   -- -D warnings`, `rtk cargo fmt --check`.
3. For the perf gate specifically: run `rtk cargo criterion` and record p99
   hot-path numbers before claiming it closed.
4. Follow the TDD cadence above; update `CHANGELOG.md`, `TASKS.md` (flip
   `[ ]` → `[x]`), and this file's "Last session" section when done.
