//! macOS sandbox implementation: Seatbelt (`sandbox-exec`).

use std::ffi::{OsStr, OsString};

use crate::support::{is_forced_sandbox_unavailable, run_unavailable_result, warn_sandbox_bypass};
use crate::{SandboxConfig, SandboxError, SandboxResult};

// ── Public-to-crate entry points ──────────────────────────────────────────────

pub(crate) fn sandbox_available_for(config: &SandboxConfig) -> bool {
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

pub(crate) fn run(config: &SandboxConfig, cmd: &str) -> Result<SandboxResult, SandboxError> {
    if is_forced_sandbox_unavailable() || !is_sandbox_exec_available() {
        return run_unavailable_result(config.required);
    }
    let profile = match build_seatbelt_profile(config) {
        Ok(p) => p,
        Err(_) if !config.required => {
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

pub(crate) fn prepare_for_exec(
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
fn probe_seatbelt_works() -> bool {
    const PROBE: &str =
        "(version 1)\n(deny default)\n(allow process*)\n(allow file-read*)\n(allow signal*)\n";
    exec_true_in_profile(PROBE)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::support::set_force_sandbox_unavailable;
    use crate::support::test_helpers::{ForceUnavailableGuard, WarnCounter};
    use crate::{
        SandboxConfig, SandboxError, SandboxExecutor, SandboxProfile, SandboxResult,
        sandbox_available_for,
    };

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
    #[cfg(target_os = "macos")]
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
        let count = counter.counter();
        tracing::subscriber::with_default(counter, || {
            let _ = super::prepare_for_exec(&cfg, OsStr::new("/usr/bin/true"), &[]);
        });
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
