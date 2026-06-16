//! Sandbox application status recorded in the audit log.

use serde::{Deserialize, Serialize};

/// Whether an OS-level sandbox profile was applied to an executed command.
///
/// Recorded in every audit entry so that a *sandbox bypass* — a command that
/// ran without confinement because the sandbox was configured but could not be
/// applied — is a first-class, queryable audit event (ROADMAP 6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxStatus {
    /// A sandbox profile was applied to the executed command.
    Active,
    /// A sandbox was configured but could not be applied; the command ran
    /// unconfined. This is the audited "sandbox bypass" event.
    Unavailable,
    /// No sandbox was configured for this invocation.
    #[default]
    #[serde(alias = "NotConfigured")]
    NotConfigured,
}

impl SandboxStatus {
    /// Legacy `sandbox_active` boolean projection, written alongside the
    /// canonical `sandbox_status` field so that older audit-log readers that
    /// only understand the boolean keep working.
    ///
    /// `Active` → `Some(true)`, `Unavailable` → `Some(false)`,
    /// `NotConfigured` → `None`.
    pub fn as_legacy_active(self) -> Option<bool> {
        match self {
            SandboxStatus::Active => Some(true),
            SandboxStatus::Unavailable => Some(false),
            SandboxStatus::NotConfigured => None,
        }
    }
}

impl From<Option<bool>> for SandboxStatus {
    /// Maps the legacy availability tri-state to a status:
    /// `None` (no sandbox configured) → `NotConfigured`,
    /// `Some(true)` (available) → `Active`,
    /// `Some(false)` (configured but unavailable) → `Unavailable`.
    fn from(available: Option<bool>) -> Self {
        match available {
            None => SandboxStatus::NotConfigured,
            Some(true) => SandboxStatus::Active,
            Some(false) => SandboxStatus::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SandboxStatus;

    #[test]
    fn default_is_not_configured() {
        assert_eq!(SandboxStatus::default(), SandboxStatus::NotConfigured);
    }

    #[test]
    fn maps_from_legacy_availability_tristate() {
        assert_eq!(SandboxStatus::from(None), SandboxStatus::NotConfigured);
        assert_eq!(SandboxStatus::from(Some(true)), SandboxStatus::Active);
        assert_eq!(SandboxStatus::from(Some(false)), SandboxStatus::Unavailable);
    }

    #[test]
    fn legacy_active_round_trips_through_from() {
        for status in [
            SandboxStatus::Active,
            SandboxStatus::Unavailable,
            SandboxStatus::NotConfigured,
        ] {
            assert_eq!(SandboxStatus::from(status.as_legacy_active()), status);
        }
    }

    #[test]
    fn serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&SandboxStatus::NotConfigured).unwrap(),
            "\"not_configured\""
        );
        assert_eq!(
            serde_json::to_string(&SandboxStatus::Unavailable).unwrap(),
            "\"unavailable\""
        );
    }
}
