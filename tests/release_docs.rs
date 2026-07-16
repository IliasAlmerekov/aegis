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
fn publishable_crates_give_every_path_dependency_a_version_requirement() {
    let crates_dir = repo_path("crates");
    for entry in fs::read_dir(&crates_dir).expect("crates directory must be readable") {
        let manifest = entry
            .expect("crate directory entry must be readable")
            .path()
            .join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let contents = fs::read_to_string(&manifest).expect("crate manifest must be readable");
        let parsed: toml::Value = toml::from_str(&contents).expect("crate manifest must parse");
        assert_path_dependencies_are_versioned(&parsed, &manifest);
    }
}

#[test]
fn local_package_validation_resolves_the_unpublished_foundation_crate() {
    let contents =
        fs::read_to_string(repo_path(".cargo/config.toml")).expect(".cargo/config.toml must exist");
    let parsed: toml::Value = toml::from_str(&contents).expect("Cargo config must parse");

    assert_eq!(
        parsed["patch"]["crates-io"]["aegis-types"]["path"].as_str(),
        Some("crates/aegis-types"),
        "local cargo package validation must resolve the unpublished workspace aegis-types crate"
    );
}

fn assert_path_dependencies_are_versioned(value: &toml::Value, manifest: &std::path::Path) {
    let toml::Value::Table(table) = value else {
        return;
    };
    if table.contains_key("path") {
        assert!(
            table.contains_key("version"),
            "{} contains a path dependency without a version requirement: {table:?}",
            manifest.display()
        );
    }
    for child in table.values() {
        assert_path_dependencies_are_versioned(child, manifest);
    }
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
