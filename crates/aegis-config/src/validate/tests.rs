use std::fs;

use super::{
    ConfigSourceMap, GLOBAL_CONFIG_DIR, GLOBAL_CONFIG_FILE, PROJECT_CONFIG_FILE, validate_config,
    validate_config_layers,
};
use crate::error::ConfigError;
use crate::{AegisConfig, AllowlistRule};
use tempfile::TempDir;
use time::{Duration, OffsetDateTime};

#[test]
fn validate_reports_warning_for_broad_rule_without_scope() {
    let config = AegisConfig {
        allowlist: vec![AllowlistRule {
            pattern: "terraform destroy *".to_string(),
            cwd: None,
            user: None,
            expires_at: None,
            reason: "broad test rule".to_string(),
        }],
        ..AegisConfig::defaults()
    };

    let report = validate_config(&config, &ConfigSourceMap::for_config(&config));
    // After scope enforcement, an unscoped rule is a compile-time error.
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.code == "invalid_allowlist_rule")
    );
    assert!(report.warnings.iter().any(|w| w.code == "missing_scope"));
}

#[test]
fn validate_reports_error_for_unscoped_rule() {
    let config = AegisConfig {
        allowlist: vec![AllowlistRule {
            pattern: "terraform destroy *".to_string(),
            cwd: None,
            user: None,
            expires_at: None,
            reason: "too broad".to_string(),
        }],
        ..AegisConfig::defaults()
    };

    let report = validate_config(&config, &ConfigSourceMap::for_config(&config));
    assert!(report.errors.iter().any(|e| e.code == "missing_scope"));
    assert!(report.warnings.iter().any(|w| w.code == "broad_pattern"));
}

#[test]
fn validate_reports_error_for_expired_rule() {
    let config = AegisConfig {
        allowlist: vec![AllowlistRule {
            pattern: "terraform destroy -target=module.test.*".to_string(),
            cwd: None,
            user: None,
            expires_at: Some(OffsetDateTime::now_utc() - Duration::days(1)),
            reason: "expired test rule".to_string(),
        }],
        ..AegisConfig::defaults()
    };

    let report = validate_config(&config, &ConfigSourceMap::for_config(&config));
    assert!(!report.errors.is_empty());
    assert!(report.errors.iter().any(|e| e.code == "expired_rule"));
}

#[test]
fn validate_reports_multiple_audit_errors() {
    let mut config = AegisConfig::defaults();
    config.audit.rotation_enabled = true;
    config.audit.max_file_size_bytes = 0;
    config.audit.retention_files = 0;

    let report = validate_config(&config, &ConfigSourceMap::for_config(&config));
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.code == "audit_max_file_size")
    );
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.code == "audit_retention_files")
    );
}

#[test]
fn validate_uses_real_file_path_in_locations() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let config_path = workspace.path().join(".aegis.toml");
    std::fs::write(
        &config_path,
        r#"
[audit]
rotation_enabled = true
max_file_size_bytes = 0
retention_files = 0
[[allow]]
pattern = "terraform destroy *"
reason = "wide"
"#,
    )
    .unwrap();

    let report = validate_config_layers(workspace.path(), Some(home.path()));

    let config_path = config_path.to_string_lossy();
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.location.contains(config_path.as_ref()))
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.location.contains(config_path.as_ref()))
    );
}

#[test]
fn validation_load_error_returns_structured_generic_code() {
    let err = ConfigError::Config("invalid config".to_string());
    let report = super::validation_load_error(&err);
    assert_eq!(report.errors[0].location, "config");
    assert_eq!(report.errors[0].code, "config_load_error");
}

#[test]
fn validate_scanner_path_runs_when_no_custom_patterns() {
    let config = AegisConfig::defaults();
    let report = validate_config(&config, &ConfigSourceMap::for_config(&config));
    assert!(
        !report
            .errors
            .iter()
            .any(|e| e.code == "invalid_custom_pattern")
    );
    assert!(!report.errors.iter().any(|e| e.code == "scanner_init_error"));
}

// ── Phase 5.2: [[rules]] validation tests ────────────────────────────────
// NOTE: PolicyPatternToken, PolicyRule, PolicyRuleDecision are referenced via
// `crate::PolicyRule` etc., which requires the implementation to add
// `pub use model::{..., PolicyPatternToken, PolicyRule, PolicyRuleDecision, WhenClause};`
// to both model.rs and lib.rs.  Until then these tests fail with E0432.

/// match_examples that genuinely match the pattern must pass validation.
#[test]
fn test_validate_match_examples_pass() {
    use super::validate_policy_rules;
    use crate::{PolicyPatternToken, PolicyRule, PolicyRuleDecision};

    let rule = PolicyRule {
        pattern: vec![
            PolicyPatternToken::Single("git".to_string()),
            PolicyPatternToken::Single("push".to_string()),
        ],
        decision: PolicyRuleDecision::Prompt,
        justification: None,
        match_examples: vec!["git push origin main".to_string()],
        not_match_examples: vec![],
        when: None,
    };

    let result = validate_policy_rules(&[rule]);
    assert!(
        result.is_ok(),
        "matching match_example should pass validation, got: {result:?}"
    );
}

/// not_match_examples that do NOT match the pattern must pass validation.
#[test]
fn test_validate_not_match_examples_pass() {
    use super::validate_policy_rules;
    use crate::{PolicyPatternToken, PolicyRule, PolicyRuleDecision};

    let rule = PolicyRule {
        pattern: vec![
            PolicyPatternToken::Single("git".to_string()),
            PolicyPatternToken::Single("push".to_string()),
        ],
        decision: PolicyRuleDecision::Prompt,
        justification: None,
        match_examples: vec![],
        not_match_examples: vec!["git status".to_string()],
        when: None,
    };

    let result = validate_policy_rules(&[rule]);
    assert!(
        result.is_ok(),
        "non-matching not_match_example should pass validation, got: {result:?}"
    );
}

/// A match_example that does NOT match the rule's pattern must produce a ConfigError.
#[test]
fn test_validate_match_example_fails_when_no_match() {
    use super::validate_policy_rules;
    use crate::{PolicyPatternToken, PolicyRule, PolicyRuleDecision};

    let rule = PolicyRule {
        pattern: vec![
            PolicyPatternToken::Single("git".to_string()),
            PolicyPatternToken::Single("push".to_string()),
        ],
        decision: PolicyRuleDecision::Prompt,
        justification: None,
        match_examples: vec!["rm -rf /".to_string()],
        not_match_examples: vec![],
        when: None,
    };

    let result = validate_policy_rules(&[rule]);
    assert!(
        result.is_err(),
        "match_example that doesn't match should produce ConfigError"
    );
    let (_, err) = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("rm -rf /"),
        "error should mention the failing example, got: {err_str}"
    );
}

/// A not_match_example that DOES match the rule's pattern must produce a ConfigError.
#[test]
fn test_validate_not_match_example_fails_when_matches() {
    use super::validate_policy_rules;
    use crate::{PolicyPatternToken, PolicyRule, PolicyRuleDecision};

    let rule = PolicyRule {
        pattern: vec![
            PolicyPatternToken::Single("git".to_string()),
            PolicyPatternToken::Single("push".to_string()),
        ],
        decision: PolicyRuleDecision::Prompt,
        justification: None,
        match_examples: vec![],
        not_match_examples: vec!["git push origin main".to_string()],
        when: None,
    };

    let result = validate_policy_rules(&[rule]);
    assert!(
        result.is_err(),
        "not_match_example that does match should produce ConfigError"
    );
    let (_, err) = result.unwrap_err();
    let err_str = err.to_string();
    assert!(
        err_str.contains("git push origin main"),
        "error should mention the failing example, got: {err_str}"
    );
}

/// A full TOML document with `[[rules]]` tables must deserialize into
/// `AegisConfig.rules` correctly.
#[test]
fn test_aegisconfig_rules_field_parses_from_toml() {
    use crate::PolicyRuleDecision;

    let toml = r#"
config_version = 1

[[rules]]
pattern       = ["git", "push", ["--force", "-f"]]
decision      = "prompt"
justification = "Force-push rewrites remote history."
match_examples     = ["git push --force origin main"]
not_match_examples = ["git push origin main"]

[[rules]]
pattern  = ["rm", "-rf", "/"]
decision = "block"
"#;

    let config: crate::AegisConfig =
        toml::from_str(toml).expect("AegisConfig should parse [[rules]] tables from TOML");

    assert_eq!(config.rules.len(), 2, "expected 2 policy rules");

    let first = &config.rules[0];
    assert_eq!(first.decision, PolicyRuleDecision::Prompt);
    assert_eq!(first.match_examples, vec!["git push --force origin main"]);
    assert_eq!(first.not_match_examples, vec!["git push origin main"]);

    let second = &config.rules[1];
    assert_eq!(second.decision, PolicyRuleDecision::Block);
}

#[test]
fn validate_config_layers_warns_when_project_attempts_audit_only_weakening() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        r#"
mode = "Audit"
allowlist_override_level = "Danger"
snapshot_policy = "None"
ci_policy = "Allow"
"#,
    )
    .unwrap();

    let report = validate_config_layers(workspace.path(), Some(home.path()));

    assert!(report.valid);
    assert!(report.errors.is_empty());
    assert!(
        report
            .warnings
            .iter()
            .any(|issue| issue.code == "project_security_ratchet"
                && issue.message.contains("mode")
                && issue.message.contains("Audit")
                && issue.message.contains("Protect")),
        "expected project mode weakening warning, got {:#?}",
        report.warnings
    );
}

#[test]
fn validate_config_layers_does_not_warn_when_project_tightens_security() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        r#"
mode = "Strict"
allowlist_override_level = "Never"
snapshot_policy = "Full"
[sandbox]
required = true
"#,
    )
    .unwrap();

    let report = validate_config_layers(workspace.path(), Some(home.path()));

    assert!(report.valid);
    assert!(
        report
            .warnings
            .iter()
            .all(|issue| issue.code != "project_security_ratchet"),
        "unexpected ratchet warnings: {:#?}",
        report.warnings
    );
}

// ── C3 ratchet expansion: warning coverage for neighboring fields ──────
// These tests pin the `project_security_ratchet` warnings that must fire when
// a project config attempts to weaken a security-critical sibling field that
// is currently last-wins (sandbox.enabled, auto_snapshot_*, allow_network,
// allow_write). The effective-value ratchet is asserted in model::tests::merge;
// here we assert the validation report surfaces an advisory warning too.

#[test]
fn validate_warns_when_project_disables_globally_enabled_sandbox() {
    // F1: project `sandbox.enabled = false` over global `enabled = true`
    // defeats the inherited sandbox entirely and must emit a ratchet warning.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[sandbox]\nenabled = true\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nenabled = false\n",
    )
    .unwrap();

    let report = validate_config_layers(workspace.path(), Some(home.path()));

    assert!(
        report.valid,
        "config must remain valid; ratchet weakening is advisory, got errors: {:#?}",
        report.errors
    );
    assert!(
        report.errors.is_empty(),
        "expected no hard errors, got {:#?}",
        report.errors
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.code == "project_security_ratchet" && w.message.contains("sandbox.enabled")),
        "expected ratchet warning for sandbox.enabled, got {:#?}",
        report.warnings
    );
}

#[test]
fn validate_warns_when_project_disables_globally_enabled_snapshot_flag() {
    // F2: project `auto_snapshot_git = false` over global `true` disables git
    // snapshots under Selective policy and must emit a ratchet warning.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_git = true\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "auto_snapshot_git = false\n",
    )
    .unwrap();

    let report = validate_config_layers(workspace.path(), Some(home.path()));

    assert!(
        report.valid,
        "config must remain valid; ratchet weakening is advisory, got errors: {:#?}",
        report.errors
    );
    assert!(
        report.warnings.iter().any(
            |w| w.code == "project_security_ratchet" && w.message.contains("auto_snapshot_git")
        ),
        "expected ratchet warning for auto_snapshot_git, got {:#?}",
        report.warnings
    );
}

#[test]
fn validate_warns_when_project_enables_network_over_global_deny() {
    // F3: project `allow_network = true` over global `false` weakens the jail
    // (network is the weaker direction) and must emit a ratchet warning.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[sandbox]\nenabled = true\nallow_network = false\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nallow_network = true\n",
    )
    .unwrap();

    let report = validate_config_layers(workspace.path(), Some(home.path()));

    assert!(
        report.valid,
        "config must remain valid; ratchet weakening is advisory, got errors: {:#?}",
        report.errors
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.code == "project_security_ratchet" && w.message.contains("allow_network")),
        "expected ratchet warning for allow_network, got {:#?}",
        report.warnings
    );
}

#[test]
fn validate_warns_when_project_expands_allow_write_over_global_base() {
    // F3: project adds `/etc` to the writable set beyond the global base
    // `/opt/data` — more paths is weaker and must emit a ratchet warning.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[sandbox]\nenabled = true\nallow_write = [\"/opt/data\"]\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nallow_write = [\"/opt/data\", \"/etc\"]\n",
    )
    .unwrap();

    let report = validate_config_layers(workspace.path(), Some(home.path()));

    assert!(
        report.valid,
        "config must remain valid; ratchet weakening is advisory, got errors: {:#?}",
        report.errors
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.code == "project_security_ratchet" && w.message.contains("allow_write")),
        "expected ratchet warning for allow_write, got {:#?}",
        report.warnings
    );
}

#[test]
fn validate_warns_when_project_relaxes_globally_required_sandbox() {
    // F8 coverage gap: all current ratchet tests use an empty home dir so
    // `base.sandbox.required` is always false. This test sets a NON-empty
    // global config with `required = true` and a project `required = false`,
    // exercising the `base.required == true && requested == false` branch.
    // Current code already emits this warning (kept = true || false = true,
    // kept != requested), so this is a GUARD test — it must stay green after
    // the ratchet expansion lands.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[sandbox]\nrequired = true\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nrequired = false\n",
    )
    .unwrap();

    let report = validate_config_layers(workspace.path(), Some(home.path()));

    assert!(
        report.valid,
        "config must remain valid; ratchet weakening is advisory, got errors: {:#?}",
        report.errors
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.code == "project_security_ratchet"
                && w.message.contains("sandbox.required")
                && w.message.contains("true")
                && w.message.contains("false")),
        "expected ratchet warning for sandbox.required (true kept, false requested), got {:#?}",
        report.warnings
    );
}
