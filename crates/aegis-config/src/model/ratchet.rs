//! Project-config security ratchet: helpers that compute the effective
//! `kept` value for security-critical fields and the warning collector that
//! reports project-layer weakening attempts.
//!
//! The merge path (`merge_layer` / `PartialSandboxSettings::merge_into`) and
//! the warning collector (`AegisConfig::project_security_ratchet_warnings`)
//! call the SAME helpers (`ratchet_bool_tighten` / `ratchet_bool_loosen` /
//! `ratchet_*` provider-target helpers / `ratchet_allow_write`) so the
//! reported `kept` value always matches what the merge actually does.

use std::path::PathBuf;

use super::partial::PartialConfig;
use super::{
    ConfigLayerPath, DockerScope, MysqlSnapshotConfig, PolicyRule, PolicyRuleDecision,
    PostgresSnapshotConfig, SnapshotPolicy, SupabaseSnapshotConfig,
    most_restrictive_allowlist_override_level, most_restrictive_ci_policy,
    most_restrictive_integrity_mode, most_restrictive_mode, most_restrictive_snapshot_policy,
};
use crate::allowlist::ConfigSourceLayer;
use crate::error::ConfigError;
use crate::snapshot::DockerScopeMode;

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

/// Ratchet `sandbox.allow_write` (a `Vec<PathBuf>` — more entries = weaker).
///
/// - Global layer: last-wins (`overlay` replaces `base` when present).
/// - Project layer: keep the intersection (`base` filtered to entries present
///   in `overlay`, preserving base order). This honors project tightening to a
///   subset (including the empty set) while preventing any expansion beyond the
///   trusted base.
pub(super) fn ratchet_allow_write(
    base: &[PathBuf],
    overlay: Option<&Vec<PathBuf>>,
    layer: ConfigSourceLayer,
) -> Vec<PathBuf> {
    match layer {
        ConfigSourceLayer::Global => overlay.cloned().unwrap_or_else(|| base.to_vec()),
        ConfigSourceLayer::Project => match overlay {
            None => base.to_vec(),
            Some(requested) => base
                .iter()
                .filter(|path| requested.contains(path))
                .cloned()
                .collect(),
        },
    }
}

/// Core ratchet for a provider's target config (`sqlite_snapshot_path`,
/// `postgres_snapshot`, `mysql_snapshot`, `supabase_snapshot`). Under the
/// Project layer, when the provider is ENABLED in the trusted base AND the
/// base target itself is enabled (non-no-op), a project overlay that would
/// disable/empty the target is rejected (keep base). Repointing to another
/// enabled (non-empty) target is permitted. Global stays last-wins.
///
/// `base_target_enabled` / `overlay_target_enabled` encode the per-provider
/// "target is a no-op" predicate (empty database / empty path).
fn ratchet_provider_target<T: Clone>(
    base: &T,
    overlay: Option<&T>,
    layer: ConfigSourceLayer,
    provider_enabled_in_base: bool,
    base_target_enabled: bool,
    overlay_target_enabled: impl Fn(&T) -> bool,
) -> T {
    match layer {
        ConfigSourceLayer::Global => overlay.cloned().unwrap_or_else(|| base.clone()),
        ConfigSourceLayer::Project => {
            // If the base did not enable the provider there is nothing to
            // protect — the project may enable + configure its own provider.
            // If the base target is itself a no-op there is equally nothing
            // to protect.
            if !provider_enabled_in_base || !base_target_enabled {
                return overlay.cloned().unwrap_or_else(|| base.clone());
            }
            match overlay {
                None => base.clone(),
                Some(o) if !overlay_target_enabled(o) => base.clone(),
                Some(o) => o.clone(),
            }
        }
    }
}

/// Ratchet the SQLite snapshot path. Target enabled = non-empty path.
pub(super) fn ratchet_sqlite_path(
    base: &String,
    overlay: Option<&String>,
    layer: ConfigSourceLayer,
    provider_enabled_in_base: bool,
) -> String {
    ratchet_provider_target(
        base,
        overlay,
        layer,
        provider_enabled_in_base,
        !base.is_empty(),
        |o| !o.is_empty(),
    )
}

/// Ratchet the PostgreSQL snapshot config. Target enabled = non-empty
/// `database`.
pub(super) fn ratchet_postgres_snapshot(
    base: &PostgresSnapshotConfig,
    overlay: Option<&PostgresSnapshotConfig>,
    layer: ConfigSourceLayer,
    provider_enabled_in_base: bool,
) -> PostgresSnapshotConfig {
    ratchet_provider_target(
        base,
        overlay,
        layer,
        provider_enabled_in_base,
        !base.database.is_empty(),
        |o| !o.database.is_empty(),
    )
}

/// Ratchet the MySQL snapshot config. Target enabled = non-empty `database`.
pub(super) fn ratchet_mysql_snapshot(
    base: &MysqlSnapshotConfig,
    overlay: Option<&MysqlSnapshotConfig>,
    layer: ConfigSourceLayer,
    provider_enabled_in_base: bool,
) -> MysqlSnapshotConfig {
    ratchet_provider_target(
        base,
        overlay,
        layer,
        provider_enabled_in_base,
        !base.database.is_empty(),
        |o| !o.database.is_empty(),
    )
}

/// Ratchet the Supabase snapshot config. Target enabled = non-empty
/// `db.database`.
pub(super) fn ratchet_supabase_snapshot(
    base: &SupabaseSnapshotConfig,
    overlay: Option<&SupabaseSnapshotConfig>,
    layer: ConfigSourceLayer,
    provider_enabled_in_base: bool,
) -> SupabaseSnapshotConfig {
    ratchet_provider_target(
        base,
        overlay,
        layer,
        provider_enabled_in_base,
        !base.db.database.is_empty(),
        |o| !o.db.database.is_empty(),
    )
}

/// Docker breadth rank (higher = broader): `All` = 2; `Labeled` = 1; `Names`
/// with non-empty `name_patterns` = 1; `Names` with empty `name_patterns` = 0
/// (no-op). Used only to detect a no-op base/overlay (rank 0) — structural
/// narrowing is decided by [`docker_scope_narrows`].
fn docker_breadth_rank(scope: &DockerScope) -> u8 {
    match scope.mode {
        DockerScopeMode::All => 2,
        DockerScopeMode::Labeled => 1,
        DockerScopeMode::Names => {
            if scope.name_patterns.is_empty() {
                0
            } else {
                1
            }
        }
    }
}

/// True iff every pattern in `base` is present (as a literal string) in
/// `overlay` — i.e. `overlay` is a literal-string superset of `base`.
fn patterns_superset(overlay: &[String], base: &[String]) -> bool {
    base.iter().all(|p| overlay.contains(p))
}

/// Whether `overlay` narrows or is incomparable with `base`'s eligible-container
/// set (so the project must not win). `base` is assumed non-no-op (caller guards
/// via [`docker_breadth_rank`]).
///
/// Semantics: only keep-or-broaden moves are permitted.
/// - `All` is the broadest mode; anything else narrows from `All`.
/// - `Labeled` ↔ `Labeled` with the SAME label is a keep (no narrowing);
///   a different label is incomparable.
/// - `Names` → `Names` is a broaden/keep iff every base pattern is present in
///   the overlay (overlay is a literal-string superset).
/// - Any cross-mode switch between `Labeled` and `Names` is incomparable.
fn docker_scope_narrows(base: &DockerScope, overlay: &DockerScope) -> bool {
    use DockerScopeMode::*;
    match (base.mode, overlay.mode) {
        (All, All) => false, // identical effective (label/patterns unused)
        (All, _) => true,    // narrowing from broadest
        (Labeled, All) => false,
        (Labeled, Labeled) => base.label != overlay.label, // different label = incomparable
        (Labeled, Names) => true,                          // incomparable mode switch
        (Names, All) => false,
        (Names, Labeled) => true, // incomparable mode switch
        (Names, Names) => !patterns_superset(&overlay.name_patterns, &base.name_patterns),
    }
}

/// Ratchet the Docker snapshot scope. Under the Project layer, when the docker
/// provider is ENABLED in the trusted base AND the base scope is not a no-op
/// (rank 0), a project overlay that NARROWS or is INCOMPARABLE with the base
/// eligible-container set is rejected (keep base + warn). Only keep-or-broaden
/// moves are permitted: `All` is the broadest; `Labeled` ↔ `Labeled` with the
/// same label is a keep; `Names` → `Names` whose overlay patterns are a
/// literal-string superset of the base patterns is a broaden/keep. Global stays
/// last-wins.
pub(super) fn ratchet_docker_scope(
    base: &DockerScope,
    overlay: Option<&DockerScope>,
    layer: ConfigSourceLayer,
    provider_enabled_in_base: bool,
) -> DockerScope {
    match layer {
        ConfigSourceLayer::Global => overlay.cloned().unwrap_or_else(|| base.clone()),
        ConfigSourceLayer::Project => {
            if !provider_enabled_in_base || docker_breadth_rank(base) == 0 {
                return overlay.cloned().unwrap_or_else(|| base.clone());
            }
            match overlay {
                None => base.clone(),
                Some(o) if docker_scope_narrows(base, o) => base.clone(),
                Some(o) => o.clone(),
            }
        }
    }
}

/// Predicate identifying a project-layer `[[rules]]` entry that attempts to
/// auto-approve (`decision = "Allow"`). Such entries are DROPPED at the project
/// merge (the project layer may only tighten via Prompt/Block, never auto-approve)
/// and surfaced as a ratchet warning. The merge path (`merge_layer` in
/// `model.rs`) and the warning collector below BOTH call this predicate so the
/// reported `kept` value ("dropped") always matches what the merge actually did.
/// Global-layer Allow entries are NOT filtered (global is trusted, last-wins).
pub(super) fn is_untrusted_allow(rule: &PolicyRule) -> bool {
    // A project-layer rule is an untrusted auto-approve if EITHER its top-level
    // `decision = "Allow"` OR its `when.then = "Allow"` — at runtime
    // `effective_decision` returns `when.then` when the env condition matches,
    // so a `decision = "prompt"` (or `"block"`) rule with `when.then = "allow"`
    // would silently auto-approve. Flag both shapes so the merge drops them and
    // the warning loop surfaces them (same predicate ⇒ parity preserved).
    rule.decision == PolicyRuleDecision::Allow
        || rule
            .when
            .as_ref()
            .is_some_and(|w| w.then == PolicyRuleDecision::Allow)
}

/// Whether a built-in snapshot provider is enabled in `base`. Under
/// `SnapshotPolicy::None` the registry materializes NO providers, so nothing
/// is ratcheted. Under `SnapshotPolicy::Full` the registry materializes every
/// built-in provider regardless of the per-plugin flags, so `Full` counts as
/// every provider enabled. Under `SnapshotPolicy::Selective` only providers
/// whose `auto_snapshot_*` flag is set are enabled.
pub(super) fn provider_enabled_in_base(
    base: &super::AegisConfig,
    auto_snapshot_flag: bool,
) -> bool {
    base.snapshot_policy != SnapshotPolicy::None
        && (base.snapshot_policy == SnapshotPolicy::Full || auto_snapshot_flag)
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

        // C3-residual Fix-2: `audit.integrity_mode` is ratcheted (stricter of
        // base/requested wins under the Project layer). Mirrors the `mode`
        // branch above — `push_ratchet_warning` only fires when `kept != requested`
        // so tightening and equal-value requests do NOT warn.
        if let Some(requested) = overlay.audit.integrity_mode {
            let kept = most_restrictive_integrity_mode(base.audit.integrity_mode, requested);
            push_ratchet_warning(
                &mut warnings,
                "audit.integrity_mode",
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
            let kept = ratchet_allow_write(
                &base.sandbox.allow_write,
                Some(&requested),
                ConfigSourceLayer::Project,
            );
            // Gate on genuine expansion (some requested path is outside the
            // trusted base) rather than `kept != requested` Debug-string
            // inequality, so a reordered-but-equal subset does not spuriously
            // warn.
            let weakened = requested
                .iter()
                .any(|p| !base.sandbox.allow_write.contains(p));
            if weakened {
                warnings.push(SecurityRatchetWarning {
                    field: "sandbox.allow_write",
                    requested: format!("{requested:?}"),
                    kept: format!("{kept:?}"),
                    location: location.clone(),
                });
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

        // C3-01: provider target config ratchet. Each helper is called with the
        // SAME arguments the merge uses, so `kept` here matches the merged value.
        if let Some(requested) = overlay.sqlite_snapshot_path.as_ref() {
            let enabled = provider_enabled_in_base(base, base.auto_snapshot_sqlite);
            let kept = ratchet_sqlite_path(
                &base.sqlite_snapshot_path,
                Some(requested),
                ConfigSourceLayer::Project,
                enabled,
            );
            push_ratchet_warning(
                &mut warnings,
                "sqlite_snapshot_path",
                format!("{requested:?}"),
                format!("{kept:?}"),
                &location,
            );
        }

        if let Some(requested) = overlay.postgres_snapshot.as_ref() {
            let enabled = provider_enabled_in_base(base, base.auto_snapshot_postgres);
            let kept = ratchet_postgres_snapshot(
                &base.postgres_snapshot,
                Some(requested),
                ConfigSourceLayer::Project,
                enabled,
            );
            push_ratchet_warning(
                &mut warnings,
                "postgres_snapshot",
                format!("{requested:?}"),
                format!("{kept:?}"),
                &location,
            );
        }

        if let Some(requested) = overlay.mysql_snapshot.as_ref() {
            let enabled = provider_enabled_in_base(base, base.auto_snapshot_mysql);
            let kept = ratchet_mysql_snapshot(
                &base.mysql_snapshot,
                Some(requested),
                ConfigSourceLayer::Project,
                enabled,
            );
            push_ratchet_warning(
                &mut warnings,
                "mysql_snapshot",
                format!("{requested:?}"),
                format!("{kept:?}"),
                &location,
            );
        }

        if let Some(requested) = overlay.supabase_snapshot.as_ref() {
            let enabled = provider_enabled_in_base(base, base.auto_snapshot_supabase);
            let kept = ratchet_supabase_snapshot(
                &base.supabase_snapshot,
                Some(requested),
                ConfigSourceLayer::Project,
                enabled,
            );
            push_ratchet_warning(
                &mut warnings,
                "supabase_snapshot",
                format!("{requested:?}"),
                format!("{kept:?}"),
                &location,
            );
        }

        if let Some(requested) = overlay.docker_scope.as_ref() {
            let enabled = provider_enabled_in_base(base, base.auto_snapshot_docker);
            let kept = ratchet_docker_scope(
                &base.docker_scope,
                Some(requested),
                ConfigSourceLayer::Project,
                enabled,
            );
            push_ratchet_warning(
                &mut warnings,
                "docker_scope",
                format!("{requested:?}"),
                format!("{kept:?}"),
                &location,
            );
        }

        // C3-residual Fix-1: each project-layer `[[rules]] decision = "Allow"`
        // is DROPPED at the merge (the project may not auto-approve via rules).
        // Uses the SAME `is_untrusted_allow` predicate as the merge path so the
        // warning fires iff a rule was actually dropped. `kept = "dropped"`
        // always differs from the requested representation, so every dropped
        // Allow surfaces a warning.
        for rule in &overlay.rules {
            if is_untrusted_allow(rule) {
                push_ratchet_warning(
                    &mut warnings,
                    "rules",
                    format!("Allow({:?})", rule.pattern),
                    "dropped".to_string(),
                    &location,
                );
            }
        }

        Ok(warnings)
    }
}
