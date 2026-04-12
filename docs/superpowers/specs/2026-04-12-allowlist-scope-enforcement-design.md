# Scoped Allowlist Enforcement Design

**Date:** 2026-04-12
**Status:** Proposed / approved in chat, pending written-spec review

## Objective

Close the main production-readiness gap in Aegis policy handling by making
runtime-effective allowlist rules require explicit scope.

In practice:

- any allowlist rule that Aegis could use at runtime to influence execution
  must include at least one scope field: `cwd` or `user`
- runtime loading and validation must fail closed on unscoped rules
- legacy string-array allowlist syntax remains readable for migration, but is
  no longer valid for runtime execution
- `config show` must still work on legacy or otherwise runtime-invalid configs
  through an inspection-only load path

## Problem Statement

The current structured allowlist schema is better than the legacy string-array
form, but it still permits rules with no scope:

```toml
[[allowlist]]
pattern = "terraform destroy *"
reason = "too broad"
```

Today, such a rule is only warned about. That is insufficient for production
readiness because it allows broad policy exceptions without an explicit scope
boundary.

This is especially important because Aegis allowlist matching is used to drive
runtime auto-approval decisions for `Warn` and `Danger` commands. In the
current product semantics, a runtime-effective allowlist rule is effectively
any allowlist rule that matches and is not expired. Therefore the simplest and
safest invariant is:

> Any runtime-effective `[[allowlist]]` rule must include `cwd` or `user`.

## Non-Goals

This change does **not**:

- change `Block` semantics
- change `allowlist_override_level` semantics
- make `broad_pattern` a hard error
- remove legacy parsing entirely
- add a new policy DSL
- change snapshot best-effort semantics

## Design Decisions

### 1. Missing scope becomes a hard invariant

An allowlist rule is invalid for runtime use if both of these are absent:

- `cwd`
- `user`

Examples:

**Valid**

```toml
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "/srv/infra"
reason = "ephemeral test teardown"
```

```toml
[[allowlist]]
pattern = "docker system prune --volumes"
user = "ci"
reason = "trusted cleanup account"
```

```toml
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "/srv/infra"
user = "ci"
reason = "fully scoped teardown"
```

**Invalid**

```toml
[[allowlist]]
pattern = "terraform destroy *"
reason = "broad and unscoped"
```

### 2. Enforcement happens in both semantic validation and compile/runtime paths

To avoid bypasses, the invariant must be enforced in two places:

#### Semantic validation

Validation surfaces must report a hard error for unscoped allowlist rules:

- runtime config loading path
- `aegis config validate`

This ensures normal product flows fail closed before execution.

#### Compile/runtime path

Allowlist compilation itself must reject unscoped rules.

This ensures direct or accidental bypass paths cannot construct a working
runtime allowlist matcher from invalid config data.

### 3. `broad_pattern` remains a warning

Wildcard breadth is still an important operator signal, but it is not as clear
or objective as missing scope. Therefore:

- `missing_scope` becomes a hard error
- `broad_pattern` remains a warning

This preserves a strong invariant without overfitting policy to pattern style.

### 4. Legacy string-array allowlist remains parseable, but not executable

Legacy syntax:

```toml
allowlist = ["terraform destroy *"]
```

continues to parse and normalize into structured rules:

```toml
[[allowlist]]
pattern = "terraform destroy *"
reason = "migrated from legacy allowlist entry"
```

However, because migrated legacy rules have no `cwd` or `user`, they are
runtime-invalid under the new invariant.

This creates a deliberate policy:

> Parse legacy, but never execute it.

That means:

- inspection flows can still read and normalize old configs
- runtime execution fails closed
- `config validate` clearly tells the operator what to fix

### 5. `config show` gets an inspection-only load path

If `config show` continues to use the same runtime-validated load path as
command execution, then legacy allowlist syntax and unscoped rules would make
`config show` unusable exactly when the operator needs it to repair config.

To preserve recovery UX, `config show` must use an inspection-only config load
path that:

- parses layered config
- performs normalization (including legacy allowlist migration)
- skips runtime requirement enforcement

It must **not** silently claim the config is valid. It only exists to let the
user inspect the effective normalized config.

### 6. `config show` output remains normalized and honest

`config show` should print legacy allowlist entries in structured form, still
without `cwd` / `user`, so the operator can see the exact rule that must be
fixed.

Expected normalized inspection output:

```toml
config_version = 1

[[allowlist]]
pattern = "terraform destroy *"
reason = "migrated from legacy allowlist entry"
```

No fake scope should be invented, and runtime validity should not be implied.

Optional future enhancement:

- emit a human-facing note in `config show` or docs explaining that these rules
  are inspection-visible but runtime-invalid until scope is added

This enhancement is not required for P0.

## Affected Code Surfaces

### Core behavior

- `src/config/model.rs`
  - config loading paths
  - runtime validation
  - inspection-only load path for `config show`
  - init template expectations
- `src/config/allowlist.rs`
  - allowlist rule validation and compilation
- `src/config/validate.rs`
  - structured validation reporting
- `src/main.rs`
  - `config show` should use inspection loading, not runtime loading

### Runtime and tests

- `src/runtime.rs`
  - unit tests covering runtime-context construction rejection
- `tests/full_pipeline.rs`
  - runtime hard-fail cases
  - `config validate` error reporting
  - `config show` repair flow
- `tests/config_integration.rs`
  - invalid unscoped allowlist cases where relevant

### Docs

- `README.md`
- `docs/config-schema.md`

## Required Behavioral Outcomes

### Runtime

The following must fail closed:

- `.aegis.toml` with unscoped structured allowlist rule
- global config with unscoped structured allowlist rule
- legacy `allowlist = ["..."]` used in runtime execution path

Expected outcome:

- config load error
- no command execution
- exit code `4`
- clear offending file path when available

### `aegis config validate`

Unscoped rules must appear as hard errors, not warnings.

Expected:

- `valid = false`
- error code dedicated to missing scope, or equivalent hard validation code
- offending entry location reported precisely

### `aegis config show`

Must continue to work for:

- legacy allowlist syntax
- runtime-invalid unscoped structured rules

Expected:

- successful output
- normalized structured allowlist representation
- no invented scope fields

## Testing Strategy

### Unit tests

Add or update unit coverage for:

- allowlist compiler rejects unscoped rules
- model/runtime validation rejects unscoped rules
- inspection load path still succeeds on legacy allowlist syntax

### Integration / end-to-end tests

Add or update tests for:

- shell-wrapper execution failing on unscoped structured allowlist config
- shell-wrapper execution failing on legacy string-array allowlist config
- `config validate --output json` reporting unscoped rule as hard error
- `config show` succeeding on legacy string-array allowlist and printing
  normalized structured output

### Regression invariants

Existing guarantees must remain true:

- `Block` is never bypassable
- `allowlist_override_level` matrix remains unchanged
- snapshot policy behavior remains unchanged
- audit logging behavior remains unchanged

## Risks

### 1. Hidden coupling between `config show` and runtime loading

If `config show` is not separated from runtime validation, repair UX regresses.

Mitigation:

- add a dedicated inspection-only load path
- add explicit tests for legacy + invalid allowlist inspection

### 2. Validation/compile path mismatch

If semantic validation rejects unscoped rules but `Allowlist::new()` still
accepts them, a bypass remains possible.

Mitigation:

- enforce the invariant in both `validate` and `compile_rule`

### 3. Over-broad doc claims

Docs may accidentally imply that legacy syntax still works at runtime.

Mitigation:

- explicitly document “readable for migration, invalid for runtime”

## Acceptance Criteria

P0 is complete when all of the following are true:

1. unscoped allowlist rules are hard errors for runtime and validation
2. allowlist compilation itself rejects unscoped rules
3. legacy string-array allowlist remains parseable for inspection
4. `config show` works on legacy/unscoped configs via inspection path
5. `broad_pattern` remains advisory-only
6. tests cover runtime failure, validation failure, and inspection success
7. docs explain the new invariant and migration behavior clearly
