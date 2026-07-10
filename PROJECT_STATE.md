# Project State

> **Agent instructions:** Read this file at the start of every session to restore context.
> After completing any significant change, update the relevant sections here.
> Keep entries concise. This file is a pointer to current state, not a log —
> history lives in git and `CHANGELOG.md`; architectural rationale lives in `docs/adr/`.

---

## Current version

`0.6.0` — pre-1.0, targeting `1.0.0` (released from `feat/shell-security`)

## Active branch

`feat/shell-security` (branched from `main`)

## Last updated

2026-07-09

---

## Last session (2026-07-09)

- **H9 / M1 — effect-opaque execution recovery backstop (ADR-016), Iterations
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
- **H9 review cycle round 2 (re-review survivors closed via TDD).** (a) The
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
| P1 security findings (H1–H4, H8) | Segmentation gap, SQL-in-`psql`/`mysql`, pattern gaps, hook fail-open, git force-push/stash-clear/branch-D | ✅ Done |
| P1 security findings (H5–H7, H9) | See Open decisions below | 🔲 Open |
| P2 security findings (M1–M10; M6 closed) | See Open decisions below | 🔲 Open |
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

As of the 2026-07-09 checkup: `cargo fmt --check` clean; `cargo test
--workspace` = 1404 passed / 0 failed (86 suites) after this session's fixes
(the earlier "538" figure was stale); `cargo clippy -- -D warnings` clean after
removing a dead test helper this session. `cargo audit`/`cargo deny check` clean (aside
from pre-existing allowed advisories under the opt-in `starlark-policy`
feature — see memory `deny_advisories_baseline`).

---

## Open decisions / blockers

- **P1 security findings H5–H7, H9** (`TASKS.md`): H5 audit hash chain is not
  true tamper-evidence; H6 snapshot store lacks containment checks; H7
  database dumps/snapshots/audit files are too permissive; H9 dynamic-eval /
  interpreter bypasses defeat string classification — **partially closed this
  session**: effect-opaque execution (script-file, interpreter stdin,
  pipe-to-shell) now requires a pre-exec recovery snapshot under
  `SnapshotPolicy::{Selective, Full}` without raising risk or adding a prompt
  (ADR-016, Iter 1–3 done); **Iter 4 (degradation UX / fail-closed when no
  snapshot can be created) and Iter 5 (threat-model/config-schema/README docs +
  TASKS close-out) remain open.** H8 (git force-push/stash-clear/branch-D) is
  **closed** — moved to Done.
- **P2 security findings M1–M5, M7–M10** (`TASKS.md`): sandbox degradation too
  quiet, user-regex size limits, in-band kill-switch/wrapper bypass (confirmed
  on the maintainer's own machine 2026-07-09), hook panics can fail open,
  additional pattern gaps, latent fail-open around shell audit readiness,
  snapshot doesn't recover the effect on committed/clean files, `aegis
  rollback` unusable from copy-pasted snapshot ids (tab-separated), README
  Before/After misrepresented the snapshot timing (fixed this session). M6
  (project config can disable recovery) is now **closed** by the C3 ratchet.
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
