use std::fs;
use std::process::Command;

use tempfile::TempDir;

fn aegis_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aegis")
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
