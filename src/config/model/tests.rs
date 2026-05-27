use super::*;
use tempfile::TempDir;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[test]
fn structured_allow_rule_deserializes() {
    let config: AegisConfig = toml::from_str(
        r#"
allowlist_override_level = "Warn"

[[allow]]
pattern = "terraform destroy -target=module.test.*"
cwd = "/srv/infra"
user = "ci"
expires_at = "2030-01-01T00:00:00Z"
reason = "ephemeral test teardown"
"#,
    )
    .unwrap();

    assert_eq!(config.allowlist.len(), 1);
    assert_eq!(
        config.allowlist[0].pattern,
        "terraform destroy -target=module.test.*"
    );
    assert_eq!(
        config.allowlist_override_level,
        AllowlistOverrideLevel::Warn
    );
}

#[test]
fn legacy_string_allowlist_is_migrated_to_structured_rules() {
    let config: AegisConfig = toml::from_str(r#"allowlist = ["terraform destroy *"]"#).unwrap();

    assert_eq!(config.allowlist.len(), 1);
    assert_eq!(config.allowlist[0].pattern, "terraform destroy *");
    assert_eq!(
        config.allowlist[0].reason,
        "migrated from legacy allowlist entry"
    );
}

#[test]
fn legacy_allowlist_table_name_deserializes_into_allow_field() {
    let config: AegisConfig = toml::from_str(
        r#"
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
cwd = "/srv/infra"
reason = "legacy table name"
expires_at = "2030-01-01T00:00:00Z"
"#,
    )
    .unwrap();

    assert_eq!(config.allowlist.len(), 1);
    assert_eq!(
        config.allowlist[0].pattern,
        "terraform destroy -target=module.test.*"
    );
    assert_eq!(config.allowlist[0].cwd, Some("/srv/infra".to_string()));
}

#[test]
fn config_version_newer_than_binary_emits_migration_error() {
    let err = toml::from_str::<AegisConfig>("config_version = 99").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("requires a newer version of Aegis"),
        "expected migration error, got: {msg}"
    );
    assert!(
        msg.contains("aegis config init"),
        "error must include downgrade instructions: {msg}"
    );
}

#[test]
fn config_version_below_minimum_emits_legacy_error() {
    let err = toml::from_str::<AegisConfig>("config_version = 0").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("below the minimum supported version"),
        "expected legacy error, got: {msg}"
    );
    assert!(
        msg.contains("aegis config init"),
        "error must include regen instructions: {msg}"
    );
}

#[test]
fn expired_rule_is_invalid_for_runtime() {
    let config = AegisConfig {
        allowlist: vec![AllowlistRule {
            pattern: "terraform destroy -target=module.test.*".to_string(),
            cwd: None,
            user: None,
            expires_at: Some(OffsetDateTime::parse("2020-01-01T00:00:00Z", &Rfc3339).unwrap()),
            reason: "expired teardown".to_string(),
        }],
        ..AegisConfig::defaults()
    };

    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("expired"));
}

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
    let config: AegisConfig = toml::from_str(r#"allowlist = ["terraform destroy *"]"#).unwrap();

    let err = config.validate_runtime_requirements().unwrap_err();
    assert!(err.to_string().contains("must declare cwd or user scope"));
}

#[test]
fn scoped_allowlist_rule_with_cwd_is_valid_for_runtime() {
    let config = AegisConfig {
        allowlist: vec![AllowlistRule {
            pattern: "terraform destroy -target=module.test.*".to_string(),
            cwd: Some("/srv/infra".to_string()),
            user: None,
            expires_at: None,
            reason: "scoped teardown".to_string(),
        }],
        ..AegisConfig::defaults()
    };

    assert!(config.validate().is_ok());
}

#[test]
fn scoped_allowlist_rule_with_user_is_valid_for_runtime() {
    let config = AegisConfig {
        allowlist: vec![AllowlistRule {
            pattern: "terraform destroy -target=module.test.*".to_string(),
            cwd: None,
            user: Some("ci".to_string()),
            expires_at: None,
            reason: "scoped teardown".to_string(),
        }],
        ..AegisConfig::defaults()
    };

    assert!(config.validate().is_ok());
}

#[test]
fn load_minimal_project_config_without_errors() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();

    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "mode = \"Audit\"\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(config.mode, Mode::Audit);
    assert!(config.custom_patterns.is_empty());
    assert!(config.allowlist.is_empty());
    assert!(config.auto_snapshot_git);
    assert!(!config.auto_snapshot_docker); // default is false (opt-in)
}

#[test]
fn postgres_snapshot_config_deserializes() {
    let config: AegisConfig = toml::from_str(
        r#"
auto_snapshot_postgres = true

[postgres_snapshot]
database = "myapp"
host = "db.local"
port = 5433
user = "appuser"
"#,
    )
    .unwrap();

    assert!(config.auto_snapshot_postgres);
    assert_eq!(config.postgres_snapshot.database, "myapp");
    assert_eq!(config.postgres_snapshot.host, "db.local");
    assert_eq!(config.postgres_snapshot.port, 5433);
    assert_eq!(config.postgres_snapshot.user, "appuser");
}

#[test]
fn supabase_snapshot_config_deserializes() {
    let config: AegisConfig = toml::from_str(
        r#"
auto_snapshot_supabase = true

[supabase_snapshot]
project_ref = "proj_123"
require_config_target_match_on_rollback = false

[supabase_snapshot.db]
database = "postgres"
host = "db.supabase.co"
port = 6543
user = "postgres"
"#,
    )
    .unwrap();

    assert!(config.auto_snapshot_supabase);
    assert_eq!(config.supabase_snapshot.project_ref, "proj_123");
    assert!(
        !config
            .supabase_snapshot
            .require_config_target_match_on_rollback
    );
    assert_eq!(config.supabase_snapshot.db.database, "postgres");
    assert_eq!(config.supabase_snapshot.db.host, "db.supabase.co");
    assert_eq!(config.supabase_snapshot.db.port, 6543);
    assert_eq!(config.supabase_snapshot.db.user, "postgres");
}

#[test]
fn mysql_snapshot_config_deserializes() {
    let config: AegisConfig = toml::from_str(
        r#"
auto_snapshot_mysql = true

[mysql_snapshot]
database = "shop"
host = "127.0.0.1"
port = 3307
user = "root"
"#,
    )
    .unwrap();

    assert!(config.auto_snapshot_mysql);
    assert_eq!(config.mysql_snapshot.database, "shop");
    assert_eq!(config.mysql_snapshot.host, "127.0.0.1");
    assert_eq!(config.mysql_snapshot.port, 3307);
    assert_eq!(config.mysql_snapshot.user, "root");
}

#[test]
fn sqlite_snapshot_config_deserializes() {
    let config: AegisConfig = toml::from_str(
        r#"
auto_snapshot_sqlite = true
sqlite_snapshot_path = "db/app.db"
"#,
    )
    .unwrap();

    assert!(config.auto_snapshot_sqlite);
    assert_eq!(config.sqlite_snapshot_path, "db/app.db");
}

#[test]
fn supabase_snapshot_defaults_are_disabled() {
    let config = AegisConfig::defaults();

    assert!(!config.auto_snapshot_supabase);
    assert!(config.supabase_snapshot.project_ref.is_empty());
    assert!(
        config
            .supabase_snapshot
            .require_config_target_match_on_rollback
    );
    assert_eq!(
        config.supabase_snapshot.db,
        PostgresSnapshotConfig::default()
    );
}

#[test]
fn db_snapshot_defaults_are_disabled() {
    let config = AegisConfig::defaults();

    assert!(!config.auto_snapshot_postgres);
    assert!(!config.auto_snapshot_mysql);
    assert!(!config.auto_snapshot_sqlite);
    assert!(config.postgres_snapshot.database.is_empty());
    assert!(config.mysql_snapshot.database.is_empty());
    assert!(config.sqlite_snapshot_path.is_empty());
}

#[test]
fn supabase_snapshot_fields_merge_by_replacement_and_scalar_override() {
    let base = AegisConfig {
        auto_snapshot_supabase: false,
        supabase_snapshot: SupabaseSnapshotConfig {
            project_ref: "base_proj".to_string(),
            require_config_target_match_on_rollback: true,
            db: PostgresSnapshotConfig {
                database: "base_db".to_string(),
                host: "base.supabase.co".to_string(),
                port: 6001,
                user: "base_user".to_string(),
            },
        },
        ..AegisConfig::defaults()
    };

    let overlay = PartialConfig {
        auto_snapshot_supabase: Some(true),
        supabase_snapshot: Some(SupabaseSnapshotConfig {
            project_ref: "overlay_proj".to_string(),
            require_config_target_match_on_rollback: false,
            db: PostgresSnapshotConfig {
                database: "overlay_db".to_string(),
                host: "overlay.supabase.co".to_string(),
                port: 6543,
                user: "overlay_user".to_string(),
            },
        }),
        ..PartialConfig::default()
    };

    let merged = AegisConfig::merge_layer(base, overlay, ConfigSourceLayer::Project);

    assert!(merged.auto_snapshot_supabase);
    assert_eq!(merged.supabase_snapshot.project_ref, "overlay_proj");
    assert!(
        !merged
            .supabase_snapshot
            .require_config_target_match_on_rollback
    );
    assert_eq!(merged.supabase_snapshot.db.database, "overlay_db");
    assert_eq!(merged.supabase_snapshot.db.host, "overlay.supabase.co");
    assert_eq!(merged.supabase_snapshot.db.port, 6543);
    assert_eq!(merged.supabase_snapshot.db.user, "overlay_user");
}

#[test]
fn partial_supabase_snapshot_overlay_replaces_entire_bundle() {
    let base = AegisConfig {
        supabase_snapshot: SupabaseSnapshotConfig {
            project_ref: "base_proj".to_string(),
            require_config_target_match_on_rollback: false,
            db: PostgresSnapshotConfig {
                database: "base_db".to_string(),
                host: "base.supabase.co".to_string(),
                port: 6001,
                user: "base_user".to_string(),
            },
        },
        ..AegisConfig::defaults()
    };

    let overlay = PartialConfig {
        supabase_snapshot: Some(SupabaseSnapshotConfig {
            project_ref: "overlay_proj".to_string(),
            ..SupabaseSnapshotConfig::default()
        }),
        ..PartialConfig::default()
    };

    let merged = AegisConfig::merge_layer(base, overlay, ConfigSourceLayer::Project);

    assert_eq!(merged.supabase_snapshot.project_ref, "overlay_proj");
    assert!(
        merged
            .supabase_snapshot
            .require_config_target_match_on_rollback
    );
    assert_eq!(
        merged.supabase_snapshot.db,
        PostgresSnapshotConfig::default()
    );
}

#[test]
fn db_snapshot_fields_merge_by_replacement_and_scalar_override() {
    let base = AegisConfig {
        auto_snapshot_postgres: false,
        postgres_snapshot: PostgresSnapshotConfig {
            database: "base_pg".to_string(),
            host: "base-pg.local".to_string(),
            port: 6001,
            user: "base_pg_user".to_string(),
        },
        auto_snapshot_mysql: false,
        mysql_snapshot: MysqlSnapshotConfig {
            database: "base_mysql".to_string(),
            host: "base-mysql.local".to_string(),
            port: 6002,
            user: "base_mysql_user".to_string(),
        },
        auto_snapshot_sqlite: false,
        sqlite_snapshot_path: "base/db.sqlite".to_string(),
        ..AegisConfig::defaults()
    };

    let overlay = PartialConfig {
        auto_snapshot_postgres: Some(true),
        postgres_snapshot: Some(PostgresSnapshotConfig {
            database: "overlay_pg".to_string(),
            host: "db.local".to_string(),
            port: 5433,
            user: "appuser".to_string(),
        }),
        auto_snapshot_mysql: Some(true),
        mysql_snapshot: Some(MysqlSnapshotConfig {
            database: "shop".to_string(),
            host: "127.0.0.1".to_string(),
            port: 3307,
            user: "root".to_string(),
        }),
        auto_snapshot_sqlite: Some(true),
        sqlite_snapshot_path: Some("db/app.db".to_string()),
        ..PartialConfig::default()
    };

    let merged = AegisConfig::merge_layer(base, overlay, ConfigSourceLayer::Project);

    assert!(merged.auto_snapshot_postgres);
    assert_eq!(merged.postgres_snapshot.database, "overlay_pg");
    assert_eq!(merged.postgres_snapshot.host, "db.local");
    assert_eq!(merged.postgres_snapshot.port, 5433);
    assert_eq!(merged.postgres_snapshot.user, "appuser");

    assert!(merged.auto_snapshot_mysql);
    assert_eq!(merged.mysql_snapshot.database, "shop");
    assert_eq!(merged.mysql_snapshot.host, "127.0.0.1");
    assert_eq!(merged.mysql_snapshot.port, 3307);
    assert_eq!(merged.mysql_snapshot.user, "root");

    assert!(merged.auto_snapshot_sqlite);
    assert_eq!(merged.sqlite_snapshot_path, "db/app.db");
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
fn project_config_overrides_global_scalars_and_vecs_are_merged() {
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

    // project wins for mode
    assert_eq!(config.mode, Mode::Audit);
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

// --- partial override cases ---

#[test]
fn global_mode_and_snapshot_used_when_project_omits_them() {
    // Global sets mode and auto_snapshot_docker; project sets only auto_snapshot_git.
    // The global values must survive into the final config.
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "mode = \"Strict\"\nauto_snapshot_docker = false\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "auto_snapshot_git = false\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert_eq!(config.mode, Mode::Strict); // from global
    assert!(!config.auto_snapshot_docker); // from global
    assert!(!config.auto_snapshot_git); // from project
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
fn project_false_wins_over_global_true_for_bool_scalar() {
    // When both files set the same bool field, the project value must win
    // even when it is `false` (so it can't be confused with "not set").
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
    fs::create_dir_all(&global_dir).unwrap();

    fs::write(
        global_dir.join(GLOBAL_CONFIG_FILE),
        "auto_snapshot_git = true\nauto_snapshot_docker = true\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join(PROJECT_CONFIG_FILE),
        "auto_snapshot_git = false\nauto_snapshot_docker = false\n",
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

    assert!(!config.auto_snapshot_git);
    assert!(!config.auto_snapshot_docker);
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

    assert_eq!(config.mode, Mode::Audit);
    assert!(!config.auto_snapshot_git);
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
fn allowlist_override_level_project_value_overrides_global() {
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
        AllowlistOverrideLevel::Danger
    );
}

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
fn snapshot_policy_merges_from_overlay() {
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
    // Project layer overrides global.
    assert_eq!(config.snapshot_policy, SnapshotPolicy::None);
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
