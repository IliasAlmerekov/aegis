# Project State

> **Agent instructions:** Read this file at the start of every session to restore context.
> After completing any significant change, update the relevant sections here.
> Keep entries concise — one or two lines each. Do not rewrite history; only update "Current" sections.

---

## Current version

`0.5.9` — pre-1.0, targeting `1.0.0` (released from `feat/shell-security`: ADR-011 npm/setup-shell + Codex hook rewrite, ADR-012 Claude absolute shim, C2 `$IFS` bypass fix)

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

- Implemented the `2026-06-24-claude-code-hook-shim-migration.md` plan (ADR-012),
  bringing the Claude Code hook to PATH-independent parity with Codex across 8
  TDD phases (red-test → green → gate → commit each):
  - Phase 1: lifted `write_executable`, `resolved_aegis_bin`, and
    `combine_outcomes` into `src/install/mod.rs` as shared `pub(crate)` helpers;
    dropped the duplicate `temporary_settings_path`/`write_executable` in
    `codex.rs`.
  - Phase 2: rewrote `scripts/hooks/claude-code.sh` from the legacy jq-based
    `aegis-rewrite.sh` script into a jq-free shim (`aegis-hook-version: 2`) that
    `exec`s the Rust `aegis hook`, byte-identical to the Codex shim except the
    header.
  - Phase 3: `aegis install-hooks --claude-code` (and `--all`) now materializes
    `~/.claude/hooks/aegis-pre-tool-use.sh` (0755, `__AEGIS_BIN__` substituted)
    and registers its absolute path in `settings.json` `PreToolUse`/`Bash`.
  - Phase 4: `apply_installation` is now prune-then-add — migrates away every
    aegis-managed legacy Bash registration (`aegis hook`, `aegis-rewrite.sh`,
    stale shim paths) by basename while preserving unrelated user hooks;
    idempotent reinstall.
  - Phase 5: `scripts/uninstall.sh` removes the new shim and prunes its
    absolute-path registration, alongside the legacy cleanup.
  - Phase 6: shared `aegis hook` deny output now emits a top-level `reason`
    mirroring `permissionDecisionReason` for Claude/Codex cross-compat.
  - Phase 7: ADR-012, ADR index, README/npm README, `docs/troubleshooting.md`,
    CHANGELOG, and PROJECT_STATE updated.
- Verification: `cargo test` green (install:: + agent_hooks), `cargo clippy
  -- -D warnings` clean, `cargo fmt --check` clean.
- Post-ADR-012 review reconciliation (commit `851c65e`):
  - `scripts/hooks/claude-code.sh` now ends with a trailing `\n` (POSIX
    convention) and its self-comment / ADR-012 consequence / the
    `render_claude_pre_tool_use_hook` doc comment were corrected from
    "byte-identical except header" to "behaviorally identical; only
    agent-specific comments differ" (the two shims cross-reference each
    other by name, so they are not byte-identical).
  - `scripts/uninstall.sh` normalizes a trailing slash on `$HOME` up front
    (guarding root `/`) so the string-built prune paths match the absolute
    path `std::path::absolute`/`Path::join` registers.
  - `tests/agent_hooks.rs::claude_install_migrates_legacy_aegis_hook_registration_to_absolute_shim`
    closes the migration seam end-to-end through the public
    `aegis install-hooks --claude-code` surface (seed a real legacy
    `aegis hook` → assert migration to the absolute shim + user-hook
    preservation).
  - Verification: 532 tests pass, file-size budget green (claude.rs 774,
    agent_hooks.rs 796), `cargo audit`/`cargo deny check` clean.

### Previous session (2026-06-24)

- Closed P0 release blocker C2 (`$IFS` obfuscation bypass):
  - `split_tokens` in `crates/aegis-parser/src/tokenizer.rs` now treats unquoted literal `$IFS` / `${IFS}` as shell word-separators via a new `ifs_marker_len` helper. The bare `$IFS` form matches only at an identifier boundary (so `$IFSHOME` stays intact); the braced `${IFS}` form is self-delimited by its closing brace. The helper clones the `Chars` iterator for lookahead (no extra allocation) and never panics.
  - The fix flows through `Parser::parse` and `logical_segments` into the scanner's direct, nested-shell (`bash -c` / `sh -c`), heredoc, and process-substitution scan paths without any scanner-side special-casing.
  - Quoted (`'$IFS'`, `"$IFS"`), escaped (`\$IFS`), partial (`$IF`, `${IFS`), and non-IFS variable forms (`$PATH`) remain opaque — confirmed by negative tests. No full variable expansion was introduced.
  - Tests added: tokenizer positive/negative cases (`tokenizer_tests.rs::ifs_obfuscation`), parser normalized-form cases (`parsing_tests.rs::parse_normalizes_*`), and scanner regressions for PS-006, FS-002, FS-003, FS-004, FS-006 incl. nested/heredoc/process-sub (`edge_cases.rs`).
- Verification: `cargo fmt --check` clean, `cargo clippy -- -D warnings` clean, full `cargo test` 519 passed, perf test `ten_thousand_safe_commands_under_25ms` green, `cargo audit` clean with the existing allowed `paste`/starlark advisory warning, `cargo deny check` clean.

### Deferred from this session
- Phase 9 `aegis doctor hooks` diagnostics not implemented (explicit follow-up in the plan).
- Unifying the two byte-identical hook shims into one templated script (tracked in ADR-012 consequences).

### Resolved this session
- Claude's registered hook command no longer stays the PATH-based bare `aegis hook`; the absolute-shim migration (deferred under ADR-011) is complete — see ADR-012.

---

## Open decisions / blockers

- Remaining P0 release blocker from the security review: project-local config weakening to audit-only (C3). C2 (`$IFS` obfuscation bypass) is now closed.
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
