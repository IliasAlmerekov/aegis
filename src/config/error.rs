//! Typed errors for configuration loading, validation, and amendment.

/// Errors raised while loading, validating, or amending Aegis configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A configuration value was invalid (parse failure, bad field, validation
    /// rule, scope error, …). Carries a human-readable message.
    #[error("{0}")]
    Config(String),

    /// An I/O error while reading or writing a config file.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
