# P2 Policy Correctness & Safety Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the legacy string allowlist with a strict structured policy model, add bounded override semantics, and ship a shared runtime/CLI validation path for P2.

**Architecture:** Keep the existing split between config parsing, allowlist matching, decision policy, and runtime orchestration. Introduce structured allowlist types in `src/config/model.rs`, a contextual matcher/validator in `src/config/allowlist.rs`, a richer decision input in `src/decision.rs`, and a shared validation surface consumed by both runtime config loading and `aegis config validate`.

**Tech Stack:** Rust 2024, `serde`, `toml`, `regex`, existing `clap` CLI, existing integration test harness in `tests/full_pipeline.rs`.

---

## File Structure

### Existing files to modify

- `src/config/model.rs` — replace legacy `Vec<String>` allowlist config with structured rules and add `AllowlistOverrideLevel`.
- `src/config/allowlist.rs` — compile and match structured rules, add context-aware matching, and expose validation warnings.
- `src/config/mod.rs` — re-export new config and validation types.
- `src/runtime.rs` — construct the new allowlist engine and pass real runtime context into matching/audit helpers.
- `src/decision.rs` — swap `strict_allowlist_override` for `allowlist_override_level` and implement the new policy matrix.
- `src/main.rs` — add `config validate`, update `config init/show`, and pass richer policy inputs through shell-wrapper execution.
- `src/watch.rs` — mirror runtime policy/input changes in watch mode.
- `tests/full_pipeline.rs` — add end-to-end coverage for new config schema, runtime policy, and validate CLI.
- `README.md` — update config examples and policy semantics.

### New files to create

- `src/config/validate.rs` — shared hard-error and warning reporting logic used by config loading and `config validate`.

### Boundaries to preserve

- Keep `src/main.rs` thin: argument parsing, command routing, output formatting only.
- Keep `src/interceptor/` untouched and synchronous.
- Do not weaken `Block` behavior in `Protect`/`Strict`.
- Do not introduce raw-string allowlist compatibility logic.

---

### Task 1: Replace config schema with structured allowlist rules

**Files:**
- Modify: `src/config/model.rs`
- Modify: `src/config/mod.rs`
- Test: `src/config/model.rs`

- [ ] **Step 1: Write failing config-model tests for the new schema**

Add/replace unit tests in `src/config/model.rs` for:

```rust
#[test]
fn structured_allowlist_rule_deserializes() {
    let config: AegisConfig = toml::from_str(
        r#"
allowlist_override_level = "Warn"

[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "/srv/infra"
user = "ci"
expires_at = "2026-01-01T00:00:00Z"
reason = "ephemeral test teardown"
"#,
    )
    .unwrap();

    assert_eq!(config.allowlist.len(), 1);
    assert_eq!(config.allowlist[0].pattern, "terraform destroy -target=module.test.*");
    assert_eq!(config.allowlist_override_level, AllowlistOverrideLevel::Warn);
}

#[test]
fn legacy_string_allowlist_is_rejected() {
    let err = toml::from_str::<AegisConfig>(r#"allowlist = ["terraform destroy *"]"#).unwrap_err();
    assert!(err.to_string().contains("invalid type"));
}

#[test]
fn expired_rule_is_invalid_for_runtime() {
    let config = AegisConfig {
        allowlist: vec![AllowlistRule {
            pattern: "terraform destroy -target=module.test.*".to_string(),
            cwd: Some("/srv/infra".to_string()),
            user: None,
            expires_at: Some("2020-01-01T00:00:00Z".parse().unwrap()),
            reason: "expired teardown".to_string(),
        }],
        ..AegisConfig::defaults()
    };

    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("expired"));
}
```

- [ ] **Step 2: Run the targeted config-model tests and confirm they fail**

Run:

```bash
rtk cargo test structured_allowlist_rule_deserializes legacy_string_allowlist_is_rejected expired_rule_is_invalid_for_runtime --lib
```

Expected: FAIL because `AllowlistRule`, `AllowlistOverrideLevel`, and the new validation path do not exist yet.

- [ ] **Step 3: Implement the structured config types and defaults**

Update `src/config/model.rs` to replace the legacy config fields with concrete types like:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum AllowlistOverrideLevel {
    #[default]
    Warn,
    Danger,
    Never,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowlistRule {
    pub pattern: String,
    pub cwd: Option<String>,
    pub user: Option<String>,
    pub expires_at: Option<OffsetDateTime>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AegisConfig {
    pub mode: Mode,
    pub custom_patterns: Vec<UserPattern>,
    pub allowlist: Vec<AllowlistRule>,
    pub allowlist_override_level: AllowlistOverrideLevel,
    pub auto_snapshot_git: bool,
    pub auto_snapshot_docker: bool,
    pub ci_policy: CiPolicy,
    pub audit: AuditConfig,
}
```

Also update:

- `INIT_TEMPLATE`
- `AegisConfig::defaults()`
- `PartialConfig`
- merge semantics for project-over-global allowlist layers
- `src/config/mod.rs` re-exports

Keep `validate()` responsible only for hard config invalidity; warnings move to Task 4.

- [ ] **Step 4: Re-run the config-model tests**

Run:

```bash
rtk cargo test structured_allowlist_rule_deserializes legacy_string_allowlist_is_rejected expired_rule_is_invalid_for_runtime --lib
```

Expected: PASS.

- [ ] **Step 5: Commit the schema change**

Run:

```bash
rtk git add src/config/model.rs src/config/mod.rs
rtk git commit -m "feat: add structured allowlist config"
```

---

### Task 2: Rebuild the allowlist engine around contextual rules

**Files:**
- Modify: `src/config/allowlist.rs`
- Modify: `src/runtime.rs`
- Test: `src/config/allowlist.rs`
- Test: `src/runtime.rs`

- [ ] **Step 1: Write failing matcher tests for scope, precedence, and warnings**

Add unit tests in `src/config/allowlist.rs` that exercise:

```rust
#[test]
fn match_requires_scope_to_fit_context() {
    let allowlist = Allowlist::new(&[AllowlistRule {
        pattern: "terraform destroy -target=module.test.*".to_string(),
        cwd: Some("/srv/infra".to_string()),
        user: Some("ci".to_string()),
        expires_at: None,
        reason: "test teardown".to_string(),
    }])
    .unwrap();

    let ctx = AllowlistContext::new(
        "terraform destroy -target=module.test.api",
        Path::new("/srv/infra"),
        "ci",
        now_utc(),
    );

    assert!(allowlist.match_reason(&ctx).is_some());
    assert!(allowlist.match_reason(&ctx.with_user("alice")).is_none());
}

#[test]
fn project_layer_beats_global_layer_when_both_match() {
    let allowlist = Allowlist::new(&[
        LayeredAllowlistRule::global(rule("terraform destroy *", "global")),
        LayeredAllowlistRule::project(rule("terraform destroy *", "project")),
    ])
    .unwrap();

    let matched = allowlist.match_reason(&ctx("terraform destroy -target=module.test.api")).unwrap();
    assert_eq!(matched.reason, "project");
    assert_eq!(matched.source_layer, AllowlistSourceLayer::Project);
}

#[test]
fn warning_flags_broad_rule_without_scope() {
    let warnings = analyze_allowlist_rule(&AllowlistRule {
        pattern: "terraform destroy *".to_string(),
        cwd: None,
        user: None,
        expires_at: None,
        reason: "broad teardown".to_string(),
    });

    assert!(warnings.iter().any(|w| w.code == "missing_scope"));
    assert!(warnings.iter().any(|w| w.code == "broad_pattern"));
}
```

- [ ] **Step 2: Run the focused allowlist tests and confirm they fail**

Run:

```bash
rtk cargo test match_requires_scope_to_fit_context project_layer_beats_global_layer_when_both_match warning_flags_broad_rule_without_scope --lib
```

Expected: FAIL because `AllowlistContext`, layered precedence, and warning analysis are not implemented.

- [ ] **Step 3: Implement the structured matcher**

Refactor `src/config/allowlist.rs` toward interfaces like:

```rust
pub enum AllowlistSourceLayer {
    Global,
    Project,
}

pub struct AllowlistContext<'a> {
    pub command: &'a str,
    pub cwd: &'a Path,
    pub user: &'a str,
    pub now: OffsetDateTime,
}

pub struct AllowlistMatch {
    pub pattern: String,
    pub reason: String,
    pub source_layer: AllowlistSourceLayer,
}

pub struct AllowlistWarning {
    pub code: &'static str,
    pub message: String,
    pub location: String,
}
```

Implementation rules:

- preserve the existing anchored whole-command glob semantics
- reject malformed rules at construction time with `Result`
- match only effective rules (pattern + scope + unexpired)
- prefer project-local rules over global rules
- within the same layer, first declared rule wins

Update `src/runtime.rs` so `RuntimeContext` builds the allowlist with layered metadata and supplies real context into `allowlist_match(...)`.

- [ ] **Step 4: Re-run the allowlist and runtime tests**

Run:

```bash
rtk cargo test match_requires_scope_to_fit_context project_layer_beats_global_layer_when_both_match warning_flags_broad_rule_without_scope config_is_shared_across_runtime_dependencies --lib
```

Expected: PASS.

- [ ] **Step 5: Commit the matcher refactor**

Run:

```bash
rtk git add src/config/allowlist.rs src/runtime.rs
rtk git commit -m "feat: add contextual allowlist matching"
```

---

### Task 3: Implement override-level policy semantics

**Files:**
- Modify: `src/decision.rs`
- Modify: `src/main.rs`
- Modify: `src/watch.rs`
- Test: `src/decision.rs`
- Test: `src/main.rs`

- [ ] **Step 1: Write failing decision-matrix tests**

Replace the old strict-override tests with matrix cases like:

```rust
#[test]
fn protect_warn_allowlist_override_level_warn_autoapproves() {
    let plan = evaluate_policy(DecisionInput {
        mode: Mode::Protect,
        risk: RiskLevel::Warn,
        in_ci: false,
        ci_policy: CiPolicy::Block,
        allowlist_match: true,
        allowlist_override_level: AllowlistOverrideLevel::Warn,
    });

    assert_eq!(plan.action, PolicyAction::AutoApprove);
}

#[test]
fn protect_danger_allowlist_override_level_warn_still_prompts() {
    let plan = evaluate_policy(DecisionInput {
        mode: Mode::Protect,
        risk: RiskLevel::Danger,
        in_ci: false,
        ci_policy: CiPolicy::Block,
        allowlist_match: true,
        allowlist_override_level: AllowlistOverrideLevel::Warn,
    });

    assert_eq!(plan.action, PolicyAction::Prompt);
    assert!(plan.should_snapshot);
}

#[test]
fn strict_block_never_bypasses_even_with_danger_override() {
    let plan = evaluate_policy(DecisionInput {
        mode: Mode::Strict,
        risk: RiskLevel::Block,
        in_ci: false,
        ci_policy: CiPolicy::Allow,
        allowlist_match: true,
        allowlist_override_level: AllowlistOverrideLevel::Danger,
    });

    assert_eq!(plan.action, PolicyAction::Block);
}
```

- [ ] **Step 2: Run the decision tests and confirm they fail**

Run:

```bash
rtk cargo test protect_warn_allowlist_override_level_warn_autoapproves protect_danger_allowlist_override_level_warn_still_prompts strict_block_never_bypasses_even_with_danger_override --lib
```

Expected: FAIL because `DecisionInput` still uses `strict_allowlist_override`.

- [ ] **Step 3: Implement the new policy input and matrix**

Update `src/decision.rs` so `DecisionInput` becomes:

```rust
pub struct DecisionInput {
    pub mode: Mode,
    pub risk: RiskLevel,
    pub in_ci: bool,
    pub ci_policy: CiPolicy,
    pub allowlist_match: bool,
    pub allowlist_override_level: AllowlistOverrideLevel,
}
```

Implement:

- `Protect`: allowlist may auto-approve `Warn` at `Warn`/`Danger`, `Danger` only at `Danger`
- `Strict`: allowlist may auto-approve `Warn` at `Warn`/`Danger`, `Danger` only at `Danger`
- `Audit`: always auto-approve and never snapshot, regardless of allowlist
- `Block`: still blocks in `Protect`/`Strict`

Then update `src/main.rs` and `src/watch.rs` to pass `config.allowlist_override_level`.

- [ ] **Step 4: Re-run the unit tests**

Run:

```bash
rtk cargo test --lib decision::
```

Expected: PASS with the new matrix coverage.

- [ ] **Step 5: Commit the policy update**

Run:

```bash
rtk git add src/decision.rs src/main.rs src/watch.rs
rtk git commit -m "feat: add allowlist override levels"
```

---

### Task 4: Add shared validation reporting and `aegis config validate`

**Files:**
- Create: `src/config/validate.rs`
- Modify: `src/config/mod.rs`
- Modify: `src/main.rs`
- Test: `src/config/validate.rs`
- Test: `tests/full_pipeline.rs`

- [ ] **Step 1: Write failing validation-unit and CLI tests**

Add unit tests in `src/config/validate.rs` and integration tests in `tests/full_pipeline.rs` such as:

```rust
#[test]
fn validate_reports_warning_for_broad_rule_without_scope() {
    let report = validate_config(&config_with_rule("terraform destroy *", None, None));
    assert!(report.errors.is_empty());
    assert!(report.warnings.iter().any(|w| w.code == "missing_scope"));
}

#[test]
fn validate_reports_error_for_expired_rule() {
    let report = validate_config(&expired_rule_config());
    assert!(!report.errors.is_empty());
    assert!(report.errors.iter().any(|e| e.code == "expired_rule"));
}
```

And in `tests/full_pipeline.rs`:

```rust
#[test]
fn config_validate_json_outputs_errors_and_warnings() {
    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json.get("errors").unwrap().is_array());
    assert!(json.get("warnings").unwrap().is_array());
}
```

- [ ] **Step 2: Run the validation tests and confirm they fail**

Run:

```bash
rtk cargo test validate_reports_warning_for_broad_rule_without_scope validate_reports_error_for_expired_rule config_validate_json_outputs_errors_and_warnings
```

Expected: FAIL because there is no validation module or CLI subcommand yet.

- [ ] **Step 3: Implement shared validation and CLI plumbing**

Create `src/config/validate.rs` with types like:

```rust
pub struct ValidationIssue {
    pub code: &'static str,
    pub message: String,
    pub location: String,
}

pub struct ValidationReport {
    pub valid: bool,
    pub errors: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationIssue>,
}

pub fn validate_config(config: &Config, source_map: &ConfigSourceMap) -> ValidationReport {
    // hard errors from config invalidity + warnings from allowlist analysis
}
```

Update `src/main.rs`:

- add `ConfigCommand::Validate`
- add `--output text|json`
- load the real config path
- print warnings separately from errors
- return `0` on no errors, `4` when errors exist

Re-export validation types from `src/config/mod.rs` only if needed by the CLI.

- [ ] **Step 4: Re-run the validation tests**

Run:

```bash
rtk cargo test validate_reports_warning_for_broad_rule_without_scope validate_reports_error_for_expired_rule config_validate_json_outputs_errors_and_warnings
```

Expected: PASS.

- [ ] **Step 5: Commit the validation command**

Run:

```bash
rtk git add src/config/validate.rs src/config/mod.rs src/main.rs tests/full_pipeline.rs
rtk git commit -m "feat: add config validate command"
```

---

### Task 5: Finish integration coverage and documentation

**Files:**
- Modify: `tests/full_pipeline.rs`
- Modify: `README.md`
- Modify: `src/config/model.rs`

- [ ] **Step 1: Add failing end-to-end regression tests**

Extend `tests/full_pipeline.rs` with coverage for:

```rust
#[test]
fn structured_allowlist_warn_override_autoapproves_warn_but_not_danger() { /* ... */ }

#[test]
fn structured_allowlist_danger_override_autoapproves_danger_and_logs_rule_reason() { /* ... */ }

#[test]
fn legacy_allowlist_schema_fails_config_load() { /* ... */ }

#[test]
fn audit_mode_stays_non_blocking_for_block_classification() { /* ... */ }
```

Also update config-init/show expectations so the generated template and emitted config mention:

- `allowlist_override_level`
- structured `[[allowlist]]`
- no legacy string examples

- [ ] **Step 2: Run the integration tests and confirm they fail**

Run:

```bash
rtk cargo test --test full_pipeline structured_allowlist_warn_override_autoapproves_warn_but_not_danger structured_allowlist_danger_override_autoapproves_danger_and_logs_rule_reason legacy_allowlist_schema_fails_config_load audit_mode_stays_non_blocking_for_block_classification
```

Expected: FAIL until the remaining runtime/doc expectations are aligned.

- [ ] **Step 3: Implement the remaining wiring and doc updates**

Update:

- `tests/full_pipeline.rs` helpers if the new CLI output requires it
- `README.md` config examples to use only:

```toml
allowlist_override_level = "Warn"

[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "/srv/infra"
user = "ci"
reason = "ephemeral test teardown"
```

- `src/config/model.rs` init template comments so they explain:
  - `Warn | Danger | Never`
  - warnings vs errors
  - `Block` never bypasses in `Protect`/`Strict`

- [ ] **Step 4: Run the full verification set**

Run:

```bash
rtk cargo fmt --check
rtk cargo clippy -- -D warnings
rtk cargo test
rtk cargo bench --bench scanner_bench
rtk cargo audit
rtk cargo deny check
```

Expected:

- `fmt`, `clippy`, `test`, `audit`, and `deny` pass
- `scanner_bench` completes without obvious regression in the scanner hot path

- [ ] **Step 5: Commit the final hardening pass**

Run:

```bash
rtk git add tests/full_pipeline.rs README.md src/config/model.rs
rtk git commit -m "test: harden P2 policy regressions"
```

---

## Self-Review

### Spec coverage

- Structured allowlist-only schema: Task 1
- Context-aware matching and precedence: Task 2
- Override-level policy semantics: Task 3
- Shared validation engine and `config validate`: Task 4
- Integration/docs/update/init/show coverage: Task 5

### Placeholder scan

- No `TODO` / `TBD`
- Every task lists exact files
- Every code-changing step includes example code/interface shape
- Every verification step includes exact `rtk` commands

### Type consistency

- `AllowlistRule` and `AllowlistOverrideLevel` are introduced first in Task 1 and reused consistently later
- `AllowlistContext`, `AllowlistMatch`, and `ValidationReport` are defined before later tasks depend on them
- `DecisionInput` is updated in Task 3 before `main.rs` / `watch.rs` consume it

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-04-10-p2-policy-correctness-safety.md`. Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**
