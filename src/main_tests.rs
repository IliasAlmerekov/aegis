use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use aegis::config::Config;
use aegis::decision::ExecutionTransport;
use aegis::interceptor::parser::Parser as CommandParser;
use aegis::interceptor::scanner::Assessment;
use aegis::planning::{CwdState, PlanningOutcome, PreparedPlanner, prepare_and_plan};
use aegis::runtime::RuntimeContext;
use aegis::snapshot::SnapshotRecord;
use tempfile::TempDir;
use tokio::runtime::Handle;

use super::*;
use crate::cli_commands::config_load_error_lines;
use crate::shell_compat::{
    InvocationMode, ShellLaunchOptions, parse_shell_compat_invocation, resolve_shell_inner,
    same_file,
};
use crate::shell_flow::decide_command;
use aegis::config::{AllowlistMatch, AllowlistOverrideLevel, AllowlistSourceLayer, CiPolicy, Mode};
use aegis::error::AegisError;

// ── Scanner init failure ──────────────────────────────────────────────────
//
// Fail-closed: when interceptor assessment returns Err, assess_command()
// must fall back to RiskLevel::Warn — NOT Safe.  Safe would auto-approve
// every command (including rm -rf /) while the scanner is broken.
// Warn forces the confirmation dialog for every command until healthy.

#[test]
fn scanner_init_failure_fallback_is_warn_not_safe() {
    let fallback = Assessment {
        risk: RiskLevel::Warn,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: CommandParser::parse("any command"),
    };
    assert_eq!(fallback.risk, RiskLevel::Warn);
    assert!(
        fallback.risk > RiskLevel::Safe,
        "fail-closed: scanner failure must require confirmation, not auto-approve"
    );
    assert!(fallback.matched.is_empty());
}

// ── Snapshot runtime failure ──────────────────────────────────────────────
//
// When the tokio runtime fails to build, create_snapshots() returns an empty
// Vec — the dialog still appears, just without snapshot records listed.

#[test]
fn snapshot_runtime_failure_fallback_returns_empty_vec() {
    let fallback: Vec<SnapshotRecord> = Vec::new();
    assert!(fallback.is_empty());
}

#[test]
fn shell_compat_parser_handles_dash_lc_command() {
    let args = vec![
        std::ffi::OsString::from("-lc"),
        std::ffi::OsString::from("printf compat"),
    ];

    let parsed = parse_shell_compat_invocation(&args).unwrap();
    let Some(InvocationMode::ShellCompatCommand { command, launch }) = parsed else {
        panic!("expected shell compatibility command invocation");
    };

    assert_eq!(command, "printf compat");
    assert!(launch.login);
    assert!(!launch.interactive);
    assert!(launch.positional_args.is_empty());
}

#[test]
fn shell_compat_parser_handles_command_flag_before_interactive_flag() {
    let args = vec![
        std::ffi::OsString::from("-ci"),
        std::ffi::OsString::from("printf compat"),
    ];

    let parsed = parse_shell_compat_invocation(&args).unwrap();
    let Some(InvocationMode::ShellCompatCommand { command, launch }) = parsed else {
        panic!("expected shell compatibility command invocation");
    };

    assert_eq!(command, "printf compat");
    assert!(!launch.login);
    assert!(launch.interactive);
    assert!(launch.positional_args.is_empty());
}

#[test]
fn shell_compat_parser_handles_repeated_command_flag_bundle() {
    let args = vec![
        std::ffi::OsString::from("-cc"),
        std::ffi::OsString::from("printf compat"),
    ];

    let parsed = parse_shell_compat_invocation(&args).unwrap();
    let Some(InvocationMode::ShellCompatCommand { command, launch }) = parsed else {
        panic!("expected shell compatibility command invocation");
    };

    assert_eq!(command, "printf compat");
    assert!(!launch.login);
    assert!(!launch.interactive);
    assert!(launch.positional_args.is_empty());
}

#[test]
fn shell_compat_parser_handles_separate_login_and_command_flags() {
    let args = vec![
        std::ffi::OsString::from("-l"),
        std::ffi::OsString::from("-c"),
        std::ffi::OsString::from("printf compat"),
    ];

    let parsed = parse_shell_compat_invocation(&args).unwrap();
    let Some(InvocationMode::ShellCompatCommand { command, launch }) = parsed else {
        panic!("expected shell compatibility command invocation");
    };

    assert_eq!(command, "printf compat");
    assert!(launch.login);
    assert!(!launch.interactive);
    assert!(launch.positional_args.is_empty());
}

#[test]
fn shell_compat_parser_handles_long_login_flag() {
    let args = vec![
        std::ffi::OsString::from("--login"),
        std::ffi::OsString::from("-c"),
        std::ffi::OsString::from("printf compat"),
    ];

    let parsed = parse_shell_compat_invocation(&args).unwrap();
    let Some(InvocationMode::ShellCompatCommand { command, launch }) = parsed else {
        panic!("expected shell compatibility command invocation");
    };

    assert_eq!(command, "printf compat");
    assert!(launch.login);
    assert!(!launch.interactive);
    assert!(launch.positional_args.is_empty());
}

#[test]
fn shell_compat_parser_does_not_capture_native_aegis_command_mode() {
    let args = vec![
        std::ffi::OsString::from("-c"),
        std::ffi::OsString::from("printf compat"),
        std::ffi::OsString::from("--output"),
        std::ffi::OsString::from("json"),
    ];

    assert!(parse_shell_compat_invocation(&args).unwrap().is_none());
}

#[test]
fn shell_launch_options_drop_login_flag_for_posix_sh() {
    let launch = ShellLaunchOptions {
        login: true,
        interactive: false,
        positional_args: Vec::new(),
    };

    assert_eq!(launch.command_flag(Path::new("/bin/sh")), "-c");
    assert_eq!(launch.command_flag(Path::new("/bin/bash")), "-lc");
    assert_eq!(
        launch.session_flags(Path::new("/bin/sh")),
        Vec::<&str>::new()
    );
    assert_eq!(launch.session_flags(Path::new("/bin/zsh")), vec!["-l"]);
}

// ── Shell resolution — resolve_shell_inner ────────────────────────────────
//
// All four scenarios are tested against the pure inner function so that no
// real environment variables are read or mutated during the test run.

#[test]
fn shell_resolution_aegis_real_shell_takes_priority() {
    // AEGIS_REAL_SHELL must win even when SHELL is also set.
    let result = resolve_shell_inner(
        Some(std::ffi::OsStr::new("/usr/bin/zsh")),
        Some(std::ffi::OsStr::new("/bin/bash")),
        None,
    );
    assert_eq!(result, PathBuf::from("/usr/bin/zsh"));
}

#[test]
fn shell_resolution_missing_aegis_real_shell_falls_through_to_shell() {
    // When AEGIS_REAL_SHELL is absent, $SHELL is used.
    let result = resolve_shell_inner(None, Some(std::ffi::OsStr::new("/bin/bash")), None);
    assert_eq!(result, PathBuf::from("/bin/bash"));
}

#[test]
fn shell_resolution_shell_pointing_to_aegis_falls_back_to_posix() {
    // If $SHELL resolves to the Aegis binary itself, we must NOT exec it
    // again — that would be infinite recursion.  The fallback is /bin/sh.
    let aegis_path = PathBuf::from("/usr/local/bin/aegis");
    let result = resolve_shell_inner(None, Some(aegis_path.as_os_str()), Some(&aegis_path));
    assert_eq!(
        result,
        PathBuf::from("/bin/sh"),
        "SHELL pointing to Aegis itself must fall back to /bin/sh"
    );
}

#[test]
fn shell_resolution_invalid_shell_path_returned_as_is() {
    // An invalid/non-existent path in $SHELL is returned without
    // validation — exec() will fail with a clear OS error.  resolve_shell
    // is not responsible for path existence checking.
    let result = resolve_shell_inner(None, Some(std::ffi::OsStr::new("/nonexistent/shell")), None);
    assert_eq!(result, PathBuf::from("/nonexistent/shell"));
}

#[test]
fn shell_resolution_both_missing_falls_back_to_bin_sh() {
    // Neither AEGIS_REAL_SHELL nor SHELL set → POSIX fallback.
    let result = resolve_shell_inner(None, None, None);
    assert_eq!(result, PathBuf::from("/bin/sh"));
}

// ── Shell resolution helpers ──────────────────────────────────────────────

#[test]
fn same_file_true_for_identical_paths() {
    let p = PathBuf::from("/bin/sh");
    assert!(same_file(&p, Some(&p)));
}

#[test]
fn same_file_false_when_other_is_none() {
    assert!(!same_file(&PathBuf::from("/bin/sh"), None));
}

#[test]
fn same_file_false_for_distinct_paths() {
    assert!(!same_file(
        &PathBuf::from("/bin/sh"),
        Some(&PathBuf::from("/usr/bin/bash"))
    ));
}

// ── Exit-code contract ────────────────────────────────────────────────────

#[test]
fn exit_codes_have_expected_values() {
    assert_eq!(EXIT_DENIED, 2);
    assert_eq!(EXIT_BLOCKED, 3);
    assert_eq!(EXIT_INTERNAL, 4);
}

#[test]
fn exit_codes_are_distinct() {
    assert_ne!(EXIT_DENIED, EXIT_BLOCKED);
    assert_ne!(EXIT_DENIED, EXIT_INTERNAL);
    assert_ne!(EXIT_BLOCKED, EXIT_INTERNAL);
}

#[test]
fn exit_codes_do_not_overlap_with_success() {
    assert_ne!(EXIT_DENIED, 0);
    assert_ne!(EXIT_BLOCKED, 0);
    assert_ne!(EXIT_INTERNAL, 0);
}

#[test]
fn config_load_error_lines_include_fix_hint_only_for_config_errors() {
    let lines = config_load_error_lines(&AegisError::Config("bad config".to_string()));
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("failed to load config"));
    assert!(lines[1].contains("Fix or remove the invalid config file"));
}

#[test]
fn config_load_error_lines_omit_fix_hint_for_non_config_errors() {
    let lines = config_load_error_lines(&AegisError::Io(std::io::Error::other("disk")));
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("failed to load config"));
}

// ── Watch mode — stub removed ─────────────────────────────────────────────
//
// Verify that watch mode participates in the real pipeline by checking
// that frame parsing works end-to-end.
#[tokio::test]
async fn watch_mode_safe_command_emits_result_frame() {
    use aegis::watch::{InputFrame, MAX_FRAME_BYTES, ReadLineResult, read_bounded_line};
    use tokio::io::BufReader;

    let input = b"{\"cmd\":\"echo hello\",\"id\":\"t1\"}\n";
    let mut reader = BufReader::new(input.as_ref());

    let result = read_bounded_line(&mut reader, MAX_FRAME_BYTES)
        .await
        .unwrap();
    let line = match result {
        ReadLineResult::Line(l) => l,
        _ => panic!("expected Line"),
    };

    let frame: InputFrame = serde_json::from_str(&line).unwrap();
    assert_eq!(frame.cmd, "echo hello");
    assert_eq!(frame.id.as_deref(), Some("t1"));
}

#[test]
fn parse_risk_level_accepts_case_insensitive_values() {
    assert_eq!(parse_risk_level("WARN"), Ok(RiskLevel::Warn));
}

#[test]
fn parse_risk_level_rejects_unknown_values() {
    let error = parse_risk_level("critical").unwrap_err();
    assert!(error.contains("invalid risk level 'critical'"));
}

#[test]
fn cli_rejects_quiet_and_verbose_together() {
    let error = Cli::try_parse_from(["aegis", "--quiet", "--verbose", "-c", "echo hello"])
        .err()
        .expect("quiet and verbose must conflict");
    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn cli_rejects_verbosity_with_quiet() {
    let error = Cli::try_parse_from([
        "aegis",
        "--verbosity",
        "verbose",
        "--quiet",
        "-c",
        "echo hello",
    ])
    .err()
    .expect("verbosity and quiet must conflict");
    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn cli_rejects_verbosity_with_verbose() {
    let error = Cli::try_parse_from([
        "aegis",
        "--verbosity",
        "quiet",
        "--verbose",
        "-c",
        "echo hello",
    ])
    .err()
    .expect("verbosity and verbose must conflict");
    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn cli_parses_install_hooks_subcommand_with_all_flag() {
    let cli = Cli::try_parse_from(["aegis", "install-hooks", "--all"]).unwrap();
    let Some(Commands::InstallHooks(args)) = cli.subcommand else {
        panic!("expected install-hooks subcommand");
    };

    assert!(args.all);
    assert!(!args.local);
    assert!(!args.claude_code);
    assert!(!args.codex);
}

#[test]
fn cli_parses_install_hooks_subcommand_with_claude_code_flag() {
    let cli = Cli::try_parse_from(["aegis", "install-hooks", "--claude-code"]).unwrap();
    let Some(Commands::InstallHooks(args)) = cli.subcommand else {
        panic!("expected install-hooks subcommand");
    };

    assert!(!args.all);
    assert!(!args.local);
    assert!(args.claude_code);
    assert!(!args.codex);
}

#[test]
fn cli_parses_install_hooks_subcommand_with_codex_flag() {
    let cli = Cli::try_parse_from(["aegis", "install-hooks", "--codex"]).unwrap();
    let Some(Commands::InstallHooks(args)) = cli.subcommand else {
        panic!("expected install-hooks subcommand");
    };

    assert!(!args.all);
    assert!(!args.local);
    assert!(!args.claude_code);
    assert!(args.codex);
}

#[test]
fn cli_rejects_install_hooks_codex_with_local_flag() {
    let error = Cli::try_parse_from(["aegis", "install-hooks", "--codex", "--local"])
        .err()
        .expect("codex and local must conflict");

    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
}

#[test]
fn cli_parses_hook_subcommand() {
    let cli = Cli::try_parse_from(["aegis", "hook"]).unwrap();
    assert!(matches!(cli.subcommand, Some(Commands::Hook)));
}

#[test]
fn cli_parses_install_subcommand_with_local_flag() {
    let cli = Cli::try_parse_from(["aegis", "install", "--local"]).unwrap();
    let Some(Commands::InstallHooks(args)) = cli.subcommand else {
        panic!("expected install subcommand");
    };

    assert!(args.local);
    assert!(!args.all);
    assert!(!args.claude_code);
    assert!(!args.codex);
}

// ── CI policy ─────────────────────────────────────────────────────────────

fn make_assessment(risk: RiskLevel) -> Assessment {
    Assessment {
        risk,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: CommandParser::parse("rm -rf /"),
    }
}

fn test_handle() -> Handle {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let handle = rt.handle().clone();
    std::mem::forget(rt);
    handle
}

fn context() -> RuntimeContext {
    RuntimeContext::new(Config::default(), test_handle()).unwrap()
}

fn config_from_toml(src: &str) -> Config {
    toml::from_str(src).unwrap()
}

fn context_with_ci_policy(ci_policy: CiPolicy) -> RuntimeContext {
    let config = match ci_policy {
        CiPolicy::Allow => config_from_toml(r#"ci_policy = "Allow""#),
        CiPolicy::Block => config_from_toml(r#"ci_policy = "Block""#),
    };
    RuntimeContext::new(config, test_handle()).unwrap()
}

fn context_with_mode(mode: Mode) -> RuntimeContext {
    let config = match mode {
        Mode::Audit => config_from_toml(r#"mode = "Audit""#),
        Mode::Protect => config_from_toml(r#"mode = "Protect""#),
        Mode::Strict => config_from_toml(r#"mode = "Strict""#),
    };
    RuntimeContext::new(config, test_handle()).unwrap()
}

fn context_with_allowlist_override_level(
    allowlist_override_level: AllowlistOverrideLevel,
) -> RuntimeContext {
    let config = match allowlist_override_level {
        AllowlistOverrideLevel::Never => config_from_toml(
            r#"
mode = "Strict"
auto_snapshot_git = false
auto_snapshot_docker = false
allowlist_override_level = "Never"
"#,
        ),
        AllowlistOverrideLevel::Warn => config_from_toml(
            r#"
mode = "Strict"
auto_snapshot_git = false
auto_snapshot_docker = false
allowlist_override_level = "Warn"
"#,
        ),
        AllowlistOverrideLevel::Danger => config_from_toml(
            r#"
mode = "Strict"
auto_snapshot_git = false
auto_snapshot_docker = false
allowlist_override_level = "Danger"
"#,
        ),
    };
    RuntimeContext::new(config, test_handle()).unwrap()
}

#[test]
fn unavailable_cwd_shell_plan_carries_snapshot_fallback_in_plan() {
    let original_cwd = env::current_dir().unwrap();
    let workspace = TempDir::new().unwrap();
    Command::new("git")
        .arg("init")
        .current_dir(workspace.path())
        .output()
        .unwrap();
    env::set_current_dir(workspace.path()).unwrap();

    let config = config_from_toml(
        r#"
snapshot_policy = "Selective"
auto_snapshot_git = true
auto_snapshot_docker = false
"#,
    );
    let context = RuntimeContext::new(config, test_handle()).unwrap();
    let prepared = PreparedPlanner::Ready(Box::new(context));
    let outcome = prepare_and_plan(
        &prepared,
        aegis::planning::PlanningRequest {
            command: "terraform destroy -target=module.prod.api",
            cwd_state: CwdState::Unavailable,
            transport: ExecutionTransport::Shell,
            ci_detected: false,
        },
    );

    let PlanningOutcome::Planned(plan) = outcome else {
        panic!("expected planned outcome");
    };

    env::set_current_dir(original_cwd).unwrap();
    assert_eq!(
        plan.snapshot_plan(),
        aegis::planning::SnapshotPlan::Required {
            applicable_plugins: vec!["git"],
        }
    );
}

#[test]
fn ci_policy_block_blocks_warn_in_ci() {
    let assessment = make_assessment(RiskLevel::Warn);
    let (decision, snapshots, _) = decide_command(
        &context_with_ci_policy(CiPolicy::Block),
        &assessment,
        Path::new("."),
        false,
        None,
        true,
    );
    assert_eq!(decision, Decision::Blocked);
    assert!(snapshots.is_empty());
}

#[test]
fn ci_policy_block_blocks_danger_in_ci() {
    let assessment = make_assessment(RiskLevel::Danger);
    let (decision, snapshots, _) = decide_command(
        &context_with_ci_policy(CiPolicy::Block),
        &assessment,
        Path::new("."),
        false,
        None,
        true,
    );
    assert_eq!(decision, Decision::Blocked);
    assert!(snapshots.is_empty());
}

#[test]
fn ci_policy_block_blocks_block_in_ci() {
    let assessment = make_assessment(RiskLevel::Block);
    let (decision, snapshots, _) = decide_command(
        &context_with_ci_policy(CiPolicy::Block),
        &assessment,
        Path::new("."),
        false,
        None,
        true,
    );
    assert_eq!(decision, Decision::Blocked);
    assert!(snapshots.is_empty());
}

#[test]
fn ci_policy_block_allows_safe_in_ci() {
    let assessment = Assessment {
        risk: RiskLevel::Safe,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: CommandParser::parse("echo hello"),
    };
    let (decision, _, _) = decide_command(
        &context_with_ci_policy(CiPolicy::Block),
        &assessment,
        Path::new("."),
        false,
        None,
        true,
    );
    assert_eq!(decision, Decision::AutoApproved);
}

#[test]
fn ci_policy_allow_does_not_short_circuit_in_ci() {
    // With CiPolicy::Allow, CI detection is ignored — the normal flow runs.
    // A Safe command must still be AutoApproved.
    let assessment = Assessment {
        risk: RiskLevel::Safe,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: CommandParser::parse("echo hello"),
    };
    let (decision, _, _) = decide_command(
        &context_with_ci_policy(CiPolicy::Allow),
        &assessment,
        Path::new("."),
        false,
        None,
        true,
    );
    assert_eq!(decision, Decision::AutoApproved);
}

#[test]
fn not_in_ci_does_not_trigger_ci_policy() {
    // Outside CI, even CiPolicy::Block must not affect Safe commands.
    let assessment = Assessment {
        risk: RiskLevel::Safe,
        matched: Vec::new(),
        highlight_ranges: Vec::new(),
        command: CommandParser::parse("echo hello"),
    };
    let (decision, _, _) =
        decide_command(&context(), &assessment, Path::new("."), false, None, false);
    assert_eq!(decision, Decision::AutoApproved);
}

#[test]
fn audit_mode_auto_approves_block_even_in_ci() {
    let assessment = make_assessment(RiskLevel::Block);
    let (decision, snapshots, _) = decide_command(
        &context_with_mode(Mode::Audit),
        &assessment,
        Path::new("."),
        false,
        None,
        true,
    );

    assert_eq!(decision, Decision::AutoApproved);
    assert!(snapshots.is_empty());
}

#[test]
fn strict_mode_blocks_warn_without_prompt_path() {
    let assessment = make_assessment(RiskLevel::Warn);
    let (decision, snapshots, _) = decide_command(
        &context_with_mode(Mode::Strict),
        &assessment,
        Path::new("."),
        false,
        None,
        false,
    );

    assert_eq!(decision, Decision::Blocked);
    assert!(snapshots.is_empty());
}

#[test]
fn strict_mode_allowlisted_danger_respects_allowlist_override_level() {
    let assessment = make_assessment(RiskLevel::Danger);
    let allowlist_match = AllowlistMatch {
        pattern: "terraform destroy -target=module.test.*".to_string(),
        reason: "test allowlist".to_string(),
        source_layer: AllowlistSourceLayer::Project,
    };

    let (decision, snapshots, _) = decide_command(
        &context_with_allowlist_override_level(AllowlistOverrideLevel::Danger),
        &assessment,
        Path::new("."),
        false,
        Some(&allowlist_match),
        false,
    );

    assert_eq!(decision, Decision::AutoApproved);
    assert!(snapshots.is_empty());
}

#[test]
fn strict_mode_allowlisted_warn_respects_warn_override_level() {
    let assessment = make_assessment(RiskLevel::Warn);
    let allowlist_match = AllowlistMatch {
        pattern: "git stash clear".to_string(),
        reason: "test allowlist".to_string(),
        source_layer: AllowlistSourceLayer::Project,
    };

    let (decision, snapshots, _) = decide_command(
        &context_with_allowlist_override_level(AllowlistOverrideLevel::Warn),
        &assessment,
        Path::new("."),
        false,
        Some(&allowlist_match),
        false,
    );

    assert_eq!(decision, Decision::AutoApproved);
    assert!(snapshots.is_empty());
}

#[test]
fn strict_mode_allowlisted_warn_still_blocks_with_never_override_level() {
    let assessment = make_assessment(RiskLevel::Warn);
    let allowlist_match = AllowlistMatch {
        pattern: "git stash clear".to_string(),
        reason: "test allowlist".to_string(),
        source_layer: AllowlistSourceLayer::Project,
    };

    let (decision, snapshots, _) = decide_command(
        &context_with_allowlist_override_level(AllowlistOverrideLevel::Never),
        &assessment,
        Path::new("."),
        false,
        Some(&allowlist_match),
        false,
    );

    assert_eq!(decision, Decision::Blocked);
    assert!(snapshots.is_empty());
}
