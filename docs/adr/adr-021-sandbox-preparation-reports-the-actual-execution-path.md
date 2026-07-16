# ADR-021 — Sandbox preparation reports the actual execution path

## Status

Accepted

## Context

Optional Sandbox degradation was derived from a separate availability probe and
surfaced only through tracing. The probe could disagree with the command that
was later prepared, while a caller might never observe the tracing subscriber.
Shell replaces its process with the prepared command, but Watch remains a
persistent process that spawns multiple children; applying process-wide
Landlock restrictions to the Watch parent would affect later commands and the
control loop itself.

The Audit record and active execution channel must describe the same
confinement path that is actually selected. Evaluation-only output cannot make
that claim because it does not attempt execution.

The existing `NotConfigured` value also cannot truthfully describe an enabled
Sandbox when an earlier policy, user, or Recovery decision prevented any
preparation attempt. The Audit format is still pre-1.0, so this ambiguity should
be removed before compatibility freezes.

## Decision

`aegis-sandbox` returns a typed prepared command containing the factual
`SandboxStatus` and performs no user-facing rendering. It exposes separate
preparation entry points for Shell exec replacement and Watch child spawning.
Preparation is side-effect-free. The exec path may apply Landlock only in its
final operation immediately before replacement; the spawn path must not
restrict the persistent parent and uses the platform child launcher (`bwrap` on
Linux or `sandbox-exec` on macOS).

The runtime appends Audit from that preparation result before execution, then
surfaces optional `Unavailable` exactly once on the active channel. Required
unavailability blocks execution. Only typed infrastructure unavailability may
fall back when optional; invalid configuration and unexpected setup errors stay
fail-closed.

`SandboxStatus` gains `NotAttempted`: `NotConfigured` means Sandbox was
disabled, while `NotAttempted` means it was enabled but neither a confined nor
fallback launch path was used, whether an earlier decision stopped execution or
preparation failed closed. Older entries without the field retain the legacy
`NotConfigured` default.

Sandbox preparation occurs only after policy approval, Snapshot attempts, and
any Recovery override. Those axes remain independent, but their observed state
is written to one final Audit entry before the command starts.

Sandbox degradation is a field on that command entry, not a second Audit event.

Optional degradation uses one stable remediation message across Shell and
Watch: `Sandbox unavailable; proceeding without confinement. Set
sandbox.required = true to block execution.`

`SandboxStatus::Active` means the configured confined launch path was prepared,
not that Aegis provides a confidentiality boundary or can prove that a later
OS-level exec/spawn completed successfully.

## Consequences

- Shell warning text and Watch warning frames share one status and message
  contract without hidden or duplicate tracing output.
- Required Watch degradation is a reason-bearing blocked result rather than a
  protocol/internal error.
- Invalid-profile and unexpected setup failures audit `Blocked`/`NotAttempted`
  but retain the internal-error surface and exit code; they never fall back.
- Audit no longer depends on a separate availability probe.
- A late Landlock error after Audit fails closed without an unconfined fallback;
  as with other exec/spawn failures, `Active` records the prepared launch path,
  not proof that the operating system completed process replacement.
- Watch commands can use the optional Sandbox without restricting the Watch
  parent process; Linux Watch receives the bwrap profile without the Shell-only
  Landlock defense-in-depth layer.
- The sandbox crate gains a dependency on the inward `aegis-types` vocabulary
  and a public typed preparation result.
- Unsupported targets preserve the same optional-unavailable versus
  required-blocked distinction instead of silently ignoring enabled Sandbox
  configuration; native Windows remains outside the 1.0 product scope.
- Evaluation-only JSON remains silent about factual Sandbox status.
- Platform documentation must describe write, network, and read behavior
  without promising confidentiality.
- The existing Audit integrity payload is not versioned by this decision;
  `sandbox_status` retains its documented hash-chain limitation.
- Deterministic degradation tests use injected Rust preparation seams and
  crate-local `cfg(test)` hooks, never a production environment or CLI bypass.
