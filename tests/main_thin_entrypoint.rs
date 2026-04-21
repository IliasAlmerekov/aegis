use std::fs;
use std::path::PathBuf;

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn main_entrypoint_does_not_embed_shell_policy_orchestration_helpers() {
    let main_rs = fs::read_to_string(repo_path("src/main.rs")).expect("src/main.rs must exist");

    for helper in [
        "fn run_planned_shell_command(",
        "fn create_snapshots_for_plan(",
        "fn append_shell_audit(",
        "fn show_block_for_plan(",
        "fn decide_command(",
        "fn execute_policy_decision(",
        "fn test_command_explanation(",
        "fn evaluate_policy_decision(",
    ] {
        assert!(
            !main_rs.contains(helper),
            "main.rs must stay thin; shell/policy orchestration helper {helper} should live in a focused module"
        );
    }
}
