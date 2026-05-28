//! Advisory analysis helpers for allowlist and blocklist rules.

use super::{AllowlistWarning, BlocklistWarning};
use crate::config::{AllowlistRule, BlockRule};

/// Produce advisory warnings for one structured allowlist rule.
///
/// This analysis is informational only. It does not participate in
/// authoritative runtime allow/deny matching, which is performed exclusively
/// by the compiled [`crate::config::Allowlist`].
pub fn analyze_allowlist_rule(rule: &AllowlistRule) -> Vec<AllowlistWarning> {
    let mut warnings = Vec::new();
    let location = warning_location(rule);

    if !has_scope(rule.cwd.as_deref()) && !has_scope(rule.user.as_deref()) {
        warnings.push(AllowlistWarning {
            code: "missing_scope",
            message: "allowlist rule has no cwd or user scope".to_string(),
            location: location.clone(),
        });
    }

    if is_broad_pattern(rule.pattern.trim()) {
        warnings.push(AllowlistWarning {
            code: "broad_pattern",
            message:
                "allowlist rule uses wildcard matching that may be broader than intended and can span compound shell commands like `&&`, `;`, or `|`"
                    .to_string(),
            location,
        });
    }

    warnings
}

/// Produce advisory warnings for one structured blocklist rule.
pub fn analyze_blocklist_rule(rule: &BlockRule) -> Vec<BlocklistWarning> {
    let mut warnings = Vec::new();
    let location = block_warning_location(rule);

    if !has_scope(rule.cwd.as_deref()) && !has_scope(rule.user.as_deref()) {
        warnings.push(BlocklistWarning {
            code: "missing_scope",
            message:
                "blocklist rule blocks globally; consider adding cwd or user scope to narrow impact"
                    .to_string(),
            location: location.clone(),
        });
    }

    if is_broad_pattern(rule.pattern.trim()) {
        warnings.push(BlocklistWarning {
            code: "broad_pattern",
            message:
                "blocklist rule uses wildcard matching that may be broader than intended and can span compound shell commands like `&&`, `;`, or `|`"
                    .to_string(),
            location,
        });
    }

    warnings
}

fn has_scope(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn is_broad_pattern(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?')
}

fn warning_location(rule: &AllowlistRule) -> String {
    format!("allowlist:{}", rule.pattern.trim())
}

fn block_warning_location(rule: &BlockRule) -> String {
    format!("blocklist:{}", rule.pattern.trim())
}
