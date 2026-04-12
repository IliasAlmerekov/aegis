use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;
use tempfile::TempDir;

fn aegis_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aegis"))
}

fn base_command(home: &Path) -> Command {
    let mut command = Command::new(aegis_bin());
    command.env("AEGIS_REAL_SHELL", "/bin/sh");
    command.env("AEGIS_CI", "0");
    command.env("HOME", home);
    command
}

#[derive(Debug, Deserialize)]
struct SecurityRegressionCorpus {
    cases: Vec<SecurityRegressionCase>,
}

#[derive(Debug, Deserialize)]
struct SecurityRegressionCase {
    name: String,
    command: String,
    expected_risk: String,
    expected_decision: String,
    expected_exit_code: i32,
    expected_block_reason: Option<String>,
}

fn load_corpus() -> SecurityRegressionCorpus {
    let path = Path::new("tests/fixtures/security_bypass_corpus.toml");
    let contents = fs::read_to_string(path).unwrap();
    toml::from_str(&contents).unwrap()
}

fn write_strict_security_config(workspace: &Path) {
    fs::write(
        workspace.join(".aegis.toml"),
        r#"
mode = "Strict"
auto_snapshot_git = false
auto_snapshot_docker = false
"#,
    )
    .unwrap();
}

fn run_case(home: &Path, workspace: &Path, case: &SecurityRegressionCase) -> (i32, Value, Vec<u8>) {
    let output = base_command(home)
        .current_dir(workspace)
        .args(["-c", &case.command, "--output", "json"])
        .output()
        .unwrap();

    let exit_code = output.status.code().unwrap_or_default();
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    (exit_code, json, output.stderr)
}

#[test]
fn security_regression_corpus_preserves_non_safe_decisions_for_bypass_attempts() {
    let corpus = load_corpus();
    let home = TempDir::new().unwrap();
    let workspace = TempDir::new().unwrap();
    write_strict_security_config(workspace.path());

    for case in &corpus.cases {
        let (exit_code, json, stderr) = run_case(home.path(), workspace.path(), case);

        assert!(
            stderr.is_empty(),
            "case {} must keep stderr empty in json mode: {}",
            case.name,
            String::from_utf8_lossy(&stderr)
        );
        assert_eq!(
            exit_code, case.expected_exit_code,
            "case {} exit code mismatch",
            case.name
        );
        assert_eq!(
            json["risk"].as_str(),
            Some(case.expected_risk.as_str()),
            "case {} risk mismatch",
            case.name
        );
        assert_eq!(
            json["decision"].as_str(),
            Some(case.expected_decision.as_str()),
            "case {} decision mismatch",
            case.name
        );
        assert!(
            json["matched_patterns"]
                .as_array()
                .is_some_and(|patterns| !patterns.is_empty()),
            "case {} must report at least one matched pattern",
            case.name
        );

        match &case.expected_block_reason {
            Some(reason) => assert_eq!(
                json["block_reason"].as_str(),
                Some(reason.as_str()),
                "case {} block_reason mismatch",
                case.name
            ),
            None => assert!(
                json.get("block_reason").is_none() || json["block_reason"].is_null(),
                "case {} should not report block_reason",
                case.name
            ),
        }
    }
}
