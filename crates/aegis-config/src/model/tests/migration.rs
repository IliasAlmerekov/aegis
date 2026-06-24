use super::*;

#[test]
fn malformed_project_config_is_fatal() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "mode = \"Strict\"\nauto_snapshot_git = false\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "mode = <<<THIS IS NOT VALID TOML\n",
    )
    .unwrap();

    let err = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap_err();
    let message = err.to_string();

    assert!(
        message.contains(
            &workspace
                .path()
                .join(PROJECT_CONFIG_FILE)
                .display()
                .to_string()
        ),
        "error must identify the malformed project config file: {message}"
    );
    assert!(
        message.contains("failed to parse"),
        "error must preserve the parse failure details: {message}"
    );
}

// ── Snapshot policy tests ───────────────────────────────────────

#[test]
fn snapshot_policy_defaults_to_selective() {
    let config = AegisConfig::defaults();
    assert_eq!(config.snapshot_policy, SnapshotPolicy::Selective);
}

#[test]
fn snapshot_policy_none_deserializes() {
    let config: AegisConfig = toml::from_str(r#"snapshot_policy = "None""#).unwrap();
    assert_eq!(config.snapshot_policy, SnapshotPolicy::None);
}

#[test]
fn snapshot_policy_selective_deserializes() {
    let config: AegisConfig = toml::from_str(r#"snapshot_policy = "Selective""#).unwrap();
    assert_eq!(config.snapshot_policy, SnapshotPolicy::Selective);
}

#[test]
fn snapshot_policy_full_deserializes() {
    let config: AegisConfig = toml::from_str(r#"snapshot_policy = "Full""#).unwrap();
    assert_eq!(config.snapshot_policy, SnapshotPolicy::Full);
}

#[test]
fn snapshot_policy_none_ignores_per_plugin_flags() {
    let config: AegisConfig = toml::from_str(
        r#"
snapshot_policy = "None"
auto_snapshot_git = true
auto_snapshot_docker = true
"#,
    )
    .unwrap();
    assert_eq!(config.snapshot_policy, SnapshotPolicy::None);
}

#[test]
fn snapshot_policy_full_enables_all_regardless_of_flags() {
    let config: AegisConfig = toml::from_str(
        r#"
snapshot_policy = "Full"
auto_snapshot_git = false
auto_snapshot_docker = false
"#,
    )
    .unwrap();
    assert_eq!(config.snapshot_policy, SnapshotPolicy::Full);
}

#[test]
fn project_snapshot_policy_cannot_weaken_global_full_to_none() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "snapshot_policy = \"Full\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "snapshot_policy = \"None\"\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(config.snapshot_policy, SnapshotPolicy::Full);
}

#[test]
fn project_snapshot_policy_can_tighten_default_selective_to_full() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "snapshot_policy = \"Full\"\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(config.snapshot_policy, SnapshotPolicy::Full);
}

#[test]
fn snapshot_policy_absent_in_overlay_keeps_base() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "snapshot_policy = \"None\"\n",
    )
    .unwrap();
    // Project sets only mode, not snapshot_policy.
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "mode = \"Audit\"\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    assert_eq!(config.snapshot_policy, SnapshotPolicy::None);
}

#[test]
fn user_pattern_deserializes_justification() {
    let pattern: UserPattern = toml::from_str(
        r#"
id = "USR-001"
category = "Cloud"
risk = "Warn"
pattern = "test"
description = "desc"
justification = "because"
"#,
    )
    .unwrap();
    assert_eq!(pattern.justification, Some("because".to_string()));
}

#[test]
fn user_pattern_justification_is_optional() {
    let pattern: UserPattern = toml::from_str(
        r#"
id = "USR-002"
category = "Cloud"
risk = "Warn"
pattern = "test"
description = "desc"
"#,
    )
    .unwrap();
    assert_eq!(pattern.justification, None);
}

#[test]
fn custom_pattern_with_justification_roundtrips_through_config() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        r#"
[[custom_patterns]]
id = "USR-JST"
category = "Cloud"
risk = "Danger"
pattern = "terraform destroy"
description = "Terraform destroy guard"
justification = "This tears down all provisioned infrastructure. Confirm you are in the correct workspace and have state backups."
safe_alt = "terraform plan -destroy"
"#,
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    assert_eq!(config.custom_patterns.len(), 1);
    assert_eq!(config.custom_patterns[0].id, "USR-JST");
    assert_eq!(
        config.custom_patterns[0].justification,
        Some("This tears down all provisioned infrastructure. Confirm you are in the correct workspace and have state backups.".to_string())
    );
}

#[test]
fn legacy_blocklist_table_name_deserializes_into_block_field() {
    let config: AegisConfig = toml::from_str(
        r#"
[[blocklist]]
pattern = "rm -rf /"
cwd = "/tmp"
reason = "never delete root"
"#,
    )
    .unwrap();

    assert_eq!(config.blocklist.len(), 1);
    assert_eq!(config.blocklist[0].pattern, "rm -rf /");
    assert_eq!(config.blocklist[0].cwd, Some("/tmp".to_string()));
    assert_eq!(config.blocklist[0].reason, "never delete root");
}

#[test]
fn find_toml_array_bounds_single_line() {
    let text = r#"mode = "Protect"
allowlist = ["terraform destroy *", "docker system prune"]
auto_snapshot_git = true
"#;
    let (start, end) = find_toml_array_bounds(text, "allowlist").unwrap();
    assert_eq!(
        &text[start..end],
        r#"allowlist = ["terraform destroy *", "docker system prune"]"#
    );
}

#[test]
fn find_toml_array_bounds_multi_line() {
    let text = r#"mode = "Protect"
allowlist = [
"terraform destroy *",
"docker system prune",
]
auto_snapshot_git = true
"#;
    let (start, end) = find_toml_array_bounds(text, "allowlist").unwrap();
    assert!(text[start..end].starts_with("allowlist = ["));
    assert!(text[start..end].ends_with(']'));
}

#[test]
fn migrate_rewrites_allowlist_table_header_to_allow() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"mode = "Protect"
[[allowlist]]
pattern = "terraform destroy *"
cwd = "/srv/infra"
reason = "legacy header"
"#,
    )
    .unwrap();

    let contents = fs::read_to_string(&path).unwrap();
    let config: PartialConfig = toml::from_str(&contents).unwrap();
    migrate_deprecated_allowlist_in_file(&contents, &path, &config.allowlist).unwrap();

    let rewritten = fs::read_to_string(&path).unwrap();
    assert!(
        rewritten.contains("[[allow]]"),
        "must rewrite [[allowlist]] to [[allow]]; got:\n{rewritten}"
    );
}

#[test]
fn migrate_removes_deprecated_allowlist_header() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"mode = "Protect"
[[allowlist]]
pattern = "terraform destroy *"
cwd = "/srv/infra"
reason = "legacy header"
"#,
    )
    .unwrap();

    let contents = fs::read_to_string(&path).unwrap();
    let config: PartialConfig = toml::from_str(&contents).unwrap();
    migrate_deprecated_allowlist_in_file(&contents, &path, &config.allowlist).unwrap();

    let rewritten = fs::read_to_string(&path).unwrap();
    assert!(
        !rewritten.contains("[[allowlist]]"),
        "must not contain deprecated header; got:\n{rewritten}"
    );
}

#[test]
fn migrate_preserves_unrelated_fields_when_rewriting_header() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"mode = "Protect"
[[allowlist]]
pattern = "terraform destroy *"
cwd = "/srv/infra"
reason = "legacy header"
"#,
    )
    .unwrap();

    let contents = fs::read_to_string(&path).unwrap();
    let config: PartialConfig = toml::from_str(&contents).unwrap();
    migrate_deprecated_allowlist_in_file(&contents, &path, &config.allowlist).unwrap();

    let rewritten = fs::read_to_string(&path).unwrap();
    assert!(
        rewritten.contains("mode = \"Protect\""),
        "must preserve other fields"
    );
}

#[test]
fn migrate_converts_legacy_string_array_to_structured_tables() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"mode = "Protect"
allowlist = ["terraform destroy *", "docker system prune"]
auto_snapshot_git = true
"#,
    )
    .unwrap();

    let contents = fs::read_to_string(&path).unwrap();
    let config: PartialConfig = toml::from_str(&contents).unwrap();
    migrate_deprecated_allowlist_in_file(&contents, &path, &config.allowlist).unwrap();

    let rewritten = fs::read_to_string(&path).unwrap();
    assert!(
        !rewritten.contains("allowlist = ["),
        "must remove legacy array; got:\n{rewritten}"
    );
}

#[test]
fn migrate_adds_allow_header_from_legacy_array() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"mode = "Protect"
allowlist = ["terraform destroy *", "docker system prune"]
auto_snapshot_git = true
"#,
    )
    .unwrap();

    let contents = fs::read_to_string(&path).unwrap();
    let config: PartialConfig = toml::from_str(&contents).unwrap();
    migrate_deprecated_allowlist_in_file(&contents, &path, &config.allowlist).unwrap();

    let rewritten = fs::read_to_string(&path).unwrap();
    assert!(
        rewritten.contains("[[allow]]"),
        "must add [[allow]] tables; got:\n{rewritten}"
    );
}

#[test]
fn migrate_preserves_first_pattern_from_legacy_array() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"mode = "Protect"
allowlist = ["terraform destroy *", "docker system prune"]
auto_snapshot_git = true
"#,
    )
    .unwrap();

    let contents = fs::read_to_string(&path).unwrap();
    let config: PartialConfig = toml::from_str(&contents).unwrap();
    migrate_deprecated_allowlist_in_file(&contents, &path, &config.allowlist).unwrap();

    let rewritten = fs::read_to_string(&path).unwrap();
    assert!(
        rewritten.contains("pattern = \"terraform destroy *\""),
        "must preserve first pattern; got:\n{rewritten}"
    );
}

#[test]
fn migrate_preserves_second_pattern_from_legacy_array() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"mode = "Protect"
allowlist = ["terraform destroy *", "docker system prune"]
auto_snapshot_git = true
"#,
    )
    .unwrap();

    let contents = fs::read_to_string(&path).unwrap();
    let config: PartialConfig = toml::from_str(&contents).unwrap();
    migrate_deprecated_allowlist_in_file(&contents, &path, &config.allowlist).unwrap();

    let rewritten = fs::read_to_string(&path).unwrap();
    assert!(
        rewritten.contains("pattern = \"docker system prune\""),
        "must preserve second pattern; got:\n{rewritten}"
    );
}

#[test]
fn migrate_carries_migration_reason_from_legacy_array() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"mode = "Protect"
allowlist = ["terraform destroy *", "docker system prune"]
auto_snapshot_git = true
"#,
    )
    .unwrap();

    let contents = fs::read_to_string(&path).unwrap();
    let config: PartialConfig = toml::from_str(&contents).unwrap();
    migrate_deprecated_allowlist_in_file(&contents, &path, &config.allowlist).unwrap();

    let rewritten = fs::read_to_string(&path).unwrap();
    assert!(
        rewritten.contains("migrated from legacy allowlist entry"),
        "must carry migration reason; got:\n{rewritten}"
    );
}

#[test]
fn migrate_preserves_unrelated_fields_when_converting_array() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"mode = "Protect"
allowlist = ["terraform destroy *", "docker system prune"]
auto_snapshot_git = true
"#,
    )
    .unwrap();

    let contents = fs::read_to_string(&path).unwrap();
    let config: PartialConfig = toml::from_str(&contents).unwrap();
    migrate_deprecated_allowlist_in_file(&contents, &path, &config.allowlist).unwrap();

    let rewritten = fs::read_to_string(&path).unwrap();
    assert!(
        rewritten.contains("mode = \"Protect\""),
        "must preserve other fields"
    );
}

#[test]
fn migrate_does_nothing_for_modern_syntax() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("config.toml");
    let original = r#"mode = "Protect"
[[allow]]
pattern = "git status"
cwd = "/srv/infra"
reason = "modern syntax"
"#;
    fs::write(&path, original).unwrap();

    let contents = fs::read_to_string(&path).unwrap();
    let config: PartialConfig = toml::from_str(&contents).unwrap();
    migrate_deprecated_allowlist_in_file(&contents, &path, &config.allowlist).unwrap();

    let rewritten = fs::read_to_string(&path).unwrap();
    assert_eq!(
        rewritten, original,
        "must not rewrite file with only modern syntax"
    );
}

#[test]
fn block_rule_deserializes_from_toml() {
    let parsed: AegisConfig = toml::from_str(
        r#"[[block]]
pattern = "x"
cwd = "/"
reason = "test"
"#,
    )
    .unwrap();
    assert_eq!(parsed.blocklist.len(), 1);
}

#[test]
fn find_toml_array_bounds_ignores_brackets_inside_literal_strings() {
    let text = r#"mode = "Protect"
allowlist = ["rm -rf /tmp/don't", "echo [done]"]
auto_snapshot_git = true
"#;
    let (start, end) = find_toml_array_bounds(text, "allowlist").unwrap();
    assert_eq!(
        &text[start..end],
        r#"allowlist = ["rm -rf /tmp/don't", "echo [done]"]"#
    );
}
