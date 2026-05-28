use super::*;

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
