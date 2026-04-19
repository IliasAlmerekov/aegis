use std::fs;
use std::process::Command;

use tempfile::TempDir;

fn aegis_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aegis")
}

fn run_aegis_in(home: &TempDir, cwd: &TempDir, args: &[&str]) -> std::process::Output {
    Command::new(aegis_bin())
        .env("HOME", home.path())
        .current_dir(cwd.path())
        .args(args)
        .output()
        .unwrap()
}

fn write_invalid_config(cwd: &TempDir) {
    fs::write(
        cwd.path().join(".aegis.toml"),
        "mode = <<<THIS IS NOT VALID TOML\n",
    )
    .unwrap();
}

#[test]
fn off_creates_disabled_flag_and_status_reports_disabled() {
    let home = TempDir::new().unwrap();
    let output = Command::new(aegis_bin())
        .env("HOME", home.path())
        .args(["off"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(home.path().join(".aegis").join("disabled").exists());

    let status = Command::new(aegis_bin())
        .env("HOME", home.path())
        .args(["status"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(status.stdout).unwrap();
    assert!(stdout.contains("toggle: disabled"));
}

#[test]
fn status_reports_disabled_but_ci_override_active() {
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(
        home.path().join(".aegis").join("disabled"),
        "timestamp=x\npid=1\n",
    )
    .unwrap();

    let status = Command::new(aegis_bin())
        .env("HOME", home.path())
        .env("CI", "true")
        .args(["status"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(status.stdout).unwrap();
    assert_eq!(status.status.code(), Some(0));
    assert!(stdout.contains("toggle: disabled"));
    assert!(stdout.contains("effective mode: enforcing (CI override)"));
}

#[test]
fn status_returns_zero_when_enabled_or_disabled() {
    let home = TempDir::new().unwrap();

    let enabled = Command::new(aegis_bin())
        .env("HOME", home.path())
        .args(["status"])
        .output()
        .unwrap();
    assert_eq!(enabled.status.code(), Some(0));

    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(
        home.path().join(".aegis").join("disabled"),
        "timestamp=x\npid=1\n",
    )
    .unwrap();

    let disabled = Command::new(aegis_bin())
        .env("HOME", home.path())
        .args(["status"])
        .output()
        .unwrap();
    assert_eq!(disabled.status.code(), Some(0));
}

#[test]
fn status_does_not_claim_ci_override_when_toggle_is_enabled() {
    let home = TempDir::new().unwrap();

    let status = Command::new(aegis_bin())
        .env("HOME", home.path())
        .env("CI", "true")
        .args(["status"])
        .output()
        .unwrap();

    let stdout = String::from_utf8(status.stdout).unwrap();
    assert_eq!(status.status.code(), Some(0));
    assert!(stdout.contains("toggle: enabled"));
    assert!(stdout.contains("effective mode: enforcing"));
    assert!(!stdout.contains("CI override"));
}

#[test]
fn off_still_disables_when_config_is_invalid() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    write_invalid_config(&cwd);

    let output = run_aegis_in(&home, &cwd, &["off"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(home.path().join(".aegis").join("disabled").exists());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("warning: toggle state changed, but audit entry could not be recorded"),
        "audit failures should be reported without rolling back the toggle; stderr:\n{stderr}"
    );
}

#[test]
fn on_still_enables_when_config_is_invalid() {
    let home = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    write_invalid_config(&cwd);
    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(
        home.path().join(".aegis").join("disabled"),
        "timestamp=x\npid=1\n",
    )
    .unwrap();

    let output = run_aegis_in(&home, &cwd, &["on"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(!home.path().join(".aegis").join("disabled").exists());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("warning: toggle state changed, but audit entry could not be recorded"),
        "audit failures should be reported without rolling back the toggle; stderr:\n{stderr}"
    );
}

#[test]
fn falsy_aegis_ci_keeps_disabled_toggle_in_passthrough_even_with_truthy_ci_env() {
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(
        home.path().join(".aegis").join("disabled"),
        "timestamp=x\npid=1\n",
    )
    .unwrap();

    for value in ["0", "false", "no"] {
        let status = Command::new(aegis_bin())
            .env("HOME", home.path())
            .env("AEGIS_CI", value)
            .env("CI", "true")
            .args(["status"])
            .output()
            .unwrap();

        let stdout = String::from_utf8(status.stdout).unwrap();
        assert_eq!(status.status.code(), Some(0));
        assert!(stdout.contains("toggle: disabled"));
        assert!(
            stdout.contains("effective mode: disabled passthrough"),
            "AEGIS_CI={value} should override truthy CI env"
        );
        assert!(!stdout.contains("CI override"));
    }
}

#[test]
fn truthy_aegis_ci_forces_enforcing_ci_override() {
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(
        home.path().join(".aegis").join("disabled"),
        "timestamp=x\npid=1\n",
    )
    .unwrap();

    for value in ["1", "true", "yes"] {
        let status = Command::new(aegis_bin())
            .env("HOME", home.path())
            .env("AEGIS_CI", value)
            .env("CI", "false")
            .args(["status"])
            .output()
            .unwrap();

        let stdout = String::from_utf8(status.stdout).unwrap();
        assert_eq!(status.status.code(), Some(0));
        assert!(stdout.contains("toggle: disabled"));
        assert!(
            stdout.contains("effective mode: enforcing (CI override)"),
            "AEGIS_CI={value} should force CI override"
        );
    }
}
