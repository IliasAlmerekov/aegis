//! Shared sandbox support and test-injection hook.
//!
//! Platform modules (`linux`, `macos`, `windows`, `unsupported`) route their
//! "sandbox unavailable" code paths through [`run_unavailable_result`] and
//! [`run_unavailable_result`] so typed behavior stays consistent across targets.

use crate::{SandboxError, SandboxResult};

// ── Test injection ────────────────────────────────────────────────────────────
//
// The forced-unavailable hook is only exercised by the Linux and macOS sandbox
// modules' "sandbox unavailable" code paths. Gate it to those targets so it is
// not dead code on native Windows (which routes to `unsupported.rs` and never
// consults the hook).
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[cfg(test)]
thread_local! {
    static FORCE_SANDBOX_UNAVAILABLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[cfg(test)]
pub(crate) fn set_force_sandbox_unavailable(val: bool) {
    FORCE_SANDBOX_UNAVAILABLE.with(|c| c.set(val));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn is_forced_sandbox_unavailable() -> bool {
    #[cfg(test)]
    return FORCE_SANDBOX_UNAVAILABLE.with(|c| c.get());
    #[cfg(not(test))]
    return false;
}

// ── Shared unavailable/bypass helpers ─────────────────────────────────────────

pub(crate) fn run_unavailable_result(required: bool) -> Result<SandboxResult, SandboxError> {
    if required {
        Err(SandboxError::Required)
    } else {
        Ok(SandboxResult::Unavailable)
    }
}

// ── Shared test helpers ────────────────────────────────────────────────────────

#[cfg(test)]
pub(crate) mod test_helpers {
    /// RAII guard that clears the forced-unavailable flag on drop.
    ///
    /// Only meaningful where the forced-unavailable hook exists (Linux/macOS);
    /// gate it to those targets so the module compiles cleanly on native
    /// Windows, which routes to `unsupported.rs` and never sets the flag.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub(crate) struct ForceUnavailableGuard;

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    impl Drop for ForceUnavailableGuard {
        fn drop(&mut self) {
            super::set_force_sandbox_unavailable(false);
        }
    }
}

// ── Common (platform-agnostic) tests ───────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::support::run_unavailable_result;
    use crate::{SandboxConfig, SandboxError, SandboxExecutor, SandboxProfile, SandboxResult};

    // ── SandboxConfig defaults ────────────────────────────────────────────────

    #[test]
    fn sandbox_config_default_has_empty_allow_write() {
        assert!(SandboxConfig::default().allow_write.is_empty());
    }

    #[test]
    fn sandbox_config_default_disallows_network() {
        assert!(!SandboxConfig::default().allow_network);
    }

    #[test]
    fn sandbox_config_default_not_required() {
        assert!(!SandboxConfig::default().required);
    }

    // ── SandboxProfile / SandboxExecutor construction ─────────────────────────

    #[test]
    fn sandbox_profile_builds_from_config() {
        let _profile = SandboxProfile::from_config(&SandboxConfig::default());
    }

    #[test]
    fn sandbox_executor_new_accepts_profile() {
        let profile = SandboxProfile::from_config(&SandboxConfig::default());
        let _executor = SandboxExecutor::new(profile);
    }

    // ── run_unavailable_result behavior ───────────────────────────────────────

    #[test]
    fn run_unavailable_result_returns_unavailable_when_not_required() {
        assert!(matches!(
            run_unavailable_result(false),
            Ok(SandboxResult::Unavailable)
        ));
    }

    #[test]
    fn run_unavailable_result_returns_required_error_when_required() {
        assert!(matches!(
            run_unavailable_result(true),
            Err(SandboxError::Required)
        ));
    }

    // ── SandboxError::Display ─────────────────────────────────────────────────

    #[test]
    fn sandbox_error_required_display_mentions_required_or_unavailable() {
        let msg = SandboxError::Required.to_string().to_lowercase();
        assert!(msg.contains("required") || msg.contains("unavailable"));
    }

    // ── Legacy test names (kept for backwards compatibility) ─────────────────

    #[test]
    fn test_sandbox_config_default_allows_no_write_paths() {
        assert!(SandboxConfig::default().allow_write.is_empty());
    }

    #[test]
    fn test_sandbox_config_default_disallows_network() {
        assert!(!SandboxConfig::default().allow_network);
    }

    #[test]
    fn test_sandbox_config_default_not_required() {
        assert!(!SandboxConfig::default().required);
    }

    #[test]
    fn test_sandbox_profile_builds_from_config() {
        let _profile = SandboxProfile::from_config(&SandboxConfig::default());
    }

    #[test]
    fn test_sandbox_executor_new_returns_executor() {
        let profile = SandboxProfile::from_config(&SandboxConfig::default());
        let _executor = SandboxExecutor::new(profile);
    }

    #[test]
    fn test_sandbox_error_implements_display() {
        let msg = SandboxError::Required.to_string();
        assert!(!msg.is_empty());
        let lower = msg.to_lowercase();
        assert!(lower.contains("required") || lower.contains("unavailable"));
    }

    #[test]
    fn test_run_non_linux_unavailable_logic() {
        match run_unavailable_result(false) {
            Ok(SandboxResult::Unavailable) => {}
            other => {
                panic!("run_unavailable_result(false) must return Ok(Unavailable), got {other:?}")
            }
        }
        match run_unavailable_result(true) {
            Err(SandboxError::Required) => {}
            other => {
                panic!("run_unavailable_result(true) must return Err(Required), got {other:?}")
            }
        }
    }
}
