use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn run_install_with_windows_override() -> Output {
    Command::new("/bin/sh")
        .arg(repo_path("scripts/install.sh"))
        .env("AEGIS_OS", "Windows")
        .env("AEGIS_ARCH", "x86_64")
        .output()
        .unwrap()
}

#[test]
fn platform_support_doc_exists_and_declares_unix_only_matrix() {
    let path = repo_path("docs/platform-support.md");
    assert!(
        path.exists(),
        "docs/platform-support.md must exist to document the support matrix"
    );

    let contents = fs::read_to_string(&path).unwrap_or_default();
    for needle in [
        "## Support matrix",
        "| Linux |",
        "| macOS |",
        "| Windows |",
        "Supported",
        "Not supported",
        "bash",
        "zsh",
        "PowerShell",
        "cmd.exe",
    ] {
        assert!(
            contents.contains(needle),
            "platform support doc must mention `{needle}`; contents:\n{contents}"
        );
    }
}

#[test]
fn readme_links_to_platform_support_policy() {
    let readme = fs::read_to_string(repo_path("README.md")).unwrap();
    assert!(
        readme.contains("[Platform support](docs/platform-support.md)"),
        "README must link to the explicit platform-support policy"
    );
}

#[test]
fn installer_rejects_windows_with_clear_error() {
    let output = run_install_with_windows_override();
    assert!(
        !output.status.success(),
        "Windows install override must fail until a dedicated strategy exists"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unsupported operating system: Windows"),
        "installer must clearly explain that Windows is unsupported; stderr:\n{stderr}"
    );
}
