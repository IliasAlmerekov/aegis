use std::fs;
use std::path::PathBuf;

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn config_schema_contract_covers_exit_code_compatibility() {
    let path = repo_path("docs/config-schema.md");
    let contents = fs::read_to_string(&path).expect("docs/config-schema.md must exist");

    assert!(
        contents.contains("## Exit-code compatibility contract"),
        "config schema doc must document exit-code compatibility contract"
    );

    for needle in [
        "`0` — command approved/executed successfully",
        "`2` — user denied in a prompt path (`prompt` decision)",
        "`3` — hard block (`block` decision)",
        "`4` — internal/config error",
        "`exit_code` in `--output json` always matches",
        "1..=255",
    ] {
        assert!(
            contents.contains(needle),
            "config schema doc must mention `{needle}`; missing compatibility contract detail"
        );
    }
}

#[test]
fn threat_model_is_current_and_documents_non_goals_honestly() {
    let path = repo_path("docs/threat-model.md");
    let contents = fs::read_to_string(&path).expect("docs/threat-model.md must exist");

    for needle in [
        "Aegis is a **heuristic command guardrail**",
        "Aegis is **not**:",
        "Aegis does not aim to provide:",
        "Residual risk",
        "Known examples:",
        "Security invariants",
        "Explicit non-goals",
        "Verification maturity note",
        "Current fuzzing coverage includes parser and scanner harnesses",
    ] {
        assert!(
            contents.contains(needle),
            "threat-model doc must keep current scope-and-limit language: {needle}"
        );
    }
}

#[test]
fn readme_links_to_contract_docs() {
    let readme = fs::read_to_string(repo_path("README.md")).expect("README.md must exist");
    assert!(
        readme.contains("[Config schema](docs/config-schema.md)"),
        "README must link to config schema contract document"
    );
    assert!(
        readme.contains("[Threat model](docs/threat-model.md)"),
        "README must link to threat model contract document"
    );
}
