//! Validation helpers for deduplication and conflict detection.

use super::AppendOutcome;
use crate::AegisConfig;
use crate::allowlist::ConfigSourceLayer;

/// Derive the config source layer from the path used for appending.
pub(super) fn config_layer_from_path(config_path: &std::path::Path) -> ConfigSourceLayer {
    if config_path.file_name().is_some_and(|n| n == ".aegis.toml") {
        ConfigSourceLayer::Project
    } else {
        ConfigSourceLayer::Global
    }
}

/// Check whether appending a rule to `target_table` would create a duplicate
/// or conflict against the parsed `config`.
///
/// Returns `Some(AppendOutcome)` when the caller should return early
/// (`SkippedDuplicate` or `Conflict`), or `None` when the append may proceed.
///
/// Duplicate detection requires `user.is_none()` because only auto-written
/// rules (no explicit user scope) are subject to deduplication.
/// Conflict detection ignores `user` entirely: a rule with the same pattern
/// and cwd in the opposite table is always reported as a conflict regardless
/// of user scope.
pub(super) fn check_dedup_and_conflict(
    config: &AegisConfig,
    pattern: &str,
    cwd: &str,
    target_table: super::TableKind,
    location: ConfigSourceLayer,
) -> Option<AppendOutcome> {
    match target_table {
        super::TableKind::Allow => {
            if config
                .allowlist
                .iter()
                .any(|r| r.pattern == pattern && r.cwd.as_deref() == Some(cwd) && r.user.is_none())
            {
                return Some(AppendOutcome::SkippedDuplicate);
            }
            if config
                .blocklist
                .iter()
                .any(|r| r.pattern == pattern && r.cwd.as_deref() == Some(cwd))
            {
                return Some(AppendOutcome::Conflict {
                    pattern: pattern.to_string(),
                    existing_location: location,
                });
            }
        }
        super::TableKind::Blocklist => {
            if config
                .blocklist
                .iter()
                .any(|r| r.pattern == pattern && r.cwd.as_deref() == Some(cwd) && r.user.is_none())
            {
                return Some(AppendOutcome::SkippedDuplicate);
            }
            if config
                .allowlist
                .iter()
                .any(|r| r.pattern == pattern && r.cwd.as_deref() == Some(cwd))
            {
                return Some(AppendOutcome::Conflict {
                    pattern: pattern.to_string(),
                    existing_location: location,
                });
            }
        }
    }
    None
}
