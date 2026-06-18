use std::fs;
use std::path::{Path, PathBuf};

use crate::error::ConfigError;

use super::AegisConfig;

pub(super) const PROJECT_CONFIG_FILE: &str = ".aegis.toml";

const INIT_TEMPLATE: &str = r#"# Aegis project configuration.
config_version = 1 # Schema version. Omit only when loading a pre-version legacy config for migration.
mode = "Protect" # Protect prompts on Warn/Danger, Audit is non-blocking audit-only, Strict blocks non-safe and indirect execution forms by default.

custom_patterns = [] # Extra user-defined patterns loaded for this project.
allowlist_override_level = "Warn" # Protect/Strict allowlist ceiling: Warn | Danger | Never.
# Warn auto-approves allowlisted Warn commands in Protect/Strict.
# Danger also auto-approves allowlisted Danger commands.
# Never disables allowlist auto-approval for non-safe commands.
# Block never bypasses in Protect/Strict.

# Structured allow rules use array-of-tables entries.
# Every runtime-effective allow rule must declare cwd or user scope.
# Legacy string-array allowlist entries stay readable for migration and
# inspection, but they are invalid for runtime until you add scope.
# [[allow]]
# pattern = "terraform destroy -target=module.test.*"
# cwd = "/srv/infra"
# user = "ci"
# expires_at = "2030-01-01T00:00:00Z"
# reason = "ephemeral test teardown"

# Structured block rules also use array-of-tables entries.
# [[block]]
# pattern = "rm -rf /"
# cwd = "/srv/infra"
# reason = "never allow recursive root deletion"

snapshot_policy = "Selective" # None = never snapshot, Selective = per-plugin flags below, Full = all plugins.
auto_snapshot_git = true # Create a Git snapshot before dangerous commands when possible (Selective only).
auto_snapshot_docker = false # Docker snapshot is opt-in (Selective only). Enable once you have tested rollback.
auto_snapshot_postgres = false # PostgreSQL snapshot before dangerous commands. Requires pg_dump on PATH and [postgres_snapshot] config.
auto_snapshot_mysql = false    # MySQL/MariaDB snapshot. Requires mysqldump on PATH and [mysql_snapshot] config.
auto_snapshot_supabase = false # Supabase project-level snapshot. Phase 1 captures a DB-only manifest bundle.
auto_snapshot_sqlite = false   # SQLite snapshot. Set sqlite_snapshot_path to your .db file path.
sqlite_snapshot_path = ""      # Path to SQLite database file (relative to the current working directory or absolute).

# PostgreSQL connection for snapshots. Credentials via PGPASSWORD env var or ~/.pgpass — never stored here.
[postgres_snapshot]
database = ""        # Database name to dump. Required when auto_snapshot_postgres = true.
host = "localhost"
port = 5432
user = ""            # Leave empty to use PGUSER env var or OS user.

# MySQL/MariaDB connection for snapshots. Credentials via MYSQL_PWD env var or ~/.my.cnf.
[mysql_snapshot]
database = ""        # Database name to dump. Required when auto_snapshot_mysql = true.
host = "localhost"
port = 3306
user = ""            # Leave empty to use MYSQL_USER env var or ~/.my.cnf.

# Supabase project-level snapshot settings. Phase 1 uses the direct PostgreSQL transport.
[supabase_snapshot]
project_ref = "" # Advisory-only project ref for audit/UI/future phases.
require_config_target_match_on_rollback = true # Fail closed if current config target differs from the manifest target.

[supabase_snapshot.db]
database = ""    # Direct PostgreSQL database name used by the Supabase provider.
host = "localhost"
port = 5432
user = ""

# Which Docker containers to include in snapshots.
# mode: Labeled (default) = only containers with opt-in label, All = every running container, Names = match by name pattern.
[docker_scope]
mode = "Labeled"
label = "aegis.snapshot" # Container must carry this label with value "true".
name_patterns = []       # Name patterns for Names mode (Docker regex, ORed).

# Prune retention for snapshot artifacts. Both rules are applied as a union:
# a snapshot is kept if it is within max_age_days OR among the newest
# max_count_per_provider for its provider. Set enabled = true and use
# `aegis snapshot prune` to preview or remove artifacts.
[prune]
enabled = false
max_count_per_provider = 10
max_age_days = 30

# CI policy: what to do when aegis detects it is running inside a CI environment.
# Block (default) — hard-block any non-safe command; no interactive dialog is shown.
# Allow           — pass-through; commands are executed without prompting (opt-in override).
ci_policy = "Block"

[audit]
# Rotate ~/.aegis/audit.jsonl after it grows beyond this many bytes.
# Rotation is disabled by default to preserve the historical single-file contract.
rotation_enabled = false
max_file_size_bytes = 10485760
retention_files = 5
compress_rotated = true
integrity_mode = "ChainSha256" # Off = no chain hashes, ChainSha256 = tamper-evident chained SHA-256.
"#;

type Result<T> = std::result::Result<T, ConfigError>;

impl AegisConfig {
    /// Return the starter `aegis.toml` template text.
    pub fn init_template() -> &'static str {
        INIT_TEMPLATE
    }

    /// Write the starter `aegis.toml` to `current_dir`. Returns the path to the
    /// new file. Errors if a config file already exists at that path.
    pub fn init_in(current_dir: &Path) -> Result<PathBuf> {
        let path = current_dir.join(PROJECT_CONFIG_FILE);
        if path.exists() {
            return Err(ConfigError::Config(format!(
                "config file already exists at {}",
                path.display()
            )));
        }

        fs::write(&path, Self::init_template())?;
        Ok(path)
    }
}
