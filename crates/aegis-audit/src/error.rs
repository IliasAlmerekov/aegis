//! Typed error for all audit I/O and serialization operations.

/// Error returned by [`crate::AuditLogger`] operations.
#[derive(thiserror::Error, Debug)]
pub enum AuditError {
    /// An audit artifact could not be validated or made owner-only.
    #[error("audit artifact '{path}' is insecure: {detail}")]
    InsecureAuditArtifact {
        /// Path whose filesystem policy check failed.
        path: String,
        /// Specific reason the artifact was rejected.
        detail: String,
    },
    /// Wrapped I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Parse or serialization error.
    #[error("audit error: {0}")]
    Parse(String),
}
