use std::env;
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
mod migration;
mod partial;
mod ratchet;
mod rules;
mod serde_helpers;
mod template;

pub use enums::{AllowlistOverrideLevel, AuditIntegrityMode, CiPolicy, Mode, SnapshotPolicy};
pub use rules::{
    AllowlistRule, AuditConfig, BlockRule, PolicyPatternToken, PolicyRule, PolicyRuleDecision,
    UserPattern, WhenClause,
};

// Bring submodule items into the `model` namespace so they remain reachable
// from `model::tests` (which does `pub use super::*`) and from the merge logic
// below. These stay private aliases — visibility is unchanged.
use partial::PartialConfig;
use serde_helpers::{
    default_config_version, deserialize_allowlist_rules, deserialize_config_version,
};
use template::PROJECT_CONFIG_FILE;

const GLOBAL_CONFIG_DIR: &str = ".config/aegis";
const GLOBAL_CONFIG_FILE: &str = "config.toml";
/// Current configuration schema version.
pub const CURRENT_CONFIG_VERSION: u32 = 1;

type Result<T> = std::result::Result<T, ConfigError>;

fn merge_project_mode(base: Mode, overlay: Option<Mode>, layer: ConfigSourceLayer) -> Mode {
    let requested = overlay.unwrap_or(base);
    match layer {
        ConfigSourceLayer::Global => requested,
        ConfigSourceLayer::Project => most_restrictive_mode(base, requested),
    }
}

fn most_restrictive_mode(left: Mode, right: Mode) -> Mode {
    if mode_rank(right) >= mode_rank(left) {
        right
    } else {
        left
    }
}

fn mode_rank(mode: Mode) -> u8 {
    match mode {
        Mode::Audit => 0,
        Mode::Protect => 1,
        Mode::Strict => 2,
    }
}

fn merge_project_allowlist_override_level(
    base: AllowlistOverrideLevel,
    overlay: Option<AllowlistOverrideLevel>,
    layer: ConfigSourceLayer,
) -> AllowlistOverrideLevel {
    let requested = overlay.unwrap_or(base);
    match layer {
        ConfigSourceLayer::Global => requested,
        ConfigSourceLayer::Project => most_restrictive_allowlist_override_level(base, requested),
    }
}

fn most_restrictive_allowlist_override_level(
    left: AllowlistOverrideLevel,
    right: AllowlistOverrideLevel,
) -> AllowlistOverrideLevel {
    if allowlist_override_level_rank(right) >= allowlist_override_level_rank(left) {
        right
    } else {
        left
    }
}

fn allowlist_override_level_rank(level: AllowlistOverrideLevel) -> u8 {
    match level {
        AllowlistOverrideLevel::Danger => 0,
        AllowlistOverrideLevel::Warn => 1,
        AllowlistOverrideLevel::Never => 2,
    }
}

fn merge_project_ci_policy(
    base: CiPolicy,
    overlay: Option<CiPolicy>,
    layer: ConfigSourceLayer,
) -> CiPolicy {
    let requested = overlay.unwrap_or(base);
    match layer {
        ConfigSourceLayer::Global => requested,
        ConfigSourceLayer::Project => most_restrictive_ci_policy(base, requested),
    }
}

fn most_restrictive_ci_policy(left: CiPolicy, right: CiPolicy) -> CiPolicy {
    if ci_policy_rank(right) >= ci_policy_rank(left) {
        right
    } else {
        left
    }
}

fn ci_policy_rank(policy: CiPolicy) -> u8 {
    match policy {
        CiPolicy::Allow => 0,
        CiPolicy::Block => 1,
    }
}

fn merge_project_snapshot_policy(
    base: SnapshotPolicy,
    overlay: Option<SnapshotPolicy>,
    layer: ConfigSourceLayer,
) -> SnapshotPolicy {
    let requested = overlay.unwrap_or(base);
    match layer {
        ConfigSourceLayer::Global => requested,
        ConfigSourceLayer::Project => most_restrictive_snapshot_policy(base, requested),
    }
}

fn most_restrictive_snapshot_policy(left: SnapshotPolicy, right: SnapshotPolicy) -> SnapshotPolicy {
    if snapshot_policy_rank(right) >= snapshot_policy_rank(left) {
        right
    } else {
        left
    }
}

fn snapshot_policy_rank(policy: SnapshotPolicy) -> u8 {
    match policy {
        SnapshotPolicy::None => 0,
        SnapshotPolicy::Selective => 1,
        SnapshotPolicy::Full => 2,
    }
}

// The ratchet helpers live in `model::ratchet`; `merge_layer` below and the
// `partial` submodule both call them so the merge path and the warning
// collector share one definition of the effective `kept` value.
use ratchet::ratchet_bool_tighten;

/// A resolved config file path together with the layer it represents.
#[derive(Debug, Clone)]
pub struct ConfigLayerPath {
    /// Whether this path is the global or project config layer.
    pub source_layer: ConfigSourceLayer,
    /// Absolute path to the config file for this layer.
    pub path: PathBuf,
}

/// Sandbox configuration — controls whether commands run inside a bwrap sandbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
#[serde(default)]
pub struct SandboxSettings {
    /// Enable the sandbox layer.
    pub enabled: bool,
    /// Fail hard if the sandbox cannot be set up (instead of falling back to unsandboxed exec).
    pub required: bool,
    /// Paths the sandboxed process is allowed to write to.
    pub allow_write: Vec<PathBuf>,
    /// Whether the sandboxed process may access the network.
    pub allow_network: bool,
}

/// Prune configuration for `aegis snapshot prune`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
#[serde(default, deny_unknown_fields)]
pub struct PruneConfig {
    /// Whether prune is enabled. Currently advisory; manual `--yes` is still required.
    pub enabled: bool,
    /// Keep the newest N snapshots per provider. `None` disables the count rule.
    pub max_count_per_provider: Option<usize>,
    /// Keep snapshots newer than this many days. `None` disables the age rule.
    pub max_age_days: Option<u32>,
}

/// Top-level Aegis runtime configuration.
///
/// Loaded in order: built-in defaults → `~/.config/aegis/config.toml` (user-global)
/// → `.aegis.toml` (project). Later layers override ordinary scalar fields.
/// Project-local security-critical fields are ratcheted so they can only tighten,
/// never loosen: `mode`, `allowlist_override_level`, `ci_policy`,
/// `snapshot_policy`, `sandbox.enabled`/`required`/`allow_network`/`allow_write`,
/// and the `auto_snapshot_*` flags. `allow`/`block` rules are concatenated. See
/// ADR-013 for the full ordering.
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
    pub(crate) custom_pattern_layers: Vec<ConfigSourceLayer>,
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
    pub(crate) allowlist_layers: Vec<ConfigSourceLayer>,
    /// Structured block-list rules (TOML: `[[block]]`).
    #[serde(default, rename = "block", alias = "blocklist")]
    pub blocklist: Vec<BlockRule>,
    /// Per-rule provenance for `blocklist`. Internal; not serialized.
    #[serde(skip)]
    pub(crate) blocklist_layers: Vec<ConfigSourceLayer>,
    /// Which layer set `audit.max_file_size_bytes`. Internal; not serialized.
    #[serde(skip)]
    pub(crate) audit_max_file_size_bytes_source: Option<ConfigSourceLayer>,
    /// Which layer set `audit.retention_files`. Internal; not serialized.
    #[serde(skip)]
    pub(crate) audit_retention_files_source: Option<ConfigSourceLayer>,
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
    /// Typed policy rules (TOML: `[[rules]]`).
    #[serde(default, rename = "rules")]
    pub rules: Vec<PolicyRule>,
    /// Sandbox layer settings.
    pub sandbox: SandboxSettings,
    /// Snapshot prune retention settings.
    pub prune: PruneConfig,
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
            rules: Vec::new(),
            sandbox: SandboxSettings::default(),
            prune: PruneConfig::default(),
        }
    }

    /// Serialize the config to a pretty-printed TOML string.
    pub fn to_toml_string(&self) -> Result<String> {
        toml::to_string_pretty(self)
            .map_err(|error| ConfigError::Config(format!("failed to serialize config: {error}")))
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
        let ratchet_tighten =
            |base_val, overlay_val| ratchet_bool_tighten(base_val, overlay_val, allowlist_layer);
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
            mode: merge_project_mode(base.mode, overlay.mode, allowlist_layer),
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
            allowlist_override_level: merge_project_allowlist_override_level(
                base.allowlist_override_level,
                overlay.allowlist_override_level,
                allowlist_layer,
            ),
            snapshot_policy: merge_project_snapshot_policy(
                base.snapshot_policy,
                overlay.snapshot_policy,
                allowlist_layer,
            ),
            auto_snapshot_git: ratchet_tighten(base.auto_snapshot_git, overlay.auto_snapshot_git),
            auto_snapshot_docker: ratchet_tighten(
                base.auto_snapshot_docker,
                overlay.auto_snapshot_docker,
            ),
            auto_snapshot_postgres: ratchet_tighten(
                base.auto_snapshot_postgres,
                overlay.auto_snapshot_postgres,
            ),
            postgres_snapshot: overlay.postgres_snapshot.unwrap_or(base.postgres_snapshot),
            auto_snapshot_mysql: ratchet_tighten(
                base.auto_snapshot_mysql,
                overlay.auto_snapshot_mysql,
            ),
            mysql_snapshot: overlay.mysql_snapshot.unwrap_or(base.mysql_snapshot),
            auto_snapshot_supabase: ratchet_tighten(
                base.auto_snapshot_supabase,
                overlay.auto_snapshot_supabase,
            ),
            supabase_snapshot: overlay.supabase_snapshot.unwrap_or(base.supabase_snapshot),
            auto_snapshot_sqlite: ratchet_tighten(
                base.auto_snapshot_sqlite,
                overlay.auto_snapshot_sqlite,
            ),
            sqlite_snapshot_path: overlay
                .sqlite_snapshot_path
                .unwrap_or(base.sqlite_snapshot_path),
            docker_scope: overlay.docker_scope.unwrap_or(base.docker_scope),
            ci_policy: merge_project_ci_policy(base.ci_policy, overlay.ci_policy, allowlist_layer),
            rules: {
                let mut r = base.rules;
                r.extend(overlay.rules);
                r
            },
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
            sandbox: overlay.sandbox.merge_into(base.sandbox, allowlist_layer),
            prune: PruneConfig {
                enabled: overlay.prune.enabled.unwrap_or(base.prune.enabled),
                max_count_per_provider: overlay
                    .prune
                    .max_count_per_provider
                    .or(base.prune.max_count_per_provider),
                max_age_days: overlay.prune.max_age_days.or(base.prune.max_age_days),
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

        crate::validate::validate_policy_rules(&self.rules)
            .map_err(|(index, err)| ConfigError::Config(format!("rules[{index}]: {err}")))?;

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

#[cfg(test)]
mod tests;
