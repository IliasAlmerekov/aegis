// Gated live Homebrew smoke test. Default `cargo test` is network-free: this
// test is a no-op unless `AEGIS_TEST_LIVE_HOMEBREW=1` is set, in which case it
// taps the published tap, installs the formula, runs `brew test`, and verifies
// `aegis --version`. Run it manually where Homebrew is available:
//   AEGIS_TEST_LIVE_HOMEBREW=1 cargo test --test homebrew_live -- --nocapture

use std::process::Command;

fn run_brew(args: &[&str]) -> std::process::Output {
    Command::new("brew")
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("failed to run brew {args:?}: {error}"))
}

#[test]
fn live_homebrew_tap_installs_and_tests_aegis() {
    if std::env::var_os("AEGIS_TEST_LIVE_HOMEBREW").is_none() {
        eprintln!("skipping live Homebrew test; set AEGIS_TEST_LIVE_HOMEBREW=1");
        return;
    }

    let tap = run_brew(&["tap", "IliasAlmerekov/aegis"]);
    assert!(
        tap.status.success(),
        "brew tap failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&tap.stdout),
        String::from_utf8_lossy(&tap.stderr)
    );

    let install = run_brew(&["install", "aegis"]);
    assert!(
        install.status.success(),
        "brew install failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );

    let test = run_brew(&["test", "aegis"]);
    assert!(
        test.status.success(),
        "brew test failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&test.stdout),
        String::from_utf8_lossy(&test.stderr)
    );

    let version = Command::new("aegis")
        .arg("--version")
        .output()
        .expect("installed aegis should run");
    assert!(
        version.status.success(),
        "aegis --version failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&version.stdout),
        String::from_utf8_lossy(&version.stderr)
    );
}
