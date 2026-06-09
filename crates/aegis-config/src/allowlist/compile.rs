//! Compilation helpers for allowlist and blocklist rules.

use regex::Regex;
use time::OffsetDateTime;

use super::{CompiledRule, ConfigSourceLayer, LayeredAllowlistRule, LayeredBlocklistRule};
use crate::error::ConfigError;

type Result<T> = std::result::Result<T, ConfigError>;

pub(crate) fn validate_scope_fields(
    rule_type: &str,
    cwd: Option<&str>,
    user: Option<&str>,
) -> Result<()> {
    if has_scope(cwd) || has_scope(user) {
        Ok(())
    } else {
        Err(ConfigError::Config(format!(
            "{rule_type} rule must declare cwd or user scope"
        )))
    }
}

pub(crate) fn validate_single_rule(rule: LayeredAllowlistRule) -> Result<()> {
    compile_rule(rule).map(|_| ())
}

pub(super) fn compile_rule(rule: LayeredAllowlistRule) -> Result<CompiledRule> {
    compile_fields(
        "allowlist",
        &rule.rule.pattern,
        &rule.rule.reason,
        rule.rule.cwd.as_deref(),
        rule.rule.user.as_deref(),
        rule.rule.expires_at,
        rule.source_layer,
    )
}

pub(super) fn compile_block_rule(rule: LayeredBlocklistRule) -> Result<CompiledRule> {
    compile_fields(
        "blocklist",
        &rule.rule.pattern,
        &rule.rule.reason,
        rule.rule.cwd.as_deref(),
        rule.rule.user.as_deref(),
        rule.rule.expires_at,
        rule.source_layer,
    )
}

fn compile_fields(
    rule_type: &str,
    pattern: &str,
    reason: &str,
    cwd: Option<&str>,
    user: Option<&str>,
    expires_at: Option<OffsetDateTime>,
    source_layer: ConfigSourceLayer,
) -> Result<CompiledRule> {
    let pattern = required_field(rule_type, "pattern", pattern)?;
    let reason = required_field(rule_type, "reason", reason)?;
    let cwd = optional_scope_field(rule_type, "cwd", cwd)?;
    let user = optional_scope_field(rule_type, "user", user)?;

    validate_scope_fields(rule_type, cwd.as_deref(), user.as_deref())?;

    let regex = Regex::new(&glob_to_regex(pattern)).map_err(|error| {
        ConfigError::Config(format!(
            "invalid {rule_type} rule pattern {:?}: {error}",
            pattern
        ))
    })?;

    Ok(CompiledRule {
        pattern: pattern.to_string(),
        cwd,
        user,
        expires_at,
        reason: reason.to_string(),
        source_layer,
        regex,
    })
}

fn required_field<'a>(rule_type: &str, field: &str, value: &'a str) -> Result<&'a str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ConfigError::Config(format!(
            "{rule_type} rule {field} must not be empty"
        )));
    }

    Ok(trimmed)
}

fn optional_scope_field(
    rule_type: &str,
    field: &str,
    value: Option<&str>,
) -> Result<Option<String>> {
    match value {
        Some(value) => Ok(Some(required_field(rule_type, field, value)?.to_string())),
        None => Ok(None),
    }
}

fn has_scope(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn glob_to_regex(pattern: &str) -> String {
    let mut regex = String::from("^");

    for ch in pattern.chars() {
        match ch {
            '*' => regex.push_str("[^;&|]+"),
            '?' => regex.push('.'),
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '[' | ']' | '\\' => {
                regex.push('\\');
                regex.push(ch);
            }
            _ => regex.push(ch),
        }
    }

    regex.push('$');
    regex
}
