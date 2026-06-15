//! Sandboxing layer for Aegis.
//!
//! Provides [`SandboxConfig`], [`SandboxProfile`], [`SandboxExecutor`],
//! [`SandboxResult`], and [`SandboxError`] for running commands inside a
//! sandbox on supported platforms:
//! - **Linux**: bwrap + Landlock
//! - **macOS**: Seatbelt (`sandbox-exec`)
//!
//! Platform-specific implementation is gated on the respective `target_os`.

use std::path::PathBuf;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::ffi::{OsStr, OsString};

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

// ── Test injection ────────────────────────────────────────────────────────────

#[cfg(test)]
thread_local! {
    static FORCE_SANDBOX_UNAVAILABLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn set_force_sandbox_unavailable(val: bool) {
    FORCE_SANDBOX_UNAVAILABLE.with(|c| c.set(val));
}

fn is_forced_sandbox_unavailable() -> bool {
    #[cfg(test)]
    return FORCE_SANDBOX_UNAVAILABLE.with(|c| c.get());
    #[cfg(not(test))]
    return false;
}

// ── Public availability query ─────────────────────────────────────────────────

/// Return `true` when the sandbox infrastructure is available for `config`.
///
/// This is a lightweight check used by callers to record audit state
/// without forking. On Windows, always returns `true` (Job Objects are
/// available on all Vista+ systems). On other non-Linux/non-macOS targets
/// always returns `false`.
pub fn sandbox_available_for(config: &SandboxConfig) -> bool {
    #[cfg(target_os = "linux")]
    {
        !is_forced_sandbox_unavailable() && is_sandbox_available(config)
    }
    #[cfg(target_os = "macos")]
    {
        // This is the single canonical availability check — it must run every
        // validation that prepare_for_exec() would run, so the audit field and
        // the actual execution path always agree. Specifically:
        //   1. is_sandbox_exec_available(): binary exists + minimal probe works
        //   2. build_seatbelt_profile(config): paths exist, are valid UTF-8
        //   3. exec_true_in_profile(&profile): the actual per-command profile
        //      is accepted by Seatbelt (not just a generic minimal profile)
        // prepare_for_exec() trusts this result and does not re-probe.
        !is_forced_sandbox_unavailable()
            && is_sandbox_exec_available()
            && build_seatbelt_profile(config)
                .map(|profile| exec_true_in_profile(&profile))
                .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        // Job Objects are available on all Windows Vista+ systems — no runtime probe needed.
        let _ = config;
        !is_forced_sandbox_unavailable()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = config;
        false
    }
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

    #[cfg(target_os = "linux")]
    pub fn run(&self, cmd: &str) -> Result<SandboxResult, SandboxError> {
        if is_forced_sandbox_unavailable() || !is_sandbox_available(&self.profile.config) {
            return run_unavailable_result(self.profile.config.required);
        }

        // NOTE: Landlock is NOT applied here because apply_landlock_restrictions
        // would restrict the Aegis parent process, not the bwrap child.
        // For the subprocess path, bwrap namespace isolation provides the
        // necessary confinement. Landlock is applied in prepare_for_exec()
        // where it is inherited across the POSIX exec() boundary.

        let bwrap_args = build_bwrap_args(&self.profile.config)?;
        let mut all_args = bwrap_args;
        all_args.extend([
            OsString::from("sh"),
            OsString::from("-c"),
            OsString::from(cmd),
        ]);

        let output = std::process::Command::new("bwrap")
            .args(&all_args)
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| SandboxError::Execution(e.to_string()))?
            .wait_with_output()
            .map_err(|e| SandboxError::Execution(e.to_string()))?;

        let exit_code = output.status.code().unwrap_or(-1);

        // bwrap prefixes its own error messages with "bwrap: " on stderr.
        if !output.stderr.is_empty() {
            let stderr_str = String::from_utf8_lossy(&output.stderr);
            if stderr_str.starts_with("bwrap:") {
                return Err(SandboxError::SetupFailed(stderr_str.trim().to_string()));
            }
        }

        Ok(SandboxResult::Success(exit_code))
    }

    /// macOS implementation: wraps the command in sandbox-exec with a Seatbelt profile.
    #[cfg(target_os = "macos")]
    pub fn run(&self, cmd: &str) -> Result<SandboxResult, SandboxError> {
        if is_forced_sandbox_unavailable() || !is_sandbox_exec_available() {
            return run_unavailable_result(self.profile.config.required);
        }
        let profile = match build_seatbelt_profile(&self.profile.config) {
            Ok(p) => p,
            Err(_) if !self.profile.config.required => {
                warn_sandbox_bypass();
                return Ok(SandboxResult::Unavailable);
            }
            Err(e) => return Err(e),
        };
        // The profile was already validated by sandbox_available_for() and by
        // exec_true_in_profile() in the caller path. Stderr is inherited so the
        // user sees any sandbox-exec diagnostics directly. We do not re-inspect
        // stderr here to avoid false-positive SetupFailed for commands that
        // happen to print "sandbox-exec:" to stderr.
        let status = std::process::Command::new("/usr/bin/sandbox-exec")
            .args(["-p", &profile, "sh", "-c", cmd])
            .status()
            .map_err(|e| SandboxError::Execution(e.to_string()))?;
        let exit_code = status.code().unwrap_or(-1);
        Ok(SandboxResult::Success(exit_code))
    }

    /// Windows implementation: runs `cmd` inside a Job Object.
    ///
    /// **MVP scope**: only `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is enforced —
    /// child processes are killed when the job handle is closed. The
    /// `allow_write` and `allow_network` fields of [`SandboxConfig`] are
    /// accepted but **not enforced** on Windows; filesystem and network
    /// restrictions require AppContainers or WFP, which are out of scope here.
    #[cfg(windows)]
    pub fn run(&self, cmd: &str) -> Result<SandboxResult, SandboxError> {
        if is_forced_sandbox_unavailable() {
            return run_unavailable_result(self.profile.config.required);
        }
        run_in_job_object(cmd)
    }

    /// Fallback: the sandbox is always unavailable on non-Linux, non-macOS, non-Windows targets.
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    pub fn run(&self, _cmd: &str) -> Result<SandboxResult, SandboxError> {
        run_unavailable_result(self.profile.config.required)
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
#[cfg(target_os = "linux")]
pub fn prepare_for_exec(
    config: &SandboxConfig,
    program: &OsStr,
    args: &[OsString],
) -> Result<std::process::Command, SandboxError> {
    if is_forced_sandbox_unavailable() || !is_sandbox_available(config) {
        if config.required {
            return Err(SandboxError::Required);
        }
        warn_sandbox_bypass();
        let mut cmd = std::process::Command::new(program);
        cmd.args(args);
        return Ok(cmd);
    }

    // Apply Landlock restrictions to the current process BEFORE exec().
    // These restrictions are inherited by the exec'd bwrap process and,
    // transitively, by the user command. bwrap's namespace setup does not
    // require writing to regular files, so the restrictions do not interfere.
    apply_landlock_restrictions(config)?;

    let mut bwrap_args = build_bwrap_args(config)?;
    bwrap_args.push(program.to_owned());
    bwrap_args.extend_from_slice(args);

    let mut cmd = std::process::Command::new("bwrap");
    cmd.args(&bwrap_args);
    Ok(cmd)
}

/// macOS variant of `prepare_for_exec`: wraps `program` and `args` inside a
/// Seatbelt (sandbox-exec) profile. When the sandbox is unavailable and
/// `required` is `false`, falls back to a direct command.
#[cfg(target_os = "macos")]
pub fn prepare_for_exec(
    config: &SandboxConfig,
    program: &OsStr,
    args: &[OsString],
) -> Result<std::process::Command, SandboxError> {
    if is_forced_sandbox_unavailable() || !is_sandbox_exec_available() {
        if config.required {
            return Err(SandboxError::Required);
        }
        warn_sandbox_bypass();
        let mut cmd = std::process::Command::new(program);
        cmd.args(args);
        return Ok(cmd);
    }
    // sandbox_available_for() already validated: binary present, profile builds
    // cleanly, and exec_true_in_profile() passed. Callers in shell_flow.rs gate
    // prepare_for_exec() on that result, so no re-probing is needed here.
    // The only remaining error path is TOCTOU (sandbox disappears between the
    // availability check and this call), which is unavoidable in exec-based design.
    let profile = match build_seatbelt_profile(config) {
        Ok(p) => p,
        Err(_) if !config.required => {
            warn_sandbox_bypass();
            let mut cmd = std::process::Command::new(program);
            cmd.args(args);
            return Ok(cmd);
        }
        Err(e) => return Err(e),
    };
    let mut cmd = std::process::Command::new("/usr/bin/sandbox-exec");
    cmd.arg("-p").arg(&profile).arg(program).args(args);
    Ok(cmd)
}

// ── macOS Seatbelt profile builder ───────────────────────────────────────────

/// Generate a Seatbelt SBPL profile string from `config`.
///
/// Canonicalizes each path in `allow_write` to prevent relative-path or
/// symlink confusion (mirrors the bwrap builder on Linux). Returns an error
/// if a path cannot be canonicalized (e.g. it does not exist).
///
/// The profile always denies by default, allows file reads (needed for system
/// libraries), process execution, and signals. Network access is allowed or
/// denied based on `config.allow_network`. Each path in `config.allow_write`
/// gets a `(allow file-write* (subpath "…"))` rule.
#[cfg(target_os = "macos")]
pub(crate) fn build_seatbelt_profile(config: &SandboxConfig) -> Result<String, SandboxError> {
    let mut profile = String::from("(version 1)\n");
    profile.push_str("(deny default)\n");
    profile.push_str("(allow file-read*)\n");
    profile.push_str("(allow process*)\n");
    profile.push_str("(allow signal*)\n");
    if config.allow_network {
        profile.push_str("(allow network*)\n");
    } else {
        profile.push_str("(deny network*)\n");
    }
    for path in &config.allow_write {
        let canonical = path.canonicalize().map_err(|e| {
            SandboxError::Execution(format!("allow_write path {}: {e}", path.display()))
        })?;
        let escaped = escape_sbpl_path(&canonical)?;
        profile.push_str(&format!("(allow file-write* (subpath \"{escaped}\"))\n"));
    }
    Ok(profile)
}

/// Escape a path for safe embedding in an SBPL string literal.
///
/// Returns a typed error when the path contains non-UTF-8 bytes — `to_string_lossy`
/// would silently substitute `\u{FFFD}`, potentially allowing a different path than
/// the one in `allow_write`. Replaces `\` with `\\` and `"` with `\"` in that order
/// to prevent SBPL string literal injection.
#[cfg(target_os = "macos")]
fn escape_sbpl_path(path: &std::path::Path) -> Result<String, SandboxError> {
    let s = path.to_str().ok_or_else(|| {
        SandboxError::Execution(format!(
            "allow_write path contains non-UTF-8 bytes: {}",
            path.display()
        ))
    })?;
    Ok(s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Return `true` if `/usr/bin/sandbox-exec` is present and the Seatbelt
/// machinery actually works on this system (probed via a dry-run).
///
/// The probe runs `sandbox-exec -p <minimal profile> /usr/bin/true`. It
/// matches the pattern of the Linux `probe_sandbox_works` to catch runtime
/// issues (e.g. SIP-stripped environments where the binary exists but
/// the kernel policy engine is absent).
#[cfg(target_os = "macos")]
pub(crate) fn is_sandbox_exec_available() -> bool {
    if !std::path::Path::new("/usr/bin/sandbox-exec").exists() {
        return false;
    }
    probe_seatbelt_works()
}

/// Run `/usr/bin/true` inside a given SBPL profile to check whether the profile
/// is accepted by the kernel's Seatbelt policy engine.
///
/// Used both as an initial availability probe (with a minimal profile) and to
/// validate the actual per-command profile before committing to exec.
#[cfg(target_os = "macos")]
fn exec_true_in_profile(profile: &str) -> bool {
    std::process::Command::new("/usr/bin/sandbox-exec")
        .args(["-p", profile, "/usr/bin/true"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run a minimal sandbox-exec probe to verify the Seatbelt policy engine
/// is functional on this system.
#[cfg(target_os = "macos")]
fn probe_seatbelt_works() -> bool {
    const PROBE: &str =
        "(version 1)\n(deny default)\n(allow process*)\n(allow file-read*)\n(allow signal*)\n";
    exec_true_in_profile(PROBE)
}

// ── Sandbox availability probe ────────────────────────────────────────────────

/// Probe whether the sandbox infrastructure is available and functional.
///
/// Uses `bwrap --version` as a quick first pass, then attempts to actually
/// create a minimal sandbox to catch runtime issues (e.g. WSL2 network
/// namespace restrictions). The probe matches the config's `allow_network`
/// setting to avoid false negatives.
#[cfg(target_os = "linux")]
fn is_sandbox_available(config: &SandboxConfig) -> bool {
    // Fast check: binary must be present and executable.
    let has_bwrap = std::process::Command::new("bwrap")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !has_bwrap {
        return false;
    }

    if !sysctl_userns_available() {
        return false;
    }

    // Real probe: actually try to create a sandbox. This catches issues like
    // WSL2 blocking NETLINK_ROUTE socket creation inside network namespaces.
    probe_sandbox_works(config.allow_network)
}

/// Run a minimal bwrap probe matching `allow_network` to verify namespace
/// creation works on this kernel.
#[cfg(target_os = "linux")]
fn probe_sandbox_works(allow_network: bool) -> bool {
    let mut probe_args: Vec<&str> = vec![
        "--ro-bind",
        "/usr",
        "/usr",
        "--ro-bind",
        "/lib",
        "/lib",
        "--ro-bind",
        "/lib64",
        "/lib64",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--unshare-all",
    ];
    if allow_network {
        probe_args.push("--share-net");
    }
    probe_args.extend(["--", "true"]);

    std::process::Command::new("bwrap")
        .args(&probe_args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ── Landlock ──────────────────────────────────────────────────────────────────

/// Apply Landlock filesystem write restrictions described by `config`.
///
/// When `allow_write` is empty, no restrictions are applied. When Landlock is
/// not supported by the kernel (ENOSYS, ABI 0), the function degrades
/// gracefully and returns `Ok(())`. This should be called in the current
/// process immediately before a POSIX `exec()` so restrictions are inherited
/// by the exec'd process.
#[cfg(target_os = "linux")]
pub(crate) fn apply_landlock_restrictions(config: &SandboxConfig) -> Result<(), SandboxError> {
    // Nothing to restrict if no write paths are configured.
    if config.allow_write.is_empty() {
        return Ok(());
    }

    let abi = landlock::detect_abi();
    if abi == 0 {
        // Kernel < 5.13 or Landlock not compiled in — degrade gracefully.
        return Ok(());
    }

    // Build handled_access_fs mask for the detected ABI.
    let mut handled_fs = landlock::ALL_WRITE_V1;
    if abi >= 2 {
        handled_fs |= landlock::ACCESS_FS_REFER;
    }
    if abi >= 3 {
        handled_fs |= landlock::ACCESS_FS_TRUNCATE;
    }

    let attr = landlock::RulesetAttr {
        handled_access_fs: handled_fs,
        handled_access_net: 0,
    };
    let size = std::mem::size_of::<landlock::RulesetAttr>();

    let ruleset = landlock::create_ruleset(&attr, size)
        .map_err(|e| SandboxError::Execution(format!("landlock create_ruleset: {e}")))?;

    for path in &config.allow_write {
        let canonical = path.canonicalize().map_err(|e| {
            SandboxError::Execution(format!("canonicalize {}: {e}", path.display()))
        })?;
        let dir_file = std::fs::File::open(&canonical)
            .map_err(|e| SandboxError::Execution(format!("open {}: {e}", canonical.display())))?;

        use std::os::unix::io::{AsFd, AsRawFd};
        let rule_attr = landlock::PathBeneathAttr {
            allowed_access: handled_fs,
            parent_fd: dir_file.as_raw_fd(),
        };
        landlock::add_path_beneath(ruleset.as_fd(), &rule_attr)
            .map_err(|e| SandboxError::Execution(format!("landlock add_rule: {e}")))?;
    }

    use std::os::unix::io::AsFd;
    landlock::restrict_self(ruleset.as_fd())
        .map_err(|e| SandboxError::Execution(format!("landlock restrict_self: {e}")))?;

    Ok(())
}

#[cfg(target_os = "linux")]
mod landlock {
    // Landlock filesystem access rights (from linux/landlock.h).
    pub const ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
    pub const ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
    pub const ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
    pub const ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
    pub const ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
    pub const ACCESS_FS_MAKE_REG: u64 = 1 << 8;
    pub const ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
    pub const ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
    pub const ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
    pub const ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
    /// ABI 2+ only.
    pub const ACCESS_FS_REFER: u64 = 1 << 13;
    /// ABI 3+ only.
    pub const ACCESS_FS_TRUNCATE: u64 = 1 << 14;

    /// All write-related accesses supported in ABI 1 (baseline).
    pub const ALL_WRITE_V1: u64 = ACCESS_FS_WRITE_FILE
        | ACCESS_FS_REMOVE_DIR
        | ACCESS_FS_REMOVE_FILE
        | ACCESS_FS_MAKE_CHAR
        | ACCESS_FS_MAKE_DIR
        | ACCESS_FS_MAKE_REG
        | ACCESS_FS_MAKE_SOCK
        | ACCESS_FS_MAKE_FIFO
        | ACCESS_FS_MAKE_BLOCK
        | ACCESS_FS_MAKE_SYM;

    pub const RULE_PATH_BENEATH: u32 = 1;
    /// Flag for `landlock_create_ruleset` that returns the ABI version instead
    /// of creating a ruleset.
    pub const CREATE_RULESET_VERSION: u32 = 1;

    #[repr(C)]
    pub struct RulesetAttr {
        pub handled_access_fs: u64,
        pub handled_access_net: u64,
    }

    #[repr(C)]
    pub struct PathBeneathAttr {
        pub allowed_access: u64,
        pub parent_fd: i32,
    }

    /// Return the Landlock ABI version supported by this kernel, or 0 if
    /// Landlock is not available (kernel < 5.13 or not compiled in).
    pub fn detect_abi() -> u32 {
        // SAFETY: SYS_landlock_create_ruleset with the version flag is a
        // read-only query syscall that cannot cause side effects.
        let ret = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<RulesetAttr>(),
                0usize,
                CREATE_RULESET_VERSION,
            )
        };
        if ret < 0 { 0 } else { ret as u32 }
    }

    pub fn create_ruleset(
        attr: &RulesetAttr,
        size: usize,
    ) -> std::io::Result<std::os::unix::io::OwnedFd> {
        use std::os::unix::io::FromRawFd;
        // SAFETY: SYS_landlock_create_ruleset creates a new file descriptor; we
        // take ownership via OwnedFd if the call succeeds.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                attr as *const _ as *const libc::c_void,
                size,
                0u32,
            )
        };
        if fd < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(unsafe { std::os::unix::io::OwnedFd::from_raw_fd(fd as std::os::unix::io::RawFd) })
        }
    }

    pub fn add_path_beneath(
        ruleset_fd: std::os::unix::io::BorrowedFd<'_>,
        attr: &PathBeneathAttr,
    ) -> std::io::Result<()> {
        use std::os::unix::io::AsRawFd;
        // SAFETY: valid file descriptors and well-formed attr struct.
        let ret = unsafe {
            libc::syscall(
                libc::SYS_landlock_add_rule,
                ruleset_fd.as_raw_fd(),
                RULE_PATH_BENEATH,
                attr as *const _ as *const libc::c_void,
                0u32,
            )
        };
        if ret != 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub fn restrict_self(ruleset_fd: std::os::unix::io::BorrowedFd<'_>) -> std::io::Result<()> {
        use std::os::unix::io::AsRawFd;
        // SAFETY: valid file descriptor; restricts the calling thread.
        let ret = unsafe {
            libc::syscall(
                libc::SYS_landlock_restrict_self,
                ruleset_fd.as_raw_fd(),
                0u32,
            )
        };
        if ret != 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

// ── bwrap argument builder ────────────────────────────────────────────────────

/// Build the `bwrap` argument list for the given `config`.
///
/// Canonicalizes each path in `allow_write` to prevent relative-path or
/// symlink confusion. Returns an error if a path cannot be canonicalized
/// (e.g. it does not exist).
#[cfg(target_os = "linux")]
pub(crate) fn build_bwrap_args(config: &SandboxConfig) -> Result<Vec<OsString>, SandboxError> {
    let mut args: Vec<OsString> = vec![
        "--ro-bind".into(),
        "/usr".into(),
        "/usr".into(),
        "--ro-bind".into(),
        "/lib".into(),
        "/lib".into(),
        "--ro-bind".into(),
        "/lib64".into(),
        "/lib64".into(),
        "--proc".into(),
        "/proc".into(),
        "--dev".into(),
        "/dev".into(),
        "--unshare-all".into(),
    ];

    if config.allow_network {
        args.push("--share-net".into());
    }

    for path in &config.allow_write {
        let canonical = path.canonicalize().map_err(|e| {
            SandboxError::Execution(format!("allow_write path {}: {e}", path.display()))
        })?;
        args.push("--bind".into());
        args.push(canonical.as_os_str().to_owned());
        args.push(canonical.as_os_str().to_owned());
    }

    Ok(args)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

pub(crate) fn run_unavailable_result(required: bool) -> Result<SandboxResult, SandboxError> {
    if required {
        Err(SandboxError::Required)
    } else {
        warn_sandbox_bypass();
        Ok(SandboxResult::Unavailable)
    }
}

/// Emit a structured warning when a configured sandbox is bypassed.
///
/// A bypass means the command will run unconfined because the sandbox could
/// not be applied and `required = false`. The audit log records this as
/// `SandboxStatus::Unavailable`; this `tracing` event surfaces it live.
fn warn_sandbox_bypass() {
    tracing::warn!(
        target: "aegis::sandbox",
        "sandbox unavailable; proceeding without confinement (set sandbox.required = true to make this a hard block)"
    );
}

#[cfg(target_os = "linux")]
pub(crate) fn sysctl_userns_available() -> bool {
    std::fs::read_to_string("/proc/sys/kernel/unprivileged_userns_clone")
        .map(|v| v.trim() == "1")
        .unwrap_or(true)
}

// ── Windows Job Objects ───────────────────────────────────────────────────────

/// Windows Job Object FFI and [`JobObject`] RAII wrapper.
///
/// All Win32 types are declared inline — no external crate dependency,
/// following the same pattern as the Linux [`landlock`] module.
#[cfg(windows)]
pub(crate) mod job_object {
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};

    /// Raw Win32 `HANDLE` — a nullable pointer to an opaque kernel object.
    pub type HANDLE = *mut std::ffi::c_void;
    type BOOL = i32;
    type DWORD = u32;

    /// Kill all processes in the job when the last job handle is closed.
    pub const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: DWORD = 0x0000_2000;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: u32 = 9;

    #[repr(C)]
    struct BasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: DWORD,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: DWORD,
        affinity: usize,
        priority_class: DWORD,
        scheduling_class: DWORD,
    }

    // IO_COUNTERS is 6 × u64 = 48 bytes.
    #[repr(C)]
    struct ExtendedLimitInformation {
        basic_limit_information: BasicLimitInformation,
        io_info: [u64; 6],
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    unsafe extern "system" {
        fn CreateJobObjectW(job_attributes: *const std::ffi::c_void, name: *const u16) -> HANDLE;
        fn SetInformationJobObject(
            job_handle: HANDLE,
            job_object_information_class: u32,
            job_object_information: *const std::ffi::c_void,
            job_object_information_length: DWORD,
        ) -> BOOL;
        fn AssignProcessToJobObject(job_handle: HANDLE, process_handle: HANDLE) -> BOOL;
        fn QueryInformationJobObject(
            job_handle: HANDLE,
            job_object_information_class: u32,
            job_object_information: *mut std::ffi::c_void,
            job_object_information_length: DWORD,
            return_length: *mut DWORD,
        ) -> BOOL;
    }

    /// An owned Windows Job Object.
    ///
    /// The kernel handle is closed automatically on drop via [`OwnedHandle`].
    pub struct JobObject {
        handle: OwnedHandle,
    }

    impl JobObject {
        /// Create an anonymous Job Object with [`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`] set.
        pub fn new() -> io::Result<Self> {
            // SAFETY: CreateJobObjectW creates a new Job Object handle.
            // Null params → anonymous, unnamed job object.
            let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if raw.is_null() {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: raw is a valid, owned HANDLE returned by the OS.
            let owned = unsafe { OwnedHandle::from_raw_handle(raw as RawHandle) };
            let job = Self { handle: owned };
            job.set_kill_on_close()?;
            Ok(job)
        }

        fn set_kill_on_close(&self) -> io::Result<()> {
            let info = ExtendedLimitInformation {
                basic_limit_information: BasicLimitInformation {
                    per_process_user_time_limit: 0,
                    per_job_user_time_limit: 0,
                    limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                    minimum_working_set_size: 0,
                    maximum_working_set_size: 0,
                    active_process_limit: 0,
                    affinity: 0,
                    priority_class: 0,
                    scheduling_class: 0,
                },
                io_info: [0u64; 6],
                process_memory_limit: 0,
                job_memory_limit: 0,
                peak_process_memory_used: 0,
                peak_job_memory_used: 0,
            };
            // SAFETY: valid HANDLE (owned by self), well-formed #[repr(C)] struct.
            let ok = unsafe {
                SetInformationJobObject(
                    self.handle.as_raw_handle() as HANDLE,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                    &info as *const _ as *const std::ffi::c_void,
                    std::mem::size_of::<ExtendedLimitInformation>() as DWORD,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        /// Return `true` if `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is set.
        pub fn has_kill_on_close_limit(&self) -> bool {
            // SAFETY: zeroed memory is valid for this plain-data C struct.
            let mut info: ExtendedLimitInformation = unsafe { std::mem::zeroed() };
            let mut ret_len: DWORD = 0;
            // SAFETY: valid HANDLE, valid buffer, correct size.
            let ok = unsafe {
                QueryInformationJobObject(
                    self.handle.as_raw_handle() as HANDLE,
                    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                    &mut info as *mut _ as *mut std::ffi::c_void,
                    std::mem::size_of::<ExtendedLimitInformation>() as DWORD,
                    &mut ret_len,
                )
            };
            ok != 0
                && (info.basic_limit_information.limit_flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE)
                    != 0
        }

        /// Assign `process_handle` to this Job Object.
        ///
        /// # Safety contract for callers
        /// `process_handle` must be a valid, open Win32 PROCESS handle for the
        /// duration of this call. The caller guarantees this by holding the
        /// `Child` alive (via `child.as_raw_handle()`).
        pub fn assign_process(&self, process_handle: HANDLE) -> io::Result<()> {
            // SAFETY: `self.handle` is valid for &self's lifetime (OwnedHandle).
            // `process_handle` is valid while the Child is in scope — see
            // `run_in_job_object` where the borrow is live across this call.
            let ok = unsafe {
                AssignProcessToJobObject(self.handle.as_raw_handle() as HANDLE, process_handle)
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }
}

#[cfg(windows)]
fn run_in_job_object(cmd: &str) -> Result<SandboxResult, SandboxError> {
    use job_object::JobObject;
    use std::os::windows::io::AsRawHandle;

    let job = JobObject::new()
        .map_err(|e| SandboxError::SetupFailed(format!("CreateJobObjectW: {e}")))?;

    let mut child = std::process::Command::new("cmd")
        .args(["/c", cmd])
        .spawn()
        .map_err(|e| SandboxError::Execution(e.to_string()))?;

    // SAFETY: child.as_raw_handle() is valid while `child` is alive.
    // TOCTOU: there is a narrow window between spawn() and AssignProcessToJobObject()
    // where the child could spawn its own children that escape the Job Object.
    // Full elimination requires CREATE_SUSPENDED + ResumeThread (future work).
    // Error path is made fail-closed by killing the child on assignment failure.
    if let Err(e) = job.assign_process(child.as_raw_handle() as job_object::HANDLE) {
        // Kill the unsandboxed child before returning the error.
        let _ = child.kill();
        return Err(SandboxError::SetupFailed(format!(
            "AssignProcessToJobObject: {e}"
        )));
    }

    let status = child
        .wait()
        .map_err(|e| SandboxError::Execution(e.to_string()))?;

    Ok(SandboxResult::Success(status.code().unwrap_or(-1)))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    struct ForceUnavailableGuard;
    impl Drop for ForceUnavailableGuard {
        fn drop(&mut self) {
            set_force_sandbox_unavailable(false);
        }
    }

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

    // ── Non-Linux fallback logic ──────────────────────────────────────────────

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

    // ── Sandbox bypass is an audit/log event (ROADMAP 6.4) ────────────────────

    /// Minimal `tracing::Subscriber` that counts WARN events on the
    /// `aegis::sandbox` target, so tests can assert a bypass was reported.
    #[derive(Clone, Default)]
    struct WarnCounter(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl tracing::Subscriber for WarnCounter {
        fn enabled(&self, _meta: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            let meta = event.metadata();
            if *meta.level() == tracing::Level::WARN && meta.target() == "aegis::sandbox" {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        fn enter(&self, _span: &tracing::span::Id) {}
        fn exit(&self, _span: &tracing::span::Id) {}
    }

    #[test]
    fn bypass_emits_warning_when_not_required() {
        let counter = WarnCounter::default();
        let count = counter.0.clone();
        tracing::subscriber::with_default(counter, || {
            let _ = run_unavailable_result(false);
        });
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn hard_block_does_not_emit_bypass_warning() {
        let counter = WarnCounter::default();
        let count = counter.0.clone();
        tracing::subscriber::with_default(counter, || {
            let _ = run_unavailable_result(true);
        });
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    // ── SandboxError::Display ─────────────────────────────────────────────────

    #[test]
    fn sandbox_error_required_display_mentions_required_or_unavailable() {
        let msg = SandboxError::Required.to_string().to_lowercase();
        assert!(msg.contains("required") || msg.contains("unavailable"));
    }

    // ── Linux: forced-unavailable via thread-local ────────────────────────────

    #[cfg(target_os = "linux")]
    #[test]
    fn forced_unavailable_with_required_true_returns_required_error() {
        set_force_sandbox_unavailable(true);
        let _guard = ForceUnavailableGuard;

        let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig {
            required: true,
            ..Default::default()
        }));
        assert!(matches!(executor.run("true"), Err(SandboxError::Required)));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn forced_unavailable_with_required_false_returns_unavailable() {
        set_force_sandbox_unavailable(true);
        let _guard = ForceUnavailableGuard;

        let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig {
            required: false,
            ..Default::default()
        }));
        assert!(matches!(
            executor.run("true"),
            Ok(SandboxResult::Unavailable)
        ));
    }

    // ── Linux: run() accepts both outcomes when sandbox may or may not work ───

    #[cfg(target_os = "linux")]
    #[test]
    fn run_with_required_false_never_returns_hard_error_from_unavailability() {
        let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig {
            required: false,
            ..Default::default()
        }));
        match executor.run("true") {
            Ok(SandboxResult::Unavailable) | Ok(SandboxResult::Success(_)) => {}
            Err(e) => panic!("unexpected error when required=false: {e}"),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn run_with_required_true_never_returns_unavailable_ok() {
        let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig {
            required: true,
            ..Default::default()
        }));
        match executor.run("true") {
            Ok(SandboxResult::Unavailable) => {
                panic!("Ok(Unavailable) must never be returned when required=true")
            }
            Ok(SandboxResult::Success(_)) | Err(_) => {}
        }
    }

    // ── Linux: sandbox_available_for reflects forced-unavailable ─────────────

    #[cfg(target_os = "linux")]
    #[test]
    fn sandbox_available_for_returns_false_when_forced_unavailable() {
        set_force_sandbox_unavailable(true);
        let _guard = ForceUnavailableGuard;
        assert!(!sandbox_available_for(&SandboxConfig::default()));
    }

    // ── Linux: Landlock restrictions (callable, gracefully degrades) ──────────

    #[cfg(target_os = "linux")]
    #[test]
    fn apply_landlock_restrictions_ok_on_empty_allow_write() {
        // No write paths → no Landlock ruleset created → Ok(()).
        assert!(apply_landlock_restrictions(&SandboxConfig::default()).is_ok());
    }

    // ── Linux: bwrap argument builder ────────────────────────────────────────

    #[cfg(target_os = "linux")]
    #[test]
    fn bwrap_args_include_bind_for_tmp_when_in_allow_write() {
        let cfg = SandboxConfig {
            allow_write: vec![PathBuf::from("/tmp")],
            ..Default::default()
        };
        let args = build_bwrap_args(&cfg).expect("build_bwrap_args failed");
        let has_bind_tmp = args.windows(3).any(|w| {
            // canonical /tmp is /tmp
            w[0].as_os_str() == "--bind" && w[1].as_os_str() == "/tmp" && w[2].as_os_str() == "/tmp"
        });
        assert!(has_bind_tmp, "expected --bind /tmp /tmp, got: {args:?}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn bwrap_args_include_share_net_when_allow_network_true() {
        let cfg = SandboxConfig {
            allow_network: true,
            ..Default::default()
        };
        let args = build_bwrap_args(&cfg).expect("build_bwrap_args failed");
        assert!(
            args.iter().any(|a| a.as_os_str() == "--share-net"),
            "expected --share-net, got: {args:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn bwrap_args_share_net_appears_before_bind_when_both_present() {
        let cfg = SandboxConfig {
            allow_write: vec![PathBuf::from("/tmp")],
            allow_network: true,
            ..Default::default()
        };
        let args = build_bwrap_args(&cfg).expect("build_bwrap_args failed");

        let share_pos = args
            .iter()
            .position(|a| a.as_os_str() == "--share-net")
            .expect("--share-net missing");
        let bind_pos = args
            .windows(3)
            .position(|w| {
                w[0].as_os_str() == "--bind"
                    && w[1].as_os_str() == "/tmp"
                    && w[2].as_os_str() == "/tmp"
            })
            .expect("--bind /tmp /tmp missing");

        assert!(share_pos < bind_pos, "--share-net must precede --bind");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn bwrap_args_fails_for_nonexistent_allow_write_path() {
        let cfg = SandboxConfig {
            allow_write: vec![PathBuf::from("/nonexistent_aegis_test_path_xyz")],
            ..Default::default()
        };
        assert!(
            build_bwrap_args(&cfg).is_err(),
            "expected Err for non-existent allow_write path"
        );
    }

    // ── Linux: sysctl probe ───────────────────────────────────────────────────

    #[cfg(target_os = "linux")]
    #[test]
    fn sysctl_userns_available_returns_true_when_file_absent() {
        let file_present =
            std::path::Path::new("/proc/sys/kernel/unprivileged_userns_clone").exists();
        if !file_present {
            assert!(
                sysctl_userns_available(),
                "must return true when sysctl file is absent"
            );
        }
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

    #[cfg(target_os = "linux")]
    #[test]
    fn test_sandbox_unavailable_is_non_fatal_when_not_required() {
        let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig {
            required: false,
            ..Default::default()
        }));
        match executor.run("true") {
            Ok(SandboxResult::Unavailable) | Ok(SandboxResult::Success(_)) => {}
            Err(e) => {
                panic!("expected Ok(Unavailable) or Ok(Success) when required=false, got Err({e})")
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_sandbox_unavailable_is_error_when_required() {
        let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig {
            required: true,
            ..Default::default()
        }));
        match executor.run("true") {
            Err(SandboxError::Required) => {}
            Ok(SandboxResult::Success(_)) => {}
            Ok(SandboxResult::Unavailable) => {
                panic!("expected Err(SandboxError::Required) or Ok(Success) when required=true")
            }
            Err(other) => {
                panic!("expected Err(SandboxError::Required) or Ok(Success), got Err({other})")
            }
        }
    }

    #[test]
    fn test_sandbox_error_implements_display() {
        let msg = SandboxError::Required.to_string();
        assert!(!msg.is_empty());
        let lower = msg.to_lowercase();
        assert!(lower.contains("required") || lower.contains("unavailable"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_forced_unavailable_with_required_returns_error() {
        set_force_sandbox_unavailable(true);
        let _guard = ForceUnavailableGuard;

        assert!(is_forced_sandbox_unavailable());

        let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig {
            required: true,
            ..Default::default()
        }));
        match executor.run("true") {
            Err(SandboxError::Required) => {}
            Ok(_) => panic!(
                "expected Err(SandboxError::Required) when forced-unavailable and required=true"
            ),
            Err(other) => panic!(
                "expected Err(SandboxError::Required) when forced-unavailable, got Err({other})"
            ),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_forced_unavailable_without_required_returns_unavailable() {
        set_force_sandbox_unavailable(true);
        let _guard = ForceUnavailableGuard;

        assert!(is_forced_sandbox_unavailable());

        let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig {
            required: false,
            ..Default::default()
        }));
        match executor.run("true") {
            Ok(SandboxResult::Unavailable) => {}
            Ok(SandboxResult::Success(code)) => {
                panic!("expected Ok(Unavailable) when forced-unavailable, got Ok(Success({code}))")
            }
            Err(e) => panic!("expected Ok(Unavailable) when forced-unavailable, got Err({e})"),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_landlock_stub_is_callable() {
        assert!(apply_landlock_restrictions(&SandboxConfig::default()).is_ok());
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

    #[cfg(target_os = "linux")]
    #[test]
    fn test_bwrap_args_include_allow_write_paths() {
        let cfg = SandboxConfig {
            allow_write: vec![std::path::PathBuf::from("/tmp")],
            ..Default::default()
        };
        let args = build_bwrap_args(&cfg).expect("build_bwrap_args must succeed for /tmp");
        let found = args.windows(3).any(|w| {
            w[0].as_os_str() == "--bind" && w[1].as_os_str() == "/tmp" && w[2].as_os_str() == "/tmp"
        });
        assert!(
            found,
            "build_bwrap_args must emit --bind /tmp /tmp for allow_write=[/tmp], got: {args:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_bwrap_args_include_share_net_when_network_allowed() {
        let cfg = SandboxConfig {
            allow_network: true,
            ..Default::default()
        };
        let args = build_bwrap_args(&cfg).expect("build_bwrap_args must succeed");
        assert!(
            args.iter().any(|a| a.as_os_str() == "--share-net"),
            "build_bwrap_args must include --share-net when allow_network=true, got: {args:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_share_net_appears_before_bind_mounts() {
        let cfg = SandboxConfig {
            allow_write: vec![std::path::PathBuf::from("/tmp")],
            allow_network: true,
            ..Default::default()
        };
        let args = build_bwrap_args(&cfg).expect("build_bwrap_args must succeed");

        let share_net_pos = args
            .iter()
            .position(|a| a.as_os_str() == "--share-net")
            .expect("--share-net must be present in args when allow_network=true");

        let bind_pos = args
            .windows(3)
            .position(|w| {
                w[0].as_os_str() == "--bind"
                    && w[1].as_os_str() == "/tmp"
                    && w[2].as_os_str() == "/tmp"
            })
            .expect("--bind /tmp /tmp must be present in args when allow_write=[/tmp]");

        assert!(
            share_net_pos < bind_pos,
            "--share-net (pos {share_net_pos}) must appear BEFORE --bind /tmp /tmp \
             (pos {bind_pos}). Full args: {args:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_sysctl_userns_available_returns_true_when_file_missing() {
        let file_present =
            std::path::Path::new("/proc/sys/kernel/unprivileged_userns_clone").exists();

        if file_present {
            let expected = std::fs::read_to_string("/proc/sys/kernel/unprivileged_userns_clone")
                .map(|v| v.trim() == "1")
                .unwrap_or(true);
            assert_eq!(sysctl_userns_available(), expected);
        } else {
            assert!(sysctl_userns_available());
        }
    }

    // ── macOS: Seatbelt profile generation ───────────────────────────────────

    #[cfg(target_os = "macos")]
    #[test]
    fn test_build_seatbelt_profile_contains_deny_default() {
        let cfg = SandboxConfig::default();
        let profile =
            super::build_seatbelt_profile(&cfg).expect("default config must build cleanly");
        assert!(
            profile.contains("(deny default)"),
            "seatbelt profile must contain '(deny default)', got: {profile}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_build_seatbelt_profile_contains_version_1() {
        let cfg = SandboxConfig::default();
        let profile =
            super::build_seatbelt_profile(&cfg).expect("default config must build cleanly");
        assert!(
            profile.contains("(version 1)"),
            "seatbelt profile must contain '(version 1)', got: {profile}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_build_seatbelt_profile_allows_write_path_when_configured() {
        let cfg = SandboxConfig {
            allow_write: vec![PathBuf::from("/tmp")],
            ..Default::default()
        };
        let profile =
            super::build_seatbelt_profile(&cfg).expect("/tmp must exist and be canonicalizable");
        // On macOS /tmp is a symlink to /private/tmp — use the canonical form.
        let canonical_tmp = PathBuf::from("/tmp")
            .canonicalize()
            .expect("/tmp must exist");
        let expected = format!(
            "(allow file-write* (subpath \"{}\"))",
            canonical_tmp.display()
        );
        assert!(
            profile.contains(&expected),
            "seatbelt profile must contain write-allow rule for {}, got: {profile}",
            canonical_tmp.display()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_build_seatbelt_profile_network_allowed_when_configured() {
        let cfg = SandboxConfig {
            allow_network: true,
            ..Default::default()
        };
        let profile =
            super::build_seatbelt_profile(&cfg).expect("network-only config must build cleanly");
        assert!(
            profile.contains("(allow network*)"),
            "seatbelt profile must contain '(allow network*)' when allow_network=true, got: {profile}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_build_seatbelt_profile_denies_network_by_default() {
        let cfg = SandboxConfig {
            allow_network: false,
            ..Default::default()
        };
        let profile =
            super::build_seatbelt_profile(&cfg).expect("default network config must build cleanly");
        assert!(
            profile.contains("(deny network*)"),
            "seatbelt profile must contain '(deny network*)' when allow_network=false, got: {profile}"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_build_seatbelt_profile_no_write_rules_when_allow_write_empty() {
        let cfg = SandboxConfig {
            allow_write: vec![],
            ..Default::default()
        };
        let profile = super::build_seatbelt_profile(&cfg)
            .expect("empty allow_write config must build cleanly");
        assert!(
            !profile.contains("allow file-write*"),
            "seatbelt profile must NOT contain 'allow file-write*' when allow_write is empty, got: {profile}"
        );
    }

    // ── macOS: sandbox_available_for with forced-unavailable ─────────────────

    /// `sandbox_available_for` on macOS must delegate to `is_sandbox_exec_available()`
    /// and respect the forced-unavailable flag.  Before Phase 6.2 the function
    /// hard-returns `false` without consulting the force flag OR the binary probe,
    /// so the forced-unavailable result is coincidentally correct but the positive
    /// case (when sandbox-exec IS present and force IS false) would be wrong.
    ///
    /// This test drives that: it calls `is_sandbox_exec_available()` directly,
    /// which does not exist yet → compile error on macOS.
    #[cfg(target_os = "macos")]
    #[test]
    fn test_sandbox_available_for_returns_false_when_forced_unavailable_macos() {
        set_force_sandbox_unavailable(true);
        let _guard = ForceUnavailableGuard;

        // Calling is_sandbox_exec_available() will fail to compile until
        // Phase 6.2 adds the function.
        let _exec_present = super::is_sandbox_exec_available();
        assert!(
            !sandbox_available_for(&SandboxConfig::default()),
            "sandbox_available_for must return false when FORCE_SANDBOX_UNAVAILABLE is set"
        );
    }

    // ── macOS: SandboxExecutor::run with forced-unavailable ───────────────────

    #[cfg(target_os = "macos")]
    #[test]
    fn test_run_macos_forced_unavailable_required_false_returns_unavailable() {
        set_force_sandbox_unavailable(true);
        let _guard = ForceUnavailableGuard;

        // Verify is_sandbox_exec_available() exists as part of Phase 6.2.
        // This call will fail to compile until the function is implemented.
        let _ = super::is_sandbox_exec_available();

        let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig {
            required: false,
            ..Default::default()
        }));
        assert!(
            matches!(executor.run("true"), Ok(SandboxResult::Unavailable)),
            "run() must return Ok(Unavailable) when forced-unavailable and required=false"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_run_macos_forced_unavailable_required_true_returns_required_error() {
        set_force_sandbox_unavailable(true);
        let _guard = ForceUnavailableGuard;

        // Verify is_sandbox_exec_available() exists as part of Phase 6.2.
        // This call will fail to compile until the function is implemented.
        let _ = super::is_sandbox_exec_available();

        let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig {
            required: true,
            ..Default::default()
        }));
        assert!(
            matches!(executor.run("true"), Err(SandboxError::Required)),
            "run() must return Err(SandboxError::Required) when forced-unavailable and required=true"
        );
    }

    // ── macOS: prepare_for_exec with forced-unavailable ───────────────────────

    #[cfg(target_os = "macos")]
    #[test]
    fn test_prepare_for_exec_macos_unavailable_required_false_returns_direct_command() {
        use std::ffi::{OsStr, OsString};

        set_force_sandbox_unavailable(true);
        let _guard = ForceUnavailableGuard;

        let cfg = SandboxConfig {
            required: false,
            ..Default::default()
        };
        let program = OsStr::new("echo");
        let args: Vec<OsString> = vec![OsString::from("hello")];

        let result = super::prepare_for_exec(&cfg, program, &args);
        assert!(
            result.is_ok(),
            "prepare_for_exec must return Ok when forced-unavailable and required=false, got: {result:?}"
        );
        let cmd = result.expect("checked above");
        // The program must NOT be sandbox-exec — it should be a direct command.
        assert_ne!(
            cmd.get_program(),
            OsStr::new("sandbox-exec"),
            "prepare_for_exec must return a direct (non-sandbox-exec) command when sandbox is unavailable"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_prepare_for_exec_macos_unavailable_required_true_returns_required_error() {
        use std::ffi::{OsStr, OsString};

        set_force_sandbox_unavailable(true);
        let _guard = ForceUnavailableGuard;

        let cfg = SandboxConfig {
            required: true,
            ..Default::default()
        };
        let program = OsStr::new("echo");
        let args: Vec<OsString> = vec![OsString::from("hello")];

        let result = super::prepare_for_exec(&cfg, program, &args);
        assert!(
            matches!(result, Err(SandboxError::Required)),
            "prepare_for_exec must return Err(SandboxError::Required) when forced-unavailable and required=true, got: {result:?}"
        );
    }

    // ── macOS: SBPL path escaping ────────────────────────────────────────────
    // Test escape_sbpl_path directly — build_seatbelt_profile canonicalizes
    // paths, so paths with special chars that don't exist on disk would fail
    // before reaching the escaping logic.

    #[cfg(target_os = "macos")]
    #[test]
    fn escape_sbpl_path_escapes_double_quote() {
        let path = std::path::Path::new("/tmp/fo\"o");
        let escaped = super::escape_sbpl_path(path).expect("ASCII path must not fail");
        assert_eq!(
            escaped, r#"/tmp/fo\"o"#,
            "double-quote must be escaped as \\\""
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn escape_sbpl_path_escapes_backslash() {
        let path = std::path::Path::new("/tmp/fo\\o");
        let escaped = super::escape_sbpl_path(path).expect("ASCII path must not fail");
        assert_eq!(
            escaped, r#"/tmp/fo\\o"#,
            "backslash must be escaped as \\\\"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn escape_sbpl_path_escapes_backslash_before_quote_to_avoid_double_escaping() {
        // Input: one backslash + one quote → correct output: \\ (escaped \) + \" (escaped ") = 3 chars.
        // Wrong order (escape " first) would produce \\\\" (4 backslashes + quote).
        let path = std::path::Path::new("/tmp/fo\\\"o");
        let escaped = super::escape_sbpl_path(path).expect("ASCII path must not fail");
        assert_eq!(escaped, r#"/tmp/fo\\\"o"#);
    }

    // ── macOS: static profile drift detection ────────────────────────────────

    #[cfg(target_os = "macos")]
    #[test]
    fn static_default_profile_matches_dynamic_output() {
        let static_profile = include_str!("../profiles/default.sbpl");
        let dynamic = super::build_seatbelt_profile(&SandboxConfig::default())
            .expect("default config must build cleanly");
        assert_eq!(
            dynamic.trim(),
            static_profile.trim(),
            "profiles/default.sbpl has drifted from build_seatbelt_profile output"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn static_network_profile_matches_dynamic_output() {
        let static_profile = include_str!("../profiles/network.sbpl");
        let dynamic = super::build_seatbelt_profile(&SandboxConfig {
            allow_network: true,
            ..Default::default()
        })
        .expect("network config must build cleanly");
        assert_eq!(
            dynamic.trim(),
            static_profile.trim(),
            "profiles/network.sbpl has drifted from build_seatbelt_profile output"
        );
    }

    // ── macOS: runtime sandbox execution tests ───────────────────────────────
    // These tests require a real sandbox-exec binary and kernel Seatbelt support.
    // They skip gracefully when the sandbox is unavailable (e.g. stripped macOS).

    #[cfg(target_os = "macos")]
    #[test]
    fn run_basic_command_succeeds_in_sandbox() {
        if !super::is_sandbox_exec_available() {
            return;
        }
        let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig::default()));
        match executor.run("true") {
            Ok(SandboxResult::Success(0)) | Ok(SandboxResult::Unavailable) => {}
            other => panic!("expected Success(0) or Unavailable, got {other:?}"),
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn run_write_to_allowed_path_succeeds() {
        if !super::is_sandbox_exec_available() {
            return;
        }
        let dir = std::env::temp_dir();
        let cfg = SandboxConfig {
            allow_write: vec![dir.clone()],
            ..Default::default()
        };
        let executor = SandboxExecutor::new(SandboxProfile::from_config(&cfg));
        let test_file = dir.join("aegis_sandbox_test_write_allowed.tmp");
        let cmd = format!("touch {}", test_file.display());
        match executor.run(&cmd) {
            Ok(SandboxResult::Success(0)) => {
                let _ = std::fs::remove_file(&test_file);
            }
            Ok(SandboxResult::Unavailable) => {}
            other => panic!("expected Success(0) for write to allowed path, got {other:?}"),
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn run_write_is_blocked_when_allow_write_is_empty() {
        if !super::is_sandbox_exec_available() {
            return;
        }
        // With (deny default) and no allow_write rules, any write must be blocked.
        let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig::default()));
        match executor.run("touch /tmp/aegis_sandbox_blocked_test.tmp") {
            Ok(SandboxResult::Success(0)) => {
                panic!("write must be blocked by sandbox when allow_write is empty")
            }
            Ok(SandboxResult::Success(_)) | Ok(SandboxResult::Unavailable) | Err(_) => {}
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn run_network_is_blocked_when_allow_network_is_false() {
        if !super::is_sandbox_exec_available() {
            return;
        }
        let cfg = SandboxConfig {
            allow_network: false,
            ..Default::default()
        };
        let executor = SandboxExecutor::new(SandboxProfile::from_config(&cfg));
        // ping requires a network socket; blocked sandbox returns non-zero.
        match executor.run("ping -c 1 -t 1 127.0.0.1 2>/dev/null") {
            Ok(SandboxResult::Success(0)) => {
                panic!("network operation must fail when allow_network=false")
            }
            Ok(SandboxResult::Success(_)) | Ok(SandboxResult::Unavailable) | Err(_) => {}
        }
    }

    // ── Windows: Job Object sandbox tests ────────────────────────────────────
    //
    // These tests are gated on `#[cfg(windows)]`. On Linux they are compiled out
    // entirely (no-op). On Windows they drive the Phase 6.3 implementation:
    //
    //   - Tests 1-4 drive the public API surface (`sandbox_available_for`, `run`).
    //   - Tests 5, 7 reference `job_object::JobObject` and
    //     `job_object::JobObject::has_kill_on_close_limit`, which do not exist yet
    //     → compile error on Windows until Phase 6.3 adds `pub(crate) mod job_object`.
    //   - Test 6 drives the actual subprocess path through a Job Object.

    #[cfg(windows)]
    mod windows_tests {
        use super::*;

        struct ForceUnavailableGuard;
        impl Drop for ForceUnavailableGuard {
            fn drop(&mut self) {
                set_force_sandbox_unavailable(false);
            }
        }

        // Test 1: sandbox_available_for returns true on Windows when the
        // forced-unavailable flag is clear.
        //
        // Currently the fallback branch always returns `false`, so this test
        // drives the addition of a `#[cfg(windows)]` branch that returns
        // `!is_forced_sandbox_unavailable()`.
        #[test]
        fn test_sandbox_available_for_returns_true_on_windows() {
            assert!(
                sandbox_available_for(&SandboxConfig::default()),
                "sandbox_available_for must return true on Windows when the sandbox flag is not forced"
            );
        }

        // Test 2: forced-unavailable flag is respected on Windows.
        #[test]
        fn test_sandbox_available_for_returns_false_when_forced_unavailable_windows() {
            set_force_sandbox_unavailable(true);
            let _guard = ForceUnavailableGuard;
            assert!(
                !sandbox_available_for(&SandboxConfig::default()),
                "sandbox_available_for must return false when FORCE_SANDBOX_UNAVAILABLE is set"
            );
        }

        // Test 3: run() + forced-unavailable + required=false → Ok(Unavailable).
        #[test]
        fn test_run_windows_forced_unavailable_required_false_returns_unavailable() {
            set_force_sandbox_unavailable(true);
            let _guard = ForceUnavailableGuard;

            let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig {
                required: false,
                ..Default::default()
            }));
            assert!(
                matches!(
                    executor.run("cmd /c exit 0"),
                    Ok(SandboxResult::Unavailable)
                ),
                "run() must return Ok(Unavailable) when forced-unavailable and required=false"
            );
        }

        // Test 4: run() + forced-unavailable + required=true → Err(Required).
        #[test]
        fn test_run_windows_forced_unavailable_required_true_returns_required_error() {
            set_force_sandbox_unavailable(true);
            let _guard = ForceUnavailableGuard;

            let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig {
                required: true,
                ..Default::default()
            }));
            assert!(
                matches!(executor.run("cmd /c exit 0"), Err(SandboxError::Required)),
                "run() must return Err(SandboxError::Required) when forced-unavailable and required=true"
            );
        }

        // Test 5: job_object::JobObject::new() succeeds.
        //
        // References `job_object::JobObject` which does not exist yet —
        // this test is a compile error on Windows until Phase 6.3 adds the type.
        #[test]
        fn test_job_object_new_succeeds() {
            let jo = super::super::job_object::JobObject::new()
                .expect("JobObject::new() must succeed on Windows");
            drop(jo);
        }

        // Test 6: run("cmd /c exit 0") through a real Job Object returns
        // Ok(Success(0)).
        //
        // This test drives the actual subprocess path: spawn a child, assign it
        // to the Job Object, wait for it to exit with code 0. Until the Windows
        // `SandboxExecutor::run` impl exists this returns Ok(Unavailable) from
        // the fallback, which does not match Ok(Success(0)).
        #[test]
        fn test_run_windows_exit_zero_returns_success_zero() {
            let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig {
                required: false,
                ..Default::default()
            }));
            match executor.run("cmd /c exit 0") {
                Ok(SandboxResult::Success(0)) => {}
                other => panic!(
                    "expected Ok(Success(0)) from 'cmd /c exit 0' in Job Object sandbox, got {other:?}"
                ),
            }
        }

        // Test 7: job_object::JobObject::has_kill_on_close_limit() returns true.
        //
        // Verifies that the Job Object was created with KILL_ON_JOB_CLOSE set,
        // preventing orphaned child processes when Aegis is killed.
        // References `has_kill_on_close_limit` which does not exist yet —
        // compile error on Windows until Phase 6.3 implements it.
        #[test]
        fn test_job_object_has_kill_on_close_limit_returns_true() {
            let jo =
                super::super::job_object::JobObject::new().expect("JobObject::new() must succeed");
            assert!(
                jo.has_kill_on_close_limit(),
                "JobObject must be created with KILL_ON_JOB_CLOSE set to prevent orphaned processes"
            );
        }
    }

    // ── Non-Linux/non-macOS fallback: sandbox must be active, not a no-op ────
    //
    // On Linux and macOS these tests are compiled out. On any other target
    // (Windows, FreeBSD, …) the current fallback unconditionally returns `false`
    // and `Ok(Unavailable)`. These tests specify what Phase 6.3 must deliver:
    // the fallback must no longer be a dead end — on Windows it must use Job
    // Objects and return real results.
    //
    // Note: after Phase 6.3 the `#[cfg(not(any(...)))]` fallback will exclude
    // Windows, so on a non-Windows/non-Linux/non-macOS target these tests would
    // still be red (acceptable — Aegis only targets Linux/macOS/Windows in v1).

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    mod fallback_platform_tests {
        use super::*;

        struct ForceUnavailableGuard;
        impl Drop for ForceUnavailableGuard {
            fn drop(&mut self) {
                set_force_sandbox_unavailable(false);
            }
        }

        // On Windows (the only non-Linux/non-macOS target Aegis supports in v1),
        // `sandbox_available_for` must return `true` after Phase 6.3.
        // Currently returns `false` → this test is RED on Windows.
        #[test]
        fn test_fallback_sandbox_available_for_returns_true() {
            assert!(
                sandbox_available_for(&SandboxConfig::default()),
                "sandbox_available_for must return true on Windows (non-Linux/non-macOS) \
                 after Phase 6.3; currently the fallback hard-returns false"
            );
        }

        // forced-unavailable must still suppress availability even on Windows.
        // This test is RED on Windows before Phase 6.3 because `sandbox_available_for`
        // currently hard-returns `false` regardless of the force flag — so the
        // assertion would accidentally pass. Once Phase 6.3 makes it return `true`
        // normally, the forced case must still return `false`. Both halves together
        // form a correct specification.
        #[test]
        fn test_fallback_forced_unavailable_returns_false() {
            set_force_sandbox_unavailable(true);
            let _guard = ForceUnavailableGuard;
            assert!(
                !sandbox_available_for(&SandboxConfig::default()),
                "sandbox_available_for must return false when forced-unavailable, even on Windows"
            );
        }

        // run() on Windows must return Ok(Success(_)), not Ok(Unavailable).
        // Currently the fallback always returns Ok(Unavailable) → RED on Windows.
        #[test]
        fn test_fallback_run_returns_success_not_unavailable() {
            let executor = SandboxExecutor::new(SandboxProfile::from_config(&SandboxConfig {
                required: false,
                ..Default::default()
            }));
            match executor.run("cmd /c exit 0") {
                Ok(SandboxResult::Success(_)) => {}
                Ok(SandboxResult::Unavailable) => panic!(
                    "run() must not return Ok(Unavailable) on Windows after Phase 6.3; \
                     currently the fallback always returns Unavailable"
                ),
                Err(e) => panic!("run() returned unexpected error: {e}"),
            }
        }
    }

    // ── macOS: prepare_for_exec production-path tests ────────────────────────
    // These cover the actual shell flow path (prepare_for_exec → Command → exec).
    // In tests we spawn instead of exec() so the test process is not replaced.

    #[cfg(target_os = "macos")]
    #[test]
    fn prepare_for_exec_returns_sandbox_exec_command_when_available() {
        use std::ffi::OsStr;
        if !super::sandbox_available_for(&SandboxConfig::default()) {
            return;
        }
        let cmd =
            super::prepare_for_exec(&SandboxConfig::default(), OsStr::new("/usr/bin/true"), &[])
                .expect("prepare_for_exec must succeed when sandbox_available_for returned true");
        assert_eq!(
            cmd.get_program(),
            "/usr/bin/sandbox-exec",
            "returned command must be sandbox-exec when sandbox is available"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn prepare_for_exec_command_runs_successfully_when_spawned() {
        use std::ffi::OsStr;
        if !super::sandbox_available_for(&SandboxConfig::default()) {
            return;
        }
        let mut cmd =
            super::prepare_for_exec(&SandboxConfig::default(), OsStr::new("/usr/bin/true"), &[])
                .expect("prepare_for_exec must succeed");
        // Use spawn+wait instead of exec() so the test process is not replaced.
        let status = cmd.status().expect("spawned command must run");
        assert!(status.success(), "sandboxed /usr/bin/true must exit 0");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn prepare_for_exec_blocks_write_outside_allow_write() {
        use std::ffi::{OsStr, OsString};
        if !super::sandbox_available_for(&SandboxConfig::default()) {
            return;
        }
        // Empty allow_write: (deny default) blocks all writes.
        let args = vec![
            OsString::from("-c"),
            OsString::from("touch /tmp/aegis_pfe_blocked.tmp"),
        ];
        let mut cmd =
            super::prepare_for_exec(&SandboxConfig::default(), OsStr::new("/bin/sh"), &args)
                .expect("prepare_for_exec must succeed");
        let status = cmd.status().expect("spawned command must run");
        assert!(
            !status.success(),
            "write to /tmp must be blocked when allow_write is empty"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn prepare_for_exec_falls_back_to_direct_when_forced_unavailable() {
        use std::ffi::OsStr;
        set_force_sandbox_unavailable(true);
        let _guard = ForceUnavailableGuard;
        let cfg = SandboxConfig {
            required: false,
            ..Default::default()
        };
        let cmd = super::prepare_for_exec(&cfg, OsStr::new("/usr/bin/true"), &[])
            .expect("must return Ok when required=false and sandbox unavailable");
        assert_ne!(
            cmd.get_program(),
            OsStr::new("/usr/bin/sandbox-exec"),
            "must return direct command (not sandbox-exec) when forced unavailable"
        );
    }

    /// The audit status (`Unavailable`) is computed separately from the live
    /// `WARN`, so this guards that the exec path actually emits the warning when
    /// it bypasses — keeping the live signal consistent with the audit record.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn prepare_for_exec_warns_on_bypass_when_forced_unavailable() {
        use std::ffi::OsStr;
        set_force_sandbox_unavailable(true);
        let _guard = ForceUnavailableGuard;
        let cfg = SandboxConfig {
            required: false,
            ..Default::default()
        };
        let counter = WarnCounter::default();
        let count = counter.0.clone();
        tracing::subscriber::with_default(counter, || {
            let _ = super::prepare_for_exec(&cfg, OsStr::new("/usr/bin/true"), &[]);
        });
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
