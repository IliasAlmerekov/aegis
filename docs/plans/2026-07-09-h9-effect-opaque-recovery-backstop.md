# H9 — ADR-016 effect-opaque execution recovery backstop

## Status

Partial — iterations 1–3 landed in `8dd5392`; iteration 4 and the remaining
public-contract alignment are open. M1 Sandbox degradation is tracked separately
in `2026-07-14-m1-sandbox-degradation-contract.md`.

## Problem

H9 is not “Aegis missed every destructive spelling.” Live checks show the scanner already
catches several reviewer examples and shape idioms:

- `find . -delete` → `Danger`
- `git clean -fdx` → `Warn`
- heredoc payloads containing destructive commands → scanned recursively
- `curl … | sh` / pipe-to-shell → `Danger`
- `bash -c "$X"` and similar indirect forms → shape-detected

The ADR-016 gap is narrower and sharper: **Effect-opaque execution**. Aegis can see that a
command hands control to another execution layer, but not the eventual filesystem,
database, or network effect. The strongest confirmed v1 case is **Script-file execution**:
`sh ./cleanup.sh`, `python3 ./cleanup.py`, `node ./cleanup.js`, `source ./x`, and `. ./x`.

Raising all such commands to `Warn` would create approval fatigue for ordinary agent work
(`sh ./configure`, build scripts, test helpers). Making sandbox confinement the default
backstop would push the hard problem into `allow_write`: permissive profiles fail to
contain harmful scripts; strict profiles break legitimate workflows.

## Decision anchor

ADR-016 is the architectural contract for this slice:

- `RiskLevel` and backstop requirements are orthogonal.
- Effect-opaque commands set `effect_opaque = true` without raising risk by default.
- The primary v1 backstop is recovery: `snapshots_required = true`.
- Confinement is a separate optional strict tier represented by
  `confinement_required`; it remains `false` in v1 until that tier is designed.
- Missing required recovery fails closed in non-interactive mode and prompts loudly in
  interactive mode.
- `SnapshotPolicy::None` is trusted/global opt-out only; project config cannot weaken
  recovery because of the C3/M6 ratchet.

Arbitrary runtime expansion, encoded payloads, interpreter library calls, and
symlink-swap TOCTOU are not acceptance criteria for H9. Aegis remains a heuristic
guardrail and ADR-010 continues to exclude full deferred shell evaluation.

## Non-goals

- Do not add filesystem `stat()` to the hot path in v1.
- Do not inspect referenced script-file contents in v1.
- Do not treat package/script runners (`npm run`, `make`, `cargo xtask`, etc.) as
  effect-opaque by default in v1.
- Do not add a fifth `RiskLevel`.
- Do not make sandbox confinement the primary mitigation for effect opacity.

## Iteration 1 — Model and audit plumbing

**Status: complete in `8dd5392`.**

Goal: represent effect opacity and confinement requirements without changing risk ordering.

Tasks:

1. Add an `effect_opaque` marker to the scanner / assessment data model.
2. Add `confinement_required` beside the existing `snapshots_required` decision plumbing.
3. Extend audit output so an entry can show:
   - `risk`
   - `effect_opaque`
   - `snapshots_required`
   - `confinement_required`
   - the reason for any required-backstop degradation
4. Keep backward compatibility for older audit entries.

Tests:

- Existing audit parsing still accepts old entries.
- New audit entries include the new fields for effect-opaque commands.
- `RiskLevel` ordering and serialized names remain unchanged.

## Iteration 2 — Bounded shape detection

**Status: complete in `8dd5392`.**

Goal: detect v1 effect-opaque forms without turning this into a general interpreter.

V1 positive forms:

- `sh ./x`, `bash ./x`, `zsh ./x`
- `python ./x.py`, `python3 ./x.py`
- `node ./x.js`
- `ruby ./x.rb`
- `perl ./x.pl`
- `source ./x`, `. ./x`
- `sh -s`, `bash -s`
- existing pipe-to-shell shapes (`… | sh`, `… | bash`) should also mark
  `effect_opaque = true` even when their `RiskLevel` is already `Danger`

Path-like token criteria:

- token contains `/`, or
- token has a known script extension: `.sh`, `.bash`, `.zsh`, `.py`, `.js`, `.mjs`,
  `.cjs`, `.rb`, `.pl`

V1 negative forms:

- inline script bodies that are already extracted and scanned (`python -c`, `node -e`)
- package/script runners (`npm run build`, `make test`, `cargo xtask`)
- interpreter invocations with ordinary flags but no script-file-looking token

Tests:

- Positive forms set `effect_opaque = true`.
- Positive forms do not raise `RiskLevel` by themselves.
- Negative forms do not set `effect_opaque`.
- Pipe-to-shell keeps its existing `RiskLevel` and also sets `effect_opaque`.

## Iteration 3 — Policy and snapshot flow

**Status: complete in `8dd5392`.**

Goal: require recovery for effect-opaque execution under normal snapshot policy.

Tasks:

1. Update policy so `effect_opaque && SnapshotPolicy::{Selective, Full}` sets
   `snapshots_required = true`.
2. Keep `SnapshotPolicy::None` as a trusted/global opt-out only.
3. Ensure project config cannot weaken this requirement under the existing ratchet.
4. Extend the shell execution flow so effect-opaque commands receive the same pre-execution
   snapshot lifecycle as `Danger` commands when `snapshots_required = true`.

Tests:

- `sh ./cleanup.sh` can remain `Safe` while setting `snapshots_required = true`.
- Successful snapshot means no extra prompt is introduced solely because of effect opacity.
- Project `.aegis.toml` cannot disable the recovery requirement.

## Iteration 4 — Degradation UX and fail-closed behavior

**Status: open; this is the remaining runtime closure criterion.**

Goal: make missing recovery loud and safe.

Tasks:

1. If `snapshots_required = true` and no snapshot can be created:
   - non-interactive mode denies / fails closed;
   - interactive mode presents a clear missing-recovery reason.
2. The prompt copy must say that script-file contents were not inspected and no recovery
   snapshot is available.
3. Record the degradation reason in the audit log.

Tests:

- Non-interactive missing snapshot denies.
- Interactive missing snapshot produces the expected explanation.
- Audit records the degradation reason.

## Iteration 5 — Documentation and release-gate alignment

**Status: partial. ADR/glossary/model docs landed; threat-model, config/public
snapshot claims, and final task close-out remain. Coordinate snapshot wording
with M8 rather than expanding H9 into a backup-system project.**

Goal: keep public claims honest.

Docs:

- `docs/threat-model.md`: explain effect-opaque execution, recovery-first mitigation, and
  sandbox-as-optional-strict-tier.
- `docs/config-schema.md`: document any new config or audit fields.
- `README.md`: avoid implying snapshots are complete backups; describe recovery as
  best-effort unless required recovery fails closed.
- `TASKS.md`: mark H9 complete only after tests and gates pass; M1 remains an
  independent Sandbox finding.
- `CHANGELOG.md` and `PROJECT_STATE.md`: update after implementation verification.

Verification:

- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
- scanner benchmark if hot-path detection changes allocate or add measurable work
