use super::*;

#[test]
fn sandbox_project_allow_network_does_not_reset_global_enabled() {
    // When global config sets [sandbox] enabled = true and allow_network = true
    // (a trusted opt-in), and the project config sets a different sandbox field
    // (required = true, a tightening), the per-field merge must keep enabled =
    // true and allow_network = true from the global layer. `allow_network` is a
    // weakening direction, so it can only be opted in globally — a project
    // cannot enable it over a denied base (see ADR-013).
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "[sandbox]\nenabled = true\nallow_network = true\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nrequired = true\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert!(
        config.sandbox.enabled,
        "global sandbox.enabled must survive a project overlay that only sets required"
    );
    assert!(
        config.sandbox.allow_network,
        "global sandbox.allow_network (trusted opt-in) must survive a project overlay"
    );
    assert!(
        config.sandbox.required,
        "project sandbox.required tightening must be kept by the ratchet"
    );
}

#[test]
fn db_snapshot_nested_tables_do_not_inherit_base_values_on_overlay_replacement() {
    let base = AegisConfig {
        postgres_snapshot: PostgresSnapshotConfig {
            database: "base_pg".to_string(),
            host: "base-pg.local".to_string(),
            port: 6001,
            user: "base_pg_user".to_string(),
        },
        mysql_snapshot: MysqlSnapshotConfig {
            database: "base_mysql".to_string(),
            host: "base-mysql.local".to_string(),
            port: 6002,
            user: "base_mysql_user".to_string(),
        },
        ..AegisConfig::defaults()
    };

    let overlay = PartialConfig {
        postgres_snapshot: Some(PostgresSnapshotConfig {
            database: "overlay_pg".to_string(),
            ..PostgresSnapshotConfig::default()
        }),
        mysql_snapshot: Some(MysqlSnapshotConfig {
            host: "mysql.overlay".to_string(),
            ..MysqlSnapshotConfig::default()
        }),
        ..PartialConfig::default()
    };

    let merged = AegisConfig::merge_layer(base, overlay, ConfigSourceLayer::Project);

    assert_eq!(merged.postgres_snapshot.database, "overlay_pg");
    assert_eq!(merged.postgres_snapshot.host, "localhost");
    assert_eq!(merged.postgres_snapshot.port, 5432);
    assert!(merged.postgres_snapshot.user.is_empty());

    assert_eq!(merged.mysql_snapshot.database, "");
    assert_eq!(merged.mysql_snapshot.host, "mysql.overlay");
    assert_eq!(merged.mysql_snapshot.port, 3306);
    assert!(merged.mysql_snapshot.user.is_empty());
}

#[test]
fn load_full_global_config_without_errors() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        r#"
mode = "Strict"
auto_snapshot_git = false
auto_snapshot_docker = true

[[allow]]
pattern = "terraform destroy -target=module.test.*"
cwd = "/srv/infra"
reason = "global terraform teardown"
expires_at = "2030-01-01T00:00:00Z"

[[allow]]
pattern = "docker system prune --volumes"
cwd = "/srv/infra"
reason = "global cleanup"
expires_at = "2030-01-01T00:00:00Z"

[[custom_patterns]]
id = "USR-001"
category = "Cloud"
risk = "Danger"
pattern = "terraform destroy"
description = "User-defined Terraform destroy rule"
safe_alt = "terraform plan"
"#,
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(config.mode, Mode::Strict);
    assert_eq!(config.allowlist.len(), 2);
    assert_eq!(config.custom_patterns.len(), 1);
    assert!(!config.auto_snapshot_git);
    assert!(config.auto_snapshot_docker);
    assert_eq!(config.custom_patterns[0].id, "USR-001");
    assert_eq!(config.custom_patterns[0].category, Category::Cloud);
    assert_eq!(config.custom_patterns[0].risk, RiskLevel::Danger);
}

#[test]
fn defaults_work_without_any_config_file() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(config, AegisConfig::defaults());
}

#[test]
fn project_config_cannot_weaken_global_mode_but_still_merges_vectors() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);

    fs::create_dir_all(&global_dir).unwrap();
    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        r#"
mode = "Strict"
auto_snapshot_git = false

[[allow]]
pattern = "global-safe-cmd"
cwd = "/srv/infra"
reason = "global command"
expires_at = "2030-01-01T00:00:00Z"

[[custom_patterns]]
id = "GLB-001"
category = "Cloud"
risk = "Danger"
pattern = "aws nuke"
description = "Global cloud nuke rule"
"#,
    )
    .unwrap();

    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        r#"
mode = "Audit"
[[allow]]
pattern = "project-safe-cmd"
cwd = "/srv/infra"
reason = "project command"
expires_at = "2030-01-01T00:00:00Z"

[[custom_patterns]]
id = "PRJ-001"
category = "Filesystem"
risk = "Warn"
pattern = "rm build"
description = "Project build dir removal"
"#,
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    // project cannot weaken global mode; Strict is kept.
    assert_eq!(config.mode, Mode::Strict);
    // global wins for auto_snapshot_git (project didn't set it)
    assert!(!config.auto_snapshot_git);
    // allowlists are merged: global first, then project
    assert_eq!(config.allowlist[0].pattern, "global-safe-cmd");
    assert_eq!(config.allowlist[1].pattern, "project-safe-cmd");
    // patterns are merged: global first, then project
    assert_eq!(config.custom_patterns.len(), 2);
    assert_eq!(config.custom_patterns[0].id, "GLB-001");
    assert_eq!(config.custom_patterns[1].id, "PRJ-001");
}

#[test]
fn project_mode_can_tighten_global_protect_to_strict() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(global_dir.join(GLOBAL_CONFIG_FILE), "mode = \"Protect\"\n").unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "mode = \"Strict\"\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(config.mode, Mode::Strict);
}

#[test]
fn project_mode_cannot_weaken_default_protect_to_audit() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "mode = \"Audit\"\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(config.mode, Mode::Protect);
}

#[test]
fn project_ci_policy_cannot_weaken_default_block_to_allow() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "ci_policy = \"Allow\"\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(config.ci_policy, CiPolicy::Block);
}

#[test]
fn project_sandbox_required_cannot_weaken_global_required_true() {
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

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert!(config.sandbox.required);
}

#[test]
fn project_sandbox_required_can_tighten_default_false_to_true() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "[sandbox]\nrequired = true\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert!(config.sandbox.required);
}

// --- partial override cases ---

#[test]
fn global_mode_and_snapshot_used_when_project_omits_them() {
    // Global sets mode and auto_snapshot_docker; the project omits them. The
    // global values must survive into the final config. (The project file is
    // present but empty, exercising the project layer without overriding the
    // global snapshot flags.)
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "mode = \"Strict\"\nauto_snapshot_docker = false\n",
    )
    .unwrap();
    fs::write(workspace.path().join(PROJECT_CONFIG_FILE), "\n").unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(config.mode, Mode::Strict); // from global
    assert!(!config.auto_snapshot_docker); // from global
}

#[test]
fn audit_rotation_settings_merge_per_field() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        r#"
[audit]
rotation_enabled = true
max_file_size_bytes = 2048
retention_files = 7
compress_rotated = true
"#,
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        r#"
[audit]
retention_files = 2
compress_rotated = false
"#,
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert!(config.audit.rotation_enabled);
    assert_eq!(config.audit.max_file_size_bytes, 2048);
    assert_eq!(config.audit.retention_files, 2);
    assert!(!config.audit.compress_rotated);
}

#[test]
fn invalid_audit_rotation_config_is_rejected() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let config_path = workspace.path().join(PROJECT_CONFIG_FILE);

    fs::write(
        &config_path,
        r#"
[audit]
rotation_enabled = true
max_file_size_bytes = 0
retention_files = 0
"#,
    )
    .unwrap();

    let err = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains(&config_path.display().to_string()),
        "validation error must identify the offending config file: {message}"
    );
    assert!(
        message.contains("audit.max_file_size_bytes") || message.contains("audit.retention_files")
    );
}

#[test]
fn invalid_custom_pattern_config_is_rejected_with_source_path() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let config_path = workspace.path().join(PROJECT_CONFIG_FILE);

    fs::write(
        &config_path,
        r#"
[[custom_patterns]]
id = "FS-001"
category = "Filesystem"
risk = "Warn"
pattern = "echo hello"
description = "Conflicts with built-in pattern id"
"#,
    )
    .unwrap();

    let err = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap_err();
    let message = err.to_string();

    assert!(
        message.contains(&config_path.display().to_string()),
        "custom pattern error must identify the offending config file: {message}"
    );
    assert!(
        message.contains("duplicate pattern id"),
        "custom pattern error must preserve scanner validation details: {message}"
    );
}

#[test]
fn load_for_rejects_malformed_allowlist_fields_with_source_path() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let config_path = workspace.path().join(PROJECT_CONFIG_FILE);

    let cases = [
        (
            "pattern",
            r#"
[[allow]]
pattern = "   "
reason = "valid reason"
expires_at = "2030-01-01T00:00:00Z"
"#,
            "pattern must not be empty",
        ),
        (
            "reason",
            r#"
[[allow]]
pattern = "terraform destroy -target=module.test.*"
reason = "   "
expires_at = "2030-01-01T00:00:00Z"
"#,
            "reason must not be empty",
        ),
        (
            "cwd",
            r#"
[[allow]]
pattern = "terraform destroy -target=module.test.*"
cwd = "   "
reason = "valid reason"
expires_at = "2030-01-01T00:00:00Z"
"#,
            "cwd must not be empty",
        ),
        (
            "user",
            r#"
[[allow]]
pattern = "terraform destroy -target=module.test.*"
user = "   "
reason = "valid reason"
expires_at = "2030-01-01T00:00:00Z"
"#,
            "user must not be empty",
        ),
    ];

    for (field, contents, expected_message) in cases {
        fs::write(&config_path, contents).unwrap();

        let err = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap_err();
        let message = err.to_string();

        assert!(
            message.contains(&config_path.display().to_string()),
            "{field} validation error must identify the offending config file: {message}"
        );
        assert!(
            message.contains(expected_message),
            "{field} validation message mismatch: {message}"
        );
    }
}

#[test]
fn no_home_dir_loads_project_config_only() {
    // When HOME is unavailable there is no global config to look for; the
    // project config and built-in defaults must still be applied correctly.
    let workspace = TempDir::new().unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "mode = \"Audit\"\nauto_snapshot_git = false\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), None).unwrap();

    assert_eq!(config.mode, Mode::Protect);
    // auto_snapshot_git defaults to true; the project's `false` is a weakening
    // attempt that the C3 ratchet ignores (no global layer to override), so the
    // stricter default survives. `mode = "Audit"` being ratcheted back to
    // `Protect` above already proves the project file was loaded.
    assert!(config.auto_snapshot_git);
    assert!(!config.auto_snapshot_docker); // default is false (opt-in)
    assert!(config.allowlist.is_empty());
}

// --- malformed project config ---

#[test]
fn allowlist_override_level_defaults_warn_and_serializes() {
    let config = AegisConfig::defaults();

    assert_eq!(
        config.allowlist_override_level,
        AllowlistOverrideLevel::Warn
    );

    let toml = config.to_toml_string().unwrap();
    assert!(toml.contains("allowlist_override_level = \"Warn\""));
}

#[test]
fn init_template_uses_array_of_tables_for_allowlist() {
    let template = AegisConfig::init_template();

    assert!(
        !template.contains("allowlist = []"),
        "template must not define an empty array that conflicts with [[allow]] entries"
    );
    assert!(
        template.contains("[[allow]]"),
        "template must show the structured allowlist entry form"
    );
    assert!(
        template.contains("Warn | Danger | Never"),
        "template must document the structured allowlist ceiling"
    );
    assert!(
        template.contains("Block never bypasses in Protect/Strict"),
        "template must state that Block cannot be bypassed"
    );
    assert!(
        template.contains("auto_snapshot_postgres = false"),
        "template must surface PostgreSQL snapshot toggles"
    );
    assert!(
        template.contains("[postgres_snapshot]"),
        "template must include the PostgreSQL snapshot section"
    );
    assert!(
        template.contains("auto_snapshot_mysql = false"),
        "template must surface MySQL snapshot toggles"
    );
    assert!(
        template.contains("[mysql_snapshot]"),
        "template must include the MySQL snapshot section"
    );
    assert!(
        template.contains("auto_snapshot_supabase = false"),
        "template must surface Supabase snapshot toggles"
    );
    assert!(
        template.contains("[supabase_snapshot]"),
        "template must include the Supabase snapshot section"
    );
    assert!(
        template.contains("[supabase_snapshot.db]"),
        "template must include the Supabase PostgreSQL transport section"
    );
    assert!(
        template.contains("auto_snapshot_sqlite = false"),
        "template must surface SQLite snapshot toggles"
    );
    assert!(
        template.contains("sqlite_snapshot_path = \"\""),
        "template must include the SQLite snapshot file path"
    );
}

#[test]
fn project_allowlist_override_level_uses_most_restrictive_value() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "allowlist_override_level = \"Never\"\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "allowlist_override_level = \"Danger\"\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(
        config.allowlist_override_level,
        AllowlistOverrideLevel::Never
    );
}

#[test]
fn project_allowlist_override_level_can_tighten_warn_to_never() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "allowlist_override_level = \"Never\"\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(
        config.allowlist_override_level,
        AllowlistOverrideLevel::Never
    );
}
