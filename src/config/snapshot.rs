use serde::{Deserialize, Serialize};

/// Which Docker containers to include in snapshots.
///
/// - `Labeled` (default) — only containers with a specific label.
/// - `All`               — every running container (legacy blanket behaviour).
/// - `Names`             — containers whose name matches one of the given patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum DockerScopeMode {
    /// Only snapshot containers carrying the opt-in label (default).
    #[default]
    Labeled,
    /// Snapshot every running container — use with care on busy hosts.
    All,
    /// Snapshot containers whose name matches at least one pattern.
    Names,
}

/// Scoping rules that decide *which* Docker containers are eligible for snapshot.
///
/// Stored under `[docker_scope]` in `aegis.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DockerScope {
    /// Selection strategy.
    pub mode: DockerScopeMode,
    /// Label selector for `Labeled` mode.  The container must carry this label
    /// with value `"true"` to be eligible (e.g. `aegis.snapshot=true`).
    pub label: String,
    /// Name patterns for `Names` mode.  Each pattern is passed as a separate
    /// `--filter name=<pat>` argument to `docker ps` (Docker ORs them).
    pub name_patterns: Vec<String>,
}

impl Default for DockerScope {
    fn default() -> Self {
        Self {
            mode: DockerScopeMode::Labeled,
            label: "aegis.snapshot".to_string(),
            name_patterns: Vec::new(),
        }
    }
}

/// Connection settings for a PostgreSQL snapshot plugin.
///
/// Credentials are provided externally via `PGPASSWORD` or `~/.pgpass`.
/// The plugin is a no-op when [`database`](Self::database) is empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PostgresSnapshotConfig {
    /// PostgreSQL database name to snapshot.
    pub database: String,
    /// PostgreSQL host name or address.
    pub host: String,
    /// PostgreSQL port.
    pub port: u16,
    /// PostgreSQL user name.
    pub user: String,
}

impl Default for PostgresSnapshotConfig {
    fn default() -> Self {
        Self {
            database: String::new(),
            host: "localhost".to_string(),
            port: 5432,
            user: String::new(),
        }
    }
}

/// Configuration settings for the Supabase snapshot provider.
///
/// `project_ref` is advisory-only for audit/UI and future phases. The
/// rollback target-match setting is part of the bundle definition for later
/// rollback flows that compare the active config target with the manifest
/// target. Phase 1 uses the direct PostgreSQL transport in [`db`](Self::db).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SupabaseSnapshotConfig {
    /// Advisory-only Supabase project reference for audit/UI and future phases.
    pub project_ref: String,
    /// Target-match preference for rollback flows that compare config targets.
    pub require_config_target_match_on_rollback: bool,
    /// Phase 1 direct PostgreSQL transport for Supabase snapshots.
    pub db: PostgresSnapshotConfig,
}

impl Default for SupabaseSnapshotConfig {
    fn default() -> Self {
        Self {
            project_ref: String::new(),
            require_config_target_match_on_rollback: true,
            db: PostgresSnapshotConfig::default(),
        }
    }
}

/// Connection settings for a MySQL snapshot plugin.
///
/// Credentials are provided externally via `MYSQL_PWD` or `~/.my.cnf`.
/// The plugin is a no-op when [`database`](Self::database) is empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MysqlSnapshotConfig {
    /// MySQL database name to snapshot.
    pub database: String,
    /// MySQL host name or address.
    pub host: String,
    /// MySQL port.
    pub port: u16,
    /// MySQL user name.
    pub user: String,
}

impl Default for MysqlSnapshotConfig {
    fn default() -> Self {
        Self {
            database: String::new(),
            host: "localhost".to_string(),
            port: 3306,
            user: String::new(),
        }
    }
}
