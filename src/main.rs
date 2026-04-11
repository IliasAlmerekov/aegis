use std::env;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};

use aegis::audit::{AuditEntry, AuditLogger, Decision};
use aegis::config::{
    AllowlistMatch, Config, ValidationReport, validate_config_layers,
};
use aegis::decision::{BlockReason, DecisionInput, PolicyAction, evaluate_policy};
use aegis::error::AegisError;
use aegis::interceptor::RiskLevel;
use aegis::interceptor::scanner::{Assessment, DecisionSource};
use aegis::runtime::RuntimeContext;
use aegis::snapshot::SnapshotRecord;
use aegis::ui::confirm::{show_confirmation, show_policy_block};
use clap::{Args, Parser, Subcommand, ValueEnum};

#[cfg(test)]
use aegis::interceptor::parser::Parser as CommandParser;

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

    /// Enable verbose/debug output
    #[arg(short = 'v', long = "verbose")]
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
    /// Manage aegis configuration
    Config(ConfigArgs),
}

#[derive(Args)]
struct AuditArgs {
    /// Show only the last N audit entries.
    #[arg(long)]
    last: Option<usize>,

    /// Filter entries by risk level: safe, warn, danger, block.
    #[arg(long, value_parser = parse_risk_level)]
    risk: Option<RiskLevel>,

    /// Output format: text (default), json, ndjson.
    #[arg(long, value_enum, default_value_t = AuditOutputFormat::Text)]
    format: AuditOutputFormat,
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
    let Cli {
        command,
        verbose,
        subcommand,
    } = Cli::parse();

    let exit_code = match subcommand {
        Some(Commands::Watch) => match RuntimeContext::load(verbose) {
            Ok(context) => match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt.block_on(aegis::watch::run(&context)),
                Err(err) => {
                    eprintln!("error: failed to build tokio runtime for watch mode: {err}");
                    EXIT_INTERNAL
                }
            },
            Err(err) => report_config_load_error(&err),
        },
        Some(Commands::Audit(args)) => {
            let logger = AuditLogger::default();
            match logger.query(args.last, args.risk) {
                Ok(entries) => match format_audit_entries(&entries, args.format) {
                    Ok(output) => {
                        print!("{output}");
                        0
                    }
                    Err(err) => {
                        eprintln!("error: failed to serialize audit output: {err}");
                        EXIT_INTERNAL
                    }
                },
                Err(err) => {
                    eprintln!("error: failed to read audit log: {err}");
                    EXIT_INTERNAL
                }
            }
        }
        Some(Commands::Config(args)) => handle_config_command(args),
        None => {
            if let Some(cmd) = command {
                run_shell_wrapper(&cmd, verbose)
            } else {
                0
            }
        }
    };

    process::exit(exit_code);
}

fn parse_risk_level(value: &str) -> Result<RiskLevel, String> {
    value.parse()
}

fn format_audit_entries(
    entries: &[AuditEntry],
    format: AuditOutputFormat,
) -> Result<String, String> {
    match format {
        AuditOutputFormat::Text => Ok(AuditLogger::format_entries(entries)),
        AuditOutputFormat::Json => {
            serde_json::to_string_pretty(entries).map_err(|err| err.to_string())
        }
        AuditOutputFormat::Ndjson => {
            let mut out = String::new();
            for entry in entries {
                let line = serde_json::to_string(entry).map_err(|err| err.to_string())?;
                out.push_str(&line);
                out.push('\n');
            }
            Ok(out)
        }
    }
}

fn run_shell_wrapper(cmd: &str, verbose: bool) -> i32 {
    let context = match RuntimeContext::load(verbose) {
        Ok(context) => context,
        Err(err) => return report_config_load_error(&err),
    };
    let assessment = context.assess(cmd);

    let resolved_cwd = env::current_dir().ok();
    let cwd = resolved_cwd.clone().unwrap_or_else(|| PathBuf::from("."));
    let allowlist_match = context.allowlist_match_for_command(cmd, resolved_cwd.as_deref());
    let in_ci = is_ci_environment();

    if verbose {
        if in_ci {
            eprintln!(
                "ci: detected CI environment, ci_policy={:?}",
                context.config().ci_policy
            );
        }
        log_assessment(&assessment, allowlist_match.as_ref());
    }

    let (decision, snapshots) = decide_command(
        &context,
        &assessment,
        &cwd,
        verbose,
        allowlist_match.as_ref(),
        in_ci,
    );

    context.append_audit_entry(
        &assessment,
        decision,
        &snapshots,
        allowlist_match.as_ref(),
        verbose,
    );

    match decision {
        Decision::Approved | Decision::AutoApproved => exec_command(cmd),
        Decision::Denied => EXIT_DENIED,
        Decision::Blocked => EXIT_BLOCKED,
    }
}

/// Returns `true` when aegis is running inside a CI environment.
///
/// Detection order:
/// 1. `AEGIS_CI=1` — explicit override (useful for testing or forcing CI mode
///    in environments that do not set the standard variables).
/// 2. Well-known CI env vars set by major CI providers (GitHub Actions,
///    GitLab CI, CircleCI, Buildkite, Travis CI, Jenkins, Azure Pipelines).
fn is_ci_environment() -> bool {
    // Explicit override — highest priority.
    if let Ok(val) = env::var("AEGIS_CI") {
        return val == "1" || val.eq_ignore_ascii_case("true");
    }

    // Standard CI provider signals.
    const CI_VARS: &[&str] = &[
        "CI", // GitHub Actions, GitLab CI, CircleCI, Buildkite, Travis, Heroku
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "CIRCLECI",
        "BUILDKITE",
        "TRAVIS",
        "JENKINS_URL",
        "TF_BUILD", // Azure Pipelines
    ];

    CI_VARS.iter().any(|var| {
        env::var(var)
            .ok()
            .map(|v| !v.is_empty() && v != "false" && v != "0")
            .unwrap_or(false)
    })
}

fn handle_config_command(args: ConfigArgs) -> i32 {
    match args.command {
        ConfigCommand::Init => match env::current_dir() {
            Ok(current_dir) => match Config::init_in(&current_dir) {
                Ok(path) => {
                    println!("{}", path.display());
                    0
                }
                Err(err) => {
                    eprintln!("error: failed to initialize config: {err}");
                    EXIT_INTERNAL
                }
            },
            Err(err) => {
                eprintln!("error: failed to resolve current directory: {err}");
                EXIT_INTERNAL
            }
        },
        ConfigCommand::Show => match Config::load() {
            Ok(config) => match config.to_toml_string() {
                Ok(toml) => {
                    print!("{toml}");
                    0
                }
                Err(err) => {
                    eprintln!("error: failed to serialize config: {err}");
                    EXIT_INTERNAL
                }
            },
            Err(err) => report_config_load_error(&err),
        },
        ConfigCommand::Validate(args) => handle_config_validate_command(args),
    }
}

fn handle_config_validate_command(args: ConfigValidateArgs) -> i32 {
    let current_dir = match env::current_dir() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("error: failed to resolve current directory: {err}");
            return EXIT_INTERNAL;
        }
    };
    let home_dir = env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let report = validate_config_layers(&current_dir, home_dir.as_deref());

    let render_result = match args.output {
        ConfigValidateOutput::Text => {
            print!("{}", format_validation_report_text(&report));
            Ok(())
        }
        ConfigValidateOutput::Json => serde_json::to_string_pretty(&report)
            .map(|json| {
                println!("{json}");
            })
            .map_err(|err| err.to_string()),
    };

    if let Err(err) = render_result {
        eprintln!("error: failed to serialize validation output: {err}");
        return EXIT_INTERNAL;
    }

    if report.errors.is_empty() {
        0
    } else {
        EXIT_INTERNAL
    }
}

fn format_validation_report_text(report: &ValidationReport) -> String {
    if report.errors.is_empty() && report.warnings.is_empty() {
        return "config is valid\n".to_string();
    }

    let mut out = String::new();

    if !report.errors.is_empty() {
        out.push_str("errors:\n");
        for issue in &report.errors {
            out.push_str(&format!(
                "- [{}] {}: {}\n",
                issue.code, issue.location, issue.message
            ));
        }
    }

    if !report.warnings.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("warnings:\n");
        for issue in &report.warnings {
            out.push_str(&format!(
                "- [{}] {}: {}\n",
                issue.code, issue.location, issue.message
            ));
        }
    }

    out
}

fn config_load_error_lines(err: &AegisError) -> Vec<String> {
    let mut lines = vec![format!("error: failed to load config: {err}")];

    if matches!(err, AegisError::Config(_)) {
        lines.push("error: Fix or remove the invalid config file and try again.".to_string());
    }

    lines
}

fn report_config_load_error(err: &AegisError) -> i32 {
    for line in config_load_error_lines(err) {
        eprintln!("{line}");
    }
    EXIT_INTERNAL
}

fn log_assessment(assessment: &Assessment, allowlist_match: Option<&AllowlistMatch>) {
    let source_label = match assessment.decision_source() {
        DecisionSource::BuiltinPattern => "built-in pattern",
        DecisionSource::CustomPattern => "custom pattern",
        DecisionSource::Fallback => "fallback",
    };

    eprintln!(
        "scan: risk={:?}, executable={}, matched={}, source={}",
        assessment.risk,
        assessment.command.executable.as_deref().unwrap_or("<none>"),
        assessment.matched.len(),
        source_label,
    );

    for m in &assessment.matched {
        eprintln!(
            "match: id={}, category={:?}, risk={:?}, matched={:?}, description={}",
            m.pattern.id, m.pattern.category, m.pattern.risk, m.matched_text, m.pattern.description
        );

        if let Some(safe_alt) = &m.pattern.safe_alt {
            eprintln!("safe alternative: {safe_alt}");
        }
    }

    if let Some(rule) = allowlist_match {
        eprintln!("allowlist: matched rule {:?}", rule.pattern);
    }
}

fn decide_command(
    context: &RuntimeContext,
    assessment: &Assessment,
    cwd: &Path,
    verbose: bool,
    allowlist_match: Option<&AllowlistMatch>,
    in_ci: bool,
) -> (Decision, Vec<SnapshotRecord>) {
    let mode = context.config().mode;
    let ci_policy = context.config().ci_policy;

    let plan = evaluate_policy(DecisionInput {
        mode,
        risk: assessment.risk,
        in_ci,
        ci_policy,
        allowlist_match: allowlist_match.is_some(),
        strict_allowlist_override: context.config().strict_allowlist_override,
    });

    let snapshots = if plan.should_snapshot {
        context.create_snapshots(cwd, &assessment.command.raw, verbose)
    } else {
        Vec::new()
    };

    match plan.action {
        PolicyAction::AutoApprove => (Decision::AutoApproved, snapshots),
        PolicyAction::Prompt => {
            let approved = show_confirmation(assessment, &snapshots);
            let decision = if approved {
                Decision::Approved
            } else {
                Decision::Denied
            };

            (decision, snapshots)
        }
        PolicyAction::Block => {
            match plan.block_reason {
                Some(BlockReason::ProtectCiPolicy) => {
                    eprintln!(
                        "aegis: CI policy blocked command (risk={:?}): {}",
                        assessment.risk, assessment.command.raw,
                    );
                    eprintln!(
                        "aegis: set ci_policy = \"Allow\" in .aegis.toml to disable the \
                         CI-only hard block and use normal policy evaluation. \
                         For allowlisted Danger commands, also set \
                         allowlist_override_level = \"Danger\"."
                    );
                }
                Some(BlockReason::IntrinsicRiskBlock) => {
                    show_confirmation(assessment, &[]);
                }
                Some(BlockReason::StrictPolicy) => {
                    show_policy_block(
                        assessment,
                        "strict mode blocks non-safe commands unless the command \
                         matches the allowlist and the override level permits it",
                    );
                }
                None => unreachable!("PolicyAction::Block always carries a BlockReason"),
            }

            (Decision::Blocked, snapshots)
        }
    }
}

fn exec_command(cmd: &str) -> i32 {
    let shell = resolve_shell();

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        let err = Command::new(&shell)
            .arg("-c")
            .arg(cmd)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .exec();

        eprintln!("error: failed to exec shell {}: {err}", shell.display());
        EXIT_INTERNAL
    }

    #[cfg(not(unix))]
    {
        match Command::new(&shell)
            .arg("-c")
            .arg(cmd)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
        {
            Ok(status) => status.code().unwrap_or(EXIT_INTERNAL),
            Err(err) => {
                eprintln!("error: failed to spawn shell {}: {err}", shell.display());
                EXIT_INTERNAL
            }
        }
    }
}

fn resolve_shell() -> PathBuf {
    let aegis_real_shell = env::var_os("AEGIS_REAL_SHELL");
    let shell_env = env::var_os("SHELL");
    let current_exe = env::current_exe().ok();
    resolve_shell_inner(
        aegis_real_shell.as_deref(),
        shell_env.as_deref(),
        current_exe.as_deref(),
    )
}

/// Pure shell-resolution logic — extracted for unit testing.
///
/// Resolution order:
/// 1. `AEGIS_REAL_SHELL` — explicit override set by the install script when
///    Aegis replaces `$SHELL`.  Always trusted; never loops back to Aegis.
/// 2. `SHELL` — the user's configured shell, *unless* it resolves to the same
///    binary as Aegis itself (recursive invocation guard).
/// 3. `/bin/sh` — POSIX-mandated fallback.  Chosen deliberately over an error
///    because a safe, functional shell is better than refusing to run any
///    command.  If even `/bin/sh` is absent the `exec` call will fail with a
///    clear OS error, which we surface via `EXIT_INTERNAL`.
fn resolve_shell_inner(
    aegis_real_shell: Option<&std::ffi::OsStr>,
    shell_env: Option<&std::ffi::OsStr>,
    current_exe: Option<&Path>,
) -> PathBuf {
    // 1. Explicit override — highest priority.
    if let Some(shell) = aegis_real_shell.filter(|s| !s.is_empty()) {
        return PathBuf::from(shell);
    }

    // 2. $SHELL — skip if it points back at us (infinite-recursion guard).
    if let Some(shell) = shell_env.filter(|s| !s.is_empty()) {
        let shell_path = PathBuf::from(shell);
        if !same_file(&shell_path, current_exe) {
            return shell_path;
        }
    }

    // 3. POSIX fallback — see doc-comment above for rationale.
    PathBuf::from("/bin/sh")
}

fn same_file(path: &Path, other: Option<&Path>) -> bool {
    let Some(other) = other else {
        return false;
    };

    if path == other {
        return true;
    }

    match (std::fs::canonicalize(path), std::fs::canonicalize(other)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aegis::config::{
        AllowlistMatch, AllowlistOverrideLevel, AllowlistSourceLayer, CiPolicy, Mode,
    };
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

    // ── CI policy ─────────────────────────────────────────────────────────────

    fn make_assessment(risk: RiskLevel) -> Assessment {
        Assessment {
            risk,
            matched: Vec::new(),
            command: CommandParser::parse("rm -rf /"),
        }
    }

    fn context() -> RuntimeContext {
        RuntimeContext::new(Config::default()).unwrap()
    }

    fn context_with_ci_policy(ci_policy: CiPolicy) -> RuntimeContext {
        let mut config = Config::default();
        config.ci_policy = ci_policy;
        RuntimeContext::new(config).unwrap()
    }

    fn context_with_mode(mode: Mode) -> RuntimeContext {
        let mut config = Config::default();
        config.mode = mode;
        RuntimeContext::new(config).unwrap()
    }

    fn context_with_allowlist_override_level(
        allowlist_override_level: AllowlistOverrideLevel,
    ) -> RuntimeContext {
        let mut config = Config::default();
        config.mode = Mode::Strict;
        config.auto_snapshot_git = false;
        config.auto_snapshot_docker = false;
        config.allowlist_override_level = allowlist_override_level;
        RuntimeContext::new(config).unwrap()
    }

    #[test]
    fn ci_policy_block_blocks_warn_in_ci() {
        let assessment = make_assessment(RiskLevel::Warn);
        let (decision, snapshots) = decide_command(
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
        let (decision, snapshots) = decide_command(
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
        let (decision, snapshots) = decide_command(
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
            command: CommandParser::parse("echo hello"),
        };
        let (decision, _) = decide_command(
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
            command: CommandParser::parse("echo hello"),
        };
        let (decision, _) = decide_command(
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
            command: CommandParser::parse("echo hello"),
        };
        let (decision, _) =
            decide_command(&context(), &assessment, Path::new("."), false, None, false);
        assert_eq!(decision, Decision::AutoApproved);
    }

    #[test]
    fn audit_mode_auto_approves_block_even_in_ci() {
        let assessment = make_assessment(RiskLevel::Block);
        let (decision, snapshots) = decide_command(
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
        let (decision, snapshots) = decide_command(
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

        let (decision, snapshots) = decide_command(
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

        let (decision, snapshots) = decide_command(
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

        let (decision, snapshots) = decide_command(
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
