use std::fs;
use std::path::PathBuf;

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn install_and_uninstall_shell_rc_updates_use_atomic_rename() {
    let install =
        fs::read_to_string(repo_path("scripts/install.sh")).expect("scripts/install.sh must exist");
    let uninstall = fs::read_to_string(repo_path("scripts/uninstall.sh"))
        .expect("scripts/uninstall.sh must exist");

    assert!(
        install.contains("mv \"${tmp_rc}\" \"${rc_file}\""),
        "install.sh must atomically replace rc files with mv"
    );
    assert!(
        !install.contains("cp \"${tmp_rc}\" \"${rc_file}\""),
        "install.sh must not copy over rc files non-atomically"
    );

    assert!(
        uninstall.contains("mv \"${tmp_rc}\" \"${rc_file}\""),
        "uninstall.sh must atomically replace rc files with mv"
    );
    assert!(
        !uninstall.contains("cp \"${tmp_rc}\" \"${rc_file}\""),
        "uninstall.sh must not copy over rc files non-atomically"
    );
}
