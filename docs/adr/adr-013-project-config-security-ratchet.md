# ADR-013: Project config uses a security ratchet

## Status

Accepted

## Context

Aegis loads built-in defaults, then global user config, then project-local
`.aegis.toml`. The project layer is untrusted input when an AI agent enters a
repository. Pure last-layer-wins semantics allowed a repository to set
`mode = "Audit"`, `allowlist_override_level = "Danger"`, and
`snapshot_policy = "None"`, weakening Aegis to audit-only behavior for
non-`Block` commands.

## Decision

Project-local config may only tighten security-critical scalar settings:
`mode`, `allowlist_override_level`, `ci_policy`, `snapshot_policy`, and
`sandbox.required`. Global config remains the user's trusted policy layer.
When a project config attempts to weaken one of these fields, Aegis keeps the
more restrictive value and `aegis config validate` reports a warning.

## Consequences

Repository-local config can still add patterns, scoped allow/block rules, and
tighter project policy. It can no longer silently disable prompts, snapshots,
CI blocking, or required sandbox behavior inherited from defaults or global
config. Users who intentionally want a weaker posture must set it in their
global config rather than letting a repository impose it.