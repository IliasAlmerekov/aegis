// Gated live npm smoke test. Default `cargo test` is network-free: this test
// is a no-op unless `AEGIS_TEST_LIVE_NPM=1` is set, in which case it packs the
// npm package, installs it globally, and verifies `aegis --version`. Run it
// manually where npm is available:
//   AEGIS_TEST_LIVE_NPM=1 cargo test --test npm_live -- --nocapture

use std::path::Path;
use std::process::Command;

fn run(command: &str, args: &[&str]) -> std::process::Output {
    Command::new(command)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("failed to run {command} {args:?}: {error}"))
}

#[test]
fn live_npm_package_installs_and_runs_aegis() {
    if std::env::var_os("AEGIS_TEST_LIVE_NPM").is_none() {
        eprintln!("skipping live npm test; set AEGIS_TEST_LIVE_NPM=1");
        return;
    }

    let package_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/npm");
    let package_dir = package_dir
        .to_str()
        .unwrap_or_else(|| panic!("package path should be valid UTF-8"));

    let pack = run("npm", &["pack", package_dir]);
    assert!(
        pack.status.success(),
        "npm pack failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&pack.stdout),
        String::from_utf8_lossy(&pack.stderr)
    );

    let install = run("npm", &["install", "-g", package_dir]);
    assert!(
        install.status.success(),
        "npm install -g failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );

    let version = run("aegis", &["--version"]);
    assert!(
        version.status.success(),
        "aegis --version failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&version.stdout),
        String::from_utf8_lossy(&version.stderr)
    );
}