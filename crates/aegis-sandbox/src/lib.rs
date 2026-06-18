//! Sandboxing layer for Aegis.
//!
//! Provides [`SandboxConfig`], [`SandboxProfile`], [`SandboxExecutor`],
//! [`SandboxResult`], and [`SandboxError`] for running commands inside a
//! sandbox on supported platforms:
//! - **Linux**: bwrap + Landlock
//! - **macOS**: Seatbelt (`sandbox-exec`)
//!
//! Native Windows is intentionally unsupported for Aegis 1.0. Windows users
//! should run Aegis inside WSL2, where it behaves as Linux.
//!
//! Platform-specific implementation lives in a private `platform` module alias
//! that resolves to `linux.rs`, `macos.rs`, or `unsupported.rs` depending on the
//! build target. Shared helpers (forced-unavailable test hook, bypass warning)
//! live in `support.rs`.

use std::path::PathBuf;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::ffi::{OsStr, OsString};

mod support;

#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;

// Native `windows` is intentionally routed to the unsupported module for
// Aegis 1.0; Windows users should run Aegis inside WSL2/Linux.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
#[path = "unsupported.rs"]
mod platform;

/// Typed error for sandbox operations.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// The sandbox was marked `required = true` but is unavailable on this system.
    #[error("sandbox is required but unavailable on this system")]
    Required,

    /// bwrap failed to set up the sandbox (namespace, mount, or permissions error).
    #[error("sandbox setup failed: {0}")]
    SetupFailed(String),

    /// A sandbox execution error occurred (e.g. failed to spawn bwrap).
    #[error("sandbox execution error: {0}")]
    Execution(String),

    /// Wrapped I/O error.
    #[error("sandbox I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Configuration for the sandbox layer.
#[derive(Debug, Clone, Default)]
pub struct SandboxConfig {
    /// Paths the sandboxed process is allowed to write to.
    pub allow_write: Vec<PathBuf>,
    /// Whether the sandboxed process is allowed to access the network.
    pub allow_network: bool,
    /// If `true`, failure to set up the sandbox is a hard error rather than a
    /// graceful fallback.
    pub required: bool,
}

/// A compiled sandbox profile derived from a [`SandboxConfig`].
#[derive(Debug, Clone)]
pub struct SandboxProfile {
    config: SandboxConfig,
}

/// Executes a command inside the sandbox described by a [`SandboxProfile`].
pub struct SandboxExecutor {
    profile: SandboxProfile,
}

/// Outcome of a sandboxed command execution.
#[derive(Debug)]
pub enum SandboxResult {
    /// The command ran successfully; the inner value is its exit code.
    Success(i32),
    /// The sandbox was unavailable and was skipped because `required` was `false`.
    Unavailable,
}

// ── Public availability query ─────────────────────────────────────────────────

/// Return `true` when the sandbox infrastructure is available for `config`.
///
/// This is a lightweight check used by callers to record audit state
/// without forking. Native Windows and other non-Linux/non-macOS targets
/// always return `false`; Windows users should run Aegis inside WSL2/Linux.
pub fn sandbox_available_for(config: &SandboxConfig) -> bool {
    platform::sandbox_available_for(config)
}

// ── Implementation ────────────────────────────────────────────────────────────

impl SandboxProfile {
    pub fn from_config(config: &SandboxConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

impl SandboxExecutor {
    pub fn new(profile: SandboxProfile) -> Self {
        Self { profile }
    }

    pub fn run(&self, cmd: &str) -> Result<SandboxResult, SandboxError> {
        platform::run(&self.profile.config, cmd)
    }
}

/// Prepare a [`std::process::Command`] suitable for POSIX `exec()` that wraps
/// `program` and `args` inside the sandbox described by `config`.
///
/// On Linux when the sandbox is available, applies Landlock filesystem
/// restrictions to the current process (inherited across exec), then returns
/// a bwrap command. When unavailable and `required` is `false`, returns a
/// direct command. When unavailable and `required` is `true`, returns
/// `Err(SandboxError::Required)`.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub fn prepare_for_exec(
    config: &SandboxConfig,
    program: &OsStr,
    args: &[OsString],
) -> Result<std::process::Command, SandboxError> {
    platform::prepare_for_exec(config, program, args)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Refactor acceptance guards (M5.1 split) ───────────────────────────────

    /// Size guard for the M5.1 split-aegis-sandbox refactor.
    ///
    /// Acceptance criterion from the plan: "No `crates/aegis-sandbox/src/*.rs`
    /// file exceeds 800 LoC." This test scans every `*.rs` direct child of
    /// `src/` at runtime and asserts each is at most 800 lines (counting all
    /// lines, matching `wc -l`). It MUST FAIL now because `lib.rs` is 2071
    /// LoC, and MUST PASS after the refactor splits the code into focused
    /// platform modules.
    #[test]
    fn no_src_file_exceeds_800_lines() {
        let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let entries = std::fs::read_dir(&src_dir)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", src_dir.display()));

        let mut offenders: Vec<(String, usize)> = Vec::new();
        for entry in entries {
            let entry = entry.unwrap_or_else(|e| panic!("failed to iterate src dir entry: {e}"));
            let path = entry.path();
            // Only direct children of src/ that end in .rs.
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let contents = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
            let line_count = contents.lines().count();
            if line_count > 800 {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_owned())
                    .unwrap_or_else(|| format!("{}", path.display()));
                offenders.push((name, line_count));
            }
        }

        assert!(
            offenders.is_empty(),
            "aegis-sandbox source files exceed 800 LoC (M5.1 acceptance gate): {offenders:?}"
        );
    }

    /// Public API presence guard for the M5.1 split-aegis-sandbox refactor.
    ///
    /// The refactor must preserve every public item listed in the plan's
    /// "No public API changes for" acceptance criterion. Constructing/valuing
    /// each type and calling each function here anchors them at compile time
    /// and runtime; if the green-tester accidentally removes or renames one,
    /// this test fails to compile or fails at runtime.
    #[test]
    fn public_api_surface_survives_refactor() {
        // SandboxConfig
        let config = SandboxConfig::default();
        // SandboxProfile
        let profile = SandboxProfile::from_config(&config);
        // SandboxExecutor (construct only — do not call run() to avoid forking)
        let _executor = SandboxExecutor::new(profile);
        // SandboxResult
        let result: SandboxResult = SandboxResult::Unavailable;
        assert!(matches!(result, SandboxResult::Unavailable));
        // SandboxError
        let err_display = SandboxError::Required.to_string();
        assert!(
            !err_display.is_empty(),
            "SandboxError::Required display is empty"
        );
        // sandbox_available_for
        let _ = sandbox_available_for(&config);

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use std::ffi::{OsStr, OsString};
            let program = OsStr::new("/usr/bin/true");
            let args: &[OsString] = &[];
            // POSIX prepare_for_exec — anchor its signature; ignore the outcome
            // (it may error or succeed depending on environment, which is fine).
            let _ = prepare_for_exec(&config, program, args);
        }
    }
}
