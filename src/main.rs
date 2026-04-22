use std::process;

use aegis::audit::{AuditTimestamp, Decision};
use aegis::interceptor::RiskLevel;
use clap::{Args, Parser, Subcommand, ValueEnum};

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
    /// Install aegis hooks into Claude Code and Codex
    #[command(alias = "install")]
    InstallHooks(InstallArgs),
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
    /// Patch ./.claude/settings.json instead of ~/.claude/settings.json when
    /// installing Claude Code hooks.
    #[arg(long, conflicts_with = "codex")]
    local: bool,

    /// Install hooks for Claude Code and Codex.
    #[arg(long, conflicts_with_all = ["claude_code", "codex"])]
    all: bool,

    /// Install only Claude Code hooks.
    #[arg(long = "claude-code", conflicts_with_all = ["all", "codex"])]
    claude_code: bool,

    /// Install only Codex hooks.
    #[arg(long, conflicts_with_all = ["all", "claude_code"])]
    codex: bool,
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
#[path = "main_tests.rs"]
mod tests;
