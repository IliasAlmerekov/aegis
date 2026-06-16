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

- [ ] **M1.2 — Retention policy + `aegis snapshot prune`**
  - Add `[snapshot]` config fields for retention (by count and/or age) with
    `#[serde(default)]`; document each field in `docs/config-schema.md`.
  - Implement `aegis snapshot prune` removing snapshots beyond the bound for
    every provider (git stashes, Docker images, SQLite/PostgreSQL/MySQL dumps).
  - _Done when:_ prune respects the configured bound; regression test asserts
    snapshots beyond the limit are removed and in-bound ones are kept.

- [ ] **M1.3 — Snapshot ordering & trigger scope**
  Codify "snapshot is taken **only on `Allow`**, **before** the (optionally
  sandboxed) execution, never for `Block`."
  - Verify the current flow in `shell_flow.rs` / `watch/runner.rs` matches this;
    fix if not.
  - _Done when:_ an integration test proves a snapshot exists before a sandboxed
    `Danger` command runs, and no snapshot is taken for a `Block`ed command.

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

- [ ] **M3.1 — `curl | sh` convenience installer**
  Global-first install script that downloads the platform binary and **verifies
  the `.sha256` checksum before writing** the binary.
  - _Done when:_ documented in README; tested end-to-end against a real release
    artifact; checksum mismatch aborts the install.

- [ ] **M3.2 — Static musl release targets**
  PRD §6 requires a statically portable binary (no C build step). `release.yml`
  currently builds `aarch64-unknown-linux-gnu`.
  - Switch Linux targets to `*-unknown-linux-musl` (x86_64 + aarch64) per DoD.
  - _Done when:_ release artifacts are static; `aarch64-unknown-linux-musl` cross
    job is green in CI (DoD §10).

- [ ] **M3.3 — Homebrew formula/tap**
  - _Done when:_ formula published to the tap and installs on macOS and Linux;
    `brew install` smoke-tested.

- [ ] **M3.4 — npm wrapper package**
  Wrapper that downloads/installs the correct platform binary for the `npm i -g`
  audience.
  - _Done when:_ `package.json` published; `npm i -g` installs the right binary
    for the host platform.

- [ ] **M3.5 — GitHub Releases with `.sha256` sidecars**
  Already partially present in `release.yml`.
  - _Done when:_ a real tag produces prebuilt binaries for all supported targets
    each with a `.sha256` sidecar (DoD §10).

---

## Milestone M4 — Platform scope reconciliation (PRD §8, §11)

The PRD drops **native Windows** (WSL2-only); `ROADMAP.md` Phase 0.5 / 6.3 still
reference native Windows CI and Job Objects. Align the repo with the PRD.

- [ ] **M4.1 — Remove native-Windows scope**
  - Drop / gate the native `windows-latest` CI job and any Windows Job Object
    sandbox code that the PRD now lists as out of scope (§11).
  - Ensure docs (`docs/platform-support.md`, README) state: Windows is supported
    **only inside WSL2**, where it behaves as Linux.
  - _Done when:_ CI matrix matches PRD §8 (Linux x86_64/aarch64, macOS
    arm64/x86_64; Windows covered transitively via WSL2/Linux); no doc claims
    native PowerShell/cmd support.

---

## Milestone M5 — Code-quality & NFR gates (PRD §6)

- [ ] **M5.1 — Split `aegis-sandbox/src/lib.rs` (2071 LoC)**
  Violates the 800-LoC budget (PRD §6). Extract into a submodule (e.g.
  `linux.rs` / `macos.rs` / `profile.rs` / `lib.rs` wiring), moving tests with
  their code.
  - _Done when:_ no file in the workspace exceeds 800 LoC; tests still pass.

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
3. **M5.1** (sandbox split) — unblocks clean review of sandbox area.
4. **M4** (platform reconciliation) — prevents shipping contradictory Windows claims.
5. **M3** (distribution) — largest effort; can parallelize formula/npm/installer.
6. **M5.2–M5.4** (CI gates) — fold into the release-hardening push.
7. **M6** (DoD sign-off) — final gate before tagging 1.0.
