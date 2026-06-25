# ADR-013: Project config uses a security ratchet

## Status

Accepted

## Context

Aegis loads built-in defaults, then global user config, then project-local
`.aegis.toml`. The project layer is untrusted input when an AI agent enters a
repository. Pure last-layer-wins semantics allowed a repository to set
`mode = "Audit"`, `allowlist_override_level = "Danger"`, and
`snapshot_policy = "None"`, weakening Aegis to audit-only behavior for
non-`Block` commands. An initial ratchet covered only a few scalar fields, but
sibling fields stayed last-wins: a project could disable `sandbox.enabled`, set
`auto_snapshot_* = false`, enable `sandbox.allow_network`, or expand
`sandbox.allow_write`, each silently defeating a stricter global base.

## Decision

Project-local config may only tighten security-critical fields, never loosen
them. The ratcheted set is: `mode`, `allowlist_override_level`, `ci_policy`,
`snapshot_policy`, `sandbox.enabled`, `sandbox.required`,
`sandbox.allow_network`, `sandbox.allow_write`, the `auto_snapshot_*` flags
(`git`, `docker`, `postgres`, `mysql`, `supabase`, `sqlite`), the provider
target config (`sqlite_snapshot_path`, `postgres_snapshot`/`mysql_snapshot`/
`supabase_snapshot` `database`, `docker_scope`), `audit.integrity_mode`, and
project-layer `[[rules]]` `decision = "Allow"`. Global config remains the
user's trusted policy layer. When a project config attempts to weaken one of
these fields, Aegis keeps the more restrictive value and
`aegis config validate` reports a warning.

Directionality is field-specific. For booleans where `true` is the stricter
value (`sandbox.enabled`, `sandbox.required`, all `auto_snapshot_*`), the
Project layer keeps `base || requested` (the stricter of base/requested wins).
For `sandbox.allow_network`, where `true` is the weaker value (it grants
network access), the Project layer keeps `base && requested`. For
`sandbox.allow_write` (a `Vec<PathBuf>` where more entries is weaker), the
Project layer keeps the trusted base set and ignores the project value
entirely. Global always stays last-layer-wins for every field.

`auto_snapshot_*` and `sandbox.enabled` close the bypass where a project could
otherwise disable snapshots or the sandbox despite a stricter `snapshot_policy`
or `sandbox.required` inherited from defaults or global config.

## Consequences

Repository-local config can still add patterns, scoped `[[allow]]`/`[[block]]`
rules (capped by `allowlist_override_level`), and tighter project policy. It
can no longer silently disable prompts, snapshots, CI blocking, the sandbox
itself, required sandbox behavior, or audit integrity inherited from defaults
or global config. Users who intentionally want a weaker posture (e.g. opting
out of git snapshots, granting network access, or disabling the audit
integrity chain) must set it in their global config rather than letting a
repository impose it. The merge path and the warning collector share the same
ratchet helpers, so the reported `kept` value always matches the effective
merged value.

Two ratchets deserve explicit call-out, because each closes a bypass with the
same shape as the original C3 attack — a repository silently auto-approving or
de-safeguarding a `Warn`/`Danger` command:

- **Project-layer `[[rules]]` `decision = "Allow"`** is dropped (and warned),
  not honored. A `[[rules]]` `Allow` auto-approves a command *before* `Mode`
  and with *no* `allowlist_override_level` ceiling (unlike an `[[allow]]`
  entry, which is capped). Without this ratchet, a repository could add a
  `[[rules]]` entry matching e.g. `git reset --hard` with `decision = "Allow"`
  and auto-approve a `Danger` command with no prompt, defeating the ratchet on
  `allowlist_override_level`. Project `[[rules]]` may still tighten
  (`Prompt`/`Block`); only `Allow` is untrusted. A project that needs an
  auto-approve must declare the rule in global config.
- **`audit.integrity_mode`** is ratcheted so a project cannot weaken it to
  `Off`. (The chain is an integrity/corruption check, not adversarial
  tamper-evidence — see TASKS.md H5 — but silently disabling even that from an
  untrusted repo is the same weakening shape and is closed here.)