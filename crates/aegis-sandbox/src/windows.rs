//! Windows sandbox implementation: Job Objects.
//!
//! All Win32 types are declared inline — no external crate dependency,
//! following the same pattern as the Linux `landlock` module.

use crate::support::{is_forced_sandbox_unavailable, run_unavailable_result};
use crate::{SandboxConfig, SandboxError, SandboxResult};

// ── Public-to-crate entry points ──────────────────────────────────────────────

pub(crate) fn sandbox_available_for(config: &SandboxConfig) -> bool {
    // Job Objects are available on all Windows Vista+ systems — no runtime probe needed.
    let _ = config;
    !is_forced_sandbox_unavailable()
}

pub(crate) fn run(config: &SandboxConfig, cmd: &str) -> Result<SandboxResult, SandboxError> {
    if is_forced_sandbox_unavailable() {
        return run_unavailable_result(config.required);
    }
    run_in_job_object(cmd)
}

// ── Windows Job Objects ───────────────────────────────────────────────────────

/// Windows Job Object FFI and [`JobObject`] RAII wrapper.
///
/// All Win32 types are declared inline — no external crate dependency,
/// following the same pattern as the Linux [`landlock`] module.
pub(crate) mod job_object {
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};

    /// Raw Win32 `HANDLE` — a nullable pointer to an opaque kernel object.
    // These type aliases keep the canonical Win32 names (HANDLE/BOOL/DWORD) to
    // match the Microsoft docs and the `windows-sys`/`winapi` crate conventions;
    // renaming to `Handle`/`Bool`/`Dword` would only obscure the FFI surface.
    #[expect(clippy::upper_case_acronyms)]
    pub type HANDLE = *mut std::ffi::c_void;
    #[expect(clippy::upper_case_acronyms)]
    type BOOL = i32;
    #[expect(clippy::upper_case_acronyms)]
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
        // Only called by `has_kill_on_close_limit`, which is a test-only verifier.
        #[cfg(test)]
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
        ///
        /// Test-only verifier: asserts the Job Object was created with
        /// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` set, so orphaned child processes
        /// are killed when Aegis exits. No non-test code path queries the limit
        /// flags, so it (and the `QueryInformationJobObject` import it needs) is
        /// gated `#[cfg(test)]` to avoid dead code in production builds.
        #[cfg(test)]
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

/// **MVP scope**: only `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is enforced —
/// child processes are killed when the job handle is closed. The
/// `allow_write` and `allow_network` fields of [`SandboxConfig`] are
/// accepted but **not enforced** on Windows; filesystem and network
/// restrictions require AppContainers or WFP, which are out of scope here.
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
//
// These tests are gated on `#[cfg(windows)]`. On Linux they are compiled out
// entirely (no-op). On Windows they drive the Phase 6.3 implementation:
//
//   - Tests 1-4 drive the public API surface (`sandbox_available_for`, `run`).
//   - Tests 5, 7 reference `job_object::JobObject` and
//     `job_object::JobObject::has_kill_on_close_limit`, which do not exist yet
//     → compile error on Windows until Phase 6.3 adds `pub(crate) mod job_object`.
//   - Test 6 drives the actual subprocess path through a Job Object.

#[cfg(all(test, windows))]
mod windows_tests {
    use crate::sandbox_available_for;
    use crate::support::set_force_sandbox_unavailable;
    use crate::support::test_helpers::ForceUnavailableGuard;
    use crate::{SandboxConfig, SandboxError, SandboxExecutor, SandboxProfile, SandboxResult};

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
        let jo =
            super::job_object::JobObject::new().expect("JobObject::new() must succeed on Windows");
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
        let jo = super::job_object::JobObject::new().expect("JobObject::new() must succeed");
        assert!(
            jo.has_kill_on_close_limit(),
            "JobObject must be created with KILL_ON_JOB_CLOSE set to prevent orphaned processes"
        );
    }
}

// ── Non-Linux/non-macOS fallback: sandbox must be active, not a no-op ────
//
// On Windows these are the same behaviors verified by `windows_tests` above;
// they are kept here under the original `#[cfg(not(any(target_os = "linux", target_os = "macos")))]`
// gate to preserve the spec coverage that drove Phase 6.3. On Windows both
// modules compile and pass; on any truly-unsupported target (FreeBSD, …) this
// module is compiled out because `windows.rs` is only built on Windows.

#[cfg(all(test, not(any(target_os = "linux", target_os = "macos"))))]
mod fallback_platform_tests {
    use crate::sandbox_available_for;
    use crate::support::set_force_sandbox_unavailable;
    use crate::support::test_helpers::ForceUnavailableGuard;
    use crate::{SandboxConfig, SandboxExecutor, SandboxProfile, SandboxResult};

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
