//! Linux sandbox implementation: bwrap + Landlock.

use std::ffi::{OsStr, OsString};

use crate::support::{is_forced_sandbox_unavailable, run_unavailable_result};
use crate::{PreparedSandboxCommand, SandboxConfig, SandboxError, SandboxResult};

// ── Public-to-crate entry points ──────────────────────────────────────────────

pub(crate) fn sandbox_available_for(config: &SandboxConfig) -> bool {
    !is_forced_sandbox_unavailable() && is_sandbox_available(config)
}

pub(crate) fn run(config: &SandboxConfig, cmd: &str) -> Result<SandboxResult, SandboxError> {
    if is_forced_sandbox_unavailable() || !is_sandbox_available(config) {
        return run_unavailable_result(config.required);
    }

    // NOTE: Landlock is NOT applied here because apply_landlock_restrictions
    // would restrict the Aegis parent process, not the bwrap child.
    // For the subprocess path, bwrap namespace isolation provides the
    // necessary confinement. Landlock is applied in prepare_for_exec()
    // where it is inherited across the POSIX exec() boundary.

    let bwrap_args = build_bwrap_args(config)?;
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

pub(crate) fn prepare_for_exec(
    config: &SandboxConfig,
    program: &OsStr,
    args: &[OsString],
) -> Result<PreparedSandboxCommand, SandboxError> {
    prepare(config, program, args, true)
}

pub(crate) fn prepare_for_spawn(
    config: &SandboxConfig,
    program: &OsStr,
    args: &[OsString],
) -> Result<PreparedSandboxCommand, SandboxError> {
    prepare(config, program, args, false)
}

fn prepare(
    config: &SandboxConfig,
    program: &OsStr,
    args: &[OsString],
    apply_exec_restrictions: bool,
) -> Result<PreparedSandboxCommand, SandboxError> {
    let mut bwrap_args = build_bwrap_args(config)?;
    if is_forced_sandbox_unavailable() || !is_sandbox_available(config) {
        if config.required {
            return Err(SandboxError::Required);
        }
        let mut cmd = std::process::Command::new(program);
        cmd.args(args);
        return Ok(PreparedSandboxCommand::unavailable(cmd));
    }

    bwrap_args.push(program.to_owned());
    bwrap_args.extend_from_slice(args);

    let mut cmd = std::process::Command::new("bwrap");
    cmd.args(&bwrap_args);
    let exec_config = apply_exec_restrictions.then(|| config.clone());
    Ok(PreparedSandboxCommand::active(cmd, exec_config))
}

// ── Sandbox availability probe ────────────────────────────────────────────────

/// Probe whether the sandbox infrastructure is available and functional.
///
/// Uses `bwrap --version` as a quick first pass, then attempts to actually
/// create a minimal sandbox to catch runtime issues (e.g. WSL2 network
/// namespace restrictions). The probe matches the config's `allow_network`
/// setting to avoid false negatives.
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

pub(crate) fn sysctl_userns_available() -> bool {
    std::fs::read_to_string("/proc/sys/kernel/unprivileged_userns_clone")
        .map(|v| v.trim() == "1")
        .unwrap_or(true)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::support::test_helpers::ForceUnavailableGuard;
    use crate::support::{is_forced_sandbox_unavailable, set_force_sandbox_unavailable};
    use crate::{
        SandboxConfig, SandboxError, SandboxExecutor, SandboxProfile, SandboxResult,
        sandbox_available_for,
    };

    use super::{
        apply_landlock_restrictions, build_bwrap_args, prepare_for_exec, sysctl_userns_available,
    };

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

    #[cfg(target_os = "linux")]
    #[test]
    fn prepare_for_exec_reports_optional_unavailability_without_rendering() {
        use aegis_types::SandboxStatus;
        use std::ffi::OsStr;

        set_force_sandbox_unavailable(true);
        let _guard = ForceUnavailableGuard;
        let cfg = SandboxConfig {
            required: false,
            ..Default::default()
        };
        let prepared = prepare_for_exec(&cfg, OsStr::new("/usr/bin/true"), &[])
            .expect("optional unavailability must prepare the direct command");

        assert_eq!(prepared.status, SandboxStatus::Unavailable);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn prepare_for_spawn_reports_optional_unavailability() {
        use aegis_types::SandboxStatus;
        use std::ffi::OsStr;

        set_force_sandbox_unavailable(true);
        let _guard = ForceUnavailableGuard;
        let cfg = SandboxConfig::default();

        let prepared = super::prepare_for_spawn(&cfg, OsStr::new("/usr/bin/true"), &[])
            .expect("optional unavailability must prepare the direct command");

        assert_eq!(prepared.status, SandboxStatus::Unavailable);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn optional_unavailability_does_not_hide_invalid_profile() {
        use std::ffi::OsStr;

        set_force_sandbox_unavailable(true);
        let _guard = ForceUnavailableGuard;
        let cfg = SandboxConfig {
            allow_write: vec![PathBuf::from("/nonexistent_aegis_test_path_xyz")],
            required: false,
            ..Default::default()
        };

        let result = prepare_for_exec(&cfg, OsStr::new("/usr/bin/true"), &[]);

        assert!(matches!(result, Err(SandboxError::Execution(_))));
    }
}
