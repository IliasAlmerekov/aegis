# P2 — Policy Correctness & Safety Design

## Problem

The runtime policy layer is partially hardened but still relies on an older
string-based allowlist model that is too coarse, too powerful, and too easy to
misconfigure. The current design leaves Aegis vulnerable to policy ambiguity,
silent weakening of intended restrictions, and a mismatch between what the user
thinks the config means and what the runtime actually enforces.

P2 closes that gap by making policy behavior strict, explicit, and testable.

## Scope

This phase covers the remaining P2 work after Ticket 2.1:

- fail-fast behavior for invalid allowlist entries
- replacement of the legacy string allowlist with a structured allowlist schema
- bounded allowlist override semantics via policy configuration
- a `aegis config validate` command with error/warning reporting

It also includes the integration work needed so runtime behavior, validation
behavior, and documentation all describe the same policy contract.

## Non-goals

- No change to scanner pattern matching semantics or `RiskLevel` meanings.
- No change to snapshot plugins beyond policy-triggered integration points.
- No dependency or CI policy changes.
- No broad redesign of audit storage format.
- No compatibility layer for the legacy `allowlist = ["..."]` schema.

## Confirmed Product Decisions

### 1. Legacy allowlist format is removed

The old string-list schema:

```toml
allowlist = ["terraform destroy *"]
```

is no longer accepted. The only supported model is a structured rule list.

### 2. Structured allowlist is the only exception mechanism

Allowlist rules become scoped policy exceptions, not blanket command-string
bypasses.

### 3. Runtime invalidity is fail-fast

The following are runtime errors that prevent command execution:

- invalid TOML or schema
- invalid allowlist rule fields
- expired `expires_at`
- conflicting or impossible values

### 4. Validation has hard errors and soft warnings

`aegis config validate` reports:

**Errors**
- broken schema/TOML
- invalid rule fields
- expired `expires_at`
- conflicting/impossible values

**Warnings**
- rule without both `cwd` and `user`
- overly broad pattern
- wildcard too early in the command structure
- `Danger` override with weak scope

### 5. `Block` remains non-bypassable in enforcement modes

No allowlist rule, CI behavior, or override setting may turn a
`RiskLevel::Block` command into an approval in `Protect` or `Strict`.

`Audit` remains an explicitly non-blocking observation mode. In that mode,
`Block` patterns are still classified and audited as `Block`, but the mode
itself does not enforce blocking. Allowlist is not the reason such a command
runs in `Audit`.

## Configuration Model

## New allowlist schema

The new config shape is:

```toml
allowlist_override_level = "Warn"

[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "/srv/infra"
user = "ci"
expires_at = "2026-01-01T00:00:00Z"
reason = "ephemeral test teardown"
```

### Rule fields

- `pattern` — required; command pattern to match
- `cwd` — optional; restricts the rule to a working directory scope
- `user` — optional; restricts the rule to a user identity
- `expires_at` — optional; RFC 3339 timestamp after which the rule becomes invalid
- `reason` — required; operator-facing justification for the exception

At least one of `cwd` or `user` is strongly recommended, but not required at
schema level. Missing both should produce a validation warning, not a parse
error.

### Pattern contract

P2 does not introduce a new policy DSL. Allowlist `pattern` keeps the current
whole-command glob model so scope stays limited:

- matching is performed against the trimmed raw command string
- the generated regex is anchored (`^...$`)
- `*` matches any substring
- `?` matches any single character
- regex metacharacters in the literal text are escaped

This means P2 strengthens policy correctness without simultaneously redesigning
the pattern language. Validation warnings such as “too broad” and “wildcard too
early” are heuristic checks on the pattern text; they do not change matching
semantics.

## New override-level config

Add:

- `allowlist_override_level = "Warn" | "Danger" | "Never"`

Recommended default:

- `Warn`

Meaning:

- `Warn` — allowlist may auto-approve `Warn`, but not `Danger`
- `Danger` — allowlist may auto-approve `Warn` and `Danger`
- `Never` — allowlist never changes runtime approval outcome for non-safe commands

## Runtime Architecture

## Separation of responsibilities

P2 should enforce one clear split:

- `src/config/model.rs` — config schema, parsing, merging, field-level validation
- `src/config/allowlist.rs` — allowlist compilation, contextual matching, quality analysis
- `src/decision.rs` — mode/risk/override semantics
- runtime wiring (`src/main.rs`, `src/watch.rs`, `src/runtime.rs`) — orchestration only
- validation surface — CLI command and shared reporting helpers

This preserves the existing goal of keeping `main.rs` thin and preventing policy
logic from drifting into execution glue.

## Allowlist runtime contract

Allowlist matching must no longer return only “matched / not matched”.

It should return a structured result that preserves which rule matched and why
it is valid for the current runtime context. The runtime context used for
matching includes:

- raw command string
- current working directory
- current user identity
- current time (for `expires_at`)

An allowlist rule is effective only if:

1. its pattern matches the command
2. its optional scope constraints match the context
3. it is not expired

If a rule is malformed, config loading fails before runtime reaches this stage.

### Matching precedence

Allowlist behavior must be deterministic when multiple rules could match.

Rule selection order:

1. discard rules that are not effective for the current context
2. prefer project-local rules over global rules
3. within the same layer, first declared rule wins

Audit and verbose output should preserve enough metadata to explain which rule
won and from which config layer it came.

### Scope resolution

- `cwd` is matched against the runtime working directory path used for command
  execution; implementations may canonicalize when safe, but planning should
  treat it as the real runtime cwd, not an environment-variable hint
- `user` is matched against the effective OS user running Aegis, not a shell
  environment variable like `$USER`

## Policy Semantics

The decision layer takes these conceptual inputs:

- `mode`
- `risk`
- `in_ci`
- `ci_policy`
- whether an effective allowlist rule matched
- `allowlist_override_level`

The decision layer returns a deterministic plan such as:

- action: `AutoApprove | Prompt | Block`
- `prompt_required`
- `should_snapshot`
- `allowlist_effective`
- `block_reason`

It remains pure and side-effect free.

## Protect mode

- `Safe` → auto-approve
- `Warn` → prompt unless an effective allowlist rule matches and override level allows `Warn`
- `Danger` → prompt + snapshot unless an effective allowlist rule matches and override level allows `Danger`
- `Block` → block

If `in_ci && ci_policy == Block`, `Warn` and `Danger` are blocked unless an
effective allowlist override is permitted by policy. `Block` remains blocked.

## Strict mode

- `Safe` → auto-approve
- `Warn` → block by default; only auto-approve when an effective allowlist rule exists and override level allows `Warn`
- `Danger` → block by default; only auto-approve + snapshot when an effective allowlist rule exists and override level allows `Danger`
- `Block` → block

CI must never weaken `Strict`.

## Audit mode

Audit remains non-blocking and non-prompting:

- `Safe | Warn | Danger | Block` → auto-approve
- no snapshots
- allowlist does not change the runtime outcome

Allowlist data may still be attached as context for validation or audit
metadata, but it must not affect Audit-mode runtime behavior.

## Explicit policy table

| Mode | Risk | No effective allowlist | Effective allowlist + `Warn` | Effective allowlist + `Danger` | Effective allowlist + `Never` |
|------|------|------------------------|-------------------------------|--------------------------------|-------------------------------|
| Protect | Safe | AutoApprove | AutoApprove | AutoApprove | AutoApprove |
| Protect | Warn | Prompt | AutoApprove | AutoApprove | Prompt |
| Protect | Danger | Prompt + Snapshot | Prompt + Snapshot | AutoApprove + Snapshot | Prompt + Snapshot |
| Protect | Block | Block | Block | Block | Block |
| Strict | Safe | AutoApprove | AutoApprove | AutoApprove | AutoApprove |
| Strict | Warn | Block | AutoApprove | AutoApprove | Block |
| Strict | Danger | Block | Block | AutoApprove + Snapshot | Block |
| Strict | Block | Block | Block | Block | Block |
| Audit | Safe/Warn/Danger/Block | AutoApprove | AutoApprove | AutoApprove | AutoApprove |

## Validation Design

## Shared validation engine

Runtime config loading and `aegis config validate` must use the same underlying
validation logic. There must not be one set of rules for startup and another for
the CLI validation command.

Recommended split:

- hard validation returns `Result<()>` / typed errors
- advisory analysis returns a list of warnings

That enables:

- runtime fail-fast on hard errors
- CLI visibility into both errors and warnings

## `aegis config validate`

Add a new subcommand:

```bash
aegis config validate
```

### Responsibilities

- parse and merge config using the real runtime path
- report hard errors
- report warnings from quality analysis
- exit `0` only when no errors are present
- remain usable in CI

### Output

Support:

- human-readable output by default
- optional `--output json`

Human-readable output should clearly separate:

- `Errors`
- `Warnings`

### JSON output contract

`--output json` should return a stable top-level object:

```json
{
  "valid": false,
  "errors": [
    { "code": "expired_rule", "message": "...", "location": "project:.aegis.toml:allowlist[1]" }
  ],
  "warnings": [
    { "code": "broad_pattern", "message": "...", "location": "project:.aegis.toml:allowlist[0]" }
  ]
}
```

Exact error codes can be finalized during planning, but the shape should remain:

- `valid: bool`
- `errors: []`
- `warnings: []`
- each item includes machine-readable `code`, human-readable `message`, and
  source `location`

## Testing Strategy

## Unit tests

Add focused coverage for:

- new allowlist schema serialization/deserialization
- expired rule rejection
- invalid field rejection
- contextual matching by `cwd`
- contextual matching by `user`
- allowlist override-level behavior matrix
- invariant that `Block` is never bypassed in enforcement modes

## Integration tests

Add or update integration coverage for:

- rejection of legacy allowlist schema
- fail-fast config load on invalid allowlist rules
- full runtime behavior for `Warn/Danger/Block` under each override level
- `aegis config validate` success with warnings
- `aegis config validate` failure with errors
- CI behavior in `Protect`
- unchanged non-blocking behavior in `Audit`

## Documentation updates

Update:

- README config examples
- `config init` template comments
- any user-facing mode / allowlist references

Documentation must explicitly state:

- legacy allowlist format was removed
- `Block` is never bypassed in `Protect` and `Strict`; `Audit` records but does not enforce
- warnings are advisory, errors are fatal
- `allowlist_override_level` controls how much power scoped exceptions have

## Parallel Execution Strategy

P2 is executable in a single phase, but not as a single unconstrained task. The
recommended implementation order is wave-based:

### Wave 1 — foundation

- config schema owner: `AllowlistRule`, `AllowlistOverrideLevel`, parsing, merge/load contract
- allowlist engine owner: rule compilation, scope matching, warning analysis

### Wave 2 — behavior and surface

- policy owner: decision semantics across `Protect`, `Strict`, `Audit`
- CLI owner: `aegis config validate`, human-readable and JSON output

### Wave 3 — hardening

- integration tests
- documentation updates
- reviewer pass
- security review

This structure enables parallelism without having multiple agents fighting over
`model.rs`, `decision.rs`, and CLI wiring at the same time.

## Acceptance Criteria Mapping

### Ticket 2.2

- invalid allowlist entries are no longer ignored
- runtime config load fails on invalid allowlist rules
- tests cover fail-fast behavior

### Ticket 2.3

- only structured `[[allowlist]]` is accepted
- matching uses command context, not just raw string equality
- schema round-trips through serde
- scope matching is tested

### Ticket 2.4

- `allowlist_override_level` affects runtime decisions
- `Warn`, `Danger`, and `Never` are covered by tests
- `Block` remains non-bypassable in enforcement modes

### Ticket 2.5

- `aegis config validate` exists
- errors and warnings are reported separately
- command is usable in CI
- optional JSON output exists
- integration tests cover both success and failure cases

## Risks and Mitigations

- **Risk:** policy logic spreads back into `main.rs`.  
  **Mitigation:** keep decision computation centralized in `src/decision.rs`.

- **Risk:** config validation and runtime behavior diverge.  
  **Mitigation:** shared validation engine for both runtime and CLI validation.

- **Risk:** broad allowlist rules quietly weaken safety.  
  **Mitigation:** runtime fail-fast for hard invalidity, CLI warnings for risky breadth.

- **Risk:** parallel implementation introduces merge conflicts in security-sensitive files.  
  **Mitigation:** use wave-based ownership boundaries instead of parallel edits to the same core files.

## Definition of Done

P2 is complete when all of the following are true:

1. legacy string allowlist format is removed
2. structured allowlist rules are the only supported exception model
3. invalid/expired/conflicting rules fail fast at runtime
4. `allowlist_override_level` governs `Warn`/`Danger` override behavior exactly as designed
5. `Block` remains non-bypassable in enforcement modes and across all CI states
6. `aegis config validate` reports errors and warnings correctly
7. runtime, validation, tests, and docs all describe the same policy contract
