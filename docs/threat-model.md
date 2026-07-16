# Aegis threat model

This document explains what Aegis is designed to protect against, what it does
to reduce risk, and where its protection intentionally stops.

It is a companion to:

- `README.md` — user-facing security model and limitations
- `docs/adr/README.md` — ADR index and shared verification guidance
- `docs/adr/adr-010-full-shell-evaluation-and-deferred-execution-remain-non-goals.md` — explicit shell-evaluation non-goals
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
4. **Recoverability** through ordinary best-effort Snapshots and bounded
   effect-opaque Required recovery

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

### Effect-opaque execution without Required recovery

**Threat:** A bounded **Effect-opaque execution** shape, such as
`sh ./cleanup.sh`, interpreter stdin, or pipe-to-shell, hands control to another
execution layer whose eventual filesystem, database, or network effect is not
visible in the assessed command text.

**Mitigations:**

- Aegis marks only the bounded ADR-016 shapes; it does not inspect the referenced
  script file or raise `RiskLevel` solely because the shape is effect-opaque.
- In Protect and Strict with `SnapshotPolicy::Selective` or
  `SnapshotPolicy::Full`, **Required recovery** means at least one Snapshot must
  be created before execution, independently of plugin applicability.
- If no Snapshot is created, non-interactive execution denies. Interactive
  execution explains the missing recovery and can proceed only through a
  one-time Recovery override (`Run once without recovery`), which cannot be
  persisted as an allowlist rule.
- Audit records `no_snapshot_available` together with the final `Denied` or
  human `Approved` decision.
- An optional Sandbox can add confinement, but is not the primary ADR-016
  backstop and is not made mandatory by Required recovery.

**Trusted opt-outs:** `Mode::Audit` remains observe-only and
`SnapshotPolicy::None` is the trusted global recovery opt-out. Neither is a
Recovery degradation.

**Residual risk:** Detection remains heuristic. Aegis does not classify every
dynamic evaluation, encoded payload, interpreter library call, package runner,
or TOCTOU change, and does not read referenced scripts during classification.

### Optional Sandbox degradation and read exposure

**Threat:** An operator enables optional confinement but the platform cannot
prepare it, or assumes confinement hides every readable file and secret.

**Mitigations:** The optional Sandbox is a best-effort write/network guardrail
add-on, not a confidentiality boundary. Shell warns on stderr and Watch emits a
protocol warning before an optional unconfined fallback; the same command Audit
entry records `sandbox_status = "unavailable"`. Setting
`sandbox.required = true` blocks when infrastructure is unavailable. Invalid
profiles and unexpected setup errors fail closed rather than falling back.

**Residual risk:** macOS permits `file-read*`; Linux exposes read-only system
mounts plus configured writable binds. A confined command may still read files
or secrets visible through those profiles. The 1.0 contract does not narrow all
read access, and the existing Audit integrity payload still does not hash
`sandbox_status`.

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
- on Unix, Aegis-created Audit directories use mode `0700`, while the active
  log, lock, rotated segments, and gzip staging artifact use mode `0600`
- Unix artifact opens reject final-component symlinks, non-regular objects, and
  other-owner files; owner-owned broad files are tightened through the opened
  descriptor before use
- rotation validates every managed active/archive/staging slot before archive
  mutation and commits gzip output from a fresh staging artifact before removing
  the active log
- entries carry timestamps and in-process sequence numbers
- optional chained SHA-256 integrity mode records `prev_hash` and `entry_hash`
- `aegis audit --verify-integrity` checks active and rotated segments

**Residual risk:** Integrity mode is configurable, not universal. If it is off,
the log is still useful operationally but has no integrity chain.

Unix no-follow applies to the Audit artifact itself, not a component-by-component
directory walk. A pre-existing custom-path parent remains caller-owned and may
be writable or owned by another user, so an actor able to rename entries there
retains races between separate open, rename, remove, and lock operations. Use a
dedicated owner-only directory for custom audit paths. Compressed rotation adds
no `fsync` or power-loss durability guarantee. Non-Unix builds retain compatible
I/O but make no mode, owner, ACL, no-follow, or Windows reparse-point guarantee
(ADR-020).

**Known limitation:** the `sandbox_status` field (which records a sandbox
bypass, `unavailable`) is intentionally outside the hash-chain payload for
backwards compatibility with logs chained before the field existed. An attacker
with write access to `audit.jsonl` could flip a recorded `unavailable` back to
`active` without invalidating the chain. Closing this requires versioning
`chain_alg` so new entries hash `sandbox_status` while old entries keep
verifying under the original layout. See the note on `AuditIntegrityPayload` in
`crates/aegis-audit/src/logger/integrity.rs`.

### 5. Snapshot failure or false recovery expectations

**Threat:** Operators assume every dangerous command is recoverable or confuse
ADR-016 Required recovery with a general backup guarantee.

**Mitigations:**

- ordinary non-effect-opaque `Danger` Snapshots remain best-effort
- bounded Effect-opaque execution under active recovery policy must create at
  least one Snapshot, receive a one-time Recovery override, or deny
- providers are applicability-checked per environment
- snapshot records are written to the audit log
- rollback resolves snapshot targets from the audit log

**Residual risk:** A successful Snapshot captures only what its plugin supports;
it is not a complete backup or universal undo. Rollback can still fail or
conflict. Required recovery proves only that at least one Snapshot record was
created before this execution.

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

### 8. Agent handoff after deny

**Threat:** An agent treats a denied risky command as a workflow obstacle and
coaches the operator to bypass the guardrail manually through shell escapes or
equivalent out-of-band execution paths.

**Mitigations:**

- agent-facing instructions require denied decisions to be respected
- runtime hook guidance forbids bypass framing and shell-escape workaround
  suggestions
- agents may still explain the block, suggest verification steps, and hand the
  final decision to the operator in neutral language

**Residual risk:** A human operator can still act manually outside Aegis. This
policy reduces agent-assisted bypass coaching; it does not turn Aegis into a
sandbox or prevent deliberate manual execution.

## Security invariants

The following properties are part of Aegis' intended contract:

- deny paths must not silently fall through to allow
- policy/setup failures must remain fail-closed
- `Block` commands must never be bypassed by allowlist or CI behavior
- non-interactive risky commands must not auto-approve
- approved commands must be audited
- ordinary Snapshot behavior must be described honestly as best-effort, while
  ADR-016 Required recovery must fail closed or receive a one-time override

## Explicit non-goals

Aegis does not aim to provide:

- OS-level isolation
- mandatory syscall, filesystem, or network confinement after approval
- perfect shell understanding
- complete detection of obfuscated or deferred execution
- guaranteed rollback fidelity
- protection against a malicious root user or a compromised host

## Verification maturity note

Current fuzzing coverage includes parser and scanner harnesses under:

- `fuzz/fuzz_targets/parser.rs`
- `fuzz/fuzz_targets/scanner.rs`

Both targets are integrated into CI with bounded runs and corpus-backed seed inputs.

## Operational guidance

Use Aegis as one layer in a larger safety posture:

- run risky automation with least-privilege accounts where possible
- keep allowlists narrow and scoped
- enable and verify audit integrity mode for stronger audit assurances
- treat snapshots as recovery aids, not guarantees
- combine Aegis with containers, VM isolation, or other OS-level controls when
  stronger containment is required

## Code and test references

- `src/decision/`
- `src/planning/core.rs`
- `src/runtime/`
- `src/ui/confirm.rs`
- `src/watch/`
- `src/snapshot/mod.rs`
- `src/snapshot/git.rs`
- `src/snapshot/docker.rs`
- `src/audit/logger.rs`
- `tests/full_pipeline.rs`
- `tests/audit_integrity.rs`
- `tests/snapshot_integration.rs`
- `README.md`
- `docs/adr/README.md`
- `docs/adr/adr-010-full-shell-evaluation-and-deferred-execution-remain-non-goals.md`
