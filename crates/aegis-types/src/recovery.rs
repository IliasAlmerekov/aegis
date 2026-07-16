//! Recovery backstop degradation recorded in the audit log (ADR-016).

use serde::{Deserialize, Serialize};

/// Why a required recovery backstop was not available for an
/// bounded `Effect-opaque execution`.
///
/// Recorded in the audit log so a degraded recovery is a first-class,
/// queryable event — distinct from `SnapshotPolicy::None`, which is a trusted
/// global opt-out, not a degradation. Orthogonal to `RiskLevel`: an
/// effect-opaque command can remain `Safe` while still requiring a snapshot,
/// and failing to create that snapshot is a degradation even though the risk
/// axis never moved.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryDegradation {
    /// A Snapshot was required for bounded effect-opaque execution under
    /// `SnapshotPolicy::Selective` / `Full`, but none could be created before
    /// execution. Non-interactive execution fails closed; interactive execution
    /// must surface this reason.
    NoSnapshotAvailable,
}
