# TASKS — Security findings blocking Aegis 1.0

> Sources: the 2026-06-23 reviewer security audit and the 2026-06-24 live
> crash-test of `aegis 0.5.9`.
>
> This file is the normalized backlog index. Each item contains only the
> finding, acceptance criteria, current status, and traceability. Implementation
> detail belongs in the linked file under `docs/plans/`; architectural decisions
> belong in `docs/adr/`; completed history belongs in git and `CHANGELOG.md`.

## Release verdict

**Not ready for 1.0.** The original critical scanner bypasses are closed, but the
ordered release backlog below still contains open containment, recovery,
fail-closed, and product-contract findings. A checkbox may be closed only after
the project Definition of Done in `~/.agents/ENGINEERING_GATES.md` is satisfied.

## Status vocabulary

- **Open** — confirmed finding with acceptance criteria not yet met.
- **Partial** — a verified slice landed, but at least one acceptance criterion
  remains open; the checkbox stays unchecked.
- **Closed** — acceptance criteria are met and verification evidence is linked.

---

## P0 — Critical

### [x] C1 — Uppercase bypasses built-in regex patterns

- **Finding:** destructive commands written in uppercase bypassed regex
  verification after the case-insensitive quick pass.
- **Acceptance criteria:** built-in regex matching is ASCII-case-insensitive;
  uppercase and mixed-case destructive examples match; custom regex semantics
  remain user-controlled.
- **Status:** **Closed** — verified 2026-06-23.
- **Traceability:** commits `60de12d`, `4d8d58b`; scanner mixed-case regression
  tests.

### [x] C2 — Literal `$IFS` command obfuscation bypasses classification

- **Finding:** unquoted literal `$IFS` / `${IFS}` can act as shell separators
  while remaining fused inside scanner tokens.
- **Acceptance criteria:** literal unquoted markers split tokens across direct,
  nested-shell, heredoc, and process-substitution paths; quoted, escaped, and
  unrelated variables remain opaque.
- **Status:** **Closed** — verified 2026-06-24.
- **Traceability:** commit `a920370`; parser and scanner `$IFS` regressions.

### [x] C3 — Project config can weaken trusted security settings

- **Finding:** project-local `.aegis.toml` could weaken mode, recovery,
  confinement, provider targets, or policy outcomes inherited from trusted
  config.
- **Acceptance criteria:** project config may only tighten security-critical
  fields; weakening attempts are ignored and surfaced by `config validate`;
  project `[[rules]] Allow` cannot silently auto-approve guarded commands.
- **Status:** **Closed** — verified 2026-06-25.
- **Traceability:** [ADR-013](docs/adr/adr-013-project-config-security-ratchet.md);
  commits `86f38ad`, `f4bd0a7`, `c834477`, `5e6ab59`; config ratchet tests.

### [x] C3-residual — Project rules and audit integrity escaped the first ratchet

- **Finding:** project `[[rules]] Allow` and `audit.integrity_mode = "Off"`
  remained last-wins after the initial C3 fix.
- **Acceptance criteria:** untrusted effective `Allow` rules are dropped and
  warned; project audit integrity can only tighten; merge and warning logic share
  the same predicates.
- **Status:** **Closed** — verified 2026-06-25.
- **Traceability:** [ADR-013](docs/adr/adr-013-project-config-security-ratchet.md);
  commit `5e6ab59`; `c3_residual` and policy-planning regressions.

### [x] C4 — Launcher and absolute-path prefixes bypass token-prefix rules

- **Finding:** token-prefix lookup used the literal first token, so absolute
  paths and launcher prefixes such as `rtk`, `sudo`, or `env` hid the effective
  program.
- **Acceptance criteria:** detection resolves the `Effective program` per scan
  target by basename-normalizing paths and stripping supported launcher prefixes;
  compound commands expose each logical target.
- **Status:** **Closed** — verified 2026-06-25.
- **Traceability:** [ADR-014](docs/adr/adr-014-launcher-and-absolute-path-normalization-for-token-prefix-detection.md);
  commit `bdfbaf9`; prefix-normalization regressions.

---

## P1 — High

### [x] H1 — Standalone `&` is not a command separator

- **Finding:** background-separated commands could remain one scan target and
  hide a destructive effective program.
- **Acceptance criteria:** standalone `&` splits logical segments without
  splitting `&&`, `&>`, `>&`, `<&`, or file-descriptor duplication forms.
- **Status:** **Closed** — verified 2026-06-25.
- **Traceability:** commit `54743de`; parser and scanner ampersand regressions.

### [x] H2 — Destructive SQL inside database CLI arguments is missed

- **Finding:** destructive SQL delivered through `psql -c`, `mysql -e`, wrappers,
  or compound forms did not match first-token SQL rules.
- **Acceptance criteria:** covered destructive SQL signatures match anywhere in
  normalized scan targets without converting SQL handling into a full parser.
- **Status:** **Closed** — verified 2026-06-30.
- **Traceability:** [ADR-015](docs/adr/adr-015-destructive-sql-detected-by-regex-not-token-prefix.md);
  commit `106ac04`; destructive-SQL delivery regressions.

### [x] H3 — High-impact destructive command families are missing

- **Finding:** destructive filesystem and cloud forms including `wipefs`,
  `unlink`, writes to `authorized_keys`, shell-rc clobbering, S3/gsutil deletion,
  and related sibling commands were unclassified.
- **Acceptance criteria:** the scoped H3 and H3-follow-up command families have
  positive and narrowness examples and pass the built-in rule validation harness.
- **Status:** **Closed** — verified 2026-07-02.
- **Traceability:** commits `e2ddd5d`, `796d4a0`; scanner `h3_gaps` and built-in
  example tests.

### [x] H4 — Agent hooks can fail open when Aegis is unavailable

- **Finding:** a missing `aegis` binary could make a managed shell hook exit
  without a deny response.
- **Acceptance criteria:** Claude and Codex hooks emit the agent-specific deny
  shape for missing binary, invalid JSON, or invalid required input.
- **Status:** **Closed** — verified 2026-07-07.
- **Traceability:** [ADR-007](docs/adr/adr-007-shell-hooks-share-one-managed-helper-but-must-not-fail-open.md);
  commit `9667a02`; `tests/agent_hooks.rs`.

### [x] H5 — Audit hash-chain claims exceed the integrity contract

- **Finding:** an unkeyed, locally stored SHA-256 chain detects accidental
  corruption and some edits, but cannot prove adversarial tamper-evidence against
  an actor who can rewrite or truncate the whole log.
- **Acceptance criteria:** public docs, CLI help, config comments, source docs,
  and user-visible verification output consistently call this an **integrity
  chain/check**; they state that it has no keyed or external anchor and do not
  claim adversarial tamper-evidence. Cryptographic anchoring is out of the 1.0
  product contract unless separately designed in a future ADR.
- **Status:** **Closed** — verified locally and by all required PR CI checks on
  2026-07-15.
- **Traceability:** [plan](docs/plans/2026-07-14-h5-audit-integrity-contract.md);
  [ADR-004](docs/adr/adr-004-snapshots-are-best-effort-audit-is-append-only.md);
  [ADR-017](docs/adr/adr-017-audit-integrity-chain-has-no-external-anchor.md);
  commit `ad9c947` (PR #122).

### [x] H6 — Snapshot paths are not proven contained in the snapshot store

- **Finding:** path validation rejects absolute paths and `..` but does not prove
  that a resolved artifact remains inside the configured snapshot root before
  overwrite or deletion.
- **Acceptance criteria:** every filesystem snapshot rollback/delete path is
  resolved beneath its trusted root; traversal, symlink escape, and sibling-prefix
  cases fail closed; legitimate stored artifacts continue to round-trip.
- **Status:** **Closed** — verified locally and by required PR CI checks on
  2026-07-15.
- **Traceability:** [plan](docs/plans/2026-07-14-h6-snapshot-path-containment.md);
  [ADR-018](docs/adr/adr-018-snapshot-path-containment.md); commit `e26c7e7`.

### [x] H7a — Snapshot artifacts inherit overly broad permissions

- **Finding:** database dumps and snapshot directories can be created with
  process-umask defaults that expose database contents or credentials to other
  local users.
- **Acceptance criteria:** newly created snapshot directories are owner-only and
  snapshot files are owner-readable/writable only on supported Unix platforms;
  existing unsafe paths are rejected or tightened before sensitive writes;
  non-Unix behavior is documented and tested without adding native-Windows scope.
- **Status:** **Closed** — verified locally on 2026-07-15.
- **Traceability:** [plan](docs/plans/2026-07-14-h7a-snapshot-artifact-permissions.md);
  [ADR-019](docs/adr/adr-019-owner-only-snapshot-artifact-permissions.md).

### [ ] H7b — Audit artifacts follow unsafe paths and inherit broad permissions

- **Finding:** audit log, rotation, and lock-file creation rely on ordinary
  `OpenOptions`/`create_dir_all`, allowing broad modes and symlink-following on
  security-artifact paths.
- **Acceptance criteria:** audit directories/files/locks are owner-only on
  supported Unix platforms; active log and rotation opens reject symlink targets
  without weakening append-only or fail-closed audit behavior; platform limits
  are explicit.
- **Status:** **Open** — split from H7.
- **Traceability:** [plan](docs/plans/2026-07-14-h7b-audit-file-hardening.md);
  `crates/aegis-audit/src/logger/writer.rs`.

### [x] H8 — Destructive Git forms lack token-prefix coverage

- **Finding:** force-push, forced branch deletion, and stash drop/clear could pass
  without the intended Git rule.
- **Acceptance criteria:** `GIT-003`, `GIT-006`/`006B`/`006C`, and `GIT-008`
  cover their destructive forms and survive launcher/path normalization.
- **Status:** **Closed** — verified in the current scanner; backlog status synced
  2026-07-09.
- **Traceability:** commit `b1b64183`; C4 commit `bdfbaf9`; built-in Git rule
  examples and scanner edge-case tests.

### [ ] H9 — ADR-016 required recovery can degrade silently

- **Finding:** ADR-016 marks bounded `Effect-opaque execution` and requests a
  recovery backstop, but execution can still proceed when no required snapshot is
  created. This finding does **not** claim that Aegis can classify arbitrary
  dynamic evaluation, encoded payloads, interpreter library calls, or TOCTOU;
  those remain outside the heuristic scanner contract.
- **Acceptance criteria:** for the bounded ADR-016 shapes already detected,
  missing required recovery denies in non-interactive execution and presents the
  missing-recovery reason in an interactive prompt; audit records the degradation
  reason; threat-model/config/public docs match ADR-016. No new risk
  level, script-file inspection, filesystem `stat()` on the hot path, or package
  runner expansion is introduced.
- **Status:** **Partial** — ADR-016 iterations 1–3 landed and were verified in
  `8dd5392`; degradation UX/fail-closed behavior and public-doc alignment remain.
- **Traceability:** [plan](docs/plans/2026-07-09-h9-effect-opaque-recovery-backstop.md);
  [ADR-016](docs/adr/adr-016-effect-opaque-execution-uses-recovery-backstops.md);
  commit `8dd5392`.

---

## P2 — Medium

### [ ] M1 — Optional Sandbox degradation is not reliably visible

- **Finding:** when optional execution confinement is configured but unavailable,
  Aegis may continue unconfined with only a tracing warning that the operator
  never sees.
- **Acceptance criteria:** `SandboxStatus::Unavailable` is surfaced on the active
  user/agent channel and recorded in audit; `required = true` still fails closed;
  docs state that the optional `Sandbox` is a write/network guardrail add-on, not
  a confidentiality boundary. Making confinement mandatory or narrowing all read
  access is not required by the 1.0 product contract.
- **Status:** **Open** — no longer coupled to H9 or ADR-016 recovery.
- **Traceability:** [plan](docs/plans/2026-07-14-m1-sandbox-degradation-contract.md);
  [ADR-003](docs/adr/adr-003-aegis-is-a-heuristic-guardrail-not-a-sandbox.md).

### [ ] M2 — Untrusted custom regexes lack resource limits

- **Finding:** project/user regex compilation has no explicit pattern-length,
  automaton-size, or DFA-size budget.
- **Acceptance criteria:** untrusted regexes use bounded builders and a documented
  input-length cap; oversized patterns fail closed with actionable validation;
  built-in scanner performance remains within the hot-path gate.
- **Status:** **Open** — confirmed.
- **Traceability:** [plan](docs/plans/2026-07-14-m2-custom-regex-limits.md).

### [ ] M3a — Disabled Toggle state is operationally invisible

- **Finding:** the intentional global `Toggle` can leave Aegis in unguarded
  passthrough for multiple sessions without a visible indication on shell-wrapper
  and hook surfaces.
- **Acceptance criteria:** `aegis off` remains an explicit operator control and is
  audited when toggled; every newly started agent session receives a visible
  disabled-state notice without corrupting hook/JSON protocols; `aegis status`
  remains authoritative; disabled passthrough semantics remain explicit in docs.
- **Status:** **Open** — split from M3; toggle auditing exists, persistent
  visibility does not.
- **Traceability:** [plan](docs/plans/2026-07-14-m3a-disabled-toggle-visibility.md);
  [ADR-005](docs/adr/adr-005-global-toggle-at-command-boundaries.md).

### [x] M3b — Non-canonical `aegis` hook commands bypass wrapping

- **Finding:** a hook that treats any command beginning with `aegis` as already
  wrapped can be bypassed with a malformed or prefixed command.
- **Acceptance criteria:** only canonical `aegis --command ...` input is passed
  through; other commands beginning with the `aegis` word deny with an actionable
  reason; ordinary commands are rewritten once.
- **Status:** **Closed** — verified 2026-06-24.
- **Traceability:** [ADR-011](docs/adr/adr-011-hooks-rewrite-transparently-in-rust-and-setup-shell-escapes.md);
  commit `091950c`; hook rewrite tests.

### [ ] M4 — Hook panics can produce no deny response

- **Finding:** an unwind across the hook entry point can leave the agent without
  a structured deny response.
- **Acceptance criteria:** panics at the hook boundary are contained and converted
  into the correct Claude/Codex deny shape; ordinary error handling and panic-free
  paths remain unchanged.
- **Status:** **Open** — confirmed.
- **Traceability:** [plan](docs/plans/2026-07-14-m4-hook-panic-fail-closed.md);
  `src/install/hook.rs`.

### [ ] M5 — Remaining point pattern gaps

- **Finding:** scoped destructive forms remain uncovered: `chmod -R 000 /`,
  `TRUNCATE` without `TABLE`, `docker volume rm`, and `npm publish`.
- **Acceptance criteria:** each accepted form has a rule with positive and
  narrowness examples; the eval harness passes; SQL additions respect ADR-015
  and program-led forms respect ADR-014.
- **Status:** **Open** — separate from completed H3/H3-follow-ups.
- **Traceability:** [plan](docs/plans/2026-07-14-m5-point-pattern-gaps.md);
  [ADR-014](docs/adr/adr-014-launcher-and-absolute-path-normalization-for-token-prefix-detection.md),
  [ADR-015](docs/adr/adr-015-destructive-sql-detected-by-regex-not-token-prefix.md).

### [x] M6 — Project config can disable recovery

- **Finding:** project config could set a weaker snapshot policy or disable
  required confinement inherited from trusted config.
- **Acceptance criteria:** project recovery/confinement settings only tighten and
  weakening attempts are ignored and warned.
- **Status:** **Closed** — subsumed by C3 and verified 2026-06-25.
- **Traceability:** [ADR-013](docs/adr/adr-013-project-config-security-ratchet.md);
  commits `86f38ad`, `f4bd0a7`, `c834477`.

### [ ] M7 — Shell execution is not type-safe on audit readiness

- **Finding:** an audit setup failure can be represented as a successful helper
  result, leaving the execute-after-audit invariant dependent on control-flow
  convention.
- **Acceptance criteria:** only an explicit audit-ready state can reach command
  execution; setup/write failures cannot collapse into success; shell-wrapper and
  watch-mode behavior remain fail closed.
- **Status:** **Open** — latent structural risk.
- **Traceability:** [plan](docs/plans/2026-07-14-m7-audit-readiness-state.md);
  `src/shell_flow.rs`.

### [ ] M8 — Snapshot and Rollback wording implies post-effect recovery

- **Finding:** Git snapshots preserve pre-execution working-tree state; they do
  not capture a later command's deletion of clean tracked files, and no snapshot
  plugin is universal. Wording that promises to undo the dangerous command
  exceeds the product contract.
- **Acceptance criteria:** README, TUI/explanation copy, threat model, glossary,
  and examples describe a `Snapshot` as a best-effort pre-execution capture and
  `Rollback` as restoration of that captured state; surfaces disclose when no
  plugin applies and do not claim full backup or universal undo. Implementing
  targeted copies or a general backup system is out of scope.
- **Status:** **Open** — reframed to the actual heuristic-guardrail and
  best-effort snapshot contract.
- **Traceability:** [plan](docs/plans/2026-07-14-m8-snapshot-product-contract.md);
  [ADR-004](docs/adr/adr-004-snapshots-are-best-effort-audit-is-append-only.md).

### [ ] M9 — Snapshot identifiers do not round-trip through the rollback CLI

- **Finding:** composite tab-separated snapshot IDs render like columns and are
  not reliably copyable as the single `aegis rollback` argument.
- **Acceptance criteria:** every listed snapshot exposes a ready-to-use rollback
  path; Git and database IDs round-trip without reconstructing literal tabs;
  legacy audit entries remain recoverable.
- **Status:** **Open** — live-confirmed for Git and MySQL.
- **Traceability:** [plan](docs/plans/2026-07-14-m9-rollback-id-round-trip.md);
  `src/rollback.rs` and snapshot plugin ID parsers.

### [x] M10 — README shows a snapshot before approval

- **Finding:** the Before/After example placed snapshot creation inside the
  confirmation dialog even though snapshots are created only after approval.
- **Acceptance criteria:** the denial example contains no snapshot claim and the
  command-flow summary follows the real sequence: dialog → approval → snapshot
  attempt → execution.
- **Status:** **Closed** — README examples are corrected; review/re-review
  completed and all required CI checks passed before PR #120 merged on
  2026-07-14.
- **Traceability:** `README.md` Before/After and command-flow examples;
  `tests/snapshot_ordering.rs::test_denied_danger_command_records_no_snapshots`;
  [PR #120](https://github.com/IliasAlmerekov/aegis/pull/120);
  [required CI run](https://github.com/IliasAlmerekov/aegis/actions/runs/29342385519).

---

## P3 — Low / informational

The following follow-ups remain outside the 1.0 release-blocker sequence unless
an implementation review promotes them:

### [ ] P3-1 — SQLite snapshot creation has a TOCTOU window

- **Finding:** existence checks and copy are separate instead of reserving the
  target atomically.
- **Acceptance criteria:** target creation is atomic and collision behavior is
  covered without overwriting existing artifacts.
- **Status:** **Open**.
- **Traceability:** [consolidated plan](docs/plans/2026-07-14-p3-follow-ups.md#p3-1--sqlite-snapshot-creation-toctou).

### [ ] P3-2 — Backslash-newline tokenization is underspecified

- **Finding:** shell line-continuation edge cases can diverge from scanner
  tokenization.
- **Acceptance criteria:** supported behavior is explicitly scoped and regression
  tested; unsupported shell evaluation remains an ADR-010 non-goal.
- **Status:** **Open**.
- **Traceability:** [consolidated plan](docs/plans/2026-07-14-p3-follow-ups.md#p3-2--backslash-newline-tokenization).

### [ ] P3-3 — Parameterized IFS expansion remains opaque

- **Finding:** C2 covers literal `$IFS` / `${IFS}`, not `${IFS:-x}`,
  `${IFS:+x}`, or runtime reassignment.
- **Acceptance criteria:** make and document a bounded detection decision without
  drifting into full shell evaluation.
- **Status:** **Open**.
- **Traceability:** [consolidated plan](docs/plans/2026-07-14-p3-follow-ups.md#p3-3--parameterized-ifs-expansion).

### [ ] P3-4 — Renderer fallback is future fail-open

- **Finding:** the final wildcard renderer arm could auto-approve a future risk
  variant.
- **Acceptance criteria:** new risk variants cannot compile or execute through an
  implicit approve fallback.
- **Status:** **Open**.
- **Traceability:** [consolidated plan](docs/plans/2026-07-14-p3-follow-ups.md#p3-4--renderer-fallback).

### [ ] P3-5 — Sandbox status is vulnerable to check/use drift

- **Finding:** recorded availability can diverge from confinement actually
  applied at execution.
- **Acceptance criteria:** the applied status is derived at the execution seam and
  audit records the actual result.
- **Status:** **Open**.
- **Traceability:** [consolidated plan](docs/plans/2026-07-14-p3-follow-ups.md#p3-5--sandbox-status-toctou).

### [ ] P3-6 — Current-directory failure falls back to `.`

- **Finding:** snapshot planning can use `.` after `current_dir()` failure.
- **Acceptance criteria:** an unresolved working directory fails explicitly and
  cannot redirect snapshot work to an ambiguous path.
- **Status:** **Open**.
- **Traceability:** [consolidated plan](docs/plans/2026-07-14-p3-follow-ups.md#p3-6--current-directory-fallback).

### [ ] P3-7 — Optional Starlark dependencies carry unmaintained advisories

- **Finding:** `cargo audit` reports allowed unmaintained crates only through the
  opt-in `starlark-policy` feature.
- **Acceptance criteria:** keep the exception documented and bounded, or remove
  the dependency chain when a supported replacement is viable.
- **Status:** **Open** — no default-build CVE.
- **Traceability:** [consolidated plan](docs/plans/2026-07-14-p3-follow-ups.md#p3-7--optional-starlark-advisories).

### [ ] P3-8 — Destructive SQL has known coverage limits

- **Finding:** SQL comments as separators and additional destructive verbs/CLI
  programs remain outside current ADR-015 patterns.
- **Acceptance criteria:** accepted additions preserve the bounded heuristic and
  include narrowness examples; SQL parsing remains out of scope.
- **Status:** **Open**.
- **Traceability:** [consolidated plan](docs/plans/2026-07-14-p3-follow-ups.md#p3-8--destructive-sql-follow-ups).

---

## Current implementation order

This is a dependency/risk order, not a calendar sprint:

1. H6 → H7a → H7b — contain and protect security artifacts.
2. H9 — finish ADR-016 missing-recovery behavior.
3. M3a — make the disabled Toggle state visible. M3b is already closed.
4. M4 → M7 — harden hook and audit fail-closed structure.
5. M9 — make Rollback operationally usable.
6. M1 — surface optional Sandbox degradation.
7. M2 → M5 — bound untrusted regexes and close scoped pattern gaps.
8. H5 → M8 — align integrity and snapshot promises with the product contract.
9. P3 follow-ups — only after release blockers, unless promoted by new evidence.

## Confirmed strengths retained from the audit

- Intrinsic `Block` remains unbreakable by allowlist, rules, mode, or CI policy.
- Classification, config, policy, confirmation, and hook failures are intended to
  fail closed.
- Aegis remains a heuristic command guardrail, not a sandbox or backup system.
