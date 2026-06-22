use std::path::{Path, PathBuf};

fn repo_path(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn read_repo_file(path: &str) -> String {
    std::fs::read_to_string(repo_path(path))
        .unwrap_or_else(|error| panic!("{path} should be readable: {error}"))
}

fn workflow() -> String {
    read_repo_file(".github/workflows/ci.yml")
}

#[test]
fn ci_defines_live_snapshot_rollback_job() {
    let ci = workflow();

    assert!(
        ci.contains("snapshot-rollback-live:"),
        "CI must define the M5.3 snapshot-rollback-live job"
    );
    assert!(
        ci.contains("name: Live snapshot/rollback (Docker + SQLite)"),
        "M5.3 job must have a clear human-readable name"
    );
    assert!(
        ci.contains("runs-on: ubuntu-latest"),
        "M5.3 job should run on ubuntu-latest where Docker and sqlite3 are available"
    );
}

#[test]
fn ci_live_snapshot_rollback_job_prepares_real_backends() {
    let ci = workflow();

    assert!(
        ci.contains("docker pull alpine"),
        "Docker live tests must pull the alpine fixture image explicitly"
    );
    assert!(
        ci.contains("sudo apt-get install -y sqlite3"),
        "SQLite live tests must install the real sqlite3 CLI"
    );
}

#[test]
fn ci_live_snapshot_rollback_job_runs_docker_and_sqlite_tests() {
    let ci = workflow();

    assert!(
        ci.contains("AEGIS_DOCKER_TESTS: \"1\""),
        "Docker live tests must opt in with AEGIS_DOCKER_TESTS=1"
    );
    assert!(
        ci.contains(
            "cargo test --test docker_integration snapshot_rollback_reverts_filesystem_change -- --exact --nocapture"
        ),
        "CI must run the real Docker snapshot rollback lifecycle test"
    );
    assert!(
        ci.contains("AEGIS_SQLITE_SNAPSHOT_TESTS: \"1\""),
        "SQLite live tests must opt in with AEGIS_SQLITE_SNAPSHOT_TESTS=1"
    );
    assert!(
        ci.contains(
            "cargo test --test snapshot_rollback_live sqlite_snapshot_rollback_restores_database_file_through_aegis_cli -- --exact --nocapture"
        ),
        "CI must run the real SQLite Aegis CLI snapshot rollback lifecycle test"
    );
}
