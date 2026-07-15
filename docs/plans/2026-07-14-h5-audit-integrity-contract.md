# H5 — Audit integrity contract alignment

## Status

Ready — grilled and confirmed. This is a wording and public-contract
correction, **not** a cryptographic anchoring project. Data formats and
verification behavior are unchanged.

## Finding

`ChainSha256` is an unkeyed chain stored with the audit log. It detects
accidental corruption, broken links, and edits not followed by a full
re-chain. An actor who can rewrite or truncate the complete local log can
produce a new internally consistent chain. Calling this adversarially
"tamper-evident" exceeds Aegis' local heuristic-guardrail contract.

## Decisions (from grilling)

1. **Terminology.** Ban `tamper-evident`, `tamper-proof`, `tamper detection`,
   and `tamper evidence` as *capability claims* for the local chain. Canonical
   term: **"audit integrity chain" / "integrity check"** (already defined in
   `CONTEXT.md:235`). Honest capability phrase: **"detects corruption and
   inconsistent edits."**
2. **Allowlist exceptions** (the wording guard tolerates these): (a) test and
   variable names describing the mechanics of corrupting a log
   (`tampered`, `detects_tampered_*`); (b) prose that explicitly *denies* the
   guarantee or describes residual risk (`CONTEXT.md:239`, `ADR-013:74`,
   `docs/threat-model.md:125`, threat-category headings); (c) historical
   `CHANGELOG.md` entries — never rewritten (append-only principle).
3. **CLI output (`aegis audit --verify-integrity`), variant B.** Success:
   `Audit integrity chain OK (...)`. Failure:
   `Audit integrity check FAILED: ...`. One honest one-line residual note on
   success (detects corruption/inconsistent edits; not a keyed/remote anchor) —
   printed once, not per line. No `tamper` wording.
4. **ADRs, variant 2.** Fix the false claim at `ADR-004:16`; write a new ADR
   (next free number) recording that keyed MAC / remote witness / monotonic
   counter / external anchor are an **explicit non-goal for 1.0**, deferred to a
   future security-model ADR.
5. **Landing copy, variant B.** Sell honest value ("flags corruption and
   inconsistent edits"), no `tamper`, no marketing disclaimer. Text only —
   design and animations untouched.
6. **Wording guard, variant A.** A Rust integration test
   (`tests/audit_integrity_wording.rs`) enumerates tracked files via
   `git ls-files`, greps the banned phrases, applies the allowlist, and fails
   with a file:line + suggested-term message. If git is unavailable the test
   fails loudly (never silently passes). Rides the existing
   `Quality (fmt, clippy, test)` check; `ci.yml` is not touched.

## Invariants (must not change)

- Serialized names: `ChainSha256`, `entry_hash`, `prev_hash`, `chain_alg`.
- `AuditIntegrityMode` variants and default (`ChainSha256`).
- `verify_integrity` behavior and existing integrity tests stay green.
- Landing design/animation; only copy strings change.

## Implementation sequence (TDD)

1. **Red:** add `tests/audit_integrity_wording.rs` asserting no banned
   capability phrase survives outside the allowlist. It fails against the
   current tree.
2. **Green — code/CLI:** reword doc comments and user-visible strings without
   touching data formats:
   - `crates/aegis-audit/**` doc comments (`logger.rs`, `logger/integrity.rs`,
     `logger/writer.rs`, `lib.rs`, `Cargo.toml` description).
   - `crates/aegis-config/src/model/{enums,rules,template}.rs`.
   - `src/main.rs` — `--verify-integrity` help + verification output (variant B).
3. **Green — schema:** regenerate `aegis-schema.json` from the config source
   doc comments (do not hand-edit the generated file).
4. **Docs/ADR/landing:** `ARCHITECTURE.md`, `PRD.md`, `ROADMAP.md`,
   `docs/config-schema.md`, `docs/release-readiness.md`,
   `docs/threat-model.md`, `docs/adr/adr-004-*` (fix line 16), new ADR + index
   update in `docs/adr/README.md`, landing `AuditSection.jsx`,
   `FeatureSection.jsx`, `TrustStrip.jsx`.
5. **Sweep:** re-run the guard and a manual grep; strong wording remains only in
   allowlisted denial/residual/historical contexts.
6. **Close-out:** update `CONTEXT.md` only if the term needs sharpening (it
   already exists — verify, don't duplicate), tick `TASKS.md` H5 per DoD,
   prepend a `CHANGELOG.md` `Security`/`Changed` entry, update
   `PROJECT_STATE.md`.

## Verification gate

- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
- `rtk git grep -n "tamper-evident\|tamper evidence\|tamper detection"` — only
  allowlisted hits remain.

## Out of scope

No HMAC / key management / remote anchoring / monotonic counters in this slice.
Any of those requires the separate security-model ADR referenced in decision 4.
