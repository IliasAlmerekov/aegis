use std::path::Path;

use serde::Serialize;
use time::OffsetDateTime;

use crate::config::Config;
use crate::config::allowlist::{Allowlist, AllowlistSourceLayer, analyze_allowlist_rule};
use crate::error::AegisError;
use crate::interceptor;

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
    custom_pattern_locations: Vec<String>,
    audit_max_file_size_bytes_location: String,
    audit_retention_files_location: String,
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

        let allowlist_locations = vector_locations(
            config.allowlist.len(),
            &config.allowlist_layers,
            "allowlist",
            project_path.as_deref(),
            global_path.as_deref(),
        );

        let custom_pattern_locations = vector_locations(
            config.custom_patterns.len(),
            &config.custom_pattern_layers,
            "custom_patterns",
            project_path.as_deref(),
            global_path.as_deref(),
        );

        let audit_max_file_size_bytes_location = scalar_field_location(
            config.audit_max_file_size_bytes_source,
            "audit.max_file_size_bytes",
            project_path.as_deref(),
            global_path.as_deref(),
        );
        let audit_retention_files_location = scalar_field_location(
            config.audit_retention_files_source,
            "audit.retention_files",
            project_path.as_deref(),
            global_path.as_deref(),
        );

        Self {
            allowlist_locations,
            custom_pattern_locations,
            audit_max_file_size_bytes_location,
            audit_retention_files_location,
        }
    }

    fn allowlist_location(&self, index: usize) -> String {
        self.allowlist_locations
            .get(index)
            .cloned()
            .unwrap_or_else(|| format!("allowlist[{index}]"))
    }

    fn custom_pattern_location(&self, index: usize) -> String {
        self.custom_pattern_locations
            .get(index)
            .cloned()
            .unwrap_or_else(|| format!("custom_patterns[{index}]"))
    }

    fn audit_max_file_size_bytes_location(&self) -> String {
        self.audit_max_file_size_bytes_location.clone()
    }

    fn audit_retention_files_location(&self) -> String {
        self.audit_retention_files_location.clone()
    }
}

/// Validate file-backed config using the same layer-by-layer checkpoints as runtime loading.
pub fn validate_config_layers(current_dir: &Path, home_dir: Option<&Path>) -> ValidationReport {
    let mut report = ValidationReport {
        valid: true,
        errors: Vec::new(),
        warnings: Vec::new(),
    };

    let layer_paths = Config::layer_paths_for(current_dir, home_dir);
    let mut merged = Config::defaults();

    if layer_paths.is_empty() {
        let source_map =
            ConfigSourceMap::for_config_with_paths(&merged, Some(current_dir), home_dir);
        merge_report(&mut report, validate_config(&merged, &source_map));
        return report;
    }

    for layer in layer_paths {
        match Config::merge_layer_path_unvalidated(merged, &layer) {
            Ok(next) => {
                merged = next;
                let source_map =
                    ConfigSourceMap::for_config_with_paths(&merged, Some(current_dir), home_dir);
                let checkpoint = validate_config(&merged, &source_map);
                let checkpoint_has_errors = !checkpoint.errors.is_empty();
                merge_report(&mut report, checkpoint);
                if checkpoint_has_errors {
                    return report;
                }
            }
            Err(err) => {
                push_unique_issue(
                    &mut report.errors,
                    ValidationIssue {
                        code: config_load_error_code(&err),
                        message: err.to_string(),
                        location: layer.path.to_string_lossy().into_owned(),
                    },
                );
                report.valid = false;
                return report;
            }
        }
    }

    report.valid = report.errors.is_empty();
    report
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
            location: source_map.audit_max_file_size_bytes_location(),
        });
    }

    if config.audit.rotation_enabled && config.audit.retention_files == 0 {
        errors.push(ValidationIssue {
            code: "audit_retention_files",
            message: "audit.retention_files must be greater than 0 when audit rotation is enabled"
                .to_string(),
            location: source_map.audit_retention_files_location(),
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

    if let Some(issue) = custom_pattern_validation_issue(config, source_map) {
        errors.push(issue);
    }

    if let Some(issue) = first_invalid_allowlist_issue(config, source_map) {
        errors.push(issue);
    }

    ValidationReport {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

/// Convert a non-file validation failure into a structured report.
pub fn validation_load_error(err: &AegisError) -> ValidationReport {
    ValidationReport {
        valid: false,
        errors: vec![ValidationIssue {
            code: config_load_error_code(err),
            message: err.to_string(),
            location: "config".to_string(),
        }],
        warnings: Vec::new(),
    }
}

fn custom_pattern_validation_issue(
    config: &Config,
    source_map: &ConfigSourceMap,
) -> Option<ValidationIssue> {
    if let Err(err) = interceptor::scanner_for(&config.custom_patterns) {
        if config.custom_patterns.is_empty() {
            return Some(ValidationIssue {
                code: "scanner_init_error",
                message: err.to_string(),
                location: "builtin_scanner".to_string(),
            });
        }
    } else {
        return None;
    }

    for index in 0..config.custom_patterns.len() {
        if let Err(err) = interceptor::scanner_for(&config.custom_patterns[..=index]) {
            return Some(ValidationIssue {
                code: "invalid_custom_pattern",
                message: err.to_string(),
                location: source_map.custom_pattern_location(index),
            });
        }
    }

    None
}

fn first_invalid_allowlist_issue(
    config: &Config,
    source_map: &ConfigSourceMap,
) -> Option<ValidationIssue> {
    let layered_rules = config.layered_allowlist_rules();
    for index in 0..layered_rules.len() {
        if let Err(err) = Allowlist::new(&layered_rules[..=index]) {
            return Some(ValidationIssue {
                code: "invalid_allowlist_rule",
                message: err.to_string(),
                location: source_map.allowlist_location(index),
            });
        }
    }

    None
}

fn merge_report(target: &mut ValidationReport, incoming: ValidationReport) {
    for issue in incoming.errors {
        push_unique_issue(&mut target.errors, issue);
    }
    for issue in incoming.warnings {
        push_unique_issue(&mut target.warnings, issue);
    }
    target.valid = target.errors.is_empty();
}

fn push_unique_issue(issues: &mut Vec<ValidationIssue>, issue: ValidationIssue) {
    if issues.iter().any(|existing| {
        existing.code == issue.code
            && existing.location == issue.location
            && existing.message == issue.message
    }) {
        return;
    }

    issues.push(issue);
}

fn config_load_error_code(err: &AegisError) -> &'static str {
    match err {
        AegisError::Config(message) if message.starts_with("failed to parse ") => {
            "config_parse_error"
        }
        AegisError::Config(_) => "config_load_error",
        _ => "config_load_error",
    }
}

fn vector_locations(
    item_count: usize,
    layers: &[AllowlistSourceLayer],
    field: &str,
    project_path: Option<&Path>,
    global_path: Option<&Path>,
) -> Vec<String> {
    let mut global_index = 0usize;
    let mut project_index = 0usize;

    (0..item_count)
        .map(|index| {
            let layer = layers
                .get(index)
                .copied()
                .unwrap_or(AllowlistSourceLayer::Project);
            let local_index = match layer {
                AllowlistSourceLayer::Global => {
                    let current = global_index;
                    global_index += 1;
                    current
                }
                AllowlistSourceLayer::Project => {
                    let current = project_index;
                    project_index += 1;
                    current
                }
            };

            format!(
                "{}:{field}[{local_index}]",
                layer_location(layer, project_path, global_path)
            )
        })
        .collect()
}

fn layer_location(
    layer: AllowlistSourceLayer,
    project_path: Option<&Path>,
    global_path: Option<&Path>,
) -> String {
    match layer {
        AllowlistSourceLayer::Global => global_path
            .map(path_string)
            .unwrap_or_else(|| "global".to_string()),
        AllowlistSourceLayer::Project => project_path
            .map(path_string)
            .unwrap_or_else(|| "project".to_string()),
    }
}

fn scalar_field_location(
    source_layer: Option<AllowlistSourceLayer>,
    field: &str,
    project_path: Option<&Path>,
    global_path: Option<&Path>,
) -> String {
    match source_layer {
        Some(layer) => format!(
            "{}:{field}",
            layer_location(layer, project_path, global_path)
        ),
        None => format!("defaults:{field}"),
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::{ConfigSourceMap, validate_config, validate_config_layers};
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
        // After scope enforcement, an unscoped rule is a compile-time error.
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.code == "invalid_allowlist_rule")
        );
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

        let report = validate_config_layers(workspace.path(), Some(home.path()));

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
    fn validation_load_error_returns_structured_generic_code() {
        let err = AegisError::Config("invalid config".to_string());
        let report = super::validation_load_error(&err);
        assert_eq!(report.errors[0].location, "config");
        assert_eq!(report.errors[0].code, "config_load_error");
    }

    #[test]
    fn validate_scanner_path_runs_when_no_custom_patterns() {
        let config = Config::defaults();
        let report = validate_config(&config, &ConfigSourceMap::for_config(&config));
        assert!(
            !report
                .errors
                .iter()
                .any(|e| e.code == "invalid_custom_pattern")
        );
        assert!(!report.errors.iter().any(|e| e.code == "scanner_init_error"));
    }
}
