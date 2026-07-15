/// Typed error hierarchy for all Aegis operations.
#[derive(thiserror::Error, Debug)]
pub enum AegisError {
    /// Snapshot creation or rollback failed.
    #[error("snapshot error: {0}")]
    Snapshot(String),

    /// `git stash pop` conflicted during rollback.
    ///
    /// The stash entry is preserved — git does not drop it on a conflicted pop.
    /// The error message includes exact recovery commands so the user can finish
    /// the restore manually without losing any work.
    #[error(
        "rollback conflict in '{cwd}': git stash pop failed for {stash_ref}.\n\
         Your changes are still saved in the stash. To recover manually:\n  \
           1. Resolve conflicts:  cd '{cwd}' && git diff\n  \
           2. Stage resolutions:  git add <files>\n  \
           3. Drop the stash:     git stash drop {stash_ref}\n\
         Details: {details}"
    )]
    RollbackConflict {
        /// The stash reference that failed to pop.
        stash_ref: String,
        /// Working directory where the rollback was attempted.
        cwd: String,
        /// Underlying error details.
        details: String,
    },

    /// The dump file that was recorded at snapshot time no longer exists.
    #[error(
        "rollback failed: dump file not found at '{path}'. The file may have been deleted manually."
    )]
    RollbackDumpNotFound {
        /// Expected path of the missing dump file.
        path: String,
    },

    /// The persisted rollback artifact no longer matches the manifest checksum.
    #[error(
        "rollback failed: artifact integrity verification failed for '{path}' (expected SHA-256 {expected_sha256}, got {actual_sha256})"
    )]
    RollbackIntegrityCheckFailed {
        /// Path to the artifact that failed verification.
        path: String,
        /// Expected SHA-256 checksum recorded at snapshot time.
        expected_sha256: String,
        /// Actual SHA-256 checksum computed at rollback time.
        actual_sha256: String,
    },

    /// Configuration loading, parsing, or validation error.
    #[error("config error: {0}")]
    Config(String),

    /// One or more prune candidates could not be deleted.
    #[error(
        "prune completed with failures: {failed_count} candidate(s) could not be deleted. \
         {pruned} snapshot(s) were pruned successfully.\n{failed}"
    )]
    PrunePartialFailure {
        /// Number of snapshots successfully pruned.
        pruned: usize,
        /// Number of snapshots that could not be deleted.
        failed_count: usize,
        /// Human-readable descriptions of each failed deletion.
        failed: String,
    },

    /// Wrapped I/O error from the standard library.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Map scanner-construction errors onto the orchestration error type.
///
/// The scanner crate raises a domain-specific [`aegis_scanner::ScannerError`]
/// that is unaware of configuration; at this boundary we fold it into the
/// binary's `Config` variant since invalid patterns reach us via config.
impl From<aegis_scanner::ScannerError> for AegisError {
    fn from(error: aegis_scanner::ScannerError) -> Self {
        Self::Config(error.to_string())
    }
}

/// Map audit-layer errors onto the orchestration error type.
impl From<aegis_audit::error::AuditError> for AegisError {
    fn from(error: aegis_audit::error::AuditError) -> Self {
        match error {
            aegis_audit::error::AuditError::InsecureAuditArtifact { path, detail } => {
                Self::Io(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    format!("audit artifact '{path}' is insecure: {detail}"),
                ))
            }
            aegis_audit::error::AuditError::Io(io) => Self::Io(io),
            aegis_audit::error::AuditError::Parse(msg) => Self::Config(msg),
        }
    }
}

/// Map snapshot-layer errors onto the orchestration error type.
///
/// [`aegis_snapshot::SnapshotError`] carries structured variants; we preserve
/// the rich `RollbackConflict` variant so the user gets full recovery
/// instructions, and fold everything else into the binary's own variants.
impl From<aegis_snapshot::SnapshotError> for AegisError {
    fn from(error: aegis_snapshot::SnapshotError) -> Self {
        match error {
            aegis_snapshot::SnapshotError::RollbackConflict {
                stash_ref,
                cwd,
                details,
            } => Self::RollbackConflict {
                stash_ref,
                cwd,
                details,
            },
            aegis_snapshot::SnapshotError::RollbackDumpNotFound { path } => {
                Self::RollbackDumpNotFound { path }
            }
            aegis_snapshot::SnapshotError::RollbackIntegrityCheckFailed {
                path,
                expected_sha256,
                actual_sha256,
            } => Self::RollbackIntegrityCheckFailed {
                path,
                expected_sha256,
                actual_sha256,
            },
            aegis_snapshot::SnapshotError::PathEscapesSnapshotStore {
                plugin,
                store,
                candidate,
            } => Self::Snapshot(format!(
                "{plugin} snapshot artifact '{candidate}' escapes snapshot store '{store}'"
            )),
            aegis_snapshot::SnapshotError::InsecureSnapshotPermissions {
                plugin,
                path,
                detail,
            } => Self::Snapshot(format!(
                "{plugin} snapshot path '{path}' does not meet owner-only permissions: {detail}"
            )),
            aegis_snapshot::SnapshotError::Config(msg) => Self::Config(msg),
            aegis_snapshot::SnapshotError::Io(io) => Self::Io(io),
            aegis_snapshot::SnapshotError::Snapshot(msg) => Self::Snapshot(msg),
            aegis_snapshot::SnapshotError::DeleteFailed {
                plugin,
                snapshot_id,
                source,
            } => Self::Snapshot(format!(
                "failed to delete {plugin} snapshot {snapshot_id}: {source}"
            )),
        }
    }
}

/// Map config-layer errors onto the orchestration error type.
///
/// `ConfigError` carries its own I/O variant, but at this boundary we fold
/// everything into `Config`; the binary surfaces config failures uniformly.
impl From<crate::config::error::ConfigError> for AegisError {
    fn from(error: crate::config::error::ConfigError) -> Self {
        match error {
            crate::config::error::ConfigError::Io(io) => Self::Io(io),
            other => Self::Config(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollback_dump_not_found_message_contains_path() {
        let err = AegisError::RollbackDumpNotFound {
            path: "/home/user/.aegis/snapshots/pg-myapp-1234.dump".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("/home/user/.aegis/snapshots/pg-myapp-1234.dump"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn rollback_integrity_check_failed_message_contains_path() {
        let err = AegisError::RollbackIntegrityCheckFailed {
            path: "/home/user/.aegis/snapshots/supabase-123/artifacts/db.dump".to_string(),
            expected_sha256: "abc123".to_string(),
            actual_sha256: "def456".to_string(),
        };
        let msg = err.to_string();

        assert!(msg.contains("/home/user/.aegis/snapshots/supabase-123/artifacts/db.dump"));
        assert!(msg.contains("expected SHA-256 abc123"));
        assert!(msg.contains("got def456"));
    }
}
