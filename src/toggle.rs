use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::audit::{AuditEntry, AuditLogger, Decision};
use crate::config::allowlist::AllowlistSourceLayer;
use crate::config::{Config, model::ConfigLayerPath};
use crate::error::AegisError;
use crate::interceptor::RiskLevel;

type Result<T> = std::result::Result<T, AegisError>;

const TOGGLE_FILE_NAME: &str = "disabled";
const AEGIS_DIR_NAME: &str = ".aegis";

/// Disabled-state toggle status.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ToggleState {
    /// Aegis is active and will inspect commands.
    Enabled,
    /// Aegis is disabled because `~/.aegis/disabled` exists.
    Disabled,
}

/// Snapshot of toggle and config state for CLI output.
#[derive(Debug, Clone)]
pub struct ToggleStatusView {
    /// Current toggle state.
    pub state: ToggleState,
    /// Absolute path to the toggle flag.
    pub flag_path: PathBuf,
    /// Human-readable config location summary.
    pub config_status: String,
    /// Whether CI is overriding the local toggle.
    pub ci_override_active: bool,
}

/// Resolve the global toggle flag path under the current user's home directory.
pub fn disabled_flag_path() -> Result<PathBuf> {
    Ok(resolve_disabled_flag_path(home_dir()?))
}

/// Return `true` when the global toggle flag is present.
pub fn is_disabled() -> Result<bool> {
    is_disabled_at(&disabled_flag_path()?)
}

/// Create or refresh `~/.aegis/disabled` with a timestamp and caller PID.
///
/// Returns `true` when the flag already existed.
pub fn disable() -> Result<bool> {
    disable_at(&disabled_flag_path()?)
}

/// Remove `~/.aegis/disabled` if it exists.
///
/// Returns `true` when the flag was present and removed.
pub fn enable() -> Result<bool> {
    enable_at(&disabled_flag_path()?)
}

/// Read the current toggle state.
///
/// This is a convenience wrapper around [`is_disabled`].
pub fn status() -> Result<ToggleState> {
    Ok(if is_disabled()? {
        ToggleState::Disabled
    } else {
        ToggleState::Enabled
    })
}

/// Snapshot the toggle state and status text for display.
pub fn status_view(ci_override_active: bool) -> Result<ToggleStatusView> {
    Ok(ToggleStatusView {
        state: status()?,
        flag_path: disabled_flag_path()?,
        config_status: config_status()?,
        ci_override_active,
    })
}

/// Append an audit entry for a toggle command using the effective audit config.
pub fn append_toggle_audit_entry(command: &str) -> Result<()> {
    audit_logger()?.append(AuditEntry::new(
        command.to_string(),
        RiskLevel::Safe,
        Vec::new(),
        Decision::Approved,
        Vec::new(),
        None,
        None,
    ))
}

fn resolve_disabled_flag_path(home_dir: PathBuf) -> PathBuf {
    home_dir.join(AEGIS_DIR_NAME).join(TOGGLE_FILE_NAME)
}

fn home_dir() -> Result<PathBuf> {
    home_dir_optional().ok_or_else(|| {
        AegisError::Config("HOME is not set; cannot resolve ~/.aegis/disabled".to_string())
    })
}

fn home_dir_optional() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn is_disabled_at(path: &Path) -> Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(false)
        }
        Err(err) => Err(err.into()),
    }
}

/// Return a concise description of the active configuration location.
///
/// This reports the highest-precedence config file that exists, matching the
/// effective layer order used by config loading. When no config file exists,
/// it falls back to a defaults-only message.
pub fn config_status() -> Result<String> {
    let current_dir = env::current_dir()?;
    let layer_paths = Config::layer_paths_for(&current_dir, home_dir_optional().as_deref());

    Ok(config_status_for_layers(&layer_paths))
}

fn disable_at(path: &Path) -> Result<bool> {
    let was_present = match fs::metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(AegisError::Config(format!(
                    "toggle path {} exists but is not a file",
                    path.display()
                )));
            }
            true
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => return Err(err.into()),
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let contents = disabled_flag_contents()?;
    fs::write(path, contents)?;
    Ok(was_present)
}

fn enable_at(path: &Path) -> Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(AegisError::Config(format!(
                    "toggle path {} exists but is not a file",
                    path.display()
                )));
            }

            fs::remove_file(path)?;
            Ok(true)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

fn disabled_flag_contents() -> Result<String> {
    let timestamp = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|err| AegisError::Config(format!("failed to format toggle timestamp: {err}")))?;
    let pid = process::id();

    Ok(format!("timestamp={timestamp}\npid={pid}\n"))
}

fn audit_logger() -> Result<AuditLogger> {
    let config = Config::load_inspection()?;
    Ok(AuditLogger::from_audit_config(&config.audit))
}

fn config_status_for_layers(layers: &[ConfigLayerPath]) -> String {
    match layers.last() {
        Some(layer) => match layer.source_layer {
            AllowlistSourceLayer::Project => {
                format!("project config: {}", layer.path.display())
            }
            AllowlistSourceLayer::Global => {
                format!("global config: {}", layer.path.display())
            }
        },
        None => "defaults (no config file found)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn disabled_flag_path_resolves_under_home() {
        let home = TempDir::new().unwrap();

        let path = resolve_disabled_flag_path(home.path().to_path_buf());

        assert_eq!(path, home.path().join(".aegis").join("disabled"));
    }

    #[test]
    fn disable_and_enable_round_trip() {
        let home = TempDir::new().unwrap();
        let path = resolve_disabled_flag_path(home.path().to_path_buf());

        assert!(!disable_at(&path).unwrap());
        assert!(is_disabled_at(&path).unwrap());
        assert!(enable_at(&path).unwrap());
        assert!(!is_disabled_at(&path).unwrap());
        assert!(!enable_at(&path).unwrap());
    }

    #[test]
    fn disable_writes_timestamp_and_pid() {
        let home = TempDir::new().unwrap();
        let path = resolve_disabled_flag_path(home.path().to_path_buf());

        disable_at(&path).unwrap();
        let contents = fs::read_to_string(&path).unwrap();

        assert!(contents.starts_with("timestamp="));
        assert!(contents.contains("\npid="));
        assert!(contents.ends_with('\n'));
    }

    #[test]
    fn is_disabled_treats_malformed_parent_as_enabled() {
        let home = TempDir::new().unwrap();
        let parent = home.path().join(".aegis");
        fs::write(&parent, "not a directory").unwrap();
        let path = parent.join("disabled");

        assert!(!is_disabled_at(&path).unwrap());
    }

    #[test]
    fn config_status_prefers_project_over_global() {
        let global = ConfigLayerPath {
            source_layer: AllowlistSourceLayer::Global,
            path: PathBuf::from("/tmp/global-config.toml"),
        };
        let project = ConfigLayerPath {
            source_layer: AllowlistSourceLayer::Project,
            path: PathBuf::from("/tmp/project-config.toml"),
        };

        assert_eq!(
            config_status_for_layers(&[global, project]),
            "project config: /tmp/project-config.toml"
        );
    }
}
