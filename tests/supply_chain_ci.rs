use std::fs;
use std::path::{Path, PathBuf};

fn repo_path(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

#[test]
fn ci_security_job_runs_full_cargo_deny_check() {
    let workflow = fs::read_to_string(repo_path(".github/workflows/ci.yml"))
        .expect("CI workflow should be readable");

    assert!(
        workflow.contains("cargo deny check"),
        "CI must run cargo deny check"
    );
    assert!(
        !workflow.contains("cargo deny check bans licenses sources"),
        "CI must not omit advisories from cargo deny check after M5.4"
    );
}

#[test]
fn ci_security_job_runs_cargo_audit() {
    let workflow = fs::read_to_string(repo_path(".github/workflows/ci.yml"))
        .expect("CI workflow should be readable");

    assert!(workflow.contains("cargo audit"), "CI must run cargo audit");
}
