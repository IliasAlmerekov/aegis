# Project State

> **Agent instructions:** Read this file at the start of every session to restore context.
> After completing any significant change, update the relevant sections here.
> Keep entries concise — one or two lines each. Do not rewrite history; only update "Current" sections.

---

## Current version

`0.6.0` — pre-1.0, targeting `1.0.0` (released from `feat/shell-security`: H2 destructive-SQL match-anywhere, H3-followups scanner hardening, C3-residual project-config ratchet, C4 token-prefix anchoring fix per ADR-014)

## Active branch

`feat/shell-security` (branched from `main`)

## Last updated

2026-07-03

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

## What was done last session (2026-07-03)

- **Release prep v0.6.0.** Bumped workspace + all 12 crate versions and inter-crate
  deps to `0.6.0` (Cargo.toml, Cargo.lock via `cargo update --workspace`),
  `packaging/npm/package.json`, README badge + `--tag v0.6.0` install line,
  `docs/releases/current-line.md`. CHANGELOG: `[Unreleased]` → `[0.6.0] — 2026-07-03`
  with fresh empty `[Unreleased]`. `cargo check` green. Tag `v0.6.0` to be cut
  from `feat/shell-security`.
- **Landing polish.** Burger menu "crossed blades" animation; neon perimeter scan
  on terminal frames; shield GLB compressed 14.9MB → 242KB (simplify + Draco);
  live typed terminal scenarios (FeatureSection tabs now drive the terminal);
  HowItWorks redesigned to "One session, three steps" — single neon terminal
  playing a continuous install→setup→intercept session with a clickable synced
  stepper; audit log tail -f animation; page-wide scroll-reveal choreography
  (`ui/Reveal.jsx`, `ui/LiveTerminal.jsx`). Landing versions updated to v0.6.0.

### Previous session (2026-07-02)

- **H3-followups reviewer fixes (iteration 2).** Added `--delete-missing-args` to FS-015
  rsync delete alt list. Narrowed DB-006 redis-cli rule via a local `redis_cli_flush_is_command`
  predicate (FS-011 pattern): only fires when `FLUSHALL`/`FLUSHDB` is the first non-option token
  (the Redis command), preventing `redis-cli GET FLUSHALL` from matching. All 536 workspace tests
  green, clippy clean, fmt clean.
- **H3-followups closed via TDD.** Implemented additive scanner hardening for all
  remaining false negatives: `FS-011` now handles `wipefs` short flag bundles
  containing `a` via a local `wipefs_all_flag_present` predicate in
  `prefix_rule.rs`; `FS-015` covers `rsync --delete*`; `FS-016` blocks
  `blkdiscard`; `FS-017` covers `sgdisk --zap-all`/`-Z`; `FS-018` covers
  destructive `parted mklabel`/`rm`; `CL-014` covers `gcloud storage rm
  --recursive`; a second `DB-006` entry covers `redis-cli FLUSHALL`/`FLUSHDB`.
  No parser/tokenizer/public-API changes. Moved the three new H3-followup positive
  test functions to `crates/aegis-scanner/src/scanner/tests/h3_followups.rs` to
  keep `basic.rs` under the 800-line budget. All 536 workspace tests green,
  `cargo clippy` clean, `cargo fmt --check` clean. Benchmark not required (no
  hot-path changes).
- Grilled and planned **H3-followups** scanner hardening. Added `Short flag bundle`
  to `CONTEXT.md` and wrote the implementation plan in
  `docs/superpowers/plans/2026-07-02-h3-followups-scanner-hardening.md`.

### Previous session (2026-06-29)

- **H1 closed via TDD, then hardened after code review.** Standalone background
  `&` is now a command separator in both `split_top_level_segments` and
  `split_top_level_command_groups` (`crates/aegis-parser/src/segmentation.rs`).
  The background-`&` decision uses a shared `ends_with_redirect_target` helper
  (single source of truth for both copies): `&` splits unless the next char is
  `&`/`>` or the preceding char is an **unescaped** redirect target (`>`/`<`).
  Backslash-parity makes the helper escape-aware. This fixed two review findings:
  a **fail-open bypass** (`echo a\> & git push --force` previously stayed one
  segment → effective program `echo` → GIT-003 never fired → `Safe`) and a
  benign `<&` over-split (`cat 0<&3`). Closes the token-prefix bypass where a
  background `&` hid a destructive segment; `echo ok & git push --force` now
  raises GIT-003 (`Warn`) and PIPE-001 fires across a background `&`.
  Fail-closed in the common case; `Intrinsic Block` and `split_pipeline_segments`
  untouched; no dependency/lockfile changes.
- Verification: parser segmentation + redirect-anti-regression + pipeline-path
  tests and three scanner end-to-end regressions (incl. escaped-`>` and
  PIPE-001-across-`&`), each RED on the pre-fix code and GREEN after.
  `cargo fmt --check` clean, `cargo clippy --all-targets -- -D warnings` clean,
  full `cargo test --workspace` 1346 passed.

## What was done last session (2026-06-28)

- **C4 / ADR-014 closed via TDD**: token-prefix and by-program-indexed detections now
  resolve an `Effective program` per scan target by stripping built-in launcher
  prefixes (`rtk`, `sudo`, `env`, `command`, `nice`, `timeout`, etc.) and
  basename-normalizing absolute program paths. Regression coverage confirms
  `/usr/bin/git reset --hard`, `rtk git clean -fd`, `sudo /bin/kill -9 1`,
  `/usr/local/bin/docker volume prune`, `timeout ... terraform destroy`, and
  `/bin/bash -c ...` hit the expected scanner rules. Review follow-ups added
  timeout option parsing (`-s`/`--signal`, `-k`/`--kill-after`, etc.), sudo
  environment-assignment stripping, conservative unknown sudo/env-flag
  candidates, stacked sudo option handling (`sudo -n -u postgres ...`), and
  one-pass effective-prefix scanning. Updated ADR-014, TASKS, CONTEXT, and
  CHANGELOG.
- Verification: focused RED tests failed before implementation; focused GREEN
  tests passed for parser/scanner/audit regressions; `cargo fmt --check`,
  `cargo clippy -- -D warnings`, and full `cargo test` (536 passed) are clean.
  `cargo audit` is clean except the existing allowed starlark/paste advisories;
  `cargo deny check` is clean. `scanner_bench`: `1000_safe_commands` unchanged
  at ~1.88 ms; dangerous/heredoc benchmarks show small local Criterion
  regressions versus the prior sample but remain sub-ms.
- **Audit tail availability fix:** `read_last_entry_from_plain_file` now skips a
  torn/truncated final audit JSONL line and walks back to the previous valid
  entry, preventing one malformed tail from bricking command interception.
  Focused regression `read_last_entry_skips_truncated_final_line` passes.

### Previous session (2026-06-25)

- **C3-residual closed** via `/implement` TDD pipeline (red → green → review,
  2 iterations; iteration-1 review surfaced a `when.then = "allow"` bypass,
  iteration-2 closed it, APPROVED):
  - **Fix 1 — project `[[rules]] Allow` dropped + warned.** A project-layer
    `[[rules]]` entry whose effective decision is `Allow` — either a top-level
    `decision = "allow"` OR a `decision = "prompt"`/`"block"` rule with
    `when.then = "allow"` (resolved at runtime by `effective_decision`, which
    reads only `rule.decision` + `rule.when.then`; both `PolicyRule` and
    `WhenClause` are `#[serde(deny_unknown_fields)]`, so no other `Allow`
    source) — is DROPPED at merge and surfaced as a `project_security_ratchet`
    warning. Unlike `[[allow]]` (capped by `allowlist_override_level`), a
    `[[rules]] Allow` auto-approves before `Mode` with no ceiling. The merge
    filter (`model.rs::merge_layer`, Project-only) and the warning loop
    (`ratchet.rs::project_security_ratchet_warnings`) share the single
    `is_untrusted_allow` predicate → merge==warning parity automatically. Global
    `[[rules]]` unfiltered (last-wins); project `Prompt`/`Block` (incl.
    `when.then = "prompt"`/`"block"`) still tighten.
  - **Fix 2 — `audit.integrity_mode` ratcheted.** `most_restrictive_integrity_mode`
    + `merge_project_integrity_mode` (shared by merge + warning); stricter of
    base/requested under Project (`ChainSha256` > `Off`), warned; global
    last-wins.
  - **Test unblock:** `tests/audit_integrity.rs::verify_integrity_rejects_legacy_log_without_chain_data`
    moved its `integrity_mode = "Off"` from the project `.aegis.toml` (now
    ratcheted, can't weaken the default `ChainSha256`) to the GLOBAL
    `$HOME/.config/aegis/config.toml` (trusted, last-wins) — preserving the
    test's intent (reject an unchained legacy log) without weakening the ratchet.
  - Regression tests in `crates/aegis-config/src/model/tests/ratchet/c3_residual.rs`
    (config-layer) and `src/planning/policy_rules.rs` (engine: dropped project
    Allow leaves a `Danger` command prompting under `Protect`).
  - Verification: `cargo test` 536 passed, `cargo fmt --check` clean,
    `cargo clippy -- -D warnings` clean, `file_size_budget` green, `cargo audit`
    clean (4 pre-existing allowed unmaintained advisories under opt-in starlark
    feature only).
- **C3 grilling session** (alignment, no code yet): verified the ADR-013 scalar
  ratchet defeats the documented attack config under defaults, but found a
  same-class residual — project-layer `[[rules]] decision="Allow"` is merged by
  concatenation with no ratchet and no per-rule provenance, and `engine.rs:28-43`
  honors it before `Mode` with no `allowlist_override_level` ceiling (unlike
  `[[allow]]`), so a repo can silently auto-approve a non-`Block` `Danger`.
  Secondary: `audit.integrity_mode` is last-layer-wins (project can set `Off`).
  Sanctioned fixes: **drop + warn** project `[[rules]] Allow` (project may still
  add `Prompt`/`Block`); **ratchet** `audit.integrity_mode`. Sharpened
  `CONTEXT.md` "Policy rule" with the auto-approve invariant; amended ADR-013
  (Decision + Consequences) to extend the ratcheted set; tracked as `C3-residual`
  in TASKS.md; flipped C3 to `[x]`. Memory `project-config-ratchet` updated.
  Next: agent writes plan + slices into iterations → `/implement`.
- Closed C3 reviewer follow-ups (C3-01…C3-04) via `/implement` TDD pipeline
  (red → green → review, APPROVED iteration 1):
  - **C3-01 (HIGH)**: ratcheted provider target config (`sqlite_snapshot_path`,
    `postgres_snapshot`/`mysql_snapshot`/`supabase_snapshot` `database`, `docker_scope`)
    so a project layer can no longer empty/narrow an ENABLED provider into a silent
    no-op (the sibling-field equivalent of the auto_snapshot_* bypass). Ratchet is
    conditional on `provider_enabled_in_base = snapshot_policy == Full ||
    auto_snapshot_<provider>` (mirrors `SnapshotRegistry::from_runtime_config` Full
    materialization). `docker_scope` ratchets on breadth rank
    (`All`=2 > `Names`-non-empty/`Labeled`=1 > `Names`-empty=0): project may broaden,
    not narrow. Repointing to another non-empty target stays allowed.
  - **C3-02 (MEDIUM)**: `sandbox.allow_write` now supports project-side tightening via
    intersection `base ∩ requested` (base order preserved); expansion (paths outside
    base) dropped + warned. Merge and warning share one `ratchet_allow_write` helper.
  - **C3-03 (MEDIUM)**: added `#[serde(deny_unknown_fields)]` to both
    `PartialSandboxSettings` and the direct `SandboxSettings`, so misspelled sandbox
    fields fail closed at parse time.
  - **C3-04 (LOW)**: replaced the vacuous `auto_snapshot_git` tightening test (git
    defaults `true`) with a table-driven test over all six `auto_snapshot_*` flags.
  - Invariant preserved: merge and `project_security_ratchet_warnings` call identical
    typed helpers with identical `provider_enabled_in_base`, so `aegis config validate`
    reports the effective merged value. Global layer stays last-wins for all ratcheted
    fields. F10/F11 (project `[[rules]] decision="allow"`; `[audit] integrity_mode="Off"`)
    confirmed out-of-scope and untouched.
  - Verification: `cargo test` 533 passed, `cargo fmt --check` clean, `cargo clippy
    -- -D warnings` clean. Test helpers extracted to `model/tests/ratchet_helpers.rs`
    to keep `ratchet.rs` under the 800-line file-size budget.
- Second C3 review pass (9.0/10 → APPROVED, iteration 1) closed 4 follow-ups:
  - **bugs-01 (MEDIUM)**: tightened `ratchet_docker_scope` from rank-only to a
    structural `docker_scope_narrows` predicate — a project can no longer drop
    base-protected containers via intra-rank moves (Names→disjoint Names, Names
    pattern-subset, `Labeled`↔`Names` cross-mode, `Labeled` label change).
    Project may only keep-or-broaden (`All` broadest; `Names`→`Names` with overlay
    patterns ⊇ base = broaden-allowed).
  - **bugs-02 (LOW)**: `provider_enabled_in_base` now gates on
    `snapshot_policy != None`; merge routes all five providers through the same
    helper (was inline `provider_full || flag`) so merge==warning holds under `None`.
  - **regressions-01 (LOW)**: `sandbox.allow_write` warning now gates on genuine
    expansion (`requested ∖ base`), not Debug-string inequality — reordered-equal
    subsets no longer spuriously warn.
  - **tests-01 (LOW)**: backfilled mysql/supabase repoint + docker same-rank guard
    tests; `tests/ratchet.rs` split into `include!` fragments (`ratchet/{c3_a,c3_b,bugs}.rs`)
    to keep every file under the 800-line budget without altering test bodies.
  - Verification: `cargo test` 533 passed, `cargo fmt --check` clean, `cargo clippy
    -- -D warnings` clean, `file_size_budget` green.

### Previous session (2026-06-24)

- Closed P0 release blocker C3 (project-local config weakening):
  - project `.aegis.toml` can only tighten security-critical fields: `mode`,
    `allowlist_override_level`, `ci_policy`, `snapshot_policy`, `sandbox.enabled`,
    `sandbox.required`, `sandbox.allow_network`, `sandbox.allow_write`, and the
    six `auto_snapshot_*` flags;
  - directionality is field-specific: `true`-is-stricter fields
    (`sandbox.enabled`/`required`, `auto_snapshot_*`) keep `base || requested`;
    `sandbox.allow_network` (`true` is weaker) keeps `base && requested`;
    `sandbox.allow_write` keeps the base set under the project layer;
  - this closes the sibling-field bypasses where a project could otherwise force
    `sandbox: None` (`enabled = false`) or disable a globally-enabled snapshot
    plugin (`auto_snapshot_* = false`) despite a stricter `snapshot_policy`/
    `sandbox.required`;
  - weakening attempts are ignored in favor of the stricter inherited value and
    surfaced as `project_security_ratchet` warnings by `aegis config validate`;
    merge and warning share the same typed ratchet helpers so the reported value
    always matches the effective merge;
  - ADR-013 documents the trusted-global / untrusted-project merge boundary.
- Pre-ADR-012 session work (now under "Previous session"):
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
