use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AegisError;
use crate::interceptor;
use crate::interceptor::RiskLevel;
use crate::interceptor::patterns::Category;

const PROJECT_CONFIG_FILE: &str = ".aegis.toml";
const GLOBAL_CONFIG_DIR: &str = ".config/aegis";
const GLOBAL_CONFIG_FILE: &str = "config.toml";

const INIT_TEMPLATE: &str = r#"# Aegis project configuration.
mode = "Protect" # Protect=prompt/block, Audit=non-blocking audit-only, Strict=block non-safe by default.

custom_patterns = [] # Extra user-defined patterns loaded for this project.
allowlist = [] # Protect and Strict consult allowlist_override_level for Warn/Danger commands.
allowlist_override_level = "Warn" # Allowlisted Warn auto-approves; Danger requires Danger level.

auto_snapshot_git = true # Create a Git snapshot before dangerous commands when possible.
auto_snapshot_docker = false # Docker snapshot is opt-in. Enable once you have tested rollback in your environment.

# CI policy: what to do when aegis detects it is running inside a CI environment.
# Block (default) — hard-block any non-safe command; no interactive dialog is shown.
# Allow           — disable the CI-only hard block and fall back to normal policy flow.
ci_policy = "Block"

[audit]
# Rotate ~/.aegis/audit.jsonl after it grows beyond this many bytes.
# Rotation is disabled by default to preserve the historical single-file contract.
rotation_enabled = false
max_file_size_bytes = 10485760
retention_files = 5
compress_rotated = true
"#;

type Result<T> = std::result::Result<T, AegisError>;

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
/// `Allow` disables the CI-only hard-block and falls back to normal policy
/// evaluation. Non-safe commands may still prompt or block. Set this only in
/// `.aegis.toml`, not globally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum CiPolicy {
    /// Hard-block all non-safe commands. No dialog. Pipeline fails fast.
    #[default]
    Block,
    /// Disable the CI-only hard-block and fall back to normal policy
    /// evaluation. Non-safe commands may still prompt or block.
    Allow,
}

/// How much an allowlisted command may override the current policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub enum AllowlistOverrideLevel {
    /// Allowlisted `Warn` commands may auto-approve; `Danger` still prompts or blocks.
    #[default]
    Warn,
    /// Allowlisted `Warn` and `Danger` commands may auto-approve.
    Danger,
    /// Allowlist never changes the approval outcome for non-safe commands.
    Never,
}

impl AllowlistOverrideLevel {
    fn from_legacy_strict_allowlist_override(
        mode: Mode,
        strict_allowlist_override: bool,
    ) -> Option<Self> {
        match mode {
            Mode::Protect | Mode::Audit => None,
            Mode::Strict => {
                if strict_allowlist_override {
                    Some(Self::Danger)
                } else {
                    Some(Self::Never)
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AllowlistOverrideLevelOrigin {
    Default,
    Explicit,
    LegacyAlias,
    LegacyStrictDefault,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AegisConfig {
    pub mode: Mode,
    pub custom_patterns: Vec<UserPattern>,
    pub allowlist: Vec<String>,
    pub allowlist_override_level: AllowlistOverrideLevel,
    pub auto_snapshot_git: bool,
    pub auto_snapshot_docker: bool,
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
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            rotation_enabled: false,
            max_file_size_bytes: 10 * 1024 * 1024,
            retention_files: 5,
            compress_rotated: true,
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
            mode: Mode::Protect,
            custom_patterns: Vec::new(),
            allowlist: Vec::new(),
            allowlist_override_level: AllowlistOverrideLevel::Warn,
            auto_snapshot_git: true,
            auto_snapshot_docker: false,
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

    pub(crate) fn load_for(current_dir: &Path, home_dir: Option<&Path>) -> Result<Self> {
        let global_path = home_dir.map(|h| h.join(GLOBAL_CONFIG_DIR).join(GLOBAL_CONFIG_FILE));
        let project_path = current_dir.join(PROJECT_CONFIG_FILE);

        let mut merged = Self::defaults();
        let mut allowlist_override_level_origin = AllowlistOverrideLevelOrigin::Default;

        if let Some(path) = global_path.as_deref().filter(|p| p.is_file()) {
            let global = PartialConfig::from_path(path)?;
            (merged, allowlist_override_level_origin) =
                Self::merge_layer(merged, allowlist_override_level_origin, global);
            merged.validate_for_path(path)?;
            merged.validate_runtime_requirements_for_path(path)?;
        }

        if project_path.is_file() {
            let project = PartialConfig::from_path(&project_path)?;
            (merged, _) = Self::merge_layer(merged, allowlist_override_level_origin, project);
            merged.validate_for_path(&project_path)?;
            merged.validate_runtime_requirements_for_path(&project_path)?;
        }

        Ok(merged)
    }

    fn merge_layer(
        base: Self,
        base_allowlist_override_level_origin: AllowlistOverrideLevelOrigin,
        overlay: PartialConfig,
    ) -> (Self, AllowlistOverrideLevelOrigin) {
        let mut custom_patterns = base.custom_patterns;
        custom_patterns.extend(overlay.custom_patterns);

        let mut allowlist = base.allowlist;
        allowlist.extend(overlay.allowlist);

        let mode = overlay.mode.unwrap_or(base.mode);
        let (allowlist_override_level, allowlist_override_level_origin) =
            if let Some(level) = overlay.allowlist_override_level {
                (level, AllowlistOverrideLevelOrigin::Explicit)
            } else if let Some(strict_allowlist_override) = overlay.strict_allowlist_override {
                match AllowlistOverrideLevel::from_legacy_strict_allowlist_override(
                    mode,
                    strict_allowlist_override,
                ) {
                    Some(level) => (level, AllowlistOverrideLevelOrigin::LegacyAlias),
                    None => match overlay.mode {
                        Some(Mode::Protect | Mode::Audit)
                            if matches!(
                                base_allowlist_override_level_origin,
                                AllowlistOverrideLevelOrigin::LegacyAlias
                                    | AllowlistOverrideLevelOrigin::LegacyStrictDefault
                            ) =>
                        {
                            (
                                AllowlistOverrideLevel::Warn,
                                AllowlistOverrideLevelOrigin::Default,
                            )
                        }
                        _ => (
                            base.allowlist_override_level,
                            base_allowlist_override_level_origin,
                        ),
                    },
                }
            } else {
                match overlay.mode {
                    Some(Mode::Strict)
                        if base_allowlist_override_level_origin
                            == AllowlistOverrideLevelOrigin::Default =>
                    {
                        (
                            AllowlistOverrideLevel::Never,
                            AllowlistOverrideLevelOrigin::LegacyStrictDefault,
                        )
                    }
                    Some(Mode::Protect | Mode::Audit)
                        if matches!(
                            base_allowlist_override_level_origin,
                            AllowlistOverrideLevelOrigin::LegacyAlias
                                | AllowlistOverrideLevelOrigin::LegacyStrictDefault
                        ) =>
                    {
                        (
                            AllowlistOverrideLevel::Warn,
                            AllowlistOverrideLevelOrigin::Default,
                        )
                    }
                    _ => (
                        base.allowlist_override_level,
                        base_allowlist_override_level_origin,
                    ),
                }
            };

        (
            Self {
                mode,
                custom_patterns,
                allowlist,
                allowlist_override_level,
                auto_snapshot_git: overlay.auto_snapshot_git.unwrap_or(base.auto_snapshot_git),
                auto_snapshot_docker: overlay
                    .auto_snapshot_docker
                    .unwrap_or(base.auto_snapshot_docker),
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
                },
            },
            allowlist_override_level_origin,
        )
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

        Ok(())
    }

    fn validate_for_path(&self, path: &Path) -> Result<()> {
        self.validate().map_err(|err| match err {
            AegisError::Config(message) => {
                AegisError::Config(format!("invalid config {}: {message}", path.display()))
            }
            other => other,
        })
    }

    fn validate_runtime_requirements_for_path(&self, path: &Path) -> Result<()> {
        interceptor::scanner_for(&self.custom_patterns)
            .map(|_| ())
            .map_err(|err| match err {
                AegisError::Config(message) => {
                    AegisError::Config(format!("invalid config {}: {message}", path.display()))
                }
                other => other,
            })
    }
}

/// Partial config used for layered merging.
/// Scalar fields are `Option` so we can distinguish "not set" from "set to
/// the default value". Vec fields default to empty and are concatenated across
/// layers (global first, then project).
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PartialConfig {
    mode: Option<Mode>,
    custom_patterns: Vec<UserPattern>,
    allowlist: Vec<String>,
    allowlist_override_level: Option<AllowlistOverrideLevel>,
    strict_allowlist_override: Option<bool>,
    auto_snapshot_git: Option<bool>,
    auto_snapshot_docker: Option<bool>,
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
}

impl PartialConfig {
    fn from_path(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)?;
        toml::from_str(&contents).map_err(|error| {
            AegisError::Config(format!("failed to parse {}: {error}", path.display()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
allowlist = ["terraform destroy -target=module.test.*", "docker system prune --volumes"]
auto_snapshot_git = false
auto_snapshot_docker = true

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
allowlist = ["global-safe-cmd"]
auto_snapshot_git = false

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
allowlist = ["project-safe-cmd"]

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
        assert_eq!(
            config.allowlist,
            vec!["global-safe-cmd", "project-safe-cmd"]
        );
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
    fn allowlist_override_level_project_value_overrides_global() {
        let workspace = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
        fs::create_dir_all(&global_dir).unwrap();

        fs::write(
            global_dir.join(GLOBAL_CONFIG_FILE),
            "allowlist_override_level = \"Warn\"\n",
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
    fn legacy_strict_allowlist_override_is_accepted_as_alias() {
        let workspace = TempDir::new().unwrap();
        fs::write(
            workspace.path().join(PROJECT_CONFIG_FILE),
            "mode = \"Strict\"\nstrict_allowlist_override = true\n",
        )
        .unwrap();

        let config = AegisConfig::load_for(workspace.path(), None).unwrap();
        assert_eq!(
            config.allowlist_override_level,
            AllowlistOverrideLevel::Danger
        );
    }

    #[test]
    fn legacy_strict_allowlist_override_false_maps_to_never_in_strict_mode() {
        let workspace = TempDir::new().unwrap();
        fs::write(
            workspace.path().join(PROJECT_CONFIG_FILE),
            "mode = \"Strict\"\nstrict_allowlist_override = false\n",
        )
        .unwrap();

        let config = AegisConfig::load_for(workspace.path(), None).unwrap();
        assert_eq!(
            config.allowlist_override_level,
            AllowlistOverrideLevel::Never
        );
    }

    #[test]
    fn legacy_strict_allowlist_override_is_ignored_in_protect_mode() {
        let workspace = TempDir::new().unwrap();
        fs::write(
            workspace.path().join(PROJECT_CONFIG_FILE),
            "mode = \"Protect\"\nstrict_allowlist_override = false\n",
        )
        .unwrap();

        let config = AegisConfig::load_for(workspace.path(), None).unwrap();
        assert_eq!(
            config.allowlist_override_level,
            AllowlistOverrideLevel::Warn
        );
    }

    #[test]
    fn strict_mode_without_override_level_preserves_legacy_blocking_default() {
        let workspace = TempDir::new().unwrap();
        fs::write(
            workspace.path().join(PROJECT_CONFIG_FILE),
            "mode = \"Strict\"\n",
        )
        .unwrap();

        let config = AegisConfig::load_for(workspace.path(), None).unwrap();
        assert_eq!(
            config.allowlist_override_level,
            AllowlistOverrideLevel::Never
        );
    }

    #[test]
    fn explicit_global_override_level_survives_project_strict_mode_without_override() {
        let workspace = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
        fs::create_dir_all(&global_dir).unwrap();

        fs::write(
            global_dir.join(GLOBAL_CONFIG_FILE),
            "allowlist_override_level = \"Warn\"\n",
        )
        .unwrap();
        fs::write(
            workspace.path().join(PROJECT_CONFIG_FILE),
            "mode = \"Strict\"\n",
        )
        .unwrap();

        let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
        assert_eq!(
            config.allowlist_override_level,
            AllowlistOverrideLevel::Warn
        );
    }

    #[test]
    fn global_strict_default_does_not_leak_into_project_protect_mode() {
        let workspace = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
        fs::create_dir_all(&global_dir).unwrap();

        fs::write(global_dir.join(GLOBAL_CONFIG_FILE), "mode = \"Strict\"\n").unwrap();
        fs::write(
            workspace.path().join(PROJECT_CONFIG_FILE),
            "mode = \"Protect\"\n",
        )
        .unwrap();

        let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
        assert_eq!(
            config.allowlist_override_level,
            AllowlistOverrideLevel::Warn
        );
    }

    #[test]
    fn ignored_non_strict_legacy_alias_does_not_weaken_project_strict_mode() {
        let workspace = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
        fs::create_dir_all(&global_dir).unwrap();

        fs::write(
            global_dir.join(GLOBAL_CONFIG_FILE),
            "mode = \"Protect\"\nstrict_allowlist_override = false\n",
        )
        .unwrap();
        fs::write(
            workspace.path().join(PROJECT_CONFIG_FILE),
            "mode = \"Strict\"\n",
        )
        .unwrap();

        let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
        assert_eq!(
            config.allowlist_override_level,
            AllowlistOverrideLevel::Never
        );
    }

    #[test]
    fn ignored_project_legacy_alias_does_not_preserve_global_strict_semantics() {
        let workspace = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let global_dir = home.path().join(GLOBAL_CONFIG_DIR);
        fs::create_dir_all(&global_dir).unwrap();

        fs::write(global_dir.join(GLOBAL_CONFIG_FILE), "mode = \"Strict\"\n").unwrap();
        fs::write(
            workspace.path().join(PROJECT_CONFIG_FILE),
            "mode = \"Protect\"\nstrict_allowlist_override = false\n",
        )
        .unwrap();

        let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
        assert_eq!(
            config.allowlist_override_level,
            AllowlistOverrideLevel::Warn
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
}
