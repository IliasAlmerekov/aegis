//! Fallback sandbox implementation for unsupported targets.
//!
//! Native Windows is intentionally unsupported for Aegis 1.0. Windows users
//! should run Aegis inside WSL2, where this crate is compiled for Linux and uses
//! the Linux sandbox implementation. On native Windows and any other target
//! that is not Linux or macOS, the sandbox is always unavailable.

use crate::support::run_unavailable_result;
use crate::{SandboxConfig, SandboxError, SandboxResult};

pub(crate) fn sandbox_available_for(config: &SandboxConfig) -> bool {
    let _ = config;
    false
}

pub(crate) fn run(config: &SandboxConfig, _cmd: &str) -> Result<SandboxResult, SandboxError> {
    run_unavailable_result(config.required)
}
