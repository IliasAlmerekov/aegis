mod support;

use std::fs;

use tempfile::TempDir;

use support::installer::*;

#[test]
fn install_script_rejects_checksum_mismatch_before_touching_bindir() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let binary_asset = temp.path().join("aegis-linux-x86_64");
    let checksum_asset = temp.path().join("aegis-linux-x86_64.sha256");
    let stub_dir = temp.path().join("stub-bin");
    let curl_stub = stub_dir.join("curl");
    let sha256sum_stub = stub_dir.join("sha256sum");

    fs::create_dir_all(&stub_dir).unwrap();
    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    write_fake_release_binary(&binary_asset);
    write_release_checksum(
        &checksum_asset,
        "aegis-linux-x86_64",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    write_curl_stub(&curl_stub);
    write_sha256sum_stub(&sha256sum_stub);

    let binary_digest = sha256_hex(&fs::read(&binary_asset).unwrap());
    let path_value = installer_path(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(!output.status.success());
    assert!(
        !bindir.join("aegis").exists(),
        "checksum mismatch must leave final bindir untouched"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("checksum verification failed"));
}

#[test]
fn install_script_rejects_missing_checksum_before_touching_bindir() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let binary_asset = temp.path().join("aegis-linux-x86_64");
    let checksum_asset = temp.path().join("aegis-linux-x86_64.sha256");
    let stub_dir = temp.path().join("stub-bin");
    let curl_stub = stub_dir.join("curl");
    let sha256sum_stub = stub_dir.join("sha256sum");

    fs::create_dir_all(&stub_dir).unwrap();
    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    write_fake_release_binary(&binary_asset);
    write_release_checksum(
        &checksum_asset,
        "aegis-linux-x86_64",
        "1111111111111111111111111111111111111111111111111111111111111111",
    );
    write_curl_stub(&curl_stub);
    write_sha256sum_stub(&sha256sum_stub);

    let binary_digest = sha256_hex(&fs::read(&binary_asset).unwrap());
    let path_value = installer_path(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_CHECKSUM_MODE", "missing"),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(!output.status.success());
    assert!(
        !bindir.join("aegis").exists(),
        "missing checksum must leave final bindir untouched"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("checksum download failed"));
}

#[test]
fn install_script_fails_when_no_supported_checksum_tool_exists() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let binary_asset = temp.path().join("aegis-linux-x86_64");
    let checksum_asset = temp.path().join("aegis-linux-x86_64.sha256");
    let stub_dir = temp.path().join("stub-bin");
    let curl_stub = stub_dir.join("curl");

    fs::create_dir_all(&stub_dir).unwrap();
    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    write_fake_release_binary(&binary_asset);
    let binary_digest = sha256_hex(&fs::read(&binary_asset).unwrap());
    write_release_checksum(&checksum_asset, "aegis-linux-x86_64", &binary_digest);
    write_curl_stub(&curl_stub);

    let path_value = installer_path(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(!output.status.success());
    assert!(
        !bindir.join("aegis").exists(),
        "missing checksum tools must leave final bindir untouched"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("no supported checksum tool found"));
}

#[test]
fn install_script_falls_back_to_shasum_when_sha256sum_is_missing() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let binary_asset = temp.path().join("aegis-linux-x86_64");
    let checksum_asset = temp.path().join("aegis-linux-x86_64.sha256");
    let stub_dir = temp.path().join("stub-bin");
    let curl_stub = stub_dir.join("curl");
    let shasum_stub = stub_dir.join("shasum");

    fs::create_dir_all(&stub_dir).unwrap();
    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    write_fake_release_binary(&binary_asset);
    let binary_digest = sha256_hex(&fs::read(&binary_asset).unwrap());
    write_release_checksum(&checksum_asset, "aegis-linux-x86_64", &binary_digest);
    write_curl_stub(&curl_stub);
    write_shasum_stub(&shasum_stub);

    let path_value = installer_path(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        output.status.success(),
        "fallback to shasum must succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(bindir.join("aegis").exists());
}
