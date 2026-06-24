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

2026-06-24

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

## What was done last session (2026-06-24)

- Fixed review follow-up for ADR-011 hook/setup-shell work:
  - Corrected the malformed raw string in `hook_rejects_malformed_json_input`, restoring the separate `hook_rejects_non_object_tool_input` fail-closed test.
  - Centralized production POSIX shell quoting in `src/install/mod.rs` and reused it from setup-shell, Codex hook rendering, and hook wrapper canonicalization.
- Implemented the `2026-06-24-npm-setup-shell-codex-hooks-root-cause.md` plan (ADR-011):
  - `setup-shell` now accepts scoped npm `@` paths; managed rc block uses POSIX single-quote escaping (`export SHELL='...'`) instead of double quotes, and validation split into `validate_real_shell_path` / `validate_aegis_binary_path` so errors name the failing path (RC1, RC2).
  - Codex `SessionStart` hook emits `additionalContext` (was the invalid `context`), fixing the invalid-session-start-JSON error (RC3).
  - Codex `PreToolUse` hook converted from a jq/python3 deny shim to a thin shim that delegates to the Rust `aegis hook` transparent rewrite (allow + `updatedInput`); removed jq/python3 dependency (RC2, RC4).
  - `aegis hook` rewrite now fails closed on non-canonical `aegis …` commands and passes canonical wrappers through; added `is_canonical_aegis_wrapper`/`decode_single_quoted`.
  - Codex pre-tool-use script embeds a shell-quoted absolute Aegis binary path (`__AEGIS_BIN__` substituted at install time).
  - npm postinstall best-effort `install-hooks --all` when `~/.claude`/`~/.codex` exist (`AEGIS_NPM_SKIP_HOOKS=1` opt-out); never creates dirs, never fails install.
  - Added ADR-011 and updated the ADR index, CHANGELOG, and docs.
- Verification: `cargo build` clean, `cargo fmt --check` clean, `cargo clippy -- -D warnings` clean, full `cargo test` 519 passed, `cargo audit` clean with existing allowed warnings, and `cargo deny check` clean.

### Deferred from this session
- Claude's registered hook command stays `aegis hook` (PATH-based); absolute-path migration is entangled with `scripts/uninstall.sh` literal `aegis hook` pruning (documented in ADR-011).
- Phase 9 `aegis doctor hooks` diagnostics not implemented (explicit follow-up in the plan).

---

## Open decisions / blockers

- Remaining P0 release blockers from the security review: `$IFS` obfuscation bypass (C2) and project-local config weakening to audit-only (C3)
- CI ARM cross-compilation (`aarch64-unknown-linux-musl`) pending
- Sandbox tests on `ubuntu-latest` / `macos-latest` with real Docker/SQLite pending
- Hot path p99 < 2 ms not yet confirmed by criterion run on current workspace
- macOS Homebrew/npm smoke still an operator follow-up
- Follow-up (P3): `tests/contracts_docs.rs::readme_links_to_contract_docs` still asserts removed install-mode vocabulary (`Local`/`Binary`); the README satisfies it only via "The old Local … Binary-only … modes have been removed". Clean the test so it stops pinning deleted modes in the README.

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
