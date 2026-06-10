use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use aegis_types::{Pattern, PatternSource};

use super::AuditIntegrityMode;
use aegis_types::Category;
use aegis_types::RiskLevel;

/// A user-defined pattern loaded from `aegis.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UserPattern {
    /// Unique identifier for this pattern.
    pub id: String,
    /// Semantic category (e.g. Filesystem, Database).
    pub category: Category,
    /// Risk level assigned when this pattern matches.
    pub risk: RiskLevel,
    /// Regex or literal pattern string.
    pub pattern: String,
    /// Human-readable explanation of what this pattern detects.
    pub description: String,
    /// Safer alternative command to suggest, if any.
    pub safe_alt: Option<String>,
    /// Optional rationale for adding this pattern.
    pub justification: Option<String>,
}

/// Convert a config-layer [`UserPattern`] into the neutral [`Pattern`] consumed
/// by the scanner. This conversion lives at the config/orchestration boundary so
/// the scanner crate never depends on config-specific types.
impl From<UserPattern> for Pattern {
    fn from(user: UserPattern) -> Self {
        Pattern {
            id: Cow::Owned(user.id),
            category: user.category,
            risk: user.risk,
            pattern: Cow::Owned(user.pattern),
            description: Cow::Owned(user.description),
            safe_alt: user.safe_alt.map(Cow::Owned),
            justification: user.justification.map(Cow::Owned),
            source: PatternSource::Custom,
        }
    }
}

mod offset_datetime_option {
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    pub fn serialize<S>(value: &Option<OffsetDateTime>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(value) => serializer
                .serialize_some(&value.format(&Rfc3339).map_err(serde::ser::Error::custom)?),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<OffsetDateTime>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Option::<String>::deserialize(deserializer)?;
        value
            .map(|value| {
                OffsetDateTime::parse(&value, &Rfc3339).map_err(|error| {
                    D::Error::custom(format!("invalid RFC 3339 timestamp: {error}"))
                })
            })
            .transpose()
    }
}

/// A structured allowlist rule with optional scope, expiry, and rationale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AllowlistRule {
    /// Command pattern to allow.
    pub pattern: String,
    /// Optional working-directory scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Optional user scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Optional expiry timestamp (RFC 3339).
    #[serde(
        default,
        with = "offset_datetime_option",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub expires_at: Option<OffsetDateTime>,
    /// Human-readable reason for allowing this pattern.
    pub reason: String,
}

/// A structured block rule with optional scope, expiry, and rationale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BlockRule {
    /// Command pattern to block.
    pub pattern: String,
    /// Optional working-directory scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Optional user scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Optional expiry timestamp (RFC 3339).
    #[serde(
        default,
        with = "offset_datetime_option",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub expires_at: Option<OffsetDateTime>,
    /// Human-readable reason for blocking this pattern.
    pub reason: String,
}

/// Audit log rotation and integrity configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AuditConfig {
    /// Enable automatic audit log rotation.
    pub rotation_enabled: bool,
    /// Max audit file size in bytes before rotation.
    pub max_file_size_bytes: u64,
    /// Number of rotated audit files to retain.
    pub retention_files: usize,
    /// Compress rotated audit files with gzip.
    pub compress_rotated: bool,
    /// Integrity chaining mode for tamper evidence.
    pub integrity_mode: AuditIntegrityMode,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            rotation_enabled: false,
            max_file_size_bytes: 10 * 1024 * 1024,
            retention_files: 5,
            compress_rotated: true,
            integrity_mode: AuditIntegrityMode::ChainSha256,
        }
    }
}
