pub mod parser;
pub mod patterns;
pub mod scanner;

use std::sync::LazyLock;

use serde::Deserialize;

use crate::error::AegisError;

static SCANNER: LazyLock<Result<scanner::Scanner, String>> = LazyLock::new(|| {
    patterns::PatternSet::load()
        .map(scanner::Scanner::new)
        .map_err(|e| e.to_string())
});

/// Classifies the risk level of a shell command.
///
/// Ordered by severity: `Safe < Warn < Danger < Block`.
///
/// `#[non_exhaustive]` ensures match arms in external crates require a wildcard,
/// preserving forward-compatibility if new levels are added in v2.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
pub enum RiskLevel {
    Safe,
    Warn,
    Danger,
    Block,
}

/// Assess a command with the loaded interceptor rules.
pub fn assess(cmd: &str) -> Result<scanner::Assessment, AegisError> {
    match &*SCANNER {
        Ok(scanner) => Ok(scanner.assess(cmd)),
        Err(message) => Err(AegisError::Config(message.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::{RiskLevel, assess};

    #[test]
    fn safe_is_less_than_warn() {
        assert!(RiskLevel::Safe < RiskLevel::Warn);
    }

    #[test]
    fn warn_is_less_than_danger() {
        assert!(RiskLevel::Warn < RiskLevel::Danger);
    }

    #[test]
    fn danger_is_less_than_block() {
        assert!(RiskLevel::Danger < RiskLevel::Block);
    }

    #[test]
    fn assess_reports_safe_for_benign_command() {
        assert_eq!(assess("echo hello world").unwrap().risk, RiskLevel::Safe);
    }
}
