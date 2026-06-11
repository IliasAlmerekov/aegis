//! Typed error for all audit I/O and serialization operations.

/// Error returned by [`crate::AuditLogger`] operations.
#[derive(thiserror::Error, Debug)]
pub enum AuditError {
    /// Wrapped I/O error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Parse or serialization error.
    #[error("audit error: {0}")]
    Parse(String),
}
