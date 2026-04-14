# Config schema

## Schema evolution

Aegis config uses an explicit `config_version` field.

This section is the repository's explicit `schema evolution` and `migration` reference.

- `config_version = 1` is the current schema.
- omitted `config_version` is treated as a legacy pre-version config input
- unsupported future versions are rejected explicitly

This document describes the current runtime contract for schema version `1`.

## Layered merge order

Effective config is merged in this order:

1. built-in defaults
2. global config: `~/.config/aegis/config.toml`
3. project config: `.aegis.toml`

Merge behavior:

- scalar fields: later layers override earlier layers
- vector fields such as `custom_patterns` and `allowlist`: layers are concatenated
- for merged vectors, global entries come first and project entries come after
- for allowlist precedence at runtime, project rules are checked before global rules

## Current schema version

```toml
config_version = 1
```

Current defaults:

```toml
config_version = 1
mode = "Protect"
allowlist_override_level = "Warn"
snapshot_policy = "Selective"
auto_snapshot_git = true
auto_snapshot_docker = false
auto_snapshot_postgres = false
auto_snapshot_mysql = false
auto_snapshot_sqlite = false
sqlite_snapshot_path = ""
ci_policy = "Block"
```

## Mode semantics

Current runtime modes are `Protect`, `Audit`, and `Strict`.

This `mode semantics` section documents the current runtime behavior.

### Protect

- `Safe` auto-approves
- `Warn` prompts unless an allowlist override makes it effective
- `Danger` prompts unless an allowlist override makes it effective
- `Block` always blocks
- when snapshots are requested, that matters only for `Danger`

### Audit

- `Safe`, `Warn`, `Danger`, and `Block` all remain non-blocking at runtime
- Audit mode does not prompt
- Audit mode does not request snapshots

### Strict

- `Safe` auto-approves
- `Warn` and `Danger` block unless an allowlist override makes them effective
- `Block` always blocks
- when an allowlisted `Danger` command is auto-approved, snapshot requirements still apply

### Prompt semantics

- interactive approval accepts only `y` / `yes`
- `Y` and `YES` are accepted after lowercase normalization
- empty input denies
- any other input denies
- read failure denies
- non-interactive prompt-required flows deny
- default is deny

## Allowlist semantics

Allowlist rules use the structured array-of-tables form:

```toml
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "/srv/infra"
user = "ci"
expires_at = "2030-01-01T00:00:00Z"
reason = "ephemeral test teardown"
```

Runtime rules:

- every runtime-effective allowlist rule must declare `cwd or user scope`
- every runtime-effective allowlist rule must declare `cwd` or `user` scope
- `pattern` and `reason` must not be empty
- if present, `cwd` and `user` must not be empty
- expired rules are invalid for runtime use
- patterns are matched against the trimmed command string
- `*` and `?` behave as glob wildcards
- exact rules match only the same command
- scoped rules match only when the current `cwd` and/or `user` also match
- project allowlist rules beat global allowlist rules when both match
- within the same layer, the first declared matching rule wins

Legacy compatibility:

- legacy examples may still appear as `allowlist = ["..."]` during migration discussions
- legacy string-array allowlists remain parseable for migration and inspection
- legacy string-array entries are normalized internally to structured rules with reason `migrated from legacy allowlist entry`
- legacy entries are `readable for migration, invalid for runtime` until they gain `cwd` and/or `user` scope

`allowlist_override_level` controls when allowlist matches change policy outcomes in `Protect` and `Strict`:

- `Warn`: allowlisted `Warn` commands may auto-approve
- `Danger`: allowlisted `Warn` and `Danger` commands may auto-approve
- `Never`: non-safe allowlist auto-approval is disabled
- `Block` never bypasses in `Protect` or `Strict`

## Snapshot policy

Snapshot requests matter only for `Danger` flows.

- `None` never requests snapshots
- `Selective` honors `auto_snapshot_git` / `auto_snapshot_docker`
- `Selective` also honors `auto_snapshot_postgres` / `auto_snapshot_mysql` / `auto_snapshot_sqlite`
- `Full` requests all applicable snapshot plugins regardless of per-plugin flags

Important details:

- if there are no applicable snapshot plugins, no snapshots are requested even for `Danger`
- `Audit` does not request snapshots
- `Warn` does not request snapshots

Example:

```toml
snapshot_policy = "Selective"
auto_snapshot_git = true
auto_snapshot_docker = false

[docker_scope]
mode = "Labeled"
label = "aegis.snapshot"
name_patterns = []
```

## Database snapshot options

The per-plugin enable booleans are only honored when `snapshot_policy = "Selective"`.
The connection and path settings are still consumed by the snapshot providers in
`Selective` and `Full` mode, so keep them accurate whenever those providers may run.

### PostgreSQL snapshots

```toml
auto_snapshot_postgres = false

[postgres_snapshot]
database = ""
host = "localhost"
port = 5432
user = ""
```

- `auto_snapshot_postgres` enables PostgreSQL snapshots before dangerous commands
- `postgres_snapshot.database` is required when PostgreSQL snapshots are enabled
- `postgres_snapshot.host` and `postgres_snapshot.port` select the database endpoint
- `postgres_snapshot.user` may be left empty to use `PGUSER` or the current OS user
- credentials must come from `PGPASSWORD` or `~/.pgpass`; never store passwords in config

### MySQL/MariaDB snapshots

```toml
auto_snapshot_mysql = false

[mysql_snapshot]
database = ""
host = "localhost"
port = 3306
user = ""
```

- `auto_snapshot_mysql` enables MySQL/MariaDB snapshots before dangerous commands
- `mysql_snapshot.database` is required when MySQL/MariaDB snapshots are enabled
- `mysql_snapshot.host` and `mysql_snapshot.port` select the database endpoint
- `mysql_snapshot.user` may be left empty to use `MYSQL_USER` or `~/.my.cnf`
- credentials must come from `MYSQL_PWD` or `~/.my.cnf`; never store passwords in config

### SQLite snapshots

```toml
auto_snapshot_sqlite = false
sqlite_snapshot_path = ""
```

- `auto_snapshot_sqlite` enables SQLite snapshots before dangerous commands
- `sqlite_snapshot_path` must point to the `.db` file, either relative to the current working directory or absolute
- SQLite snapshots do not use a username/password block; the database file path is the only required setting

## CI policy

`ci_policy` is a runtime policy input, not the GitHub Actions workflow definition.

Supported values:

- `Block`
- `Allow`

Current runtime behavior:

- in `Protect`, `ci_policy = Block` blocks non-safe commands instead of prompting
- in `Protect`, `ci_policy = Allow` does not short-circuit the normal policy flow
- with `ci_policy = Allow`, non-safe commands still follow the usual prompt path, so non-interactive confirmation surfaces can still deny
- `Strict` is not weakened by CI
- `Audit` remains non-blocking
- `Block` risk remains blocked regardless of CI policy

## JSON output contract

`aegis --output json` currently emits schema version `1`.

Top-level fields:

- `schema_version`
- `command`
- `risk`
- `decision`
- `exit_code`
- `mode`
- `ci_state`
- `matched_patterns`
- `allowlist_match`
- `snapshots_created`
- `snapshot_plan`
- `execution`
- optional `block_reason`
- `decision_source`

Current decision labels:

- `auto_approve`
- `prompt`
- `block`

Current execution contract:

- `execution.mode` is `evaluation_only`
- `execution.will_execute` is `false`

Example:

```json
{
  "schema_version": 1,
  "command": "rm -rf /tmp",
  "risk": "danger",
  "decision": "prompt",
  "exit_code": 2,
  "mode": "protect",
  "ci_state": { "detected": false, "policy": "block" },
  "matched_patterns": [],
  "allowlist_match": { "matched": false, "effective": false },
  "snapshots_created": [],
  "snapshot_plan": { "requested": true, "applicable_plugins": [] },
  "execution": { "mode": "evaluation_only", "will_execute": false },
  "decision_source": "builtin_pattern"
}
```

Notes:

- `matched_patterns[*]` includes pattern metadata, matched text, and optional `safe_alternative`
- `allowlist_match.pattern` and `allowlist_match.reason` are optional
- `block_reason` is optional and uses values such as `intrinsic_risk_block`, `strict_policy`, and `protect_ci_policy`
- `decision_source` is one of `builtin_pattern`, `custom_pattern`, or `fallback`

## Compatibility policy

- new releases should preserve compatibility for schema version `1`
- known legacy configs may be normalized into the current structured form
- unsupported future schema versions are rejected explicitly
- docs must follow current runtime behavior instead of guessing future semantics

## Deprecated fields

There are no active deprecated fields in schema version `1`.

If future config changes deprecate fields, the migration story must explain:

- what changed
- what replaces it
- whether old inputs are auto-migrated or rejected
- which schema evolution boundary introduced the deprecation
