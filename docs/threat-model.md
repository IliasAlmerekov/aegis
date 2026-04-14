# Aegis threat model

This document explains what Aegis is designed to protect against, what it does
to reduce risk, and where its protection intentionally stops.

It is a companion to:

- `README.md` — user-facing security model and limitations
- `docs/architecture-decisions.md` — especially ADR-010
- `docs/config-schema.md` — runtime policy, allowlist, snapshot, and audit settings

## Security posture

Aegis is a **heuristic command guardrail** for shell execution.

It is designed to reduce damage from:

- accidental destructive commands issued by AI agents
- well-intentioned but mistaken human commands
- unattended non-interactive execution of risky commands

Aegis is **not**:

- a sandbox
- a privilege boundary
- a complete shell interpreter
- a defense against a determined adversary actively trying to evade detection

Approved commands still run with the operator's normal OS permissions.

## Assets Aegis tries to protect

1. **User data and workstation state** from common destructive shell commands
2. **Execution intent** by requiring explicit approval for risky commands
3. **Audit history** of what was approved, denied, blocked, or auto-approved
4. **Recoverability** for some dangerous commands through best-effort snapshots

## Trust boundaries

### Trusted only with validation

- project `.aegis.toml`
- global config under `~/.config/aegis/`
- allowlist rules after runtime validation
- audit log entries when integrity mode is enabled and verified

### Untrusted inputs

- raw command text passed to `aegis -c ...`
- NDJSON frames passed to `aegis watch`
- current working directory and surrounding repo/container state
- shell semantics that happen after parsing, such as aliases or runtime expansion

### External dependencies outside Aegis control

- the real shell executed after approval
- `git` and `docker` subprocesses used by snapshot providers
- the terminal/TTY used for confirmation

## Main attack and failure scenarios

### 1. Direct destructive commands

**Threat:** An agent or operator runs a recognisable destructive command such as
`rm -rf`, `git reset --hard`, destructive package/database/cloud actions, or a
network-to-shell pipeline.

**Mitigations:**

- synchronous parser + scanner pipeline on the hot path
- `Safe` / `Warn` / `Danger` / `Block` risk model
- `Warn` and `Danger` require confirmation in normal interactive use
- `Block` commands are hard-stopped

**Residual risk:** Coverage is heuristic. Commands outside the current pattern
set may still execute.

### 2. Silent approval in CI or other non-interactive contexts

**Threat:** A risky command runs without a human present because stdin is closed,
piped, or the caller is an automated agent runner.

**Mitigations:**

- non-interactive `Warn` and `Danger` commands are denied fail-closed
- `Block` remains blocked
- `Protect` mode can additionally hard-block non-safe commands in CI via
  `ci_policy = "Block"`
- watch-mode confirmation uses `/dev/tty` rather than the NDJSON control stream

**Residual risk:** Safe commands still auto-approve by design. A deliberately
over-broad allowlist can also permit risky commands.

### 3. Allowlist abuse or overreach

**Threat:** An allowlist rule unintentionally suppresses approval for commands
that should still require operator attention.

**Mitigations:**

- structured allowlist matching supports `cwd`, `user`, and expiry scoping
- project rules take precedence over global rules
- `allowlist_override_level` limits what non-safe risks can auto-approve
- `RiskLevel::Block` is never bypassable by allowlist
- allowlist decisions are recorded in the audit log when effective

**Residual risk:** `Warn`/`Danger` commands can still be auto-approved if the
operator explicitly configures a matching rule. Misconfiguration is therefore a
meaningful operational risk.

### 4. Audit-log tampering or ambiguity

**Threat:** Someone modifies audit entries after the fact or operators cannot
tell whether the log remained intact across rotation.

**Mitigations:**

- audit log is append-only JSONL
- entries carry timestamps and in-process sequence numbers
- optional chained SHA-256 integrity mode records `prev_hash` and `entry_hash`
- `aegis audit --verify-integrity` checks active and rotated segments

**Residual risk:** Integrity mode is configurable, not universal. If it is off,
the log is still useful operationally but not tamper-evident.

### 5. Snapshot failure or false recovery expectations

**Threat:** Operators assume dangerous commands are always recoverable.

**Mitigations:**

- snapshots are attempted only when policy requires them
- providers are applicability-checked per environment
- snapshot records are written to the audit log
- rollback resolves snapshot targets from the audit log

**Residual risk:** Snapshots and rollback are **best-effort**, not guaranteed.
Plugin failures do not create a security boundary, and rollback can still fail
or conflict.

### 6. Evasion through shell tricks or deferred execution

**Threat:** A caller avoids detection by encoding, assembling, or deferring the
dangerous behavior until after Aegis has finished scanning raw command text.

**Known examples:**

- encoded or obfuscated shell
- `eval "$(some_function)"`
- one command writes a script and a later command runs it
- alias/function expansion that changes behavior after parsing
- indirect payloads outside the current recognisable pattern set

**Mitigations:**

- nested and pipeline-aware scanning for some common cases
- explicit documentation of these gaps so operators do not over-trust the tool

**Residual risk:** This is a core non-goal. Aegis is not meant to stop a
determined adversary trying to bypass heuristic detection.

### 7. Watch-mode input abuse

**Threat:** A caller feeds malformed or oversized NDJSON frames to `aegis watch`
to cause unsafe behavior or resource exhaustion.

**Mitigations:**

- per-frame size cap (`1 MiB`) enforced before allocation
- invalid frames are rejected with structured errors
- fatal stdout/control-channel failure terminates watch mode fail-closed

**Residual risk:** Watch mode still trusts the host process boundary and the
real shell once a command is approved.

## Security invariants

The following properties are part of Aegis' intended contract:

- deny paths must not silently fall through to allow
- policy/setup failures must remain fail-closed
- `Block` commands must never be bypassed by allowlist or CI behavior
- non-interactive risky commands must not auto-approve
- approved commands must be audited
- snapshot behavior must be described honestly as best-effort

## Explicit non-goals

Aegis does not aim to provide:

- OS-level isolation
- syscall, filesystem, or network confinement after approval
- perfect shell understanding
- complete detection of obfuscated or deferred execution
- guaranteed rollback fidelity
- protection against a malicious root user or a compromised host

## Verification maturity note

Current fuzzing coverage is intentionally partial:

- parser fuzzing exists under `fuzz/fuzz_targets/parser.rs`
- scanner fuzzing is still a follow-on gap

That is acceptable for an early `0.1.x` local-guardrail positioning, but it is
not enough to justify stronger security or production-maturity claims on its
own.

## Operational guidance

Use Aegis as one layer in a larger safety posture:

- run risky automation with least-privilege accounts where possible
- keep allowlists narrow and scoped
- enable and verify audit integrity mode for stronger audit assurances
- treat snapshots as recovery aids, not guarantees
- combine Aegis with containers, VM isolation, or other OS-level controls when
  stronger containment is required

## Code and test references

- `src/decision.rs`
- `src/planning/core.rs`
- `src/runtime.rs`
- `src/ui/confirm.rs`
- `src/watch.rs`
- `src/snapshot/mod.rs`
- `src/snapshot/git.rs`
- `src/snapshot/docker.rs`
- `src/audit/logger.rs`
- `tests/full_pipeline.rs`
- `tests/audit_integrity.rs`
- `tests/snapshot_integration.rs`
- `README.md`
- `docs/architecture-decisions.md`
