use serde::Serialize;
use time::OffsetDateTime;

use crate::config::Config;
use crate::config::allowlist::{Allowlist, AllowlistSourceLayer, analyze_allowlist_rule};
use crate::error::AegisError;
use crate::interceptor;

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
}

impl ConfigSourceMap {
    /// Build a source map for the effective config.
    pub fn for_config(config: &Config) -> Self {
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
                    AllowlistSourceLayer::Global => "global",
                    AllowlistSourceLayer::Project => "project",
                };

                format!("{layer_name}.allowlist[{index}]")
            })
            .collect();

        Self {
            allowlist_locations,
        }
    }

    fn allowlist_location(&self, index: usize) -> String {
        self.allowlist_locations
            .get(index)
            .cloned()
            .unwrap_or_else(|| format!("allowlist[{index}]"))
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
            location: "audit.max_file_size_bytes".to_string(),
        });
    }

    if config.audit.rotation_enabled && config.audit.retention_files == 0 {
        errors.push(ValidationIssue {
            code: "audit_retention_files",
            message: "audit.retention_files must be greater than 0 when audit rotation is enabled"
                .to_string(),
            location: "audit.retention_files".to_string(),
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
    ValidationReport {
        valid: false,
        errors: vec![ValidationIssue {
            code,
            message: err.to_string(),
            location: "config".to_string(),
        }],
        warnings: Vec::new(),
    }
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

#[cfg(test)]
mod tests {
    use super::{ConfigSourceMap, validate_config};
    use crate::config::{AllowlistRule, Config};
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
}
