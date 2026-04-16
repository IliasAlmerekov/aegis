use std::fs;
use std::path::PathBuf;

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn cargo_package_version() -> String {
    let contents = fs::read_to_string(repo_path("Cargo.toml"))
        .expect("Cargo.toml must exist to verify release-doc version references");
    let parsed: toml::Value = toml::from_str(&contents).expect("Cargo.toml must parse as TOML");
    parsed["package"]["version"]
        .as_str()
        .expect("Cargo.toml package.version must be a string")
        .to_owned()
}

#[test]
fn current_line_doc_exists_and_describes_the_live_pre_1_0_line() {
    let version = cargo_package_version();
    let path = repo_path("docs/releases/current-line.md");
    assert!(
        path.exists(),
        "docs/releases/current-line.md must exist to describe the live release line"
    );

    let contents = fs::read_to_string(&path).unwrap_or_default();
    for needle in [
        format!("Aegis current release line (v{version})"),
        format!("Cargo.toml` version `{version}"),
        "current public / MVP posture".to_owned(),
        "best-effort snapshots".to_owned(),
        "no claim that a `v1.0.0` release has already been published".to_owned(),
    ] {
        assert!(
            contents.contains(&needle),
            "current release-line doc must mention `{needle}`; contents:\n{contents}"
        );
    }
}

#[test]
fn planned_v1_release_doc_is_explicitly_future_facing() {
    let version = cargo_package_version();
    let path = repo_path("docs/releases/v1.0.0.md");
    let contents = fs::read_to_string(&path).expect("docs/releases/v1.0.0.md must exist");

    for needle in [
        "Planned Aegis v1.0.0 release summary".to_owned(),
        "future release contract".to_owned(),
        "forward-looking".to_owned(),
        format!("current crate version is `{version}`"),
        "future `v1.0.0` tag".to_owned(),
        "manual checksum-first flow in".to_owned(),
        "verification story:".to_owned(),
        "install script remains a convenience path".to_owned(),
    ] {
        assert!(
            contents.contains(&needle),
            "planned v1 release doc must mention `{needle}`; contents:\n{contents}"
        );
    }
}
