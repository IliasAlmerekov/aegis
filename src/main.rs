use std::process;

use aegis::audit::{AuditTimestamp, Decision};
use aegis::interceptor::RiskLevel;
use clap::{Args, Parser, Subcommand, ValueEnum};

#[cfg(test)]
use aegis::config::Config;
#[cfg(test)]
use aegis::decision::ExecutionTransport;
#[cfg(test)]
use aegis::interceptor::parser::Parser as CommandParser;
#[cfg(test)]
use aegis::interceptor::scanner::Assessment;
#[cfg(test)]
use aegis::planning::{CwdState, PlanningOutcome, PreparedPlanner, prepare_and_plan};
#[cfg(test)]
use aegis::runtime::RuntimeContext;
#[cfg(test)]
use aegis::snapshot::SnapshotRecord;
#[cfg(test)]
use std::env;
#[cfg(test)]
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::process::Command;
#[cfg(test)]
use tokio::runtime::Handle;

mod cli_commands;
mod cli_dispatch;
mod install;
mod policy_output;
mod rollback;
mod shell_compat;
mod shell_flow;
mod shell_wrapper;

#[derive(Parser)]
#[command(
    name = "aegis",
    version,
    about = "A terminal proxy that intercepts AI agent commands"
)]
struct Cli {
    /// Command to intercept (shell wrapper mode)
    #[arg(short = 'c', long = "command")]
    command: Option<String>,

    /// Shell-wrapper output format: text (default) or evaluation-only json.
    #[arg(long, value_enum, default_value_t = CommandOutputFormat::Text)]
    output: CommandOutputFormat,

    /// Control Aegis text output detail: quiet, standard, or verbose.
    #[arg(
        long,
        value_enum,
        default_value_t = OutputVerbosity::Standard,
        conflicts_with_all = ["quiet", "verbose"]
    )]
    verbosity: OutputVerbosity,

    /// Shorthand for `--verbosity quiet`.
    #[arg(long, conflicts_with = "verbose")]
    quiet: bool,

    /// Shorthand for `--verbosity verbose`.
    #[arg(short = 'v', long = "verbose", conflicts_with = "quiet")]
    verbose: bool,

    #[command(subcommand)]
    subcommand: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Read NDJSON command frames from stdin and stream results to stdout
    Watch,
    /// View the audit log
    Audit(AuditArgs),
    /// Enable Aegis by removing ~/.aegis/disabled
    On,
    /// Disable Aegis by creating ~/.aegis/disabled
    Off,
    /// Show the current toggle state and active config path
    Status,
    /// Roll back a previously recorded snapshot
    Rollback(RollbackArgs),
    /// Manage aegis configuration
    Config(ConfigArgs),
    /// Run as a Claude Code PreToolUse hook — rewrites Bash commands through aegis
    Hook,
    /// Install aegis hooks into Claude Code config
    Install(InstallArgs),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum CommandOutputFormat {
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputVerbosity {
    Quiet,
    Standard,
    Verbose,
}

impl OutputVerbosity {
    fn from_cli(verbosity: Self, quiet: bool, verbose: bool) -> Self {
        if quiet {
            Self::Quiet
        } else if verbose {
            Self::Verbose
        } else {
            verbosity
        }
    }

    fn is_verbose(self) -> bool {
        matches!(self, Self::Verbose)
    }
}

#[derive(Args)]
struct AuditArgs {
    /// Show only the last N audit entries.
    #[arg(long)]
    last: Option<usize>,

    /// Filter entries by risk level: safe, warn, danger, block.
    #[arg(long, value_parser = parse_risk_level)]
    risk: Option<RiskLevel>,

    /// Show only entries at or after this RFC 3339 timestamp.
    #[arg(long, value_parser = parse_audit_timestamp)]
    since: Option<AuditTimestamp>,

    /// Show only entries at or before this RFC 3339 timestamp.
    #[arg(long, value_parser = parse_audit_timestamp)]
    until: Option<AuditTimestamp>,

    /// Filter to commands containing this case-sensitive substring.
    #[arg(long)]
    command_contains: Option<String>,

    /// Filter entries by decision: approved, denied, auto-approved, blocked.
    #[arg(long, value_parser = parse_decision)]
    decision: Option<Decision>,

    /// Output format: text (default), json, ndjson.
    #[arg(long, value_enum, default_value_t = AuditOutputFormat::Text)]
    format: AuditOutputFormat,

    /// Show an aggregated summary instead of individual entries.
    #[arg(long)]
    summary: bool,

    /// Verify tamper-evident hash chaining across all audit segments.
    #[arg(long)]
    verify_integrity: bool,
}

#[derive(Args)]
struct RollbackArgs {
    /// Snapshot ID copied from `aegis audit`
    snapshot_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum AuditOutputFormat {
    Text,
    Json,
    Ndjson,
}

#[derive(Args)]
struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Args)]
struct InstallArgs {
    /// Patch ./.claude/settings.json instead of ~/.claude/settings.json
    #[arg(long)]
    local: bool,
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Create a project-local .aegis.toml in the current directory
    Init,
    /// Print the active config after applying search order and defaults
    Show,
    /// Validate the active config and report errors/warnings
    Validate(ConfigValidateArgs),
}

#[derive(Args)]
struct ConfigValidateArgs {
    /// Validation output format.
    #[arg(long, value_enum, default_value_t = ConfigValidateOutput::Text)]
    output: ConfigValidateOutput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ConfigValidateOutput {
    Text,
    Json,
}

// ── Exit-code contract ────────────────────────────────────────────────────────
//
// Aegis uses a small set of reserved exit codes so that callers (AI agents,
// CI pipelines, shell scripts) can distinguish *why* a command did not run
// from a normal command failure.
//
// | Code | Meaning                                                          |
// |------|------------------------------------------------------------------|
// |  0   | Success — command was approved and exited 0.                     |
// | 1-N  | Pass-through — the underlying command ran and returned this code.|
// |  2   | Denied — user pressed 'n' at the confirmation dialog.           |
// |  3   | Blocked — command matched a Block-level pattern; no dialog shown.|
// |  4   | Aegis/config error — internal failure or config validation failed |
// |      |   (e.g. `aegis config validate` found hard errors).              |
//
// Codes 2, 3, and 4 are only returned when Aegis prevents execution; they
// are never returned by a successfully launched child process.

/// The user explicitly denied the command at the confirmation dialog.
const EXIT_DENIED: i32 = 2;
/// The command matched a `Block`-level pattern and was hard-stopped.
const EXIT_BLOCKED: i32 = 3;
/// Aegis/config failure prevented execution or validation from succeeding.
const EXIT_INTERNAL: i32 = 4;

fn main() {
    let invocation = match shell_compat::parse_invocation_mode() {
        Ok(invocation) => invocation,
        Err(message) => {
            eprintln!("error: {message}");
            process::exit(2);
        }
    };

    // Build one Tokio runtime for the entire process lifetime.
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            eprintln!("error: failed to build tokio runtime: {err}");
            process::exit(EXIT_INTERNAL);
        }
    };
    let handle = rt.handle().clone();

    let exit_code = match invocation {
        shell_compat::InvocationMode::Cli(cli) => cli_dispatch::run_cli(cli, &rt, handle),
        shell_compat::InvocationMode::ShellCompatCommand { command, launch } => {
            shell_wrapper::run_shell_wrapper(
                &command,
                CommandOutputFormat::Text,
                OutputVerbosity::Standard,
                handle,
                &launch,
            )
        }
        shell_compat::InvocationMode::ShellCompatSession { launch } => {
            shell_compat::exec_shell_session(&launch)
        }
    };

    process::exit(exit_code);
}

fn parse_risk_level(value: &str) -> Result<RiskLevel, String> {
    value.parse()
}

fn parse_audit_timestamp(value: &str) -> Result<AuditTimestamp, String> {
    AuditTimestamp::parse_rfc3339(value)
}

fn parse_decision(value: &str) -> Result<Decision, String> {
    value.parse()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_commands::config_load_error_lines;
    use crate::shell_compat::{
        InvocationMode, ShellLaunchOptions, parse_shell_compat_invocation, resolve_shell_inner,
        same_file,
    };
    use crate::shell_flow::decide_command;
    use aegis::config::{
        AllowlistMatch, AllowlistOverrideLevel, AllowlistSourceLayer, CiPolicy, Mode,
    };
    use aegis::error::AegisError;
    use tempfile::TempDir;

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
        let result =
            resolve_shell_inner(None, Some(std::ffi::OsStr::new("/nonexistent/shell")), None);
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
    fn cli_parses_hook_subcommand() {
        let cli = Cli::try_parse_from(["aegis", "hook"]).unwrap();
        assert!(matches!(cli.subcommand, Some(Commands::Hook)));
    }

    #[test]
    fn cli_parses_install_subcommand_with_local_flag() {
        let cli = Cli::try_parse_from(["aegis", "install", "--local"]).unwrap();
        let Some(Commands::Install(args)) = cli.subcommand else {
            panic!("expected install subcommand");
        };

        assert!(args.local);
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

    fn context_with_ci_policy(ci_policy: CiPolicy) -> RuntimeContext {
        let mut config = Config::default();
        config.ci_policy = ci_policy;
        RuntimeContext::new(config, test_handle()).unwrap()
    }

    fn context_with_mode(mode: Mode) -> RuntimeContext {
        let mut config = Config::default();
        config.mode = mode;
        RuntimeContext::new(config, test_handle()).unwrap()
    }

    fn context_with_allowlist_override_level(
        allowlist_override_level: AllowlistOverrideLevel,
    ) -> RuntimeContext {
        let mut config = Config::default();
        config.mode = Mode::Strict;
        config.auto_snapshot_git = false;
        config.auto_snapshot_docker = false;
        config.allowlist_override_level = allowlist_override_level;
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

        let mut config = Config::default();
        config.snapshot_policy = aegis::config::SnapshotPolicy::Selective;
        config.auto_snapshot_git = true;
        config.auto_snapshot_docker = false;
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
}
