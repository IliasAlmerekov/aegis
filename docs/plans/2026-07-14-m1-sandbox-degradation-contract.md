# M1 — Sandbox degradation contract

## Status

Draft — requires a finding-specific `grill-with-docs` session before TDD.
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

- Surface `SandboxStatus::Unavailable` through the protocol-safe active channel:
  interactive stderr/TUI, structured JSON, watch result frame, and audit.
- Preserve zero-noise behavior for `NotConfigured`.
- Preserve hard failure when `required = true`.
- Document actual write/network/read scope of current platform profiles without
  claiming a general sandbox.

## TDD seams

1. Shell wrapper: optional unavailable confinement executes only after a visible
   warning and records `Unavailable`.
2. JSON/watch: structured output carries degradation without corrupting stdout
   protocol framing.
3. Required sandbox: unavailability denies execution.
4. Unconfigured sandbox: no degradation warning.

## Implementation sequence

1. Add protocol-level failing tests for optional-unavailable status.
2. Centralize user-facing degradation rendering from the actual execution result.
3. Align audit and documentation with the same status vocabulary.
4. Run platform-specific sandbox tests where available.

## Verification

- Focused sandbox + shell/watch/JSON integration tests
- `rtk cargo test --workspace`
- `rtk cargo clippy -- -D warnings`
- `rtk cargo fmt --check`
- `rtk cargo audit`
- `rtk cargo deny check`
