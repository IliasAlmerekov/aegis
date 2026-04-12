use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::allowlist::{Allowlist, AllowlistSourceLayer, LayeredAllowlistRule};
use crate::error::AegisError;
use crate::interceptor;
use crate::interceptor::RiskLevel;
use crate::interceptor::patterns::Category;

const PROJECT_CONFIG_FILE: &str = ".aegis.toml";
const GLOBAL_CONFIG_DIR: &str = ".config/aegis";
const GLOBAL_CONFIG_FILE: &str = "config.toml";
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

# Structured allowlist rules use array-of-tables entries.
# [[allowlist]]
# pattern = "terraform destroy -target=module.test.*"
# cwd = "/srv/infra"
# user = "ci"
# expires_at = "2030-01-01T00:00:00Z"
# reason = "ephemeral test teardown"

snapshot_policy = "Selective" # None = never snapshot, Selective = per-plugin flags below, Full = all plugins.
auto_snapshot_git = true # Create a Git snapshot before dangerous commands when possible (Selective only).
auto_snapshot_docker = false # Docker snapshot is opt-in (Selective only). Enable once you have tested rollback.

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
integrity_mode = "Off" # Off = no chain hashes, ChainSha256 = tamper-evident chained SHA-256.
"#;

type Result<T> = std::result::Result<T, AegisError>;

#[derive(Debug, Clone)]
pub(crate) struct ConfigLayerPath {
    pub source_layer: AllowlistSourceLayer,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum Mode {
    #[default]
    Protect,
    Audit,
    Strict,
}

/// What aegis does when it detects a CI environment.
///
/// `Block` is the safe default: no interactive TTY is available in CI, so
/// prompting would hang the pipeline.  Instead, non-safe commands are
/// hard-blocked and the pipeline fails fast with a clear error message.
///
/// `Allow` is an explicit opt-in override for cases where a project has
/// audited its CI pipeline and is confident that destructive commands are
/// intentional (e.g., a release script that runs `terraform destroy` in a
/// tear-down job).  Set this only in `.aegis.toml`, not globally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum CiPolicy {
    /// Hard-block all non-safe commands. No dialog. Pipeline fails fast.
    #[default]
    Block,
    /// Pass-through: commands run without prompting. Use only when you have
    /// deliberately reviewed the CI pipeline for destructive commands.
    Allow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum AuditIntegrityMode {
    #[default]
    Off,
    ChainSha256,
}

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

/// Controls when and how snapshot plugins run before dangerous commands.
///
/// - `None`      — never snapshot.
/// - `Selective` — only plugins enabled by `auto_snapshot_git` / `auto_snapshot_docker`.
/// - `Full`      — run every registered plugin regardless of per-plugin flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum SnapshotPolicy {
    /// Never create snapshots.
    None,
    /// Honour per-plugin flags (default — backwards-compatible).
    #[default]
    Selective,
    /// Run all snapshot plugins unconditionally.
    Full,
}

/// Maximum override level that structured allowlist rules may grant for
/// non-safe commands in Protect/Strict mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum AllowlistOverrideLevel {
    #[default]
    Warn,
    Danger,
    Never,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserPattern {
    pub id: String,
    pub category: Category,
    pub risk: RiskLevel,
    pub pattern: String,
    pub description: String,
    pub safe_alt: Option<String>,
}

mod offset_datetime_option {
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    pub fn serialize<S>(value: &Option<OffsetDateTime>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(value) => serializer
                .serialize_some(&value.format(&Rfc3339).map_err(serde::ser::Error::custom)?),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<OffsetDateTime>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<String>::deserialize(deserializer)?;
        value
            .map(|value| {
                OffsetDateTime::parse(&value, &Rfc3339).map_err(|error| {
                    D::Error::custom(format!("invalid RFC 3339 timestamp: {error}"))
                })
            })
            .transpose()
    }
}

/// A structured allowlist rule with optional scope, expiry, and rationale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowlistRule {
    pub pattern: String,
    pub cwd: Option<String>,
    pub user: Option<String>,
    #[serde(default, with = "offset_datetime_option")]
    pub expires_at: Option<OffsetDateTime>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AegisConfig {
    #[serde(
        default = "default_config_version",
        deserialize_with = "deserialize_config_version"
    )]
    pub config_version: u32,
    pub mode: Mode,
    pub custom_patterns: Vec<UserPattern>,
    #[serde(skip)]
    pub(crate) custom_pattern_layers: Vec<AllowlistSourceLayer>,
    #[serde(default, deserialize_with = "deserialize_allowlist_rules")]
    pub allowlist: Vec<AllowlistRule>,
    #[serde(skip)]
    pub(crate) allowlist_layers: Vec<AllowlistSourceLayer>,
    #[serde(skip)]
    pub(crate) audit_max_file_size_bytes_source: Option<AllowlistSourceLayer>,
    #[serde(skip)]
    pub(crate) audit_retention_files_source: Option<AllowlistSourceLayer>,
    pub allowlist_override_level: AllowlistOverrideLevel,
    pub snapshot_policy: SnapshotPolicy,
    pub auto_snapshot_git: bool,
    pub auto_snapshot_docker: bool,
    pub docker_scope: DockerScope,
    pub ci_policy: CiPolicy,
    pub audit: AuditConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuditConfig {
    pub rotation_enabled: bool,
    pub max_file_size_bytes: u64,
    pub retention_files: usize,
    pub compress_rotated: bool,
    pub integrity_mode: AuditIntegrityMode,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            rotation_enabled: false,
            max_file_size_bytes: 10 * 1024 * 1024,
            retention_files: 5,
            compress_rotated: true,
            integrity_mode: AuditIntegrityMode::Off,
        }
    }
}

impl Default for AegisConfig {
    fn default() -> Self {
        Self::defaults()
    }
}

impl AegisConfig {
    pub fn load() -> Result<Self> {
        let current_dir = env::current_dir()?;
        let home_dir = env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from);

        Self::load_for(&current_dir, home_dir.as_deref())
    }

    pub fn defaults() -> Self {
        Self {
            config_version: CURRENT_CONFIG_VERSION,
            mode: Mode::Protect,
            custom_patterns: Vec::new(),
            custom_pattern_layers: Vec::new(),
            allowlist: Vec::new(),
            allowlist_layers: Vec::new(),
            audit_max_file_size_bytes_source: None,
            audit_retention_files_source: None,
            allowlist_override_level: AllowlistOverrideLevel::Warn,
            snapshot_policy: SnapshotPolicy::Selective,
            auto_snapshot_git: true,
            auto_snapshot_docker: false,
            docker_scope: DockerScope::default(),
            ci_policy: CiPolicy::Block,
            audit: AuditConfig::default(),
        }
    }

    pub fn to_toml_string(&self) -> Result<String> {
        toml::to_string_pretty(self)
            .map_err(|error| AegisError::Config(format!("failed to serialize config: {error}")))
    }

    pub fn init_template() -> &'static str {
        INIT_TEMPLATE
    }

    pub fn init_in(current_dir: &Path) -> Result<PathBuf> {
        let path = current_dir.join(PROJECT_CONFIG_FILE);
        if path.exists() {
            return Err(AegisError::Config(format!(
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
    pub(crate) fn validate_runtime_requirements(&self) -> Result<()> {
        self.validate()?;
        interceptor::scanner_for(&self.custom_patterns).map(|_| ())?;
        Allowlist::new(&self.layered_allowlist_rules()).map(|_| ())?;
        Ok(())
    }

    pub(crate) fn load_for(current_dir: &Path, home_dir: Option<&Path>) -> Result<Self> {
        Self::load_for_internal(current_dir, home_dir, true)
    }

    pub(crate) fn layer_paths_for(
        current_dir: &Path,
        home_dir: Option<&Path>,
    ) -> Vec<ConfigLayerPath> {
        let global_path = home_dir.map(|h| h.join(GLOBAL_CONFIG_DIR).join(GLOBAL_CONFIG_FILE));
        let project_path = current_dir.join(PROJECT_CONFIG_FILE);
        let mut layers = Vec::new();

        if let Some(path) = global_path.filter(|path| path.is_file()) {
            layers.push(ConfigLayerPath {
                source_layer: AllowlistSourceLayer::Global,
                path,
            });
        }

        if project_path.is_file() {
            layers.push(ConfigLayerPath {
                source_layer: AllowlistSourceLayer::Project,
                path: project_path,
            });
        }

        layers
    }

    pub(crate) fn merge_layer_path_unvalidated(
        base: Self,
        layer: &ConfigLayerPath,
    ) -> Result<Self> {
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
            merged = Self::merge_layer(merged, global, AllowlistSourceLayer::Global);
            if validate_runtime_requirements {
                merged.validate_runtime_requirements_for_path(path)?;
            }
        }

        if project_path.is_file() {
            let project = PartialConfig::from_path(&project_path)?;
            merged = Self::merge_layer(merged, project, AllowlistSourceLayer::Project);
            if validate_runtime_requirements {
                merged.validate_runtime_requirements_for_path(&project_path)?;
            }
        }

        Ok(merged)
    }

    fn merge_layer(
        base: Self,
        overlay: PartialConfig,
        allowlist_layer: AllowlistSourceLayer,
    ) -> Self {
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

        Self {
            config_version: overlay.config_version.unwrap_or(base.config_version),
            mode: overlay.mode.unwrap_or(base.mode),
            custom_patterns,
            custom_pattern_layers,
            allowlist,
            allowlist_layers,
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
            return Err(AegisError::Config(
                "audit.max_file_size_bytes must be greater than 0 when audit rotation is enabled"
                    .to_string(),
            ));
        }

        if self.audit.rotation_enabled && self.audit.retention_files == 0 {
            return Err(AegisError::Config(
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
            return Err(AegisError::Config(format!(
                "allowlist rule '{}' is expired and cannot be used at runtime",
                rule.pattern
            )));
        }

        Ok(())
    }

    fn validate_runtime_requirements_for_path(&self, path: &Path) -> Result<()> {
        self.validate_runtime_requirements()
            .map_err(|err| match err {
                AegisError::Config(message) => {
                    AegisError::Config(format!("invalid config {}: {message}", path.display()))
                }
                other => other,
            })
    }

    pub(crate) fn layered_allowlist_rules(&self) -> Vec<LayeredAllowlistRule> {
        self.allowlist
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, rule)| {
                let source_layer = self
                    .allowlist_layers
                    .get(index)
                    .copied()
                    .unwrap_or(AllowlistSourceLayer::Project);

                LayeredAllowlistRule { rule, source_layer }
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
    #[serde(default, deserialize_with = "deserialize_allowlist_rules")]
    allowlist: Vec<AllowlistRule>,
    allowlist_override_level: Option<AllowlistOverrideLevel>,
    snapshot_policy: Option<SnapshotPolicy>,
    auto_snapshot_git: Option<bool>,
    auto_snapshot_docker: Option<bool>,
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

impl PartialConfig {
    fn from_path(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)?;
        toml::from_str(&contents).map_err(|error| {
            AegisError::Config(format!("failed to parse {}: {error}", path.display()))
        })
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
    if version == CURRENT_CONFIG_VERSION {
        Ok(version)
    } else {
        Err(format!(
            "unsupported config_version {version}; supported version is {CURRENT_CONFIG_VERSION}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    #[test]
    fn structured_allowlist_rule_deserializes() {
        let config: AegisConfig = toml::from_str(
            r#"
allowlist_override_level = "Warn"

[[allowlist]]
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
    fn unsupported_config_version_is_rejected() {
        let err = toml::from_str::<AegisConfig>("config_version = 99").unwrap_err();
        assert!(err.to_string().contains("unsupported config_version"));
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

[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
reason = "global terraform teardown"
expires_at = "2030-01-01T00:00:00Z"

[[allowlist]]
pattern = "docker system prune --volumes"
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

[[allowlist]]
pattern = "global-safe-cmd"
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
[[allowlist]]
pattern = "project-safe-cmd"
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
            message.contains("audit.max_file_size_bytes")
                || message.contains("audit.retention_files")
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
[[allowlist]]
pattern = "   "
reason = "valid reason"
expires_at = "2030-01-01T00:00:00Z"
"#,
                "pattern must not be empty",
            ),
            (
                "reason",
                r#"
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
reason = "   "
expires_at = "2030-01-01T00:00:00Z"
"#,
                "reason must not be empty",
            ),
            (
                "cwd",
                r#"
[[allowlist]]
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
[[allowlist]]
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
            "template must not define an empty array that conflicts with [[allowlist]] entries"
        );
        assert!(
            template.contains("[[allowlist]]"),
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
}
