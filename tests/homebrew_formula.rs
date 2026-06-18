use std::path::Path;

fn repo_file(path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()))
}

fn formula() -> String {
    repo_file("packaging/homebrew/Formula/aegis.rb")
}

#[test]
fn homebrew_formula_should_install_release_binary_assets_for_all_supported_platforms() {
    let formula = formula();

    for asset in [
        "aegis-linux-x86_64",
        "aegis-linux-aarch64",
        "aegis-macos-x86_64",
        "aegis-macos-aarch64",
    ] {
        assert!(
            formula.contains(asset),
            "Homebrew formula must reference release asset {asset}"
        );
    }

    assert!(
        formula.contains("on_macos do"),
        "formula must branch for macOS assets"
    );
    assert!(
        formula.contains("on_linux do"),
        "formula must branch for Linux assets"
    );
}

#[test]
fn homebrew_formula_should_pin_each_download_with_a_sha256() {
    let formula = formula();
    let sha_count = formula
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("sha256 \"")
                && trimmed.ends_with('"')
                && trimmed
                    .trim_start_matches("sha256 \"")
                    .trim_end_matches('"')
                    .chars()
                    .all(|ch| ch.is_ascii_hexdigit())
                && trimmed
                    .trim_start_matches("sha256 \"")
                    .trim_end_matches('"')
                    .len()
                    == 64
        })
        .count();

    assert_eq!(
        sha_count, 4,
        "formula must pin exactly four binary downloads with 64-hex SHA256 values"
    );
}

#[test]
fn homebrew_formula_should_install_only_the_aegis_binary() {
    let formula = formula();

    assert!(
        formula.contains("bin.install"),
        "formula must install the downloaded binary into Homebrew's bin directory"
    );
    assert!(
        formula.contains("=> \"aegis\""),
        "formula must rename the platform asset to the executable name aegis"
    );
    assert!(
        !formula.contains("curl -fsSL"),
        "Homebrew formula must not shell out to the convenience curl installer"
    );
    assert!(
        !formula.contains(".bashrc") && !formula.contains(".zshrc"),
        "Homebrew formula must not mutate user shell rc files during install"
    );
}

#[test]
fn homebrew_formula_should_have_a_non_interactive_runtime_test() {
    let formula = formula();

    assert!(
        formula.contains("test do"),
        "formula must include a Homebrew test block"
    );
    assert!(
        formula.contains("#{bin}/aegis -c"),
        "formula test should exercise Aegis command execution, not only --version"
    );
    assert!(
        formula.contains("brew-test"),
        "formula test should assert a deterministic safe command output"
    );
}

#[test]
fn homebrew_formula_updater_should_exist_and_fail_closed_on_missing_release_input() {
    let script = repo_file("scripts/update-homebrew-formula.sh");

    assert!(
        script.contains("set -eu"),
        "updater script must fail closed on unset variables and command failures"
    );
    assert!(
        script.contains("usage()"),
        "updater script must provide a usage path"
    );
    assert!(
        script.contains("aegis-linux-x86_64.sha256")
            && script.contains("aegis-linux-aarch64.sha256")
            && script.contains("aegis-macos-x86_64.sha256")
            && script.contains("aegis-macos-aarch64.sha256"),
        "updater must fetch all four release checksum sidecars"
    );
    assert!(
        script.contains("grep -Eq '^[[:xdigit:]]{64}$'"),
        "updater must validate checksum format before writing the formula"
    );
}

#[test]
fn homebrew_formula_should_explain_post_install_setup_caveats() {
    let formula = formula();

    assert!(
        formula.contains("def caveats"),
        "formula must explain Homebrew-specific post-install setup"
    );
    assert!(
        formula.contains("aegis install-hooks --all"),
        "caveats should tell users how to install supported agent hooks"
    );
    assert!(
        formula.contains("AEGIS_REAL_SHELL"),
        "caveats should explain shell proxy setup is explicit after brew install"
    );
}

#[test]
fn readme_should_document_homebrew_install_without_overclaiming_shell_setup() {
    let readme = repo_file("README.md");

    assert!(
        readme.contains("brew tap IliasAlmerekov/aegis"),
        "README must document the tap command"
    );
    assert!(
        readme.contains("brew install aegis"),
        "README must document brew install"
    );
    assert!(
        readme.contains("Homebrew installs the binary only"),
        "README must explain that Homebrew does not run the global shell installer"
    );
}

#[test]
fn release_readiness_should_track_homebrew_evidence() {
    let docs = repo_file("docs/release-readiness.md");

    assert!(
        docs.contains("Homebrew"),
        "release readiness docs must mention Homebrew"
    );
    assert!(
        docs.contains("brew install"),
        "release readiness docs must require brew install evidence"
    );
    assert!(
        docs.contains("macOS") && docs.contains("Linux"),
        "release readiness docs must require macOS and Linux smoke-test evidence"
    );
}

#[test]
fn homebrew_formula_should_download_raw_binaries_without_decompression() {
    let formula = formula();
    let url_lines: Vec<&str> = formula
        .lines()
        .filter(|line| line.trim_start().starts_with("url \""))
        .collect();

    assert_eq!(
        url_lines.len(),
        4,
        "formula must have exactly four binary download urls"
    );
    assert!(
        url_lines
            .iter()
            .all(|line| line.contains("using: :nounzip")),
        "every raw-binary url must opt out of archive decompression with using: :nounzip"
    );
}
