# M1 — Sandbox degradation contract

## Status

Implemented and locally verified 2026-07-16; required PR CI pending.
Independent of H9/ADR-016 recovery.

## Finding

When optional confinement is configured with `required = false`, an unavailable
sandbox can degrade to normal execution with only `tracing::warn!`. The active
operator or agent channel may never display that warning.

## Product boundary

`Sandbox` is optional execution confinement and a best-effort guardrail add-on.
It is not Aegis' primary security boundary, does not promise confidentiality, and
need not become mandatory by default. `required = true` is the explicit
fail-closed operator choice.

## Scope

- Surface `SandboxStatus::Unavailable` through each execution surface's
  protocol-safe active channel: interactive stderr/TUI, the Watch NDJSON
  protocol, and Audit.
- Keep `aegis --command ... --output json` evaluation-only. Because that path
  does not execute a command or attempt confinement, it must not claim a
  factual `SandboxStatus`; reporting sandbox configuration or intent there is
  outside M1.
- Bring Watch execution under the same optional `Sandbox` contract as Shell.
  Watch must use a spawn-safe confinement preparation path, rather than
  treating every configured Sandbox as unavailable or continuing to spawn the
  real shell directly.
- On Linux Watch, `bwrap` applying the configured mount/network profile is
  sufficient for `SandboxStatus::Active`. Landlock remains defense in depth on
  the exec-replacement Shell path; it must not be applied process-wide to the
  persistent Watch parent.
- Keep `aegis-sandbox` presentation-free: preparation returns a typed outcome
  and does not emit `tracing` or terminal output. A shared runtime renderer owns
  the stable degradation code/status/message, with Shell and Watch adapting it
  to stderr and NDJSON respectively. This guarantees one active-channel signal
  instead of a hidden or duplicated warning.
- Replace the ambiguous preparation return with a typed
  `PreparedSandboxCommand { command, status: SandboxStatus }`. Keep separate
  public `prepare_for_exec` (Shell, exec-safe Landlock allowed) and
  `prepare_for_spawn` (Watch, never restrict the persistent parent) entry
  points. A configured preparation returns `Active` or optional `Unavailable`;
  `NotConfigured` is supplied by the caller when no Sandbox was requested, and
  required unavailability remains a typed error.
- Keep preparation side-effect-free. Shell applies Landlock only in the final
  exec operation after Audit succeeds and immediately replaces the process;
  Watch preparation never applies it. A late Landlock error fails closed and
  must not trigger an optional unconfined fallback.
- On optional Watch degradation, emit a structured warning frame before the
  unconfined child starts:

  ```json
  {"type":"warning","id":"...","code":"sandbox_unavailable","sandbox_status":"unavailable","message":"Sandbox unavailable; proceeding without confinement. Set sandbox.required = true to block execution."}
  ```

  Normal child output and the final result frame follow it. `Active` and
  `NotConfigured` emit no additional frame.
- When required unavailability blocks Watch execution, the final result frame
  remains a blocked result (`exit_code = 3`) and adds optional diagnostic
  fields: `code = "sandbox_required_unavailable"`, `sandbox_status =
  "unavailable"`, and `message = "Required Sandbox unavailable; command not
  executed."`. Other result frames omit those fields for compatibility.
- Use the same stable optional-degradation message on Shell stderr and in the
  Watch warning frame: `Sandbox unavailable; proceeding without confinement.
  Set sandbox.required = true to block execution.` Shell adds only its normal
  `warning:` presentation prefix.
- Optional degradation does not add a second approval prompt. It emits exactly
  one active-channel warning per attempted execution and then preserves the
  existing command decision; operators who require fail-closed behavior use
  `sandbox.required = true`.
- Required unavailability is a final system block, not an internal execution
  error and not a `Sandbox bypass`: do not start the command, append Audit with
  `Decision::Blocked` and `sandbox_status = "unavailable"`, show the required
  Sandbox reason, and return the blocked exit code (`3`).
- Preserve fail-closed Audit ordering: prepare the real command, append its
  final decision and Sandbox status, emit any optional-degradation warning, and
  only then execute. If Audit append fails, emit neither a misleading
  "proceeding" warning nor child output.
- Preserve the execution lifecycle ordering shared with ADR-016: final
  policy/user approval, Snapshot attempts, any Recovery override, Sandbox
  preparation, one final Audit entry, optional warning, then execution.
  Commands already denied by policy or Recovery never prepare or report
  Sandbox degradation. If required Sandbox unavailability blocks after a
  Recovery override, the same Audit entry keeps the observed
  `recovery_degradation` and records the final `Blocked` decision plus
  `SandboxStatus::Unavailable`.
- Optional fallback applies only to a typed infrastructure-unavailable outcome
  (for example, a missing platform tool, unsupported platform, or unavailable
  kernel capability). Invalid configuration, profile-construction failures,
  and unexpected setup errors remain fail-closed internal errors and must not
  be relabelled as `SandboxStatus::Unavailable`.
- Document the implemented platform profiles rather than a cross-platform
  confidentiality promise: macOS permits `file-read*`; Linux exposes its
  read-only system mounts plus explicitly bound writable paths. The Sandbox
  primarily guards writes and network access, does not promise to hide files or
  secrets, and does not narrow all read access as part of the 1.0 contract.
- Do not version the Audit integrity payload in M1. The correct
  `sandbox_status` is appended to the Audit entry, while the documented legacy
  limitation that this field is outside the existing hash-chain payload remains
  a separate follow-up.
- Provide preparation behavior on every compile target. Unsupported targets
  return optional `Unavailable` with a direct command or required unavailability
  with no command; they must never reinterpret an enabled Sandbox as
  `NotConfigured`. Native Windows remains wholly unsupported by the Aegis 1.0
  product contract.
- Add `SandboxStatus::NotAttempted` before the 1.0 Audit schema freezes.
  `NotConfigured` means Sandbox is disabled; `NotAttempted` means it is enabled
  but neither a confined nor fallback launch path was used, whether an earlier
  policy/user/Recovery decision stopped the command or preparation failed
  closed; `Active` and `Unavailable` retain their preparation meanings. Legacy
  entries without a status continue to deserialize as `NotConfigured`.
- Invalid-profile and unexpected setup failures append one final Audit entry
  with `Decision::Blocked`, `SandboxStatus::NotAttempted`, and any already
  observed Snapshot/Recovery facts, then surface an internal setup error with
  exit code `4`. They never emit the optional-degradation warning or execute a
  fallback command.
- Record Sandbox status only on the command's existing Audit entry; do not append
  a second degradation event. Optional execution keeps its approved decision
  with `Unavailable`; required unavailability records `Blocked`/`Unavailable`;
  setup failure records `Blocked`/`NotAttempted`; and a policy/user/Recovery
  stop before preparation keeps its final decision with `NotAttempted`.
- Derive `SandboxStatus` from the command preparation that will actually be
  executed. A separate availability probe is not authoritative and must not be
  used for either the visible degradation or the Audit record.
- Preserve zero-noise behavior for `NotConfigured`.
- Preserve hard failure when `required = true`.
- Document actual write/network/read scope of current platform profiles without
  claiming a general sandbox.

## TDD seams

1. Shell wrapper: optional unavailable confinement executes only after a visible
   stderr warning and records `Unavailable`; no additional approval is asked.
2. Watch: one `warning` frame carrying `sandbox_status = "unavailable"` is
   emitted before child output without corrupting stdout protocol framing.
3. Required Sandbox: unavailability emits a reason-bearing blocked result, records
   `Decision::Blocked` plus `SandboxStatus::Unavailable`, and never starts the
   command.
4. Unconfigured sandbox: no degradation warning.
5. Invalid profiles and unexpected setup errors remain fail-closed even when
   `required = false`, audit `Blocked`/`NotAttempted`, and return exit code `4`.
6. Tests force typed preparation outcomes through injected Rust seams; no
   production environment variable or hidden CLI option may disable Sandbox.
   Platform tests continue to exercise real bwrap/Seatbelt only where available.

## Implementation sequence

Land as one atomic M1 PR so the public preparation API and both execution
surfaces cannot drift across intermediate merges. Implement it as five TDD
iterations:

1. Add `SandboxStatus::NotAttempted`, legacy Audit deserialization coverage,
   and decision/status truth-table tests.
2. Add presentation-free, side-effect-free typed preparation for Shell exec and
   Watch spawn, including unsupported targets and error taxonomy.
3. Move Shell to the lifecycle ordering established above; test optional
   warning, required block, setup failure, Audit failure, and Recovery state.
4. Apply Sandbox preparation to Watch; add warning/result protocol frames,
   streaming-order tests, required blocking, and active bwrap/Seatbelt coverage.
5. Synchronize public/config/threat/architecture docs, add wording contracts,
   run platform-specific tests where available, then complete the project gates.

## Documentation surfaces

- `README.md`: optional write/network guardrail, active-channel degradation,
  and no confidentiality promise.
- `docs/config-schema.md`: required/fallback behavior and the actual
  platform-specific read scope.
- `docs/threat-model.md`: mitigation, residual confidentiality/read risk, and
  the unchanged Audit hash-chain limitation.
- `PRD.md` and `ROADMAP.md`: replace the tracing-only bypass contract.
- `ARCHITECTURE.md`: typed Shell/Watch preparation and fail-closed Audit
  ordering.
- `CONTEXT.md`: canonical Sandbox definition (already sharpened during the
  grill).
- Documentation contract tests: reject stale tracing-only language and stronger
  confidentiality/read-isolation claims.

## Verification

- Focused sandbox + shell/watch/JSON integration tests
- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`

## Architecture decision

The typed Shell/Watch preparation split and user-channel ownership are recorded
in [ADR-021](../adr/adr-021-sandbox-preparation-reports-the-actual-execution-path.md).
