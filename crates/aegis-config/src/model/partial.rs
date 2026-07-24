use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::ConfigError;

use super::migration::migrate_deprecated_allowlist_in_file;
use super::ratchet::{
    ratchet_allow_write, ratchet_bool_loosen, ratchet_bool_tighten, ratchet_language_budget,
    ratchet_script_file_limit_bytes, ratchet_trusted_aliases,
};
use super::serde_helpers::{deserialize_allowlist_rules, deserialize_optional_config_version};
use super::{
    AllowlistOverrideLevel, AllowlistRule, AuditIntegrityMode, BlockRule, CiPolicy,
    ConfigSourceLayer, DockerScope, LanguageAnalysisConfig, Mode, MysqlSnapshotConfig, PolicyRule,
    PostgresSnapshotConfig, SandboxSettings, SnapshotPolicy, SupabaseSnapshotConfig, TrustedAlias,
    UserPattern,
};

type Result<T> = std::result::Result<T, ConfigError>;

/// Partial view of [`PruneConfig`] used during layered config merge.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PartialPruneConfig {
    pub(super) enabled: Option<bool>,
    pub(super) max_count_per_provider: Option<usize>,
    pub(super) max_age_days: Option<u32>,
}

/// Partial view of [`SandboxSettings`] used during layered config merge.
///
/// Allows individual sandbox fields to be set per-layer without resetting
/// fields that were not mentioned in a later layer.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PartialSandboxSettings {
    enabled: Option<bool>,
    required: Option<bool>,
    allow_write: Option<Vec<PathBuf>>,
    allow_network: Option<bool>,
}

impl PartialSandboxSettings {
    pub(super) fn merge_into(
        self,
        base: SandboxSettings,
        source_layer: ConfigSourceLayer,
    ) -> SandboxSettings {
        SandboxSettings {
            enabled: ratchet_bool_tighten(base.enabled, self.enabled, source_layer),
            required: ratchet_bool_tighten(base.required, self.required, source_layer),
            allow_write: ratchet_allow_write(
                &base.allow_write,
                self.allow_write.as_ref(),
                source_layer,
            ),
            allow_network: ratchet_bool_loosen(
                base.allow_network,
                self.allow_network,
                source_layer,
            ),
        }
    }

    pub(super) fn required(&self) -> Option<bool> {
        self.required
    }

    pub(super) fn enabled(&self) -> Option<bool> {
        self.enabled
    }

    pub(super) fn allow_network(&self) -> Option<bool> {
        self.allow_network
    }

    pub(super) fn allow_write(&self) -> Option<Vec<PathBuf>> {
        self.allow_write.clone()
    }
}

/// Partial view of [`LanguageAnalysisConfig`] used during layered config merge.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PartialLanguageAnalysisConfig {
    inline_source_limit_bytes: Option<u64>,
    script_file_limit_bytes: Option<u64>,
    max_script_files: Option<u64>,
    max_depth: Option<u64>,
    max_targets: Option<u64>,
    max_aggregate_bytes: Option<u64>,
    timeout_ms: Option<u64>,
    trusted_aliases: Option<Vec<TrustedAlias>>,
}

impl PartialLanguageAnalysisConfig {
    pub(super) fn merge_into(
        self,
        base: LanguageAnalysisConfig,
        source_layer: ConfigSourceLayer,
    ) -> LanguageAnalysisConfig {
        LanguageAnalysisConfig {
            inline_source_limit_bytes: ratchet_language_budget(
                base.inline_source_limit_bytes,
                self.inline_source_limit_bytes,
                super::rules::LANGUAGE_ANALYSIS_INLINE_SOURCE_MAX_BYTES,
                source_layer,
            ),
            script_file_limit_bytes: ratchet_script_file_limit_bytes(
                base.script_file_limit_bytes,
                self.script_file_limit_bytes,
                source_layer,
            ),
            max_script_files: ratchet_language_budget(
                base.max_script_files,
                self.max_script_files,
                super::rules::LANGUAGE_ANALYSIS_MAX_SCRIPT_FILES,
                source_layer,
            ),
            max_depth: ratchet_language_budget(
                base.max_depth,
                self.max_depth,
                super::rules::LANGUAGE_ANALYSIS_MAX_DEPTH,
                source_layer,
            ),
            max_targets: ratchet_language_budget(
                base.max_targets,
                self.max_targets,
                super::rules::LANGUAGE_ANALYSIS_MAX_TARGETS,
                source_layer,
            ),
            max_aggregate_bytes: ratchet_language_budget(
                base.max_aggregate_bytes,
                self.max_aggregate_bytes,
                super::rules::LANGUAGE_ANALYSIS_MAX_AGGREGATE_BYTES,
                source_layer,
            ),
            timeout_ms: ratchet_language_budget(
                base.timeout_ms,
                self.timeout_ms,
                super::rules::LANGUAGE_ANALYSIS_TIMEOUT_MS,
                source_layer,
            ),
            trusted_aliases: ratchet_trusted_aliases(
                &base.trusted_aliases,
                self.trusted_aliases.as_ref(),
                source_layer,
            ),
        }
    }

    pub(super) fn script_file_limit_bytes(&self) -> Option<u64> {
        self.script_file_limit_bytes
    }

    pub(super) fn trusted_aliases(&self) -> Option<Vec<TrustedAlias>> {
        self.trusted_aliases.clone()
    }

    pub(super) fn budget_fields(&self) -> [(&'static str, Option<u64>); 6] {
        [
            (
                "language_analysis.inline_source_limit_bytes",
                self.inline_source_limit_bytes,
            ),
            ("language_analysis.max_script_files", self.max_script_files),
            ("language_analysis.max_depth", self.max_depth),
            ("language_analysis.max_targets", self.max_targets),
            (
                "language_analysis.max_aggregate_bytes",
                self.max_aggregate_bytes,
            ),
            ("language_analysis.timeout_ms", self.timeout_ms),
        ]
    }
}

/// Partial config used for layered merging.
/// Scalar fields are `Option` so we can distinguish "not set" from "set to
/// the default value". Vec fields default to empty and are concatenated across
/// layers (global first, then project).
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PartialConfig {
    #[serde(default, deserialize_with = "deserialize_optional_config_version")]
    pub(super) config_version: Option<u32>,
    pub(super) mode: Option<Mode>,
    pub(super) custom_patterns: Vec<UserPattern>,
    #[serde(
        default,
        rename = "allow",
        alias = "allowlist",
        deserialize_with = "deserialize_allowlist_rules"
    )]
    pub(super) allowlist: Vec<AllowlistRule>,
    #[serde(default, rename = "block", alias = "blocklist")]
    pub(super) blocklist: Vec<BlockRule>,
    pub(super) allowlist_override_level: Option<AllowlistOverrideLevel>,
    pub(super) snapshot_policy: Option<SnapshotPolicy>,
    pub(super) auto_snapshot_git: Option<bool>,
    pub(super) auto_snapshot_docker: Option<bool>,
    pub(super) auto_snapshot_postgres: Option<bool>,
    pub(super) postgres_snapshot: Option<PostgresSnapshotConfig>,
    pub(super) auto_snapshot_mysql: Option<bool>,
    pub(super) mysql_snapshot: Option<MysqlSnapshotConfig>,
    pub(super) auto_snapshot_supabase: Option<bool>,
    pub(super) supabase_snapshot: Option<SupabaseSnapshotConfig>,
    pub(super) auto_snapshot_sqlite: Option<bool>,
    pub(super) sqlite_snapshot_path: Option<String>,
    pub(super) docker_scope: Option<DockerScope>,
    pub(super) ci_policy: Option<CiPolicy>,
    pub(super) audit: PartialAuditConfig,
    #[serde(default, rename = "rules")]
    pub(super) rules: Vec<PolicyRule>,
    pub(super) sandbox: PartialSandboxSettings,
    pub(super) prune: PartialPruneConfig,
    pub(super) language_analysis: PartialLanguageAnalysisConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct PartialAuditConfig {
    pub(super) rotation_enabled: Option<bool>,
    pub(super) max_file_size_bytes: Option<u64>,
    pub(super) retention_files: Option<usize>,
    pub(super) compress_rotated: Option<bool>,
    pub(super) integrity_mode: Option<AuditIntegrityMode>,
}

impl PartialConfig {
    pub(super) fn from_path(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)?;
        let config: Self = toml::from_str(&contents).map_err(|error| {
            ConfigError::Config(format!("failed to parse {}: {error}", path.display()))
        })?;

        let deprecated = contents.contains("[[allowlist]]") || contents.contains("allowlist = [");
        if deprecated {
            migrate_deprecated_allowlist_in_file(&contents, path, &config.allowlist)?;
        }

        Ok(config)
    }

    pub(super) fn sandbox_required(&self) -> Option<bool> {
        self.sandbox.required()
    }

    pub(super) fn sandbox_enabled(&self) -> Option<bool> {
        self.sandbox.enabled()
    }

    pub(super) fn sandbox_allow_network(&self) -> Option<bool> {
        self.sandbox.allow_network()
    }

    pub(super) fn sandbox_allow_write(&self) -> Option<Vec<PathBuf>> {
        self.sandbox.allow_write()
    }

    pub(super) fn language_analysis_script_file_limit_bytes(&self) -> Option<u64> {
        self.language_analysis.script_file_limit_bytes()
    }

    pub(super) fn language_analysis_trusted_aliases(&self) -> Option<Vec<TrustedAlias>> {
        self.language_analysis.trusted_aliases()
    }

    pub(super) fn language_analysis_budget_fields(&self) -> [(&'static str, Option<u64>); 6] {
        self.language_analysis.budget_fields()
    }
}
