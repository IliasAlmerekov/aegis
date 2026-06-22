use std::path::{Path, PathBuf};

fn repo_path(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn read_repo_file(path: &str) -> String {
    std::fs::read_to_string(repo_path(path))
        .unwrap_or_else(|error| panic!("{path} should be readable: {error}"))
}

fn assert_fuzz_target_declared(manifest: &str, target: &str) {
    assert!(
        manifest.contains(&format!("name = \"{target}\"")),
        "fuzz/Cargo.toml must declare fuzz target {target}"
    );
    assert!(
        manifest.contains(&format!("path = \"fuzz_targets/{target}.rs\"")),
        "fuzz/Cargo.toml must point {target} at fuzz_targets/{target}.rs"
    );
}

fn assert_ci_runs_target(ci: &str, target: &str) {
    assert!(
        ci.contains(&format!("Run {target} fuzz")),
        "CI must include a named step for {target} fuzzing"
    );
    assert!(
        ci.contains(&format!(
            "cargo +nightly fuzz run {target} fuzz/corpus/{target} -- -runs=100000"
        )),
        "CI must run {target} fuzzing with the committed corpus and -runs=100000"
    );
}

#[test]
fn fuzz_manifest_declares_all_prd_targets() {
    let manifest = read_repo_file("fuzz/Cargo.toml");

    for target in ["parser", "scanner", "heredoc"] {
        assert_fuzz_target_declared(&manifest, target);
    }
}

#[test]
fn ci_runs_each_fuzz_target_for_at_least_100000_iterations() {
    let ci = read_repo_file(".github/workflows/ci.yml");

    for target in ["parser", "scanner", "heredoc"] {
        assert_ci_runs_target(&ci, target);
    }
}

#[test]
fn fuzz_corpora_are_committed_for_all_prd_targets() {
    for target in ["parser", "scanner", "heredoc"] {
        let corpus_dir = repo_path(&format!("fuzz/corpus/{target}"));
        let entries: Vec<_> = std::fs::read_dir(&corpus_dir)
            .unwrap_or_else(|error| {
                panic!(
                    "fuzz corpus directory {} should be readable: {error}",
                    corpus_dir.display()
                )
            })
            .collect::<Result<_, _>>()
            .unwrap_or_else(|error| {
                panic!(
                    "fuzz corpus directory {} should not contain unreadable entries: {error}",
                    corpus_dir.display()
                )
            });

        assert!(
            entries
                .iter()
                .any(|entry| entry.file_type().is_ok_and(|kind| kind.is_file())),
            "fuzz/corpus/{target} must contain at least one committed seed file"
        );
    }
}
