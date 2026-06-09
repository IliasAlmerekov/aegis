//! The unified pattern vocabulary shared by the scanner and policy engine.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::RiskLevel;

/// Whether a pattern was compiled into the binary or loaded from user config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternSource {
    /// Shipped with Aegis.
    Builtin,
    /// Loaded from `aegis.toml`.
    Custom,
}

/// Which class of operation the pattern guards against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "PascalCase")]
pub enum Category {
    /// File system operations (delete, overwrite, chmod, etc.).
    Filesystem,
    /// Git history rewrite operations.
    Git,
    /// Database DDL/DML operations.
    Database,
    /// Cloud-provider resource mutations.
    Cloud,
    /// Docker container and image management.
    Docker,
    /// Process and system-level operations.
    Process,
    /// Package manager operations.
    Package,
}

/// A sequence of pattern tokens to match against command tokens.
pub type PrefixPattern = Vec<PatternToken>;

/// One position in a prefix pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternToken {
    /// Exact single token match.
    Single(Cow<'static, str>),
    /// One of several alternative tokens.
    Alts(Vec<Cow<'static, str>>),
    /// Matches exactly one arbitrary token (like `.` in regex).
    Any,
    /// Matches zero or more arbitrary tokens (like `.*` in regex).
    AnyStar,
}

/// Unified runtime pattern.
///
/// Both built-in and user-defined patterns are normalized into the same
/// `Cow<'static, str>`-backed runtime representation.
///
/// This type can carry either borrowed static strings or owned runtime
/// strings, allowing scanner consumers to operate on one normalized shape
/// without depending on how a given pattern was materialized.
#[derive(Debug, Clone)]
pub struct Pattern {
    /// Unique pattern identifier (e.g. `"FS-001"`).
    pub id: Cow<'static, str>,
    /// Semantic category.
    pub category: Category,
    /// Assigned risk level when this pattern matches.
    pub risk: RiskLevel,
    /// Regex or literal pattern string.
    pub pattern: Cow<'static, str>,
    /// Human-readable description of what this pattern detects.
    pub description: Cow<'static, str>,
    /// Safer alternative to suggest, if any.
    pub safe_alt: Option<Cow<'static, str>>,
    /// Optional rationale for this pattern.
    pub justification: Option<Cow<'static, str>>,
    /// Whether the pattern is built-in or user-defined.
    pub source: PatternSource,
}
