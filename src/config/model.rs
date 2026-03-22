use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AegisError;
use crate::interceptor::RiskLevel;
use crate::interceptor::patterns::Category;

const PROJECT_CONFIG_FILE: &str = ".aegis.toml";
const GLOBAL_CONFIG_DIR: &str = ".config/aegis";
const GLOBAL_CONFIG_FILE: &str = "config.toml";

const INIT_TEMPLATE: &str = r#"# Aegis project configuration.
mode = "Protect" # Protect = prompt/block (default). Audit and Strict are not yet implemented.

custom_patterns = [] # Extra user-defined patterns loaded for this project.
allowlist = [] # Commands matching these patterns are trusted and may skip prompts in future phases.

auto_snapshot_git = true # Create a Git snapshot before dangerous commands when possible.
auto_snapshot_docker = true # Create a Docker snapshot before dangerous commands when possible.
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
    pub auto_snapshot_git: bool,
    pub auto_snapshot_docker: bool,
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
            auto_snapshot_git: true,
            auto_snapshot_docker: true,
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

        let global = global_path
            .as_deref()
            .filter(|p| p.is_file())
            .map(PartialConfig::from_path)
            .transpose()?
            .unwrap_or_default();

        let project = if project_path.is_file() {
            match PartialConfig::from_path(&project_path) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(
                        path = %project_path.display(),
                        error = %e,
                        "project config is malformed — skipping, using global config and defaults"
                    );
                    PartialConfig::default()
                }
            }
        } else {
            PartialConfig::default()
        };

        Ok(Self::merge(global, project))
    }

    fn merge(global: PartialConfig, project: PartialConfig) -> Self {
        let defaults = Self::defaults();

        let mut custom_patterns = global.custom_patterns;
        custom_patterns.extend(project.custom_patterns);

        let mut allowlist = global.allowlist;
        allowlist.extend(project.allowlist);

        Self {
            mode: project.mode.or(global.mode).unwrap_or(defaults.mode),
            custom_patterns,
            allowlist,
            auto_snapshot_git: project
                .auto_snapshot_git
                .or(global.auto_snapshot_git)
                .unwrap_or(defaults.auto_snapshot_git),
            auto_snapshot_docker: project
                .auto_snapshot_docker
                .or(global.auto_snapshot_docker)
                .unwrap_or(defaults.auto_snapshot_docker),
        }
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
    auto_snapshot_git: Option<bool>,
    auto_snapshot_docker: Option<bool>,
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
        assert!(config.auto_snapshot_docker);
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
        assert!(config.auto_snapshot_docker); // default
        assert!(config.allowlist.is_empty());
    }

    // --- malformed project config ---

    #[test]
    fn malformed_project_config_falls_back_to_global() {
        // If the project config cannot be parsed, aegis must not abort — it
        // should skip the project config, apply the global config, and log a
        // warning.  This keeps aegis functional even when a developer
        // introduces a syntax error in their local .aegis.toml.
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

        // Must succeed (not propagate the parse error).
        let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();

        // Global values are applied because the malformed project file was skipped.
        assert_eq!(config.mode, Mode::Strict);
        assert!(!config.auto_snapshot_git);
        // Vec fields are empty because neither file contributed any entries.
        assert!(config.allowlist.is_empty());
        assert!(config.custom_patterns.is_empty());
    }
}
