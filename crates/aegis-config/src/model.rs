use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::allowlist::{
    Allowlist, Blocklist, ConfigSourceLayer, LayeredAllowlistRule, LayeredBlocklistRule,
};
use super::snapshot::{
    DockerScope, MysqlSnapshotConfig, PostgresSnapshotConfig, SupabaseSnapshotConfig,
};
use crate::error::ConfigError;

/// Validate that `patterns` compile into a working scanner.
///
/// Converts config-layer [`UserPattern`]s into the neutral `Pattern` shape and
/// builds an [`aegis_scanner::Scanner`] to surface regex/ID errors. This is the
/// config/scanner boundary — the scanner never sees config types.
pub(crate) fn validate_custom_patterns(patterns: &[UserPattern]) -> Result<()> {
    let converted: Vec<aegis_scanner::Pattern> = patterns.iter().cloned().map(Into::into).collect();
    aegis_scanner::PatternSet::from_sources(&converted)
        .and_then(aegis_scanner::Scanner::try_new)
        .map(|_| ())
        // Fold into `Config` (not a distinct variant) so the per-file path
        // wrapping in `validate_runtime_requirements_for_path` still applies.
        .map_err(|err| ConfigError::Config(err.to_string()))
}

mod enums;
mod rules;

pub use enums::{AllowlistOverrideLevel, AuditIntegrityMode, CiPolicy, Mode, SnapshotPolicy};
pub use rules::{AllowlistRule, AuditConfig, BlockRule, UserPattern};

const PROJECT_CONFIG_FILE: &str = ".aegis.toml";
const GLOBAL_CONFIG_DIR: &str = ".config/aegis";
const GLOBAL_CONFIG_FILE: &str = "config.toml";
/// Current configuration schema version.
pub const CURRENT_CONFIG_VERSION: u32 = 1;
const LEGACY_ALLOWLIST_REASON: &str = "migrated from legacy allowlist entry";

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

/// A resolved config file path together with the layer it represents.
#[derive(Debug, Clone)]
pub struct ConfigLayerPath {
    /// Whether this path is the global or project config layer.
    pub source_layer: ConfigSourceLayer,
    /// Absolute path to the config file for this layer.
    pub path: PathBuf,
}

/// Top-level Aegis runtime configuration.
///
/// Loaded in order: built-in defaults → `~/.config/aegis/config.toml` (user-global)
/// → `.aegis.toml` (project). Later layers override scalar fields; `allow`/`block`
/// rules are concatenated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AegisConfig {
    /// Schema version. Must equal [`CURRENT_CONFIG_VERSION`].
    #[serde(
        default = "default_config_version",
        deserialize_with = "deserialize_config_version"
    )]
    pub config_version: u32,
    /// Operating mode: `Protect`, `Audit`, or `Strict`.
    pub mode: Mode,
    /// Extra user-defined patterns merged with built-in patterns at runtime.
    pub custom_patterns: Vec<UserPattern>,
    /// Per-pattern provenance (which layer each `custom_patterns` entry came from). Internal; not serialized.
    #[serde(skip)]
    pub custom_pattern_layers: Vec<ConfigSourceLayer>,
    /// Structured allow-list rules (TOML: `[[allow]]`).
    #[serde(
        default,
        rename = "allow",
        alias = "allowlist",
        deserialize_with = "deserialize_allowlist_rules"
    )]
    pub allowlist: Vec<AllowlistRule>,
    /// Per-rule provenance for `allowlist`. Internal; not serialized.
    #[serde(skip)]
    pub allowlist_layers: Vec<ConfigSourceLayer>,
    /// Structured block-list rules (TOML: `[[block]]`).
    #[serde(default, rename = "block", alias = "blocklist")]
    pub blocklist: Vec<BlockRule>,
    /// Per-rule provenance for `blocklist`. Internal; not serialized.
    #[serde(skip)]
    pub blocklist_layers: Vec<ConfigSourceLayer>,
    /// Which layer set `audit.max_file_size_bytes`. Internal; not serialized.
    #[serde(skip)]
    pub audit_max_file_size_bytes_source: Option<ConfigSourceLayer>,
    /// Which layer set `audit.retention_files`. Internal; not serialized.
    #[serde(skip)]
    pub audit_retention_files_source: Option<ConfigSourceLayer>,
    /// Maximum risk level the allow-list may auto-approve in Protect/Strict mode.
    pub allowlist_override_level: AllowlistOverrideLevel,
    /// Controls which snapshot plugins run before dangerous commands.
    pub snapshot_policy: SnapshotPolicy,
    /// Enable Git snapshots before dangerous commands.
    pub auto_snapshot_git: bool,
    /// Enable Docker container snapshots before dangerous commands.
    pub auto_snapshot_docker: bool,
    /// Enable PostgreSQL snapshots before dangerous commands.
    pub auto_snapshot_postgres: bool,
    /// PostgreSQL connection details used when snapshotting is enabled.
    pub postgres_snapshot: PostgresSnapshotConfig,
    /// Enable MySQL snapshots before dangerous commands.
    pub auto_snapshot_mysql: bool,
    /// MySQL connection details used when snapshotting is enabled.
    pub mysql_snapshot: MysqlSnapshotConfig,
    /// Enable Supabase snapshots before dangerous commands.
    pub auto_snapshot_supabase: bool,
    /// Supabase snapshot provider settings used when snapshotting is enabled.
    /// Layered config replaces this provider config as a whole.
    pub supabase_snapshot: SupabaseSnapshotConfig,
    /// Enable SQLite snapshots before dangerous commands.
    pub auto_snapshot_sqlite: bool,
    /// Path to a SQLite database file, relative to the current working
    /// directory or absolute.
    pub sqlite_snapshot_path: String,
    /// Which Docker containers to include in snapshots.
    pub docker_scope: DockerScope,
    /// Behaviour when a CI environment is detected.
    pub ci_policy: CiPolicy,
    /// Audit log rotation and integrity settings.
    pub audit: AuditConfig,
}

impl Default for AegisConfig {
    fn default() -> Self {
        Self::defaults()
    }
}

impl AegisConfig {
    /// Load the effective config for the current working directory.
    pub fn load() -> Result<Self> {
        let current_dir = env::current_dir()?;
        let home_dir = env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);

        Self::load_for(&current_dir, home_dir.as_deref())
    }

    /// Load config without triggering runtime validation (for `aegis config show`).
    pub fn load_inspection() -> Result<Self> {
        let current_dir = env::current_dir()?;
        let home_dir = env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);

        Self::load_for_inspection(&current_dir, home_dir.as_deref())
    }

    /// Return the built-in default configuration.
    pub fn defaults() -> Self {
        Self {
            config_version: CURRENT_CONFIG_VERSION,
            mode: Mode::Protect,
            custom_patterns: Vec::new(),
            custom_pattern_layers: Vec::new(),
            allowlist: Vec::new(),
            allowlist_layers: Vec::new(),
            blocklist: Vec::new(),
            blocklist_layers: Vec::new(),
            audit_max_file_size_bytes_source: None,
            audit_retention_files_source: None,
            allowlist_override_level: AllowlistOverrideLevel::Warn,
            snapshot_policy: SnapshotPolicy::Selective,
            auto_snapshot_git: true,
            auto_snapshot_docker: false,
            auto_snapshot_postgres: false,
            postgres_snapshot: PostgresSnapshotConfig::default(),
            auto_snapshot_mysql: false,
            mysql_snapshot: MysqlSnapshotConfig::default(),
            auto_snapshot_supabase: false,
            supabase_snapshot: SupabaseSnapshotConfig::default(),
            auto_snapshot_sqlite: false,
            sqlite_snapshot_path: String::new(),
            docker_scope: DockerScope::default(),
            ci_policy: CiPolicy::Block,
            audit: AuditConfig::default(),
        }
    }

    /// Serialize the config to a pretty-printed TOML string.
    pub fn to_toml_string(&self) -> Result<String> {
        toml::to_string_pretty(self)
            .map_err(|error| ConfigError::Config(format!("failed to serialize config: {error}")))
    }

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

    /// Validate config invariants required before constructing runtime state.
    ///
    /// This covers semantic config checks plus scanner and allowlist
    /// compilation so direct `RuntimeContext::new` callers get the same
    /// fail-closed guarantees as file-loaded configs.
    pub fn validate_runtime_requirements(&self) -> Result<()> {
        self.validate()?;
        validate_custom_patterns(&self.custom_patterns)?;
        Allowlist::from_layered_rules(&self.layered_allowlist_rules()).map(|_| ())?;
        Blocklist::from_layered_rules(&self.layered_blocklist_rules()).map(|_| ())?;
        Ok(())
    }

    /// Load and validate config for a specific working directory and home dir.
    pub fn load_for(current_dir: &Path, home_dir: Option<&Path>) -> Result<Self> {
        Self::load_for_internal(current_dir, home_dir, true)
    }

    /// Load config for a specific directory without runtime validation.
    pub fn load_for_inspection(current_dir: &Path, home_dir: Option<&Path>) -> Result<Self> {
        Self::load_for_internal(current_dir, home_dir, false)
    }

    /// Resolve the ordered list of existing config layer files (global, then project).
    pub fn layer_paths_for(current_dir: &Path, home_dir: Option<&Path>) -> Vec<ConfigLayerPath> {
        let global_path = home_dir.map(|h| h.join(GLOBAL_CONFIG_DIR).join(GLOBAL_CONFIG_FILE));
        let project_path = current_dir.join(PROJECT_CONFIG_FILE);
        let mut layers = Vec::new();

        if let Some(path) = global_path.filter(|path| path.is_file()) {
            layers.push(ConfigLayerPath {
                source_layer: ConfigSourceLayer::Global,
                path,
            });
        }

        if project_path.is_file() {
            layers.push(ConfigLayerPath {
                source_layer: ConfigSourceLayer::Project,
                path: project_path,
            });
        }

        layers
    }

    /// Merge a single config layer file into `base` without runtime validation.
    pub fn merge_layer_path_unvalidated(base: Self, layer: &ConfigLayerPath) -> Result<Self> {
        let overlay = PartialConfig::from_path(&layer.path)?;
        Ok(Self::merge_layer(base, overlay, layer.source_layer))
    }

    fn load_for_internal(
        current_dir: &Path,
        home_dir: Option<&Path>,
        validate_runtime_requirements: bool,
    ) -> Result<Self> {
        let global_path = home_dir.map(|h| h.join(GLOBAL_CONFIG_DIR).join(GLOBAL_CONFIG_FILE));
        let project_path = current_dir.join(PROJECT_CONFIG_FILE);

        let mut merged = Self::defaults();

        if let Some(path) = global_path.as_deref().filter(|p| p.is_file()) {
            let global = PartialConfig::from_path(path)?;
            merged = Self::merge_layer(merged, global, ConfigSourceLayer::Global);
            if validate_runtime_requirements {
                merged.validate_runtime_requirements_for_path(path)?;
            }
        }

        if project_path.is_file() {
            let project = PartialConfig::from_path(&project_path)?;
            merged = Self::merge_layer(merged, project, ConfigSourceLayer::Project);
            if validate_runtime_requirements {
                merged.validate_runtime_requirements_for_path(&project_path)?;
            }
        }

        Ok(merged)
    }

    fn merge_layer(base: Self, overlay: PartialConfig, allowlist_layer: ConfigSourceLayer) -> Self {
        let mut custom_patterns = base.custom_patterns;
        let custom_pattern_count = overlay.custom_patterns.len();
        custom_patterns.extend(overlay.custom_patterns);

        let mut custom_pattern_layers = base.custom_pattern_layers;
        custom_pattern_layers.extend(std::iter::repeat_n(allowlist_layer, custom_pattern_count));

        let mut allowlist = base.allowlist;
        let allowlist_count = overlay.allowlist.len();
        allowlist.extend(overlay.allowlist);

        let mut allowlist_layers = base.allowlist_layers;
        allowlist_layers.extend(std::iter::repeat_n(allowlist_layer, allowlist_count));

        let mut blocklist = base.blocklist;
        let blocklist_count = overlay.blocklist.len();
        blocklist.extend(overlay.blocklist);

        let mut blocklist_layers = base.blocklist_layers;
        blocklist_layers.extend(std::iter::repeat_n(allowlist_layer, blocklist_count));

        Self {
            config_version: overlay.config_version.unwrap_or(base.config_version),
            mode: overlay.mode.unwrap_or(base.mode),
            custom_patterns,
            custom_pattern_layers,
            allowlist,
            allowlist_layers,
            blocklist,
            blocklist_layers,
            audit_max_file_size_bytes_source: if overlay.audit.max_file_size_bytes.is_some() {
                Some(allowlist_layer)
            } else {
                base.audit_max_file_size_bytes_source
            },
            audit_retention_files_source: if overlay.audit.retention_files.is_some() {
                Some(allowlist_layer)
            } else {
                base.audit_retention_files_source
            },
            allowlist_override_level: overlay
                .allowlist_override_level
                .unwrap_or(base.allowlist_override_level),
            snapshot_policy: overlay.snapshot_policy.unwrap_or(base.snapshot_policy),
            auto_snapshot_git: overlay.auto_snapshot_git.unwrap_or(base.auto_snapshot_git),
            auto_snapshot_docker: overlay
                .auto_snapshot_docker
                .unwrap_or(base.auto_snapshot_docker),
            auto_snapshot_postgres: overlay
                .auto_snapshot_postgres
                .unwrap_or(base.auto_snapshot_postgres),
            postgres_snapshot: overlay.postgres_snapshot.unwrap_or(base.postgres_snapshot),
            auto_snapshot_mysql: overlay
                .auto_snapshot_mysql
                .unwrap_or(base.auto_snapshot_mysql),
            mysql_snapshot: overlay.mysql_snapshot.unwrap_or(base.mysql_snapshot),
            auto_snapshot_supabase: overlay
                .auto_snapshot_supabase
                .unwrap_or(base.auto_snapshot_supabase),
            supabase_snapshot: overlay.supabase_snapshot.unwrap_or(base.supabase_snapshot),
            auto_snapshot_sqlite: overlay
                .auto_snapshot_sqlite
                .unwrap_or(base.auto_snapshot_sqlite),
            sqlite_snapshot_path: overlay
                .sqlite_snapshot_path
                .unwrap_or(base.sqlite_snapshot_path),
            docker_scope: overlay.docker_scope.unwrap_or(base.docker_scope),
            ci_policy: overlay.ci_policy.unwrap_or(base.ci_policy),
            audit: AuditConfig {
                rotation_enabled: overlay
                    .audit
                    .rotation_enabled
                    .unwrap_or(base.audit.rotation_enabled),
                max_file_size_bytes: overlay
                    .audit
                    .max_file_size_bytes
                    .unwrap_or(base.audit.max_file_size_bytes),
                retention_files: overlay
                    .audit
                    .retention_files
                    .unwrap_or(base.audit.retention_files),
                compress_rotated: overlay
                    .audit
                    .compress_rotated
                    .unwrap_or(base.audit.compress_rotated),
                integrity_mode: overlay
                    .audit
                    .integrity_mode
                    .unwrap_or(base.audit.integrity_mode),
            },
        }
    }

    fn validate(&self) -> Result<()> {
        if self.audit.rotation_enabled && self.audit.max_file_size_bytes == 0 {
            return Err(ConfigError::Config(
                "audit.max_file_size_bytes must be greater than 0 when audit rotation is enabled"
                    .to_string(),
            ));
        }

        if self.audit.rotation_enabled && self.audit.retention_files == 0 {
            return Err(ConfigError::Config(
                "audit.retention_files must be greater than 0 when audit rotation is enabled"
                    .to_string(),
            ));
        }

        let now = OffsetDateTime::now_utc();
        if let Some(rule) = self
            .allowlist
            .iter()
            .find(|rule| rule.expires_at.is_some_and(|expires_at| expires_at <= now))
        {
            return Err(ConfigError::Config(format!(
                "allowlist rule '{}' is expired and cannot be used at runtime",
                rule.pattern
            )));
        }

        if let Some(rule) = self
            .blocklist
            .iter()
            .find(|rule| rule.expires_at.is_some_and(|expires_at| expires_at <= now))
        {
            return Err(ConfigError::Config(format!(
                "blocklist rule '{}' is expired and cannot be used at runtime",
                rule.pattern
            )));
        }

        Ok(())
    }

    fn validate_runtime_requirements_for_path(&self, path: &Path) -> Result<()> {
        self.validate_runtime_requirements()
            .map_err(|err| match err {
                ConfigError::Config(message) => {
                    ConfigError::Config(format!("invalid config {}: {message}", path.display()))
                }
                other => other,
            })
    }

    /// Return the layered allowlist input annotated with source layer.
    ///
    /// This preserves per-rule provenance from the layered config merge so
    /// later allowlist compilation can distinguish project-vs-global entries
    /// while compiling the effective runtime matcher.
    pub fn layered_allowlist_rules(&self) -> Vec<LayeredAllowlistRule> {
        self.allowlist
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, rule)| {
                let source_layer = self
                    .allowlist_layers
                    .get(index)
                    .copied()
                    .unwrap_or(ConfigSourceLayer::Project);

                LayeredAllowlistRule { rule, source_layer }
            })
            .collect()
    }

    /// Return the layered blocklist input annotated with source layer.
    pub fn layered_blocklist_rules(&self) -> Vec<LayeredBlocklistRule> {
        self.blocklist
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, rule)| {
                let source_layer = self
                    .blocklist_layers
                    .get(index)
                    .copied()
                    .unwrap_or(ConfigSourceLayer::Project);

                LayeredBlocklistRule { rule, source_layer }
            })
            .collect()
    }
}

/// Partial config used for layered merging.
/// Scalar fields are `Option` so we can distinguish "not set" from "set to
/// the default value". Vec fields default to empty and are concatenated across
/// layers (global first, then project).
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PartialConfig {
    #[serde(default, deserialize_with = "deserialize_optional_config_version")]
    config_version: Option<u32>,
    mode: Option<Mode>,
    custom_patterns: Vec<UserPattern>,
    #[serde(
        default,
        rename = "allow",
        alias = "allowlist",
        deserialize_with = "deserialize_allowlist_rules"
    )]
    allowlist: Vec<AllowlistRule>,
    #[serde(default, rename = "block", alias = "blocklist")]
    blocklist: Vec<BlockRule>,
    allowlist_override_level: Option<AllowlistOverrideLevel>,
    snapshot_policy: Option<SnapshotPolicy>,
    auto_snapshot_git: Option<bool>,
    auto_snapshot_docker: Option<bool>,
    auto_snapshot_postgres: Option<bool>,
    postgres_snapshot: Option<PostgresSnapshotConfig>,
    auto_snapshot_mysql: Option<bool>,
    mysql_snapshot: Option<MysqlSnapshotConfig>,
    auto_snapshot_supabase: Option<bool>,
    supabase_snapshot: Option<SupabaseSnapshotConfig>,
    auto_snapshot_sqlite: Option<bool>,
    sqlite_snapshot_path: Option<String>,
    docker_scope: Option<DockerScope>,
    ci_policy: Option<CiPolicy>,
    audit: PartialAuditConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PartialAuditConfig {
    rotation_enabled: Option<bool>,
    max_file_size_bytes: Option<u64>,
    retention_files: Option<usize>,
    compress_rotated: Option<bool>,
    integrity_mode: Option<AuditIntegrityMode>,
}

/// Find the text bounds of a TOML array assignment `key = [...]`.
///
/// Returns `(start, end)` byte indices in `text` covering the entire
/// `key = [ ... ]` declaration, or `None` if not found.
fn find_toml_array_bounds(text: &str, key: &str) -> Option<(usize, usize)> {
    let prefix = format!("{key} = [");
    let start = text.find(&prefix)?;
    let mut depth = 1usize;
    let mut in_string = false;
    let mut in_literal = false;
    let mut escaped = false;

    for (i, ch) in text[start + prefix.len()..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_literal {
            if ch == '\'' {
                in_literal = false;
            }
            continue;
        }
        if ch == '\\' && !in_literal {
            escaped = true;
            continue;
        }
        if ch == '"' && !in_string {
            in_string = true;
        } else if ch == '"' && in_string {
            in_string = false;
        } else if ch == '\'' && !in_string {
            in_literal = true;
        } else if !in_string && !in_literal {
            match ch {
                '[' => depth += 1,
                ']' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some((start, start + prefix.len() + i + 1));
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Migrate deprecated `allowlist` syntax in a config file to `[[allow]]`.
///
/// Called after parsing succeeds so the file is known-valid TOML.
/// Replaces `[[allowlist]]` with `[[allow]]` and converts `allowlist = [...]`
/// to equivalent `[[allow]]` tables.  The write is atomic (temp file + rename).
fn migrate_deprecated_allowlist_in_file(
    contents: &str,
    path: &Path,
    migrated_rules: &[AllowlistRule],
) -> Result<()> {
    let mut new_contents = contents.to_string();
    let mut migrated = false;

    // 1. Replace deprecated table headers.
    if contents.contains("[[allowlist]]") {
        new_contents = new_contents.replace("[[allowlist]]", "[[allow]]");
        migrated = true;
    }

    // 2. Convert legacy string array to structured tables.
    if contents.contains("allowlist = [")
        && let Some((start, end)) = find_toml_array_bounds(contents, "allowlist")
    {
        let mut replacement = String::new();
        for rule in migrated_rules {
            let body = toml::to_string_pretty(rule).map_err(|error| {
                ConfigError::Config(format!("failed to serialize migrated rule: {error}"))
            })?;
            replacement.push_str(&format!("[[allow]]\n{body}"));
        }
        new_contents.replace_range(start..end, &replacement);
        migrated = true;
    }

    if migrated {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp_path = path.with_extension(format!("tmp.{pid}.{nanos}"));
        {
            let mut tmp = fs::File::create(&tmp_path)?;
            std::io::Write::write_all(&mut tmp, new_contents.as_bytes())?;
            tmp.sync_all()?;
        }
        fs::rename(&tmp_path, path).inspect_err(|_| {
            let _ = fs::remove_file(&tmp_path);
        })?;
        tracing::info!(
            "Migrated deprecated allowlist syntax to [[allow]] in {}",
            path.display()
        );
    }

    Ok(())
}

impl PartialConfig {
    fn from_path(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)?;
        let config: Self = toml::from_str(&contents).map_err(|error| {
            ConfigError::Config(format!("failed to parse {}: {error}", path.display()))
        })?;

        let deprecated = contents.contains("[[allowlist]]") || contents.contains("allowlist = [");
        if deprecated {
            migrate_deprecated_allowlist_in_file(&contents, path, &config.allowlist)?;
        }

        Ok(config)
    }
}

fn deserialize_allowlist_rules<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<AllowlistRule>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum AllowlistField {
        Structured(Vec<AllowlistRule>),
        Legacy(Vec<String>),
    }

    let field = Option::<AllowlistField>::deserialize(deserializer)?;
    Ok(match field {
        None => Vec::new(),
        Some(AllowlistField::Structured(rules)) => rules,
        Some(AllowlistField::Legacy(patterns)) => patterns
            .into_iter()
            .map(|pattern| AllowlistRule {
                pattern,
                cwd: None,
                user: None,
                expires_at: None,
                reason: LEGACY_ALLOWLIST_REASON.to_string(),
            })
            .collect(),
    })
}

fn default_config_version() -> u32 {
    CURRENT_CONFIG_VERSION
}

fn deserialize_config_version<'de, D>(deserializer: D) -> std::result::Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let version = u32::deserialize(deserializer)?;
    validate_config_version(version).map_err(serde::de::Error::custom)
}

fn deserialize_optional_config_version<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<u32>::deserialize(deserializer)?
        .map(validate_config_version)
        .transpose()
        .map_err(serde::de::Error::custom)
}

fn validate_config_version(version: u32) -> std::result::Result<u32, String> {
    match version.cmp(&CURRENT_CONFIG_VERSION) {
        std::cmp::Ordering::Equal => Ok(version),
        std::cmp::Ordering::Greater => Err(format!(
            "config_version {version} requires a newer version of Aegis \
             (this binary supports schema version {CURRENT_CONFIG_VERSION}).\n\
             To upgrade: install a newer Aegis release that supports schema {version}, \
             then run `aegis config validate` to confirm compatibility.\n\
             To downgrade the config to schema {CURRENT_CONFIG_VERSION}: \
             run `aegis config init` to regenerate a fresh config file."
        )),
        std::cmp::Ordering::Less => Err(format!(
            "config_version {version} is below the minimum supported version \
             ({CURRENT_CONFIG_VERSION}); run `aegis config init` to regenerate your config."
        )),
    }
}

#[cfg(test)]
mod tests;
