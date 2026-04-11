use serde::Serialize;
use time::OffsetDateTime;

use crate::config::Config;
use crate::config::allowlist::{Allowlist, AllowlistSourceLayer, analyze_allowlist_rule};
use crate::error::AegisError;
use crate::interceptor;
use std::path::Path;

const PROJECT_CONFIG_FILE: &str = ".aegis.toml";
const GLOBAL_CONFIG_DIR: &str = ".config/aegis";
const GLOBAL_CONFIG_FILE: &str = "config.toml";

/// A single config validation issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationIssue {
    /// Stable machine-readable issue code.
    pub code: &'static str,
    /// Human-readable issue detail.
    pub message: String,
    /// Best-effort location of the issue.
    pub location: String,
}

/// Aggregated validation output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    /// True when there are no hard errors.
    pub valid: bool,
    /// Hard validation failures.
    pub errors: Vec<ValidationIssue>,
    /// Advisory warnings.
    pub warnings: Vec<ValidationIssue>,
}

/// Source-map metadata used to enrich issue locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigSourceMap {
    allowlist_locations: Vec<String>,
    scalar_source_path: String,
}

impl ConfigSourceMap {
    /// Build a source map for the effective config.
    pub fn for_config(config: &Config) -> Self {
        Self::for_config_with_paths(config, None, None)
    }

    /// Build a source map for the effective config using resolved config paths.
    pub fn for_config_with_paths(
        config: &Config,
        current_dir: Option<&Path>,
        home_dir: Option<&Path>,
    ) -> Self {
        let project_path = current_dir.map(|dir| dir.join(PROJECT_CONFIG_FILE));
        let global_path =
            home_dir.map(|home| home.join(GLOBAL_CONFIG_DIR).join(GLOBAL_CONFIG_FILE));

        let scalar_source_path = project_path
            .as_deref()
            .filter(|path| path.is_file())
            .or_else(|| global_path.as_deref().filter(|path| path.is_file()))
            .map(path_string)
            .unwrap_or_else(|| "defaults".to_string());

        let allowlist_locations = config
            .allowlist
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let layer = config
                    .allowlist_layers
                    .get(index)
                    .copied()
                    .unwrap_or(AllowlistSourceLayer::Project);
                let layer_name = match layer {
                    AllowlistSourceLayer::Global => global_path
                        .as_deref()
                        .map(path_string)
                        .unwrap_or_else(|| "global".to_string()),
                    AllowlistSourceLayer::Project => project_path
                        .as_deref()
                        .map(path_string)
                        .unwrap_or_else(|| "project".to_string()),
                };

                format!("{layer_name}:allowlist[{index}]")
            })
            .collect();

        Self {
            allowlist_locations,
            scalar_source_path,
        }
    }

    fn allowlist_location(&self, index: usize) -> String {
        self.allowlist_locations
            .get(index)
            .cloned()
            .unwrap_or_else(|| format!("allowlist[{index}]"))
    }

    fn scalar_location(&self, field: &str) -> String {
        format!("{}:{field}", self.scalar_source_path)
    }
}

/// Validate an effective config.
pub fn validate_config(config: &Config, source_map: &ConfigSourceMap) -> ValidationReport {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if config.audit.rotation_enabled && config.audit.max_file_size_bytes == 0 {
        errors.push(ValidationIssue {
            code: "audit_max_file_size",
            message:
                "audit.max_file_size_bytes must be greater than 0 when audit rotation is enabled"
                    .to_string(),
            location: source_map.scalar_location("audit.max_file_size_bytes"),
        });
    }

    if config.audit.rotation_enabled && config.audit.retention_files == 0 {
        errors.push(ValidationIssue {
            code: "audit_retention_files",
            message: "audit.retention_files must be greater than 0 when audit rotation is enabled"
                .to_string(),
            location: source_map.scalar_location("audit.retention_files"),
        });
    }

    let now = OffsetDateTime::now_utc();
    for (index, rule) in config.allowlist.iter().enumerate() {
        let location = source_map.allowlist_location(index);

        if rule.expires_at.is_some_and(|expires_at| expires_at <= now) {
            errors.push(ValidationIssue {
                code: "expired_rule",
                message: format!(
                    "allowlist rule '{}' is expired and cannot be used at runtime",
                    rule.pattern
                ),
                location: location.clone(),
            });
        }

        for warning in analyze_allowlist_rule(rule) {
            warnings.push(ValidationIssue {
                code: warning.code,
                message: warning.message,
                location: location.clone(),
            });
        }
    }

    if let Err(err) = interceptor::scanner_for(&config.custom_patterns) {
        errors.push(ValidationIssue {
            code: "invalid_custom_pattern",
            message: err.to_string(),
            location: "custom_patterns".to_string(),
        });
    }

    if let Err(err) = Allowlist::new(&config.layered_allowlist_rules()) {
        errors.push(ValidationIssue {
            code: "invalid_allowlist_rule",
            message: err.to_string(),
            location: "allowlist".to_string(),
        });
    }

    ValidationReport {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

/// Convert a load failure into a structured report.
pub fn validation_load_error(err: &AegisError) -> ValidationReport {
    let code = config_error_code(err);
    let location = config_error_location(err).unwrap_or_else(|| "config".to_string());
    ValidationReport {
        valid: false,
        errors: vec![ValidationIssue {
            code,
            message: err.to_string(),
            location,
        }],
        warnings: Vec::new(),
    }
}

fn config_error_location(err: &AegisError) -> Option<String> {
    let AegisError::Config(message) = err else {
        return None;
    };

    let invalid_prefix = "invalid config ";
    if let Some(value) = message.strip_prefix(invalid_prefix) {
        return value
            .split_once(':')
            .map(|(path, _)| path.trim().to_string())
            .filter(|path| !path.is_empty());
    }

    let parse_prefix = "failed to parse ";
    if let Some(value) = message.strip_prefix(parse_prefix) {
        return value
            .split_once(':')
            .map(|(path, _)| path.trim().to_string())
            .filter(|path| !path.is_empty());
    }

    None
}

fn config_error_code(err: &AegisError) -> &'static str {
    let AegisError::Config(message) = err else {
        return "config_load_error";
    };

    if message.contains("is expired") {
        return "expired_rule";
    }

    if message.contains("audit.max_file_size_bytes") {
        return "audit_max_file_size";
    }

    if message.contains("audit.retention_files") {
        return "audit_retention_files";
    }

    if message.contains("duplicate pattern id") {
        return "invalid_custom_pattern";
    }

    if message.contains("invalid allowlist rule")
        || message.contains("allowlist rule pattern")
        || message.contains("allowlist rule pattern must not be empty")
    {
        return "invalid_allowlist_rule";
    }

    "config_load_error"
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::{ConfigSourceMap, validate_config};
    use crate::config::{AllowlistRule, Config};
    use crate::error::AegisError;
    use tempfile::TempDir;
    use time::{Duration, OffsetDateTime};

    #[test]
    fn validate_reports_warning_for_broad_rule_without_scope() {
        let config = Config {
            allowlist: vec![AllowlistRule {
                pattern: "terraform destroy *".to_string(),
                cwd: None,
                user: None,
                expires_at: None,
                reason: "broad test rule".to_string(),
            }],
            ..Config::defaults()
        };

        let report = validate_config(&config, &ConfigSourceMap::for_config(&config));
        assert!(report.errors.is_empty());
        assert!(report.warnings.iter().any(|w| w.code == "missing_scope"));
    }

    #[test]
    fn validate_reports_error_for_expired_rule() {
        let config = Config {
            allowlist: vec![AllowlistRule {
                pattern: "terraform destroy -target=module.test.*".to_string(),
                cwd: None,
                user: None,
                expires_at: Some(OffsetDateTime::now_utc() - Duration::days(1)),
                reason: "expired test rule".to_string(),
            }],
            ..Config::defaults()
        };

        let report = validate_config(&config, &ConfigSourceMap::for_config(&config));
        assert!(!report.errors.is_empty());
        assert!(report.errors.iter().any(|e| e.code == "expired_rule"));
    }

    #[test]
    fn validate_reports_multiple_audit_errors() {
        let mut config = Config::defaults();
        config.audit.rotation_enabled = true;
        config.audit.max_file_size_bytes = 0;
        config.audit.retention_files = 0;

        let report = validate_config(&config, &ConfigSourceMap::for_config(&config));
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.code == "audit_max_file_size")
        );
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.code == "audit_retention_files")
        );
    }

    #[test]
    fn validate_uses_real_file_path_in_locations() {
        let home = TempDir::new().unwrap();
        let workspace = TempDir::new().unwrap();
        let config_path = workspace.path().join(".aegis.toml");
        std::fs::write(
            &config_path,
            r#"
[audit]
rotation_enabled = true
max_file_size_bytes = 0
retention_files = 0
[[allowlist]]
pattern = "terraform destroy *"
reason = "wide"
"#,
        )
        .unwrap();

        let config = Config::load_for_unvalidated(workspace.path(), Some(home.path())).unwrap();
        let source_map = ConfigSourceMap::for_config_with_paths(
            &config,
            Some(workspace.path()),
            Some(home.path()),
        );
        let report = validate_config(&config, &source_map);

        let config_path = config_path.to_string_lossy();
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.location.contains(config_path.as_ref()))
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.location.contains(config_path.as_ref()))
        );
    }

    #[test]
    fn validation_load_error_extracts_config_path() {
        let err = AegisError::Config("invalid config /tmp/work/.aegis.toml: bad value".to_string());
        let report = super::validation_load_error(&err);
        assert_eq!(report.errors[0].location, "/tmp/work/.aegis.toml");
    }
}
