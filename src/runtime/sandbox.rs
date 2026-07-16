//! Stable active-channel diagnostics for optional Sandbox execution.

/// Stable diagnostic code for optional Sandbox degradation.
pub const SANDBOX_UNAVAILABLE_CODE: &str = "sandbox_unavailable";

/// Stable message emitted when optional confinement is unavailable.
pub const SANDBOX_UNAVAILABLE_MESSAGE: &str = "Sandbox unavailable; proceeding without confinement. Set sandbox.required = true to block execution.";

/// Stable diagnostic code for required Sandbox unavailability.
pub const SANDBOX_REQUIRED_UNAVAILABLE_CODE: &str = "sandbox_required_unavailable";

/// Stable message emitted when required confinement blocks execution.
pub const SANDBOX_REQUIRED_UNAVAILABLE_MESSAGE: &str =
    "Required Sandbox unavailable; command not executed.";
