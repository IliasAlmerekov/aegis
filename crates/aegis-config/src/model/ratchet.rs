//! Project-config security ratchet: helpers that compute the effective
//! `kept` value for security-critical fields and the warning collector that
//! reports project-layer weakening attempts.
//!
//! The merge path (`merge_layer` / `PartialSandboxSettings::merge_into`) and
//! the warning collector (`AegisConfig::project_security_ratchet_warnings`)
//! call the SAME helpers (`ratchet_bool_tighten` / `ratchet_bool_loosen`) so
//! the reported `kept` value always matches what the merge actually does.

use super::partial::PartialConfig;
use super::{
    ConfigLayerPath, most_restrictive_allowlist_override_level, most_restrictive_ci_policy,
    most_restrictive_mode, most_restrictive_snapshot_policy,
};
use crate::allowlist::ConfigSourceLayer;
use crate::error::ConfigError;

type Result<T> = std::result::Result<T, ConfigError>;

/// Ratchet a boolean where `true` is the stricter value (`sandbox.enabled`,
/// `sandbox.required`, `auto_snapshot_*`). Under the Project layer the stricter
/// of base/requested wins (`base || requested`); Global stays last-layer-wins.
pub(super) fn ratchet_bool_tighten(
    base: bool,
    overlay: Option<bool>,
    layer: ConfigSourceLayer,
) -> bool {
    let requested = overlay.unwrap_or(base);
    match layer {
        ConfigSourceLayer::Global => requested,
        ConfigSourceLayer::Project => base || requested,
    }
}

/// Ratchet a boolean where `true` is the weaker value (`sandbox.allow_network`).
/// Under the Project layer the stricter of base/requested wins
/// (`base && requested`); Global stays last-layer-wins.
pub(super) fn ratchet_bool_loosen(
    base: bool,
    overlay: Option<bool>,
    layer: ConfigSourceLayer,
) -> bool {
    let requested = overlay.unwrap_or(base);
    match layer {
        ConfigSourceLayer::Global => requested,
        ConfigSourceLayer::Project => base && requested,
    }
}

/// A project-local config value attempted to weaken a security-critical setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SecurityRatchetWarning {
    pub(crate) field: &'static str,
    pub(crate) requested: String,
    pub(crate) kept: String,
    pub(crate) location: String,
}

/// Push a ratchet warning when the `kept` value differs from what the project
/// `requested`. Centralizing the `kept != requested` guard keeps the warning
/// collector in lock-step with the merge helpers that compute `kept`.
fn push_ratchet_warning(
    warnings: &mut Vec<SecurityRatchetWarning>,
    field: &'static str,
    requested: String,
    kept: String,
    location: &str,
) {
    if kept != requested {
        warnings.push(SecurityRatchetWarning {
            field,
            requested,
            kept,
            location: location.to_string(),
        });
    }
}

impl super::AegisConfig {
    /// Compare a project layer's requested values against the current base
    /// config and report any security-critical weakening attempts that the
    /// ratchet will ignore during merge.
    pub(crate) fn project_security_ratchet_warnings(
        base: &Self,
        layer: &ConfigLayerPath,
    ) -> Result<Vec<SecurityRatchetWarning>> {
        if layer.source_layer != ConfigSourceLayer::Project {
            return Ok(Vec::new());
        }

        let overlay = PartialConfig::from_path(&layer.path)?;
        let mut warnings = Vec::new();
        let location = layer.path.to_string_lossy().into_owned();

        if let Some(requested) = overlay.mode {
            let kept = most_restrictive_mode(base.mode, requested);
            push_ratchet_warning(
                &mut warnings,
                "mode",
                format!("{requested:?}"),
                format!("{kept:?}"),
                &location,
            );
        }

        if let Some(requested) = overlay.allowlist_override_level {
            let kept =
                most_restrictive_allowlist_override_level(base.allowlist_override_level, requested);
            push_ratchet_warning(
                &mut warnings,
                "allowlist_override_level",
                format!("{requested:?}"),
                format!("{kept:?}"),
                &location,
            );
        }

        if let Some(requested) = overlay.snapshot_policy {
            let kept = most_restrictive_snapshot_policy(base.snapshot_policy, requested);
            push_ratchet_warning(
                &mut warnings,
                "snapshot_policy",
                format!("{requested:?}"),
                format!("{kept:?}"),
                &location,
            );
        }

        if let Some(requested) = overlay.ci_policy {
            let kept = most_restrictive_ci_policy(base.ci_policy, requested);
            push_ratchet_warning(
                &mut warnings,
                "ci_policy",
                format!("{requested:?}"),
                format!("{kept:?}"),
                &location,
            );
        }

        if let Some(requested) = overlay.sandbox_required() {
            let kept = ratchet_bool_tighten(
                base.sandbox.required,
                Some(requested),
                ConfigSourceLayer::Project,
            );
            push_ratchet_warning(
                &mut warnings,
                "sandbox.required",
                requested.to_string(),
                kept.to_string(),
                &location,
            );
        }

        if let Some(requested) = overlay.sandbox_enabled() {
            let kept = ratchet_bool_tighten(
                base.sandbox.enabled,
                Some(requested),
                ConfigSourceLayer::Project,
            );
            push_ratchet_warning(
                &mut warnings,
                "sandbox.enabled",
                requested.to_string(),
                kept.to_string(),
                &location,
            );
        }

        if let Some(requested) = overlay.sandbox_allow_network() {
            let kept = ratchet_bool_loosen(
                base.sandbox.allow_network,
                Some(requested),
                ConfigSourceLayer::Project,
            );
            push_ratchet_warning(
                &mut warnings,
                "sandbox.allow_network",
                requested.to_string(),
                kept.to_string(),
                &location,
            );
        }

        if let Some(requested) = overlay.sandbox_allow_write() {
            let kept = base.sandbox.allow_write.clone();
            let weakened = requested
                .iter()
                .any(|path| !base.sandbox.allow_write.contains(path));
            if weakened {
                push_ratchet_warning(
                    &mut warnings,
                    "sandbox.allow_write",
                    format!("{requested:?}"),
                    format!("{kept:?}"),
                    &location,
                );
            }
        }

        for (field, base_value, requested_value) in [
            (
                "auto_snapshot_git",
                base.auto_snapshot_git,
                overlay.auto_snapshot_git,
            ),
            (
                "auto_snapshot_docker",
                base.auto_snapshot_docker,
                overlay.auto_snapshot_docker,
            ),
            (
                "auto_snapshot_postgres",
                base.auto_snapshot_postgres,
                overlay.auto_snapshot_postgres,
            ),
            (
                "auto_snapshot_mysql",
                base.auto_snapshot_mysql,
                overlay.auto_snapshot_mysql,
            ),
            (
                "auto_snapshot_supabase",
                base.auto_snapshot_supabase,
                overlay.auto_snapshot_supabase,
            ),
            (
                "auto_snapshot_sqlite",
                base.auto_snapshot_sqlite,
                overlay.auto_snapshot_sqlite,
            ),
        ] {
            if let Some(requested) = requested_value {
                let kept =
                    ratchet_bool_tighten(base_value, Some(requested), ConfigSourceLayer::Project);
                push_ratchet_warning(
                    &mut warnings,
                    field,
                    requested.to_string(),
                    kept.to_string(),
                    &location,
                );
            }
        }

        Ok(warnings)
    }
}
