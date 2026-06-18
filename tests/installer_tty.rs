mod support;

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use support::installer::*;

#[test]
fn script_tty_args_use_bsd_command_form_on_macos() {
    let args = script_tty_args(ScriptFlavor::Bsd);

    assert_eq!(
        args,
        [
            "-q",
            "/dev/null",
            "/bin/sh",
            "-c",
            "cat \"$AEGIS_INSTALLER_SCRIPT\" | /bin/sh",
        ]
    );
}

#[test]
fn script_tty_args_use_util_linux_command_form_on_linux() {
    let args = script_tty_args(ScriptFlavor::UtilLinux);

    assert_eq!(
        args,
        [
            "-qec",
            "cat \"$AEGIS_INSTALLER_SCRIPT\" | /bin/sh",
            "/dev/null",
        ]
    );
}

#[test]
fn install_script_global_first_flow_in_tty_session() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");

    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_checksum_ready_release(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();

    let output = run_piped_script_with_tty(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("AEGIS_REAL_SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
        "",
    );

    assert!(
        output.status.success(),
        "piped install must succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("How would you like to set up Aegis?"),
        "global-first installer must not prompt for Local/Global/Binary; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("Aegis installed globally."),
        "installer should confirm the global default; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("Use `aegis off` to disable temporarily."),
        "installer should advertise the new toggle flow; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("Agent hook setup completed automatically.")
            || stdout.contains(
                "Agent hook setup skipped; no supported agent directories were detected."
            )
            || stdout.contains("Agent hook setup failed."),
        "installer must print an honest hook-setup outcome; stdout=\n{stdout}"
    );

    let rc_contents = fs::read_to_string(&rc_file).unwrap();
    let aegis_path = bindir.join("aegis");
    let expected_block = managed_block(Path::new("/bin/bash"), &aegis_path);
    assert!(
        rc_contents.contains(&expected_block),
        "global-first install must write the managed shell wrapper block; rc contents:\n{rc_contents}"
    );
}
