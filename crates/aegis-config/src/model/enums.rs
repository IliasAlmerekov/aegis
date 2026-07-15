use serde::{Deserialize, Serialize};

// The policy-configuration enums (`Mode`, `CiPolicy`, `SnapshotPolicy`,
// `AllowlistOverrideLevel`) live in `aegis-types` so the policy engine can
// consume them without depending on the config crate. Re-exported here so
// existing `crate::*` call sites stay stable.
pub use aegis_types::{AllowlistOverrideLevel, CiPolicy, Mode, SnapshotPolicy};

/// Audit log integrity protection mode.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, schemars::JsonSchema,
)]
#[serde(rename_all = "PascalCase")]
pub enum AuditIntegrityMode {
    /// No integrity chaining.
    Off,
    /// Chained SHA-256 integrity check (default).
    #[default]
    ChainSha256,
}
