//! Human decision outcome of the interception flow.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// User-visible outcome of the interception flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Decision {
    /// User explicitly approved the command.
    Approved,
    /// User explicitly denied the command.
    Denied,
    /// Approved automatically by allowlist or safe path.
    AutoApproved,
    /// Blocked by policy or because the command is too dangerous.
    Blocked,
    /// Snapshot was removed by `aegis snapshot prune`.
    Pruned,
}

impl fmt::Display for Decision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Decision::Approved => "approved",
            Decision::Denied => "denied",
            Decision::AutoApproved => "auto-approved",
            Decision::Blocked => "blocked",
            Decision::Pruned => "pruned",
        };

        f.write_str(value)
    }
}

impl FromStr for Decision {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "approved" => Ok(Self::Approved),
            "denied" => Ok(Self::Denied),
            "auto-approved" => Ok(Self::AutoApproved),
            "blocked" => Ok(Self::Blocked),
            "pruned" => Ok(Self::Pruned),
            other => Err(format!(
                "invalid decision '{other}', expected one of: approved, denied, auto-approved, blocked, pruned"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Decision;

    #[test]
    fn display_uses_hyphenated_auto_approved() {
        assert_eq!(Decision::AutoApproved.to_string(), "auto-approved");
    }

    #[test]
    fn from_str_round_trips_through_display() {
        for decision in [
            Decision::Approved,
            Decision::Denied,
            Decision::AutoApproved,
            Decision::Blocked,
            Decision::Pruned,
        ] {
            assert_eq!(decision.to_string().parse::<Decision>().unwrap(), decision);
        }
    }

    #[test]
    fn from_str_rejects_unknown_value() {
        assert!("maybe".parse::<Decision>().is_err());
    }

    #[test]
    fn test_decision_pruned_variant_exists() {
        let decision = Decision::Pruned;
        assert_eq!(decision, Decision::Pruned);
    }

    #[test]
    fn test_decision_pruned_round_trips_through_display_and_from_str() {
        let decision = Decision::Pruned;
        let text = decision.to_string();
        let parsed: Decision = text
            .parse()
            .expect("Pruned must parse from its display text");
        assert_eq!(parsed, Decision::Pruned);
    }

    #[test]
    fn test_decision_pruned_serializes_to_snake_case() {
        let json = serde_json::to_string(&Decision::Pruned).expect("Pruned must serialize");
        assert!(json.contains("Pruned"), "expected 'Pruned' in JSON: {json}");
    }
}
