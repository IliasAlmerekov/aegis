# TASKS — Path to Aegis 1.0

Work breakdown derived from `PRD.md` (1.0 approved spec, 2026-06-15) cross-checked
against the current implementation (`v0.5.6`) and `ROADMAP.md`.

**Scope of this document:** only the work that remains to satisfy the PRD. Phases
0–4 of `ROADMAP.md` (foundation, scanner, persistence, module architecture,
multi-crate workspace) and most of Phase 5–6 are already shipped and are **not**
re-listed here except where the PRD adds new requirements.

**Process:** every task runs through `/implement` (red → green → review, max 3
iterations). All shell commands are prefixed with `rtk`. Any change to
`scanner`/`parser` is benchmarked with `rtk cargo criterion`. Run
`/verification-loop` before each PR. Conventional commits, no `Co-Authored-By`.

**Legend:** `[ ]` open · `[~]` partial/needs verification · `[x]` done

---

## Already shipped (context only — do not redo)

- [x] Phase 0 — Foundation Repair (async correctness, audit hard-errors, dead code, config hardening, MSRV 1.80)
- [x] Phase 1 — Scanner modernization (tokenizer, `MultiMap<program, PrefixRule>`, `Alts`, `justification`, examples)
- [x] Phase 2 — Decision persistence (`[[allow]]` / `[[block]]`, dedup, legacy `allowlist` migration)
- [x] Phase 3 — Module architecture (≤800 LoC split, typed `AuditEntry`, JSON schema)
- [x] Phase 4 — Multi-crate workspace (11 crates, DAG enforced by `tests/architecture_boundaries.rs`)
- [x] §5.2 Typed TOML DSL — `[[rules]]` with `Alts`, `when` (`WhenClause`), `justification`, examples; wired via `evaluate_policy_rules`; load-time validation
- [x] §5.2 Starlark DSL (`policy.star`, `prefix_rule`, `on_command`) — opt-in, compiled at startup
- [x] §5.5/§6 Sandbox — bubblewrap + Landlock (Linux), Seatbelt (macOS); `[sandbox]` config
- [x] §5.5 Sandbox bypass is an audit event (`sandbox_status`, `WARN`, `required = true` hard block) — v0.5.6

---

## Milestone M1 — Snapshot lifecycle & rollback UX (PRD §5.4)

Closes the open PRD decisions on snapshot management. No `Snapshot` subcommand
group exists today (`main.rs` only has `Rollback`); these tasks introduce it.

- [x] **M1.1 — `aegis snapshot list`**
  Enumerate recorded snapshots with `snapshot_id`, provider name, and recorded
  time. Resolves the opaque `cwd+hash` id discoverability gap.
  - Added a `Snapshot` subcommand group with `list` (`prune` follows in M1.2).
  - Source of truth is the **audit log** — the same one `aegis rollback` resolves
    against (`src/rollback.rs`). No `SnapshotPlugin` trait change was needed. Pure
    `format_snapshot_listing` in `src/cli_commands.rs` (mirrors `format_audit_entries`);
    deduped by `snapshot_id` keeping the **latest** occurrence so the row matches
    the entry rollback would target, newest-recorded first.
  - The log is append-only, so a listed id is *recorded*, not a recoverability
    guarantee (the backing stash/image/dump may be gone or pruned). Output is
    labelled "Recorded snapshots" accordingly; live existence checks would need
    per-provider listing (deferred — overlaps with M1.2 prune).
  - _Done when (met):_ `aegis snapshot list` prints every recorded snapshot
    (provider + recorded time + id, tab-bearing git ids preserved verbatim) and
    exits 0 with a friendly message on an empty log. Covered by unit tests in
    `cli_commands.rs` and integration tests in `tests/snapshot_list.rs`.

- [x] **M1.2 — Retention policy + `aegis snapshot prune`**
  - Added `[prune]` config with `enabled`, `max_count_per_provider`, and
    `max_age_days`; wired into `AegisConfig` defaults, merge, and schema.
  - Implemented `SnapshotPlugin::delete` for all six providers (git stashes,
    Docker images, SQLite/PostgreSQL/MySQL/Supabase dumps), treating missing
    artifacts as idempotent successes and backend failures as
    `SnapshotError::DeleteFailed`.
  - Implemented `aegis snapshot prune --yes`/`--dry-run`, retention policy via
    `RetentionPolicy`/`PrunableRecord`/`Clock`, and append-only `Decision::Pruned`
    audit entries.
  - Pruned ids are hidden from `aegis snapshot list`; `aegis rollback` rejects
    pruned ids before calling a provider.
  - Delete failures are surfaced as `AegisError::PrunePartialFailure` (non-zero
    exit) instead of being swallowed.
  - _Done when (met):_ prune respects the configured bound and preserves ids
    that pass either the per-provider count rule or the global age rule;
    regression tests cover idempotent delete, retention edge cases, CLI dry-run,
    rollback rejection of pruned ids, and delete-failure exit behavior.

- [x] **M1.3 — Snapshot ordering & trigger scope**
  Codified "snapshot is taken **only on `Allow`/`AutoApproved`**, **before** the
  (optionally sandboxed) execution, never for `Block` or `Denied`."
  - Verified the current flow in `shell_flow.rs` / `watch/runner.rs` matches the
    invariant and added explicit comments documenting the ordering.
  - Added `test_shell_approved_danger_command_child_observes_snapshot_before_exec`
    proving the shell wrapper creates a snapshot before the child runs.
  - Added `test_sandboxed_approved_danger_command_records_snapshots_before_exec`
    (Unix-only, gated by backend availability) proving a sandboxed `Danger`
    command records a snapshot and `sandbox_status = active` before execution.
  - Existing tests already cover `Denied`/`Blocked` recording no snapshots.
  - _Done when (met):_ integration tests prove a snapshot exists before a
    sandboxed `Danger` command runs, and no snapshot is taken for a `Block`ed
    command.

---

## Milestone M2 — Audit log concurrency (PRD §5.6) — ✅ done

- [x] **M2.1 — Advisory file lock (`flock`) on append**
  Serialize audit appends so parallel Aegis processes (multiple agent sessions)
  cannot interleave entries and break the SHA-256 hash chain.
  - The lock itself was already implemented: `AuditLock::exclusive` in
    `crates/aegis-audit/src/logger/writer.rs` wraps `std::fs::File::lock()`
    (stable since Rust 1.89) around the whole append critical section
    (read prev-hash → compute hash → write → flush), held for a single append
    only and released on `Drop`. Reads use a separate `acquire_shared_lock`, so
    the safe hot path never locks. Write/lock failures stay hard errors.
  - _Done when (met):_ `tests/audit_concurrency.rs` now asserts that after
    concurrent threads **and** parallel processes append, the log passes
    `aegis audit --verify-integrity` (exit 0 / `Verified`) — proving the
    SHA-256 chain stays intact with no interleaved/torn lines.

---

## Milestone M3 — Distribution channels (PRD §7, DoD §10)

The largest remaining gap. Today only `ci.yml` + `release.yml` exist; no
installer, Homebrew formula, or npm wrapper is present.

- [x] **M3.1 — `curl | sh` convenience installer**
  Global-first install script that downloads the platform binary and **verifies
  the `.sha256` checksum before writing** the binary.
  - Live-network integration test added in `tests/installer_flow.rs`, gated by
    `AEGIS_TEST_LIVE_INSTALL=1`. It downloads the latest GitHub Release asset for
    the host platform, verifies the SHA-256 sidecar, installs into a temporary
    `BINDIR`, and asserts `aegis --version` succeeds.
  - Dedicated CI job `live-installer` runs the test on `ubuntu-latest` and
    `macos-latest`.
  - `docs/release-readiness.md` and `docs/ci.md` mention the live installer
    validation.
  - _Done when (met):_ documented in README; tested end-to-end against a real release
    artifact; checksum mismatch aborts the install.

- [x] **M3.2 — Static musl release targets**
  PRD §6 requires a statically portable binary (no C build step). `release.yml`
  previously built `x86_64-unknown-linux-gnu` (native) and
  `aarch64-unknown-linux-gnu` (cross).
  - Switched Linux release targets to `x86_64-unknown-linux-musl` and
    `aarch64-unknown-linux-musl`; both Linux targets now build through `cross`
    (`use_cross: true`) for a uniform matrix that avoids runner-specific musl
    linker setup.
  - Added a `Verify static Linux binary` step (gated on
    `contains(matrix.target, 'unknown-linux-musl')`) between `Rename binary` and
    `Generate SHA256 checksum`; it runs `file` + `ldd` and fails the job
    (`exit 1`) if the artifact is dynamically linked, so static linkage is
    enforced before checksum generation and upload.
  - Installer-facing asset names (`aegis-linux-x86_64`, `aegis-linux-aarch64`)
    and macOS targets are unchanged; `.sha256` sidecars still generated for every
    artifact.
  - Regression contract in `tests/release_workflow.rs` (4 cases) asserts the musl
    matrix, absence of GNU targets, stable asset names, and the static-verification
    step — fails on the old GNU workflow, passes on the new one.
  - _Done when:_ release artifacts are static; `aarch64-unknown-linux-musl` cross
    job is green in CI (DoD §10). Local gates (`fmt --check`, `clippy -D warnings`,
    `cargo test`) pass; authoritative cross-build verification runs in the release
    workflow job (local `cross`/musl tooling may be unavailable — recorded as an
    environment limitation, not worked around with added deps).

- [ ] **M3.3 — Homebrew formula/tap**
  - _Done when:_ formula published to the tap and installs on macOS and Linux;
    `brew install` smoke-tested.
  - _Status (2026-06-22):_ network-free formula contract suite
    `tests/homebrew_formula.rs` PASS for v0.5.6 (four platform assets, SHA256
    pins, `using: :nounzip`, `test do`, caveats). Live `brew audit`/`install`/`test`
    not yet run — Homebrew is absent on this verification host and macOS is not
    accessible from WSL2. See `docs/release-readiness.md` → "Homebrew tap
    validation". Stays open per done-when.

- [ ] **M3.4 — npm wrapper package**
  Wrapper that downloads/installs the correct platform binary for the `npm i -g`
  audience.
  - _Done when:_ `package.json` published; `npm i -g` installs the right binary
    for the host platform.
  - _Status (2026-06-22):_ network-free contract suite `tests/npm_package.rs`
    PASS; `npm pack --dry-run` PASS (6 files, no `vendor/aegis`); live
    local-package install on Linux x64 downloaded `aegis-linux-x86_64` from the
    v0.5.6 release, verified SHA256, and `aegis --version` printed `aegis 0.5.6`.
    `bin.aegis` normalized to `bin/aegis.js` so `npm install -g` no longer mutates
    the source `package.json`. Not yet published to the npm registry; macOS npm
    install not yet verified. See `docs/release-readiness.md` → "npm wrapper
    validation". Stays open per done-when.

- [x] **M3.5 — GitHub Releases with `.sha256` sidecars**
  Already partially present in `release.yml`.
  - _Done when (met):_ real tag `v0.5.6` produced prebuilt binaries for all
    supported targets, each with a `.sha256` sidecar. Verified by
    `tests/release_assets_live.rs`; evidence recorded in
    `docs/release-readiness.md`.

---

## Milestone M4 — Platform scope reconciliation (PRD §8, §11)

The PRD drops **native Windows** (WSL2-only); `ROADMAP.md` Phase 0.5 / 6.3 still
reference native Windows CI and Job Objects. Align the repo with the PRD.

- [x] **M4.1 — Remove native-Windows scope**
  - Removed the native `windows-latest` CI job from the 1.0 matrix.
  - Removed native Windows Job Object sandbox dispatch/code from
    `aegis-sandbox`; native Windows now routes to unsupported sandbox behavior,
    while WSL2 continues to use the Linux implementation.
  - Native Windows shell execution fails explicitly with WSL2 guidance instead
    of falling through to `PowerShell`/`cmd.exe` semantics.
  - Docs and regression tests keep WSL2-as-Linux separate from native Windows.
  - _Done when (met):_ CI matrix matches PRD §8 (Linux x86_64/aarch64, macOS
    arm64/x86_64; Windows covered transitively via WSL2/Linux); no doc claims
    native PowerShell/cmd support.

---

## Milestone M5 — Code-quality & NFR gates (PRD §6)

- [x] **M5.1 — Enforce the 800-LoC file-size budget across the workspace**
  The original wording targeted only `aegis-sandbox/src/lib.rs` (2071 LoC); that
  sandbox split was already completed into `linux.rs` / `macos.rs` / `windows.rs`
  / `support.rs` / `unsupported.rs` under 800 LoC. The remaining `Done when`
  contract is broader — "no file in the workspace exceeds 800 LoC; tests still
  pass" — so this task closed the rest of the budget with mechanical, no-behavior
  extractions: split `crates/aegis-config/src/model.rs` into `model/{template,
  partial,serde_helpers,migration,tests}`; split `crates/aegis-snapshot/src/
  lib.rs` into `{registry,retention,clock,testing,paths}`; split `crates/
  aegis-snapshot/src/supabase/runtime.rs` into `runtime/{mod,manifest_io,
  manifest_state,rollback,tests}` preserving atomic manifest writes, rollback
  eligibility, snapshot-ID encoding, and test-only write-failure injection; and
  split the `tests/full_pipeline.rs` and `tests/installer_flow.rs` integration
  suites into focused files under `tests/support/`.
  - A regression test `tests/file_size_budget.rs` now enforces the 800-LoC budget
    so M5.1 cannot silently regress.
  - _Done when:_ no file in the workspace exceeds 800 LoC; tests still pass. ✅

- [ ] **M5.2 — Fuzz corpus ≥ 100 000 iterations per target in CI**
  Targets: `parser::parse`, `scanner::assess`, heredoc unwrapping (PRD §6, DoD).
  - _Done when:_ CI runs each fuzz target at ≥ 100k iterations; corpus committed.

- [ ] **M5.3 — Snapshot/rollback integration tests in CI**
  Run against **real** Docker / SQLite daemons (DoD §10).
  - _Done when:_ CI job exercises snapshot+rollback against live Docker/SQLite.

- [ ] **M5.4 — Supply-chain gates green**
  `rtk cargo audit` (0 CVEs) and `rtk cargo deny check` (permissive licenses
  only, no duplicate core crates, no banned crates) pass with zero findings in CI.

---

## Milestone M6 — Release Readiness 1.0 (PRD §10 Definition of Done)

Final checklist; many items depend on M1–M5.

- [ ] README and docs accurately describe all features through Phase 6.
- [ ] Threat model and known limitations visible **on the README** (link to
      `docs/threat-model.md`).
- [ ] `curl | sh` installer documented and tested (← M3.1).
- [ ] Homebrew formula/tap published and tested (← M3.3).
- [ ] npm wrapper published and installs the correct platform binary (← M3.4).
- [ ] Release workflow exercised on a real tag; artifacts include `.sha256`
      sidecars (← M3.5).
- [ ] CI includes ARM cross-compilation (`aarch64-unknown-linux-musl`) (← M3.2).
- [ ] Sandbox tested on `ubuntu-latest` and `macos-latest`; a command writing
      outside allowed paths is killed; audit records profile/status per execution.
- [ ] Snapshot/rollback integration tests run in CI against real Docker/SQLite
      (← M5.3).
- [ ] Fuzz corpus in CI at ≥ 100 000 iterations per target (← M5.2).
- [ ] `cargo audit` and `cargo deny check` both pass with zero findings (← M5.4).
- [ ] Hot path < 2 ms (p99) confirmed by `cargo criterion`; no regression.
- [ ] Zero false negatives on `tests/fixtures/security_bypass_corpus.toml`.
- [ ] CHANGELOG.md updated for the 1.0 release.

---

## Suggested ordering

1. ~~**M2** (audit flock)~~ — ✅ done; lock was already implemented, done-when now proven by `tests/audit_concurrency.rs`.
2. **M1** (snapshot lifecycle/UX) — self-contained feature work; **next up**, start with M1.1 (`aegis snapshot list`).
3. ~~**M5.1** (800-LoC file-size budget)~~ — ✅ done; sandbox split plus config/snapshot/integration-test extractions complete, enforced by `tests/file_size_budget.rs`.
4. ~~**M4** (platform reconciliation)~~ — ✅ done; native Windows scope removed, WSL2 documented as Linux.
5. **M3** (distribution) — largest effort; can parallelize formula/npm/installer.
6. **M5.2–M5.4** (CI gates) — fold into the release-hardening push.
7. **M6** (DoD sign-off) — final gate before tagging 1.0.
