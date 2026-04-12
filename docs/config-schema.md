# Config schema

## Schema evolution

Aegis config now uses an explicit `config_version` field.

- `config_version = 1` is the current schema.
- If `config_version` is omitted, Aegis treats the file as a legacy pre-version config and loads it with backwards-compatibility rules where possible.
- Unknown future versions fail closed instead of being guessed.

This document is the schema evolution policy for Aegis config.

## Current schema version

```toml
config_version = 1
```

New configs created by `aegis config init` and normalized configs printed by `aegis config show` use the current version.

## Migration

### Legacy allowlist format

Older configs may use the legacy string-array form:

```toml
allowlist = ["terraform destroy *"]
```

Aegis migrates that form into the structured schema internally and `config show` prints the normalized form:

```toml
[[allowlist]]
pattern = "terraform destroy *"
reason = "migrated from legacy allowlist entry"
```

That legacy form is readable for migration, invalid for runtime. It remains usable for inspection and repair workflows such as `aegis config show`, but runtime loading and `aegis config validate` require every runtime-effective allowlist rule to declare cwd or user scope.

The required follow-up is to replace the legacy entry with an explicit structured rule, add `cwd` and/or `user`, and keep a real reason. For example:

```toml
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "/srv/infra"
reason = "ephemeral test teardown"
```

### Mode semantics

`mode` remains versioned by schema policy, not by guesswork.

If mode semantics ever change in a future schema version:

- the new behavior must ship under a new `config_version`
- the migration notes must describe the old and new mode semantics explicitly
- `config show` must reflect the effective normalized meaning

Current version `1` keeps the existing `Protect`, `Audit`, and `Strict` semantics.

### Deprecated fields

There are no active deprecated config fields in schema version `1`.

This section defines the deprecated fields policy for future schema versions.

When deprecations are introduced, the migration path must include:

- what field is deprecated
- what replacement field to use
- whether the old field is still auto-migrated or hard-rejected
- in which `config_version` the old field will stop working

## Compatibility policy

- New releases must not break known legacy configs blindly.
- Legacy compatibility may normalize old data into the latest structured representation.
- Unsupported future schemas are rejected explicitly so users are told to upgrade Aegis or downgrade the config.
