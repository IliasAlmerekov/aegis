# Scoped Allowlist Enforcement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make any runtime-effective allowlist rule require explicit scope (`cwd` or `user`), while preserving a repair-friendly inspection path for legacy and otherwise runtime-invalid configs.

**Architecture:** Keep the existing split between config parsing, validation, allowlist compilation, and CLI wiring. Add one shared “missing scope” invariant enforced both semantically and during allowlist compilation, and add a separate inspection-only config load path so `aegis config show` can still normalize legacy configs without making them runtime-valid.

**Tech Stack:** Rust 2024, serde/toml config model, existing allowlist compiler in `src/config/allowlist.rs`, CLI in `src/main.rs`, end-to-end tests in `tests/full_pipeline.rs`

---

## File Structure

- `src/config/allowlist.rs`
  - Own the hard compile-time invariant that unscoped allowlist rules are invalid.
- `src/config/model.rs`
  - Own runtime validation and expose an inspection-only load path for `config show`.
- `src/config/validate.rs`
  - Surface unscoped rules as hard validation errors, while keeping `broad_pattern` as a warning.
- `src/main.rs`
  - Route `config show` through inspection loading instead of runtime loading.
- `src/runtime.rs`
  - Keep unit tests proving runtime context construction rejects unscoped rules.
- `tests/full_pipeline.rs`
  - Own end-to-end behavior for runtime fail-closed, `config validate`, and `config show`.
- `README.md`
  - Document that allowlist rules must be scoped for runtime use.
- `docs/config-schema.md`
  - Document readable-but-runtime-invalid legacy allowlist migration behavior.

---

## Milestones

1. Enforce missing-scope invariant in allowlist compilation.
2. Enforce the same invariant in config validation and runtime loading.
3. Preserve `config show` via inspection-only loading.
4. Update end-to-end tests and docs.

---

## Task Graph

- Task 1 (`src/config/allowlist.rs`) must land before Task 2 because config validation should reuse the same invariant language.
- Task 2 (`src/config/model.rs`, `src/config/validate.rs`, `src/runtime.rs`) must land before Task 3 because `config show` needs a deliberate inspection-only exception.
- Task 3 (`src/main.rs`, `tests/full_pipeline.rs`) depends on Task 2.
- Task 4 (docs) depends on the final behavior from Tasks 1–3.

---

## Task Details

### Task 1: Reject unscoped allowlist rules in the compiler

**Files:**
- Modify: `src/config/allowlist.rs`

- [ ] **Step 1: Add failing unit tests for unscoped rule rejection and broad-pattern warning preservation**

Add these tests near the existing `warning_flags_broad_rule_without_scope` test in `src/config/allowlist.rs`:

```rust
    #[test]
    fn unscoped_rule_is_rejected_by_allowlist_compilation() {
        let err = Allowlist::new(&[AllowlistRule {
            pattern: "terraform destroy *".to_string(),
            cwd: None,
            user: None,
            expires_at: None,
            reason: "too broad".to_string(),
        }])
        .expect_err("unscoped allowlist rule must be rejected");

        assert!(err.to_string().contains("must declare cwd or user scope"));
    }

    #[test]
    fn cwd_scoped_rule_still_compiles() {
        let allowlist = Allowlist::new(&[AllowlistRule {
            pattern: "terraform destroy -target=module.test.*".to_string(),
            cwd: Some("/srv/infra".to_string()),
            user: None,
            expires_at: None,
            reason: "scoped teardown".to_string(),
        }]);

        assert!(allowlist.is_ok());
    }

    #[test]
    fn broad_pattern_warning_still_exists_for_scoped_rule() {
        let warnings = analyze_allowlist_rule(&AllowlistRule {
            pattern: "terraform destroy *".to_string(),
            cwd: Some("/srv/infra".to_string()),
            user: None,
            expires_at: None,
            reason: "scoped but broad".to_string(),
        });

        assert!(!warnings.iter().any(|w| w.code == "missing_scope"));
        assert!(warnings.iter().any(|w| w.code == "broad_pattern"));
    }
```

- [ ] **Step 2: Run the focused allowlist tests to verify RED**

Run:

```bash
rtk cargo test unscoped_rule_is_rejected_by_allowlist_compilation cwd_scoped_rule_still_compiles broad_pattern_warning_still_exists_for_scoped_rule --lib
```

Expected: the new rejection test fails because unscoped rules still compile today.

- [ ] **Step 3: Implement the missing-scope compile guard**

In `src/config/allowlist.rs`, add a helper near `has_scope(...)`:

```rust
fn validate_scope(rule: &AllowlistRule) -> Result<()> {
    if has_scope(rule.cwd.as_deref()) || has_scope(rule.user.as_deref()) {
        Ok(())
    } else {
        Err(AegisError::Config(
            "allowlist rule must declare cwd or user scope".to_string(),
        ))
    }
}
```

Then call it at the top of `compile_rule(...)` after `cwd` / `user` have been normalized:

```rust
    validate_scope(&rule.rule)?;
```

If the borrow checker complains because `rule.rule` is partially moved, refactor `compile_rule(...)` to destructure fields into locals before constructing `CompiledAllowlistRule`.

- [ ] **Step 4: Re-run the focused allowlist tests to verify GREEN**

Run:

```bash
rtk cargo test unscoped_rule_is_rejected_by_allowlist_compilation cwd_scoped_rule_still_compiles broad_pattern_warning_still_exists_for_scoped_rule --lib
```

Expected: all three tests pass.

- [ ] **Step 5: Commit**

```bash
rtk git add src/config/allowlist.rs
rtk git commit -m "fix: reject unscoped allowlist rules"
```

### Task 2: Make validation and runtime loading fail closed on unscoped rules

**Files:**
- Modify: `src/config/model.rs`
- Modify: `src/config/validate.rs`
- Modify: `src/runtime.rs`

- [ ] **Step 1: Add failing runtime/model tests for unscoped rule rejection**

In `src/config/model.rs`, add tests near the malformed allowlist/runtime validation tests:

```rust
    #[test]
    fn unscoped_allowlist_rule_is_invalid_for_runtime_validation() {
        let config = AegisConfig {
            allowlist: vec![AllowlistRule {
                pattern: "terraform destroy *".to_string(),
                cwd: None,
                user: None,
                expires_at: None,
                reason: "legacy broad rule".to_string(),
            }],
            ..AegisConfig::defaults()
        };

        let err = config.validate_runtime_requirements().unwrap_err();
        assert!(err.to_string().contains("must declare cwd or user scope"));
    }

    #[test]
    fn legacy_allowlist_remains_parseable_but_fails_runtime_requirements() {
        let config: AegisConfig =
            toml::from_str(r#"allowlist = ["terraform destroy *"]"#).unwrap();

        let err = config.validate_runtime_requirements().unwrap_err();
        assert!(err.to_string().contains("must declare cwd or user scope"));
    }
```

In `src/runtime.rs`, add:

```rust
    #[test]
    fn runtime_context_rejects_unscoped_allowlist_rules() {
        use crate::config::AllowlistRule;

        let mut config = Config::default();
        config.allowlist = vec![AllowlistRule {
            pattern: "terraform destroy *".to_string(),
            cwd: None,
            user: None,
            expires_at: None,
            reason: "too broad".to_string(),
        }];

        let err = RuntimeContext::new(config, test_handle())
            .expect_err("runtime context must reject unscoped allowlist rules");
        assert!(err.to_string().contains("must declare cwd or user scope"));
    }
```

- [ ] **Step 2: Add failing `config validate` tests for hard-error behavior**

In `src/config/validate.rs`, add:

```rust
    #[test]
    fn validate_reports_error_for_unscoped_rule() {
        let config = Config {
            allowlist: vec![AllowlistRule {
                pattern: "terraform destroy *".to_string(),
                cwd: None,
                user: None,
                expires_at: None,
                reason: "too broad".to_string(),
            }],
            ..Config::defaults()
        };

        let report = validate_config(&config, &ConfigSourceMap::for_config(&config));
        assert!(report.errors.iter().any(|e| e.code == "missing_scope"));
        assert!(report.warnings.iter().any(|w| w.code == "broad_pattern"));
    }
```

- [ ] **Step 3: Run the focused validation/runtime tests to verify RED**

Run:

```bash
rtk cargo test unscoped_allowlist_rule_is_invalid_for_runtime_validation legacy_allowlist_remains_parseable_but_fails_runtime_requirements runtime_context_rejects_unscoped_allowlist_rules validate_reports_error_for_unscoped_rule --lib
```

Expected: at least the validation test fails because `missing_scope` is still only a warning.

- [ ] **Step 4: Promote missing scope to a hard validation error**

In `src/config/validate.rs`, inside the allowlist loop in `validate_config(...)`, add an explicit error before warning processing:

```rust
        if rule.cwd.as_deref().is_none_or(|value| value.trim().is_empty())
            && rule.user.as_deref().is_none_or(|value| value.trim().is_empty())
        {
            errors.push(ValidationIssue {
                code: "missing_scope",
                message: "allowlist rule must declare cwd or user scope".to_string(),
                location: location.clone(),
            });
        }
```

Keep the existing `analyze_allowlist_rule(rule)` loop so `broad_pattern` still appears as a warning.

No separate production-code change should be needed in `src/config/model.rs` beyond relying on `validate_runtime_requirements()` continuing to call `Allowlist::new(...)`, unless test output requires a clearer message wrapper.

- [ ] **Step 5: Re-run the focused validation/runtime tests to verify GREEN**

Run:

```bash
rtk cargo test unscoped_allowlist_rule_is_invalid_for_runtime_validation legacy_allowlist_remains_parseable_but_fails_runtime_requirements runtime_context_rejects_unscoped_allowlist_rules validate_reports_error_for_unscoped_rule --lib
```

Expected: all four tests pass.

- [ ] **Step 6: Commit**

```bash
rtk git add src/config/model.rs src/config/validate.rs src/runtime.rs
rtk git commit -m "fix: fail closed on unscoped allowlist rules"
```

### Task 3: Preserve repair UX with inspection-only `config show`

**Files:**
- Modify: `src/config/model.rs`
- Modify: `src/main.rs`
- Modify: `tests/full_pipeline.rs`

- [ ] **Step 1: Add failing end-to-end tests for runtime failure + `config show` success**

In `tests/full_pipeline.rs`, add:

```rust
#[test]
fn unscoped_structured_allowlist_fails_runtime_execution() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
[[allowlist]]
pattern = "terraform destroy *"
reason = "too broad"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", "printf should-not-run"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("must declare cwd or user scope")
    );
}

#[test]
fn config_validate_reports_missing_scope_as_error_for_legacy_allowlist() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"allowlist = ["terraform destroy *"]"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["code"] == "missing_scope"));
}

#[test]
fn config_show_uses_inspection_path_for_legacy_allowlist() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"allowlist = ["terraform destroy *"]"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "show"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("[[allowlist]]"));
    assert!(stdout.contains("pattern = \"terraform destroy *\""));
    assert!(stdout.contains("reason = \"migrated from legacy allowlist entry\""));
}
```

- [ ] **Step 2: Run the focused end-to-end tests to verify RED**

Run:

```bash
rtk cargo test --test full_pipeline unscoped_structured_allowlist_fails_runtime_execution config_validate_reports_missing_scope_as_error_for_legacy_allowlist config_show_uses_inspection_path_for_legacy_allowlist
```

Expected: `config show` currently fails because it uses runtime loading, or the runtime failure case does not yet fail with the new invariant.

- [ ] **Step 3: Add inspection-only config loading**

In `src/config/model.rs`, add:

```rust
    pub fn load_for_inspection(current_dir: &Path, home_dir: Option<&Path>) -> Result<Self> {
        Self::load_for_internal(current_dir, home_dir, false)
    }
```

If you want a convenience wrapper mirroring `load()`, also add:

```rust
    pub fn load_inspection() -> Result<Self> {
        let current_dir = env::current_dir()?;
        let home_dir = env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);

        Self::load_for_inspection(&current_dir, home_dir.as_deref())
    }
```

Then update the `ConfigCommand::Show` branch in `src/main.rs` to call the inspection loader instead of `Config::load()`.

- [ ] **Step 4: Re-run the focused end-to-end tests to verify GREEN**

Run:

```bash
rtk cargo test --test full_pipeline unscoped_structured_allowlist_fails_runtime_execution config_validate_reports_missing_scope_as_error_for_legacy_allowlist config_show_uses_inspection_path_for_legacy_allowlist
```

Expected: all three tests pass.

- [ ] **Step 5: Commit**

```bash
rtk git add src/config/model.rs src/main.rs tests/full_pipeline.rs
rtk git commit -m "feat: keep config show usable for legacy allowlist repair"
```

### Task 4: Update docs and starter config text

**Files:**
- Modify: `src/config/model.rs`
- Modify: `README.md`
- Modify: `docs/config-schema.md`

- [ ] **Step 1: Add failing documentation expectations test**

In `tests/config_schema_docs.rs`, extend the schema-doc expectations:

```rust
        "cwd or user scope",
        "readable for migration, invalid for runtime",
```
```

And in `tests/full_pipeline.rs`, extend the init-template assertions in `config_init_writes_versioned_template_with_structured_allowlist_docs`:

```rust
    assert!(contents.contains("allowlist rule must declare cwd or user scope"));
```

Use the exact wording you intend to ship in the template/docs.

- [ ] **Step 2: Run the focused docs tests to verify RED**

Run:

```bash
rtk cargo test --test full_pipeline config_init_writes_versioned_template_with_structured_allowlist_docs
rtk cargo test --test config_schema_docs
```

Expected: at least one assertion fails because the docs/template do not yet mention the new invariant.

- [ ] **Step 3: Update template and docs**

Update `src/config/model.rs` init template comments so the allowlist example and explanatory text state that runtime-effective allowlist rules must declare `cwd` or `user`.

Update `README.md`:

- state that allowlist rules must be scoped for runtime use
- state that legacy string-array allowlist is normalized for inspection but invalid for runtime until scope is added

Update `docs/config-schema.md`:

- explicitly document “readable for migration, invalid for runtime”
- explain that migrated legacy entries must be repaired by adding `cwd` and/or `user`

- [ ] **Step 4: Re-run the focused docs tests to verify GREEN**

Run:

```bash
rtk cargo test --test full_pipeline config_init_writes_versioned_template_with_structured_allowlist_docs
rtk cargo test --test config_schema_docs
```

Expected: both test targets pass.

- [ ] **Step 5: Commit**

```bash
rtk git add src/config/model.rs README.md docs/config-schema.md tests/config_schema_docs.rs tests/full_pipeline.rs
rtk git commit -m "docs: require scoped allowlist rules"
```

---

## Verification Plan

After all tasks complete, run:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
```

If the touched tests are too slow during iteration, use the focused commands from the tasks first, then run the full suite once at the end.

Optional but recommended if the diff touches snapshot/config interactions more than expected:

```bash
rtk cargo test --test config_integration
rtk cargo test --test cli_integration
```

---

## Rollback Plan

If the inspection-only load path introduces regressions:

1. revert the `config show` loader change in `src/main.rs`
2. revert the new inspection-load API in `src/config/model.rs`
3. keep the failing tests for missing-scope runtime enforcement staged separately until the inspection path is repaired

If the missing-scope invariant proves too disruptive:

1. revert the compile-time guard in `src/config/allowlist.rs`
2. revert the validation hard error in `src/config/validate.rs`
3. restore prior warning-only behavior

Do **not** relax `Block` semantics, snapshot semantics, or audit semantics as part of rollback.

---

## Confirmation

This plan implements the approved design:

- parse legacy allowlist syntax
- hard-fail runtime and `config validate` on unscoped rules
- keep `config show` usable through an inspection path
- preserve `broad_pattern` as a warning

