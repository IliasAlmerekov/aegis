//! Shared post-attempt Required recovery status.

use crate::snapshot::SnapshotRecord;
use aegis_types::RecoveryDegradation;

/// Post-attempt state for an active ADR-016 Required recovery obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryStatus {
    /// At least one required Snapshot was created.
    Ready,
    /// No required Snapshot was created.
    Degraded(RecoveryDegradation),
}

/// Derive the shared ADR-016 Recovery status from policy and runtime facts.
#[must_use]
pub fn recovery_status(
    effect_opaque: bool,
    snapshots_required: bool,
    snapshots: &[SnapshotRecord],
) -> Option<RecoveryStatus> {
    if !effect_opaque || !snapshots_required {
        return None;
    }

    Some(if snapshots.is_empty() {
        RecoveryStatus::Degraded(RecoveryDegradation::NoSnapshotAvailable)
    } else {
        RecoveryStatus::Ready
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_recovery_is_ready_when_a_snapshot_was_created() {
        let snapshots = [SnapshotRecord {
            plugin: "git",
            snapshot_id: "stash@{0}".to_string(),
        }];

        assert_eq!(
            recovery_status(true, true, &snapshots),
            Some(RecoveryStatus::Ready)
        );
    }

    #[test]
    fn required_recovery_is_degraded_when_no_snapshot_was_created() {
        assert_eq!(
            recovery_status(true, true, &[]),
            Some(RecoveryStatus::Degraded(
                RecoveryDegradation::NoSnapshotAvailable
            ))
        );
    }

    #[test]
    fn recovery_opt_out_has_no_required_status() {
        assert_eq!(recovery_status(true, false, &[]), None);
    }

    #[test]
    fn ordinary_danger_snapshot_failure_has_no_h9_recovery_status() {
        assert_eq!(recovery_status(false, true, &[]), None);
    }
}
