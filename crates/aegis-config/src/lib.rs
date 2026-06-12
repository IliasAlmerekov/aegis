#![deny(missing_docs)]

//! Aegis configuration: data model, layered loader, validation, JSON schema,
//! and decision-persistence (`amend`).
//!
//! Depends on `aegis-types` for the shared vocabulary and `aegis-scanner` to
//! validate that custom patterns compile. Errors are surfaced as the crate's
//! own [`ConfigError`]; the binary maps them onto its orchestration error type.

pub mod allowlist;
pub mod amend;
/// Typed configuration errors.
pub mod error;
/// Config data model — [`AegisConfig`] and related types.
pub mod model;
pub mod pattern_match;
pub mod snapshot;
pub mod validate;

pub use error::ConfigError;

pub use allowlist::{
    Allowlist, AllowlistContext, AllowlistMatch, AllowlistWarning, Blocklist, BlocklistMatch,
    BlocklistWarning, ConfigSourceLayer, LayeredAllowlistRule, LayeredBlocklistRule,
    analyze_allowlist_rule, analyze_blocklist_rule,
};
pub use amend::{
    AppendOutcome, active_config_path_for_append, append_allow_rule, append_block_rule,
};
pub use model::{
    AegisConfig, AllowlistOverrideLevel, AllowlistRule, AuditConfig, AuditIntegrityMode, BlockRule,
    CiPolicy, Mode, PolicyPatternToken, PolicyRule, PolicyRuleDecision, SnapshotPolicy,
    UserPattern, WhenClause,
};
pub use pattern_match::policy_pattern_matches;
pub use snapshot::{
    DockerScope, DockerScopeMode, MysqlSnapshotConfig, PostgresSnapshotConfig,
    SupabaseSnapshotConfig,
};
pub use validate::{
    ConfigSourceMap, ValidationIssue, ValidationReport, validate_config, validate_config_layers,
    validate_policy_rules, validation_load_error,
};
