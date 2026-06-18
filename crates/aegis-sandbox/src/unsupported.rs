//! Fallback sandbox implementation for unsupported targets.
//!
//! On any target that is not Linux, macOS, or Windows, the sandbox is always
//! unavailable. Callers with `required = false` get [`SandboxResult::Unavailable`]
//! (with a bypass warning); callers with `required = true` get
//! [`SandboxError::Required`].

use crate::support::run_unavailable_result;
use crate::{SandboxConfig, SandboxError, SandboxResult};

pub(crate) fn sandbox_available_for(config: &SandboxConfig) -> bool {
    let _ = config;
    false
}

pub(crate) fn run(config: &SandboxConfig, _cmd: &str) -> Result<SandboxResult, SandboxError> {
    run_unavailable_result(config.required)
}
