# H9 — ADR-016 effect-opaque execution recovery backstop

## Status

Design locked on 2026-07-15; implementation not started. Iterations 1–3 landed
in `8dd5392`. The remaining work is the required-recovery runtime gate and
public-contract alignment described below. H9 stays **Partial** until the code,
docs, review/re-review cycle, local gates, and required PR CI checks pass.

M1 optional Sandbox degradation and M8 general Snapshot/Rollback wording remain
separate findings. H7b audit-artifact hardening is locally verified with PR CI
pending; H9 must preserve its shared audit/threat-model/glossary diff rather than
overwrite it.

## Problem

H9 is not “Aegis missed every destructive spelling.” The scanner already catches
many direct and recursively visible effects. The ADR-016 gap is narrower:
**Effect-opaque execution**. Aegis can identify a bounded command shape that hands
control to another execution layer, but cannot determine the eventual filesystem,
database, or network effect from the assessed command text.

The strongest v1 case is **Script-file execution**: `sh ./cleanup.sh`,
`python3 ./cleanup.py`, `node ./cleanup.js`, `source ./x`, and `. ./x`. Bounded
interpreter-stdin (`sh -s`) and existing pipe-to-shell shapes also carry the
`effect_opaque` fact. Arbitrary dynamic evaluation, encoded payloads, interpreter
library calls, package runners, and TOCTOU remain outside H9.

Iterations 1–3 added the fact, bounded detection, policy/audit plumbing, and
pre-execution Snapshot lifecycle. The remaining failure is semantic: current
policy can turn `snapshots_required` back to `false` when no Snapshot plugin is
applicable, and runtime execution continues when an attempted required Snapshot
produces zero records. Absence of recovery therefore erases the requirement that
was meant to guard the opaque effect.

## Decision anchor

ADR-016 remains the architectural decision. The 2026-07-15 grilling session
clarified the following implementation contract:

1. `RiskLevel` and Required recovery remain orthogonal. No risk level changes.
2. In `Protect` and `Strict`, bounded effect-opaque execution under
   `SnapshotPolicy::{Selective, Full}` requires at least one successfully created
   Snapshot. The requirement exists independently of plugin applicability.
3. `Mode::Audit` and effective `SnapshotPolicy::None` are intentional trusted
   opt-outs. They do not produce a Recovery degradation.
4. Ordinary non-effect-opaque `Danger` Snapshot behavior remains best-effort per
   ADR-004. H9 must not silently turn every failed Danger Snapshot into a new
   execution denial.
5. An allowlist match, global `[[rules]] Allow`, Safe auto-approval, ordinary
   approval, or persisted `ApproveAlways` rule cannot waive Required recovery.
6. Required recovery is `Ready` when at least one Snapshot was created and
   `Degraded` when none was created.
7. Non-interactive Recovery degradation denies. Interactive execution may proceed
   only through a visible, one-time Recovery override. That override cannot be
   persisted as an allowlist rule.
8. Shell and Watch share one typed Recovery status calculation. Their terminal
   adapters remain separate.
9. Audit records both the final decision and
   `RecoveryDegradation::NoSnapshotAvailable`. A human Recovery override changes
   an otherwise auto-approved execution to `Decision::Approved`.
10. `ExecutionTransport::Evaluation` remains evaluation-only: it reports the
    requested Snapshot plan and applicable plugins but does not simulate a runtime
    Snapshot attempt or Recovery degradation.

## Non-goals

- No filesystem `stat()` or referenced-file read on the scanner hot path.
- No referenced script-file inspection.
- No package/script-runner expansion (`npm run`, `make`, `cargo xtask`, and
  siblings).
- No new `RiskLevel`, `Decision`, exit code, or JSON evaluation schema version.
- No mandatory Sandbox or new confinement tier.
- No general Snapshot guarantee, backup system, or universal Rollback promise.
- No raw plugin error strings in the stable Audit log contract.
- No redesign of Snapshot plugin applicability or rollback fidelity.

## Verified base — Iterations 1–3

**Complete in `8dd5392`.**

- `Assessment.effect_opaque` is a direct fact orthogonal to `RiskLevel`.
- `PolicyDecision` carries `snapshots_required` and the reserved
  `confinement_required` axis.
- Audit entries have backward-compatible optional `effect_opaque`,
  `snapshots_required`, `confinement_required`, and `recovery_degradation`
  fields.
- Bounded script-file, interpreter-stdin, and pipe-to-shell shapes are detected
  without changing their risk solely because they are effect-opaque.
- Effect-opaque commands enter the existing pre-execution Snapshot lifecycle when
  an applicable plugin is found.
- Project config cannot weaken trusted recovery settings under the C3 ratchet.
- Runtime audit construction records the ADR-016 assessment and policy facts.

Do not reopen scanner detection during the remaining H9 work unless a failing
regression proves the existing verified base is insufficient.

## Iteration 4A — requirement independent of availability

**Goal:** preserve Required recovery even when planning finds no applicable
Snapshot plugin, without changing ordinary Danger semantics.

### Red

Add focused policy/planning regressions proving:

- effect-opaque + `Protect` + `Selective` + no applicable plugin still yields
  `snapshots_required = true` and `SnapshotPlan::Required { applicable_plugins:
  [] }`;
- the same holds in `Strict` / `Full` when the command would otherwise execute;
- effective `SnapshotPolicy::None` and `Mode::Audit` remain opt-outs;
- `Mode::Audit` remains a recovery opt-out even when a global policy rule
  matches; do not rely only on the current late `Mode::Audit` match arm because
  policy-rule evaluation precedes it;
- an ordinary non-effect-opaque `Danger` command with no applicable plugin keeps
  its existing best-effort behavior;
- Safe auto-approval and global `[[rules]] Allow` cannot remove the effect-opaque
  recovery requirement;
- the project-layer ratchet still rejects recovery weakening.

### Green

Adjust policy so effect opacity establishes the requirement under active recovery
policy before plugin applicability is considered. Keep the current applicability
check for ordinary non-effect-opaque Danger Snapshot planning. Allow a required
Snapshot plan to carry an empty applicable-plugin list; that empty list is evidence
for the later runtime gate, not evidence that the requirement disappeared.

No scanner or parser change belongs in this slice.

## Iteration 4B — shared Recovery status and focused prompt

**Goal:** represent the post-attempt recovery fact once and give both execution
surfaces the same decision input.

### Red

Add unit tests for one shared pure helper/type:

- effect-opaque + active requirement + one or more records → `Ready`;
- effect-opaque + active requirement + zero records →
  `Degraded(NoSnapshotAvailable)`;
- an explicit opt-out produces no required Recovery status;
- ordinary non-effect-opaque Danger with zero records does not activate the H9
  gate.

### Green

Add the smallest shared runtime/planning API usable by the root Shell flow and
library Watch flow. It may expose `Option<RecoveryStatus>` so the post-attempt
states remain only `Ready` and `Degraded`; absence means Required recovery did
not apply. Keep Snapshot records and plugin-specific failures in the Snapshot
subsystem.

Add dedicated testable TUI renderers/adapters for:

- stdin-TTY Shell prompting;
- `/dev/tty` Watch prompting;
- non-interactive denial.

The message must be shape-neutral because the same boolean fact covers script
files, interpreter stdin, and pipe-to-shell. It must communicate all of:

- Aegis could not determine the eventual effect from assessed command text;
- no required Snapshot was created;
- proceeding would run without the ADR-016 recovery backstop.

The only interactive choices are **Run once without recovery** and **Deny**.
Do not expose `ApproveAlways` / `DenyAlways` or write config from this prompt.
Verbose/tracing output may retain plugin-specific warnings; the stable prompt and
Audit schema use `NoSnapshotAvailable` rather than raw error strings.

## Iteration 4C — Shell fail-closed execution and audit

**Goal:** apply the shared Recovery status after Snapshot creation and before
audit/execution on the Shell transport.

Preserve this ordering:

1. ordinary policy decision / risk approval;
2. Snapshot attempt for an approved or auto-approved plan;
3. shared Recovery status evaluation;
4. one-time Recovery override prompt when degraded and interactive;
5. append exactly one final AuditEntry;
6. execute only after the audit append succeeds.

Consequences:

- a Safe effect-opaque command can reach a recovery prompt without changing its
  `RiskLevel`;
- a Warn/Danger effect-opaque command may show the ordinary risk prompt first and
  a separate recovery prompt after its approved Snapshot attempt fails;
- non-interactive degradation, EOF/read failure, or explicit Deny records
  `Decision::Denied`, an empty Snapshot list, and
  `recovery_degradation = no_snapshot_available`, then returns exit code `2`;
- **Run once without recovery** records `Decision::Approved` plus the degradation
  and then executes;
- failure to append the degraded AuditEntry returns the existing internal-error
  exit code and never executes the child;
- a successful Snapshot records no Recovery degradation and introduces no extra
  prompt.

### Required Shell proofs

- no TTY: child side effect absent; exit `2`; degraded denied AuditEntry present;
- interactive Deny: child side effect absent; same audit facts;
- interactive Run once: child side effect present; `Approved` degraded AuditEntry;
- successful Snapshot: no recovery prompt and no degradation field;
- an auto-approved effect-opaque command and a global policy-rule Allow still
  pass through this gate.

Use a temporary marker or equivalent observable child effect; do not accept a
test that proves only the helper return value.

## Iteration 4D — Watch parity

**Goal:** apply the same Recovery status to the Watch execution transport without
corrupting its NDJSON control stream.

- Keep Snapshot creation gated on the final ordinary approval decision.
- Evaluate Recovery status before `append_watch_audit_entry` and child spawn.
- Use the existing `/dev/tty` surface for a one-time Recovery override.
- When `/dev/tty` is unavailable, emit the existing denied NDJSON result and exit
  contract; never write prompt text into the protocol stream.
- Record the same audit decision/degradation matrix as Shell.
- Do not implement separate hook behavior: installed hooks already route the
  command through `aegis --command`, which reaches the Shell flow.

### Required Watch proofs

- no TTY: denied NDJSON result and no child side effect;
- TTY Deny: no child side effect and degraded denied audit;
- TTY Run once: child executes and audit records `Approved` plus degradation;
- successful Snapshot: existing no-extra-prompt behavior remains.

If full pseudo-terminal coverage is impractical in the first red test, isolate
the Watch orchestration behind a testable prompt adapter, but retain at least one
integration proof that the NDJSON execution surface cannot run a degraded command
without an available TTY.

## Audit compatibility matrix

| Scenario | Final decision | Snapshots | Recovery degradation | Execute |
|---|---|---:|---|---:|
| Required recovery ready | existing approved/auto-approved decision | `>= 1` | omitted | yes |
| Degraded, non-interactive | `Denied` | `0` | `no_snapshot_available` | no |
| Degraded, interactive Deny | `Denied` | `0` | `no_snapshot_available` | no |
| Degraded, Run once | `Approved` | `0` | `no_snapshot_available` | yes |
| Audit/None opt-out | existing decision | current opt-out behavior | omitted | existing behavior |
| Ordinary non-opaque Danger Snapshot failure | existing decision | `0` | omitted by H9 | existing ADR-004 behavior |

Keep `recovery_degradation` optional and omitted when absent. Older AuditEntry
JSONL without ADR-016 fields must continue to deserialize. Do not bump an audit
or evaluation schema version solely for this change.

## Iteration 5 — documentation and release-gate alignment

**Goal:** make public claims match the implemented distinction between ordinary
best-effort Snapshots and ADR-016 Required recovery.

After the runtime behavior is green, update:

- `docs/threat-model.md`: add Effect-opaque execution as a bounded scenario;
  explain recovery-first mitigation, non-interactive denial, one-time Recovery
  override, optional Sandbox tier, and the remaining heuristic limits;
- `docs/config-schema.md`: remove “Snapshot requests matter only for Danger” and
  document Protect/Strict, Audit, `SnapshotPolicy::None`, no-plugin degradation,
  and the ordinary-Danger best-effort boundary;
- `README.md`: add a concise public explanation without claiming complete backup,
  universal undo, script inspection, or sandbox enforcement;
- `crates/aegis-config` doc comments and config template comments whose current
  “before dangerous commands” wording excludes effect-opaque recovery;
- tracked `aegis-schema.json`: regenerate it from the updated source schema and
  verify the diff rather than editing generated text independently;
- ADR-016: keep the accepted decision and the ordinary-Danger/one-time-override
  clarification recorded here;
- `CHANGELOG.md` and `PROJECT_STATE.md`: update only after implementation and
  verification;
- `TASKS.md`: keep H9 Partial until review/re-review, local gates, and required PR
  CI pass; close it in the required follow-up bookkeeping step.

Do not absorb M8's general Snapshot/Rollback copy rewrite. H9 edits only statements
that contradict Required recovery or its explicit opt-outs.

Add or extend tracked-doc contract tests so future wording cannot collapse the
model back to “Snapshots only for Danger.” JSON evaluation stays schema version
`1`, `execution.mode = evaluation_only`, and does not claim that a runtime
Snapshot attempt occurred.

## Likely implementation touchpoints

The TDD cycle should confirm exact placement before editing, but the current
runtime path points to:

- `crates/aegis-policy/src/engine.rs` and `engine/tests.rs`;
- `src/planning/core.rs` / `src/planning/types.rs`;
- a focused shared recovery-status module under `src/runtime/` or the equivalent
  smallest public library boundary required by Shell and Watch;
- `crates/aegis-tui/` focused recovery prompt renderer/adapters;
- `src/shell_flow.rs`;
- `src/watch/runner.rs` and Watch protocol tests;
- `src/runtime/context.rs` audit options/building;
- focused Shell/Watch/audit integration tests;
- the Iteration 5 public/config docs listed above.

Do not put this logic in `src/main.rs`, duplicate the recovery predicate between
Shell and Watch, or change `crates/aegis-scanner/` without new evidence.

## TDD and engineering gates

Implement Iterations 4A–4D red-green in order. After focused tests are green:

1. run `code-review` on Standards and Spec axes;
2. run adversarial `re-review`/skeptic on every finding;
3. fix only confirmed findings through TDD and confirm closure, capped at two
   rounds;
4. update Iteration 5 docs and living project state after behavior is verified;
5. run:
   - `rtk cargo test --workspace`
   - `rtk cargo clippy -- -D warnings`
   - `rtk cargo fmt --check`
   - `rtk cargo audit`
   - `rtk cargo deny check`
   - `rtk git diff --check`

The scanner benchmark is not required if the implementation respects this plan
and leaves parser/scanner detection untouched. If the hot path changes, run the
scanner benchmark and treat the `< 2 ms` budget as a release gate.

H9 remains unchecked until all required PR CI contexts pass and the follow-up
`TASKS.md` closure commit records the merge evidence.

## Traceability

- Finding: `TASKS.md#h9--adr-016-required-recovery-can-degrade-silently`
- Decision: `docs/adr/adr-016-effect-opaque-execution-uses-recovery-backstops.md`
- Verified base: `8dd5392`
- Design clarification: 2026-07-15 `grill-with-docs` session
