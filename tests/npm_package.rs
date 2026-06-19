use std::path::Path;

fn repo_file(path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()))
}

#[test]
fn npm_package_should_define_global_aegis_binary_and_postinstall() {
    let package = repo_file("packaging/npm/package.json");

    assert!(
        package.contains("\"name\": \"@iliasalmerekov/aegis\""),
        "npm package should use the confirmed scoped package name"
    );
    assert!(
        package.contains("\"bin\"") && package.contains("\"aegis\": \"./bin/aegis.js\""),
        "npm package must expose a global aegis command"
    );
    assert!(
        package.contains("\"postinstall\": \"node scripts/install.js\""),
        "npm package must download and verify the native binary during postinstall"
    );
    assert!(
        package.contains("\"os\": [") && package.contains("\"darwin\"") && package.contains("\"linux\""),
        "npm package must declare supported operating systems"
    );
    assert!(
        package.contains("\"cpu\": [") && package.contains("\"x64\"") && package.contains("\"arm64\""),
        "npm package must declare supported CPU architectures"
    );
}

#[test]
fn npm_installer_should_map_all_release_assets_and_verify_sha256() {
    let installer = repo_file("packaging/npm/scripts/install.js");

    for asset in [
        "aegis-linux-x86_64",
        "aegis-linux-aarch64",
        "aegis-macos-x86_64",
        "aegis-macos-aarch64",
    ] {
        assert!(
            installer.contains(asset),
            "installer must know how to install release asset {asset}"
        );
    }

    assert!(
        installer.contains("createHash(\"sha256\")"),
        "installer must verify SHA256 before accepting the downloaded binary"
    );
    assert!(
        installer.contains("0o755"),
        "installer must make the native binary executable"
    );
    assert!(
        !installer.contains("curl | sh")
            && !installer.contains("curl -fsSL")
            && !installer.contains(".bashrc")
            && !installer.contains(".zshrc"),
        "npm installer must not shell out to curl installer or mutate shell rc files"
    );
}

#[test]
fn npm_checksums_should_pin_each_supported_asset() {
    let checksums = repo_file("packaging/npm/checksums.json");

    for asset in [
        "aegis-linux-x86_64",
        "aegis-linux-aarch64",
        "aegis-macos-x86_64",
        "aegis-macos-aarch64",
    ] {
        assert!(
            checksums.contains(asset),
            "checksums.json must pin {asset}"
        );
    }

    let sha_count = checksums
        .split('"')
        .filter(|part| part.len() == 64 && part.chars().all(|ch| ch.is_ascii_hexdigit()))
        .count();

    assert_eq!(
        sha_count, 4,
        "checksums.json must contain exactly four 64-hex SHA256 values"
    );
}

#[test]
fn npm_updater_should_fetch_all_sidecars_and_fail_closed() {
    let script = repo_file("scripts/update-npm-package.sh");

    assert!(
        script.contains("set -eu"),
        "npm updater must fail closed on unset variables and command failures"
    );
    assert!(
        script.contains("aegis-linux-x86_64.sha256")
            && script.contains("aegis-linux-aarch64.sha256")
            && script.contains("aegis-macos-x86_64.sha256")
            && script.contains("aegis-macos-aarch64.sha256"),
        "npm updater must fetch all four checksum sidecars"
    );
    assert!(
        script.contains("grep -Eq '^[[:xdigit:]]{64}$'"),
        "npm updater must validate checksum format"
    );
    assert!(
        script.contains("checksums.json"),
        "npm updater must write packaging/npm/checksums.json"
    );
}

#[test]
fn npm_installer_should_fail_closed_and_support_test_overrides() {
    let installer = repo_file("packaging/npm/scripts/install.js");

    assert!(
        installer.contains("process.env.AEGIS_NPM_PLATFORM")
            && installer.contains("process.env.AEGIS_NPM_ARCH"),
        "installer should support deterministic platform/arch overrides for tests"
    );
    assert!(
        installer.contains("Unsupported platform or architecture"),
        "installer must fail closed on unsupported hosts"
    );
    assert!(
        installer.contains("SHA256 mismatch"),
        "installer must fail closed on checksum mismatch"
    );
    assert!(
        installer.contains("AEGIS_NPM_SKIP_DOWNLOAD"),
        "installer should support a no-network test path"
    );
}