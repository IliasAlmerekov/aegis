//! Config subcommand behavior: `config show`/`init`/`validate` projection,
//! layered config source attribution, legacy allowlist migration, and
//! per-rule error location reporting.
//!
//! Split from the original `tests/full_pipeline.rs` (behavior-preserving move).

mod support;

use std::fs;

use serde_json::Value;
use tempfile::TempDir;

use support::*;

#[test]
fn config_show_prints_effective_allowlist_override_level() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
mode = "Strict"
allowlist_override_level = "Danger"
[[allow]]
pattern = "terraform destroy -target=module.test.*"
cwd = "/srv/infra"
reason = "ephemeral test teardown"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "show"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("mode = \"Strict\""));
    assert!(stdout.contains("allowlist_override_level = \"Danger\""));
    assert!(stdout.contains("[[allow]]"));
    assert!(stdout.contains("pattern = \"terraform destroy -target=module.test.*\""));
    assert!(stdout.contains("reason = \"ephemeral test teardown\""));
    assert!(
        !stdout.contains("allowlist = ["),
        "config show must emit structured allowlist entries, not legacy string-array syntax"
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
    assert!(
        json["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["code"] == "missing_scope")
    );
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
    assert!(stdout.contains("[[allow]]"));
    assert!(stdout.contains("pattern = \"terraform destroy *\""));
    assert!(stdout.contains("reason = \"migrated from legacy allowlist entry\""));
}

#[test]
fn config_init_writes_truthful_mode_comments() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "init"])
        .output()
        .unwrap();

    assert!(output.status.success());

    let contents = fs::read_to_string(workspace.path().join(".aegis.toml")).unwrap();
    assert!(contents.contains("config_version = 1"));
    assert!(contents.contains("Protect prompts on Warn/Danger"));
    assert!(contents.contains("Audit is non-blocking audit-only"));
    assert!(contents.contains("Strict blocks non-safe and indirect execution forms by default"));
    assert!(contents.contains("allowlist_override_level = \"Warn\""));
    assert!(contents.contains("[[allow]]"));
    assert!(contents.contains("Protect/Strict allowlist ceiling"));
    assert!(contents.contains("allow rule must declare cwd or user scope"));
    assert!(contents.contains("Warn auto-approves allowlisted Warn commands in Protect/Strict"));
    assert!(contents.contains("Danger also auto-approves allowlisted Danger commands"));
    assert!(contents.contains("Never disables allowlist auto-approval for non-safe commands"));
    assert!(contents.contains("Block never bypasses in Protect/Strict"));
    assert!(
        !contents.contains("allowlist = ["),
        "init template must not fall back to legacy string-array syntax"
    );
    assert!(!contents.contains("not yet implemented"));
}

#[test]
fn config_validate_json_outputs_errors_and_warnings() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let config_path = workspace.path().join(".aegis.toml");

    fs::write(
        &config_path,
        r#"
[audit]
rotation_enabled = true
max_file_size_bytes = 0
retention_files = 0

[[allow]]
pattern = "terraform destroy *"
reason = "broad rule"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let errors = json.get("errors").unwrap().as_array().unwrap();
    let warnings = json.get("warnings").unwrap().as_array().unwrap();

    assert!(
        errors.iter().any(|e| e["code"] == "audit_max_file_size"),
        "missing audit_max_file_size error: {errors:?}"
    );
    assert!(
        errors.iter().any(|e| e["code"] == "audit_retention_files"),
        "missing audit_retention_files error: {errors:?}"
    );
    assert!(
        warnings.iter().any(|w| w["code"] == "missing_scope"),
        "missing missing_scope warning: {warnings:?}"
    );
    let config_path = config_path.to_string_lossy();
    assert!(
        errors.iter().any(|e| {
            e["location"]
                .as_str()
                .is_some_and(|location| location.contains(config_path.as_ref()))
        }),
        "expected at least one error location to contain config path {config_path}; errors: {errors:?}"
    );
    assert!(
        warnings.iter().any(|w| {
            w["location"]
                .as_str()
                .is_some_and(|location| location.contains(config_path.as_ref()))
        }),
        "expected at least one warning location to contain config path {config_path}; warnings: {warnings:?}"
    );
}

#[test]
fn config_validate_layered_scalar_errors_point_to_actual_source_files() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let global_dir = home.path().join(".config/aegis");
    let global_path = global_dir.join("config.toml");
    let project_path = workspace.path().join(".aegis.toml");

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        &global_path,
        r#"
[audit]
rotation_enabled = true
max_file_size_bytes = 1024
"#,
    )
    .unwrap();
    fs::write(
        &project_path,
        r#"
[audit]
retention_files = 0
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let errors = json["errors"].as_array().unwrap();

    let retention_error = errors
        .iter()
        .find(|e| e["code"] == "audit_retention_files")
        .unwrap();

    assert!(
        retention_error["location"]
            .as_str()
            .is_some_and(|location| location.contains(project_path.to_string_lossy().as_ref())),
        "retention_files location should reference project config path: {retention_error:?}"
    );
    assert!(
        errors.iter().all(|e| e["code"] != "audit_max_file_size"),
        "max_file_size error should not be present when global layer is valid: {errors:?}"
    );
    assert!(
        errors.iter().all(|e| {
            !e["location"]
                .as_str()
                .is_some_and(|location| location.contains(global_path.to_string_lossy().as_ref()))
        }),
        "no error should be attributed to global layer in this scenario: {errors:?}"
    );
}

#[test]
fn config_validate_reports_global_stage_error_even_if_project_overrides_value() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let global_dir = home.path().join(".config/aegis");
    let global_path = global_dir.join("config.toml");
    let project_path = workspace.path().join(".aegis.toml");

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        &global_path,
        r#"
[audit]
rotation_enabled = true
max_file_size_bytes = 0
"#,
    )
    .unwrap();
    fs::write(
        &project_path,
        r#"
[audit]
max_file_size_bytes = 1024
"#,
    )
    .unwrap();

    let validate_output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(validate_output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&validate_output.stdout).unwrap();
    let error = json["errors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["code"] == "audit_max_file_size")
        .unwrap();

    assert!(
        error["location"]
            .as_str()
            .is_some_and(|location| location.contains(global_path.to_string_lossy().as_ref())),
        "audit_max_file_size should reference global config path: {error:?}"
    );

    let runtime_output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["-c", "printf ok"])
        .output()
        .unwrap();
    assert_eq!(runtime_output.status.code(), Some(4));
    assert!(
        runtime_output.stdout.is_empty(),
        "runtime must fail closed and not execute shell command"
    );
}

#[test]
fn config_validate_stops_after_global_hard_failure() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let global_dir = home.path().join(".config/aegis");
    let global_path = global_dir.join("config.toml");
    let project_path = workspace.path().join(".aegis.toml");

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        &global_path,
        r#"
[audit]
rotation_enabled = true
max_file_size_bytes = 0
"#,
    )
    .unwrap();
    fs::write(
        &project_path,
        r#"
[[allow]]
pattern = "terraform destroy *"
reason = "would warn if reached"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let errors = json["errors"].as_array().unwrap();
    let warnings = json["warnings"].as_array().unwrap();

    assert!(
        errors.iter().any(|e| e["code"] == "audit_max_file_size"),
        "expected global audit error; got {errors:?}"
    );
    assert!(
        warnings.is_empty(),
        "project warnings should be absent because global hard failure stops processing: {warnings:?}"
    );
}

#[test]
fn config_validate_project_rule_uses_file_local_index_in_location() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let global_dir = home.path().join(".config/aegis");
    let global_path = global_dir.join("config.toml");
    let project_path = workspace.path().join(".aegis.toml");

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        &global_path,
        r#"
[[allow]]
pattern = "terraform destroy -target=module.global.api"
cwd = "/srv/global"
user = "ci"
reason = "scoped global"
"#,
    )
    .unwrap();
    fs::write(
        &project_path,
        r#"
[[allow]]
pattern = "terraform destroy *"
reason = "broad project"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let error = json["errors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["code"] == "invalid_allowlist_rule")
        .unwrap();
    let location = error["location"].as_str().unwrap();
    assert!(
        location.contains(project_path.to_string_lossy().as_ref())
            && location.contains("allowlist[0]"),
        "project rule location should use project-local index 0: {error:?}"
    );
}

#[test]
fn config_validate_invalid_custom_pattern_reports_offending_entry_only() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let global_dir = home.path().join(".config/aegis");
    let global_path = global_dir.join("config.toml");
    let project_path = workspace.path().join(".aegis.toml");

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        &global_path,
        r#"
[[custom_patterns]]
id = "USR-GLOBAL-001"
category = "Filesystem"
risk = "Warn"
pattern = "echo global"
description = "global custom pattern"
"#,
    )
    .unwrap();
    fs::write(
        &project_path,
        r#"
[[custom_patterns]]
id = "FS-001"
category = "Filesystem"
risk = "Warn"
pattern = "echo bad"
description = "duplicate built-in id"

[[custom_patterns]]
id = "USR-PROJ-002"
category = "Filesystem"
risk = "Warn"
pattern = "echo later"
description = "would be valid"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let error = json["errors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["code"] == "invalid_custom_pattern")
        .unwrap();

    let location = error["location"].as_str().unwrap();
    assert!(
        location.contains(project_path.to_string_lossy().as_ref())
            && location.contains("custom_patterns[0]"),
        "custom pattern error should point to first offending project entry: {error:?}"
    );
    assert!(
        !location.contains(global_path.to_string_lossy().as_ref()),
        "custom pattern error should not be attributed to unrelated global entries: {error:?}"
    );
}

#[test]
fn config_validate_invalid_allowlist_reports_offending_entry_only() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    let global_dir = home.path().join(".config/aegis");
    let global_path = global_dir.join("config.toml");
    let project_path = workspace.path().join(".aegis.toml");

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        &global_path,
        r#"
[[allow]]
pattern = "terraform destroy -target=module.global.api"
cwd = "/srv/global"
reason = "global valid"
"#,
    )
    .unwrap();
    fs::write(
        &project_path,
        r#"
[[allow]]
pattern = ""
reason = "invalid project rule"

[[allow]]
pattern = "terraform destroy -target=module.project.api"
reason = "never reached"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate", "--output", "json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let error = json["errors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["code"] == "invalid_allowlist_rule")
        .unwrap();

    let location = error["location"].as_str().unwrap();
    assert!(
        location.contains(project_path.to_string_lossy().as_ref())
            && location.contains("allowlist[0]"),
        "allowlist error should point to first offending project entry: {error:?}"
    );
    assert!(
        !location.contains(global_path.to_string_lossy().as_ref()),
        "allowlist error should not be attributed to unrelated global entries: {error:?}"
    );
}

#[test]
fn config_validate_warnings_only_exits_zero_and_prints_text_report() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
[[allow]]
pattern = "terraform destroy *"
cwd = "/srv/infra"
reason = "broad warning only"
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "validate"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("warnings:"));
    assert!(stdout.contains("[broad_pattern]"));
    assert!(!stdout.contains("errors:"));
}

#[test]
fn legacy_allowlist_schema_is_migrated_by_config_show() {
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(".aegis.toml"),
        r#"
mode = "Strict"
allowlist = ["terraform destroy *"]
"#,
    )
    .unwrap();

    let output = base_command(home.path())
        .current_dir(workspace.path())
        .args(["config", "show"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "legacy config should migrate cleanly"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config_version = 1"));
    assert!(stdout.contains("[[allow]]"));
    assert!(stdout.contains("pattern = \"terraform destroy *\""));
    assert!(stdout.contains("reason = \"migrated from legacy allowlist entry\""));
    assert!(!stdout.contains("allowlist = ["));
}
