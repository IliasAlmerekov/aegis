use std::fs;
use std::path::PathBuf;

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read(relative: &str) -> String {
    fs::read_to_string(repo_path(relative))
        .unwrap_or_else(|err| panic!("failed to read {relative}: {err}"))
}

#[test]
fn ci_should_not_run_native_windows_job_for_1_0() {
    let ci = read(".github/workflows/ci.yml");

    assert!(
        !ci.contains("runs-on: windows-latest"),
        "PRD §8 supports Windows only through WSL2/Linux; native windows-latest CI must be removed"
    );
    assert!(
        !ci.contains("name: Windows (compile + unit tests)"),
        "native Windows compile/unit-test job contradicts the 1.0 platform matrix"
    );
}

#[test]
fn sandbox_crate_should_not_dispatch_to_native_windows_module() {
    let lib = read("crates/aegis-sandbox/src/lib.rs");

    assert!(
        !lib.contains("#[path = \"windows.rs\"]"),
        "aegis-sandbox must not dispatch to a native Windows Job Object module in 1.0"
    );
    assert!(
        !repo_path("crates/aegis-sandbox/src/windows.rs").exists(),
        "native Windows Job Object implementation must be removed for M4.1"
    );
    assert!(
        lib.contains("target_os = \"windows\"") || lib.contains("windows"),
        "lib.rs should still document or cfg-route native Windows as unsupported"
    );
}

#[test]
fn shell_compat_should_not_execute_native_windows_shells() {
    let shell_compat = read("src/shell_compat.rs");

    assert!(
        shell_compat.contains("native Windows is unsupported")
            || shell_compat.contains("Native Windows is unsupported"),
        "native Windows shell execution should fail with an explicit unsupported message"
    );
    assert!(
        !shell_compat.contains("executor.run(cmd)"),
        "native Windows must not run commands through the removed sandbox executor path"
    );
}

#[test]
fn docs_should_keep_wsl2_as_linux_and_native_windows_unsupported() {
    let platform_doc = read("docs/platform-support.md");
    let readme = read("README.md");

    for contents in [&platform_doc, &readme] {
        assert!(
            contents.contains("WSL2"),
            "docs must keep the supported Windows-host path: WSL2/Linux"
        );
        assert!(
            contents.contains("PowerShell") && contents.contains("cmd.exe"),
            "docs must explicitly say native Windows shells are unsupported"
        );
    }
}
