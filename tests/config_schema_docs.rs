use std::fs;
use std::path::PathBuf;

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn config_schema_doc_exists_and_describes_versioning_and_migration() {
    let path = repo_path("docs/config-schema.md");
    assert!(
        path.exists(),
        "docs/config-schema.md must exist to document schema evolution"
    );

    let contents = fs::read_to_string(&path).unwrap_or_default();
    for needle in [
        "config_version",
        "schema evolution",
        "allowlist = [",
        "[[allowlist]]",
        "mode semantics",
        "deprecated fields",
        "migration",
    ] {
        assert!(
            contents.contains(needle),
            "config schema doc must mention `{needle}`; contents:\n{contents}"
        );
    }
}

#[test]
fn readme_links_to_config_schema_policy() {
    let readme = fs::read_to_string(repo_path("README.md")).unwrap();
    assert!(
        readme.contains("[Config schema](docs/config-schema.md)"),
        "README must link to the explicit config-schema policy"
    );
}
