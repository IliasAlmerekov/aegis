use std::fs;
use std::path::PathBuf;

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn current_line_doc_exists_and_describes_the_live_pre_1_0_line() {
    let path = repo_path("docs/releases/current-line.md");
    assert!(
        path.exists(),
        "docs/releases/current-line.md must exist to describe the live release line"
    );

    let contents = fs::read_to_string(&path).unwrap_or_default();
    for needle in [
        "Aegis current release line (v0.2.0)",
        "Cargo.toml` version `0.2.0",
        "current public / MVP posture",
        "best-effort snapshots",
        "no claim that a `v1.0.0` release has already been published",
    ] {
        assert!(
            contents.contains(needle),
            "current release-line doc must mention `{needle}`; contents:\n{contents}"
        );
    }
}

#[test]
fn planned_v1_release_doc_is_explicitly_future_facing() {
    let path = repo_path("docs/releases/v1.0.0.md");
    let contents = fs::read_to_string(&path).expect("docs/releases/v1.0.0.md must exist");

    for needle in [
        "Planned Aegis v1.0.0 release summary",
        "future release contract",
        "forward-looking",
        "current crate version is `0.2.0`",
        "future `v1.0.0` tag",
    ] {
        assert!(
            contents.contains(needle),
            "planned v1 release doc must mention `{needle}`; contents:\n{contents}"
        );
    }
}
