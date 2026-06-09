//! Risk classification for shell commands.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Classifies the risk level of a shell command.
///
/// Ordered by severity: `Safe < Warn < Danger < Block`.
///
/// `#[non_exhaustive]` ensures match arms in external crates require a wildcard,
/// preserving forward-compatibility if new levels are added in v2.
#[non_exhaustive]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, schemars::JsonSchema,
)]
pub enum RiskLevel {
    /// No risk detected; command is safe to execute.
    Safe,
    /// Potentially dangerous; prompt for confirmation.
    Warn,
    /// Dangerous; strong confirmation required.
    Danger,
    /// Never allow this command to run.
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

#[cfg(test)]
mod tests {
    use super::RiskLevel;

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
    fn display_renders_lowercase_name() {
        assert_eq!(RiskLevel::Danger.to_string(), "danger");
    }

    #[test]
    fn from_str_parses_case_insensitively_with_surrounding_whitespace() {
        assert_eq!("  BLOCK ".parse::<RiskLevel>().unwrap(), RiskLevel::Block);
    }

    #[test]
    fn from_str_rejects_unknown_value() {
        assert!("nuke".parse::<RiskLevel>().is_err());
    }
}
