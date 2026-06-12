//! Typed error hierarchy for the `aegis-snapshot` crate.

/// Typed error for snapshot creation and rollback operations.
#[derive(thiserror::Error, Debug)]
pub enum SnapshotError {
    /// Snapshot creation or rollback operation failed.
    #[error("snapshot error: {0}")]
    Snapshot(String),

    /// Configuration error that prevents a snapshot registry from being built.
    #[error("snapshot config error: {0}")]
    Config(String),

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

    /// Wrapped I/O error from the standard library.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
