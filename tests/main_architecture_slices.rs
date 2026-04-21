use std::fs;
use std::path::PathBuf;

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn read_main_rs() -> String {
    fs::read_to_string(repo_path("src/main.rs")).expect("src/main.rs must exist")
}

#[test]
fn main_entrypoint_does_not_embed_audit_config_toggle_or_rollback_handlers() {
    let main_rs = read_main_rs();

    for helper in [
        "fn handle_config_command(",
        "fn handle_toggle_on_command(",
        "fn handle_toggle_off_command(",
        "fn handle_toggle_status_command(",
        "fn handle_rollback_command(",
        "fn handle_config_validate_command(",
        "fn format_validation_report_text(",
        "fn config_load_error_lines(",
        "fn report_config_load_error(",
        "fn format_audit_entries(",
        "fn format_audit_summary(",
    ] {
        assert!(
            !main_rs.contains(helper),
            "main.rs must stay thin; handler/helper {helper} should live in a focused command module"
        );
    }
}

#[test]
fn main_entrypoint_does_not_embed_shell_compatibility_or_launch_helpers() {
    let main_rs = read_main_rs();

    for helper in [
        "enum InvocationMode {",
        "fn parse_invocation_mode(",
        "fn parse_shell_compat_invocation(",
        "fn starts_with_shell_compat_flags(",
        "fn parse_shell_compat_command(",
        "fn exec_command(",
        "fn exec_shell_session(",
        "fn shell_supports_login_flag(",
        "fn resolve_shell(",
        "fn resolve_shell_inner(",
        "fn same_file(",
    ] {
        assert!(
            !main_rs.contains(helper),
            "main.rs must stay thin; shell helper {helper} should live in a dedicated shell module"
        );
    }
}

#[test]
fn main_entrypoint_does_not_embed_cli_dispatch() {
    let main_rs = read_main_rs();

    assert!(
        !main_rs.contains("fn run_cli("),
        "main.rs must stay thin; CLI dispatch should live in a dedicated module"
    );
}

#[test]
fn main_entrypoint_does_not_embed_shell_wrapper_orchestration_or_output_helpers() {
    let main_rs = read_main_rs();

    for helper in [
        "fn run_shell_wrapper(",
        "fn report_setup_failure(",
        "fn render_json_outcome(",
        "fn run_shell_text_outcome(",
        "fn log_assessment(",
        "fn emit_policy_evaluation_json(",
    ] {
        assert!(
            !main_rs.contains(helper),
            "main.rs must stay thin; shell-wrapper helper {helper} should live in a focused wrapper/output module"
        );
    }
}
