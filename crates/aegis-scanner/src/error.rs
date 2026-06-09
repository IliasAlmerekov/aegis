//! Typed errors for scanner construction and pattern validation.

/// Errors raised while loading patterns or building a [`crate::Scanner`].
///
/// The scanner is deliberately ignorant of where patterns originate (built-in
/// TOML, user config, …); these variants describe *what* is wrong, not the
/// source. The orchestration layer maps them onto its own error type.
#[derive(Debug, thiserror::Error)]
pub enum ScannerError {
    /// A pattern or prefix rule failed validation (missing field, duplicate id,
    /// id conflict, …).
    #[error("invalid pattern {id}: {reason}")]
    InvalidPattern {
        /// Identifier of the offending pattern (may be empty if the id itself is missing).
        id: String,
        /// Human-readable explanation of why the pattern is invalid.
        reason: String,
    },

    /// The pattern set could not be assembled (e.g. the embedded TOML failed to parse).
    #[error("failed to build scanner: {0}")]
    Build(String),
}
