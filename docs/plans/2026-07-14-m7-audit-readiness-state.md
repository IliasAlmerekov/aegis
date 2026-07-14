# M7 — Audit readiness state

## Status

Needs a focused design grill before TDD because it changes the execution-state
API. If the selected type-state crosses crate/public boundaries, record the
decision in an ADR.

## Finding

Shell-flow helpers can represent `SetupFailure` as `Ok(())`. The current call
graph appears to prevent unsafe execution, but the invariant “execution is
reachable only after audit readiness” is encoded by convention rather than by a
state the compiler can enforce.

## Scope

- Map shell-wrapper, watch, and evaluation flows that construct audit context.
- Introduce the smallest explicit ready/not-ready result that makes execution
  consume proof of readiness.
- Audit append failure remains fail closed; setup failure cannot become success.
- Keep `main.rs` orchestration-only and place business logic in the existing
  shell/runtime modules.
- Avoid a speculative framework: one typed seam for the real invariant.

## Candidate seam

`prepare_audit(...) -> Result<AuditReady, AuditSetupError>` followed by an
execution function that requires `AuditReady`. The grill must verify whether
watch mode needs the same type or an adapter around its existing framing.

## TDD seams

1. A forced setup failure never invokes the real-shell test double.
2. An append/write failure never invokes it either.
3. A valid ready state executes once and records one audit entry.
4. Watch and JSON surfaces preserve their existing structured failure output.

## Implementation sequence

1. Confirm the public behavior seams and state ownership.
2. Add the setup-failure non-execution regression.
3. Introduce the minimal ready type and migrate one shell path.
4. Migrate watch/evaluation paths only where the invariant applies.

## Verification

- Focused full-pipeline audit and watch tests
- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`

