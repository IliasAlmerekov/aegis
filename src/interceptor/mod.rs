pub mod parser;
pub mod patterns;
pub mod scanner;

use std::fmt;
use std::str::FromStr;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum RiskLevel {
    Safe,
    Warn,
    Danger,
    Block,
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            RiskLevel::Safe => "safe",
            RiskLevel::Warn => "warn",
            RiskLevel::Danger => "danger",
            RiskLevel::Block => "block",
        };

        f.write_str(value)
    }
}

impl FromStr for RiskLevel {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "safe" => Ok(RiskLevel::Safe),
            "warn" => Ok(RiskLevel::Warn),
            "danger" => Ok(RiskLevel::Danger),
            "block" => Ok(RiskLevel::Block),
            other => Err(format!(
                "invalid risk level '{other}', expected one of: safe, warn, danger, block"
            )),
        }
    }
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
