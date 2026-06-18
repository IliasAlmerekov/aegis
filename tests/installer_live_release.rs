mod support;

use std::fs;
use std::process::Command;

use tempfile::TempDir;

use support::installer::*;

#[test]
fn install_script_skips_agent_setup_honestly_without_detected_agents() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");
    let rogue_log = temp.path().join("rogue-aegis.log");
    let home = temp.path().join("home");

    fs::create_dir_all(&home).unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_real_binary_release(&temp, &stub_dir);
    write_failing_aegis_on_path(&stub_dir.join("aegis"), &rogue_log);

    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let home_str = home.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("HOME", &home_str),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("AEGIS_REAL_SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        output.status.success(),
        "release install must succeed even without detectable agents: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected_follow_up = format!(
        "If you install Claude Code or Codex later, run:\n  {}/aegis install-hooks --all",
        bindir.display()
    );
    assert!(
        stdout.contains("Agent hook setup skipped; no supported agent directories were detected."),
        "installer should explain that no hooks were installed when no agent dirs are present; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains(&expected_follow_up),
        "installer should point users at the installed binary for follow-up hook setup; stdout=\n{stdout}"
    );
    assert!(
        !stdout.contains("Agent hook setup completed automatically."),
        "installer must not claim success when no agent dirs were detected; stdout=\n{stdout}"
    );
    assert!(
        bindir.join("aegis").exists(),
        "binary must still be installed"
    );
    assert!(
        !home.join(".claude").join("settings.json").exists(),
        "no agent dirs should mean no Claude settings are created"
    );
    assert!(
        !home.join(".codex").join("hooks.json").exists(),
        "no agent dirs should mean no Codex hook files are created"
    );
    assert!(
        !rogue_log.exists(),
        "installer must invoke the installed binary directly instead of PATH aegis"
    );
}

#[test]
fn install_script_auto_installs_codex_hooks_via_installed_binary() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");
    let rogue_log = temp.path().join("rogue-aegis.log");
    let home = temp.path().join("home");

    fs::create_dir_all(home.join(".codex")).unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_real_binary_release(&temp, &stub_dir);
    write_failing_aegis_on_path(&stub_dir.join("aegis"), &rogue_log);

    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let home_str = home.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("HOME", &home_str),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("AEGIS_REAL_SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        output.status.success(),
        "release install must auto-install Codex hooks when Codex is detected: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Agent hook setup completed automatically."),
        "installer should report successful automatic hook setup; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("Codex: hooks installed")
            || stdout.contains("Codex: hooks already present, skipping"),
        "installer should show Codex hook setup output from the installed binary; stdout=\n{stdout}"
    );
    let codex_hooks = home.join(".codex").join("hooks.json");
    assert!(codex_hooks.exists(), "Codex hooks.json should be created");
    assert!(
        home.join(".codex")
            .join("hooks")
            .join("aegis-pre-tool-use.sh")
            .exists(),
        "Codex pre-tool-use hook should be installed"
    );
    assert!(
        home.join(".codex")
            .join("hooks")
            .join("aegis-session-start.sh")
            .exists(),
        "Codex session-start hook should be installed"
    );
    assert!(
        !home.join(".claude").join("settings.json").exists(),
        "Codex-only install should not seed Claude settings"
    );
    assert!(
        !rogue_log.exists(),
        "installer must invoke the installed binary directly instead of PATH aegis"
    );
}

#[test]
fn install_script_auto_installs_claude_hooks_via_installed_binary() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");
    let home = temp.path().join("home");

    fs::create_dir_all(home.join(".claude")).unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_real_binary_release(&temp, &stub_dir);

    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let home_str = home.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("HOME", &home_str),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("AEGIS_REAL_SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        output.status.success(),
        "release install must auto-install Claude hooks when Claude is detected: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Agent hook setup completed automatically."),
        "installer should report successful automatic hook setup; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("Claude Code: hook installed")
            || stdout.contains("Claude Code: hook already present, skipping"),
        "installer should show Claude hook setup output from the installed binary; stdout=\n{stdout}"
    );
    let claude_settings = fs::read_to_string(home.join(".claude").join("settings.json")).unwrap();
    assert!(
        claude_settings.contains("\"command\": \"aegis hook\""),
        "Claude settings should point at the aegis hook subcommand; settings.json=\n{claude_settings}"
    );
    assert!(
        !home.join(".codex").join("hooks.json").exists(),
        "Claude-only install should not seed Codex hooks"
    );
}

#[test]
fn install_script_auto_installs_both_agent_hooks_via_installed_binary() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");
    let home = temp.path().join("home");

    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::create_dir_all(home.join(".codex")).unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_real_binary_release(&temp, &stub_dir);

    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let home_str = home.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("HOME", &home_str),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("AEGIS_REAL_SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        output.status.success(),
        "release install must auto-install both hook sets when both agents are detected: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Agent hook setup completed automatically."));
    assert!(
        stdout.contains("Claude Code: hook installed")
            || stdout.contains("Claude Code: hook already present, skipping")
    );
    assert!(
        stdout.contains("Codex: hooks installed")
            || stdout.contains("Codex: hooks already present, skipping")
    );
    assert!(home.join(".claude").join("settings.json").exists());
    assert!(home.join(".codex").join("hooks.json").exists());
}

#[test]
fn install_script_repeated_install_keeps_agent_hook_setup_idempotent() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");
    let home = temp.path().join("home");

    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::create_dir_all(home.join(".codex")).unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_real_binary_release(&temp, &stub_dir);

    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let home_str = home.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let envs = [
        ("AEGIS_BINDIR", bindir_str.as_str()),
        ("AEGIS_SHELL_RC", rc_file_str.as_str()),
        ("AEGIS_OS", "linux"),
        ("AEGIS_ARCH", "x86_64"),
        ("HOME", home_str.as_str()),
        ("PATH", path_value.as_str()),
        ("SHELL", "/bin/bash"),
        ("AEGIS_REAL_SHELL", "/bin/bash"),
        ("TEST_BINARY_ASSET", binary_asset_str.as_str()),
        ("TEST_CHECKSUM_ASSET", checksum_asset_str.as_str()),
        ("TEST_BINARY_DIGEST", binary_digest.as_str()),
    ];

    let first_output = run_script("install.sh", &envs);
    assert!(
        first_output.status.success(),
        "first release install must succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&first_output.stdout),
        String::from_utf8_lossy(&first_output.stderr)
    );

    let claude_before = fs::read_to_string(home.join(".claude").join("settings.json")).unwrap();
    let codex_before = fs::read_to_string(home.join(".codex").join("hooks.json")).unwrap();

    let second_output = run_script("install.sh", &envs);
    assert!(
        second_output.status.success(),
        "second release install must succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&second_output.stdout),
        String::from_utf8_lossy(&second_output.stderr)
    );

    let stdout = String::from_utf8_lossy(&second_output.stdout);
    assert!(
        stdout.contains("Agent hook setup completed automatically."),
        "successful re-install should still report automatic hook setup; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("Claude Code: hook already present, skipping"),
        "re-install should report Claude idempotence; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("Codex: hooks already present, skipping"),
        "re-install should report Codex idempotence; stdout=\n{stdout}"
    );
    assert_eq!(
        fs::read_to_string(home.join(".claude").join("settings.json")).unwrap(),
        claude_before,
        "re-install should not rewrite Claude settings when already configured"
    );
    assert_eq!(
        fs::read_to_string(home.join(".codex").join("hooks.json")).unwrap(),
        codex_before,
        "re-install should not rewrite Codex hooks when already configured"
    );
}

#[test]
fn test_installer_live_github_release_download() {
    if std::env::var("AEGIS_TEST_LIVE_INSTALL").ok().as_deref() != Some("1") {
        return;
    }

    let asset = host_asset_name().expect("unsupported host platform for live installer test");
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");

    fs::create_dir_all(&bindir).unwrap();
    fs::write(&rc_file, "").unwrap();

    let real_path = std::env::var("PATH").unwrap_or_default();
    let path_value = format!("{}:{}", installer_path(&temp, &stub_dir), real_path);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let (os, arch) = asset
        .strip_prefix("aegis-")
        .unwrap()
        .split_once('-')
        .unwrap();

    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", os),
            ("AEGIS_ARCH", arch),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("AEGIS_REAL_SHELL", "/bin/bash"),
        ],
    );

    assert!(
        output.status.success(),
        "live installer must succeed: stdout=
{}
stderr=
{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let aegis_path = bindir.join("aegis");
    assert!(
        aegis_path.exists(),
        "live installer must place binary into bindir"
    );

    let version_output = Command::new(&aegis_path).arg("--version").output().unwrap();
    assert!(
        version_output.status.success(),
        "installed aegis --version must succeed: stdout=
{}
stderr=
{}",
        String::from_utf8_lossy(&version_output.stdout),
        String::from_utf8_lossy(&version_output.stderr)
    );
}
