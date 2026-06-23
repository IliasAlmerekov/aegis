# Project State

> **Agent instructions:** Read this file at the start of every session to restore context.
> After completing any significant change, update the relevant sections here.
> Keep entries concise — one or two lines each. Do not rewrite history; only update "Current" sections.

---

## Current version

`0.5.8` — pre-1.0, targeting `1.0.0`

## Active branch

`feat/shell-security` (branched from `main`)

## Last updated

2026-06-23

---

## Milestone status

| Milestone | Title | Status |
|-----------|-------|--------|
| Phase 0–4 | Foundation → Multi-crate workspace | ✅ Done |
| M1 | Snapshot lifecycle & rollback UX | ✅ Done |
| M2 | Audit log hardening | ✅ Done |
| M3 | Distribution (installer, musl, brew, npm, releases) | ✅ Done |
| M4 | Scope reduction (drop native Windows) | ✅ Done |
| M5.1 | 800-LoC file-size budget | ✅ Done |
| M5.2 | Fuzz corpus CI (≥ 100 000 iters/target) | ✅ Done |
| M5.3 | Snapshot/rollback CI integration tests | ✅ Done |
| M5.4 | Supply-chain gates green | ✅ Done |
| 1.0 docs gate | README, threat model, docs accuracy | ✅ Done |
| 1.0 perf gate | Hot path < 2 ms (p99) via criterion | 🔲 Open |
| 1.0 test gate | Zero false-negatives on security bypass corpus | 🔲 Open |

---

## What was done last session (2026-06-23)

- Rewrote `README.md` to a minimal public contract (What / Why / Install / How it works) with a visible threat-model link and an honest "heuristic, not a sandbox" statement
- Updated landing page **content only**, preserving the existing design (3D shield and section layout untouched): installer/Homebrew/npm/Cargo, `aegis setup-shell` opt-in, `v0.5.8`, and honest audit wording (append-only; tamper-evident when hash-chain integrity is enabled) replacing the prior overclaim
- Removed non-production tracked artifacts not used by the landing runtime: `test_q` (stray ELF binary), `landing/pencil.pen`, `landing/DESIGN.md`, `landing/tokens.json`, unused image assets; ignored `landing/node_modules`/`landing/dist` cleaned locally
- Reconciled M6 status: marked evidence-backed docs items in `docs/release-readiness.md` and `TASKS.md`; left perf, security-corpus, ARM cross-compile, and macOS smoke gates open

---

## Open decisions / blockers

- CI ARM cross-compilation (`aarch64-unknown-linux-musl`) pending
- Sandbox tests on `ubuntu-latest` / `macos-latest` with real Docker/SQLite pending
- Hot path p99 < 2 ms not yet confirmed by criterion run on current workspace
- macOS Homebrew/npm smoke still an operator follow-up

---

## Key files to read first

| File | Why |
|------|-----|
| `TASKS.md` | Full task breakdown with done/open status |
| `ROADMAP.md` | Phase definitions and milestone goals |
| `CONVENTION.md` | Authoritative style, security, and architecture contract |
| `docs/adr/` | All architectural decisions (ADR-001 through ADR-010) |
| `CHANGELOG.md` | Release history + [Unreleased] changes on current branch |
| `src/main.rs` | CLI entry point — orchestration only |
| `crates/` | All 11 library crates (aegis-types, aegis-parser, aegis-scanner, …) |

---

## Architecture snapshot

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

DAG boundaries enforced by `tests/architecture_boundaries.rs`.
