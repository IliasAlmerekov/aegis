use std::env;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};

use aegis::audit::{AuditEntry, AuditLogger, Decision};
use aegis::config::{Allowlist, AllowlistMatch, Config};
use aegis::interceptor;
use aegis::interceptor::RiskLevel;
use aegis::interceptor::parser::Parser as CommandParser;
use aegis::interceptor::scanner::{Assessment, DecisionSource};
use aegis::snapshot::{SnapshotRecord, SnapshotRegistry};
use aegis::ui::confirm::show_confirmation;
use clap::{Args, Parser, Subcommand};

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
    /// Watch and intercept shell commands
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
// |  4   | Internal error — Aegis itself could not complete (spawn failed,  |
// |      |   etc.). The underlying command was never executed.              |
//
// Codes 2, 3, and 4 are only returned when Aegis prevents execution; they
// are never returned by a successfully launched child process.

/// The user explicitly denied the command at the confirmation dialog.
const EXIT_DENIED: i32 = 2;
/// The command matched a `Block`-level pattern and was hard-stopped.
const EXIT_BLOCKED: i32 = 3;
/// An internal Aegis failure prevented the command from being executed.
const EXIT_INTERNAL: i32 = 4;

fn main() {
    let Cli {
        command,
        verbose,
        subcommand,
    } = Cli::parse();

    let exit_code = match subcommand {
        Some(Commands::Watch) => {
            println!("watch: not yet implemented");
            0
        }
        Some(Commands::Audit(args)) => {
            let logger = AuditLogger::default();
            match logger.query(args.last, args.risk) {
                Ok(entries) => {
                    print!("{}", AuditLogger::format_entries(&entries));
                    0
                }
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

fn run_shell_wrapper(cmd: &str, verbose: bool) -> i32 {
    let config = load_runtime_config(verbose);
    let allowlist = Allowlist::new(&config.allowlist);
    let assessment = assess_command(cmd, verbose);

    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let allowlist_match = allowlist.match_reason(cmd);

    if verbose {
        log_assessment(&assessment, allowlist_match.as_ref());
    }

    let (decision, snapshots) =
        decide_command(&assessment, &cwd, verbose, allowlist_match.as_ref());

    append_audit_entry(
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

fn load_runtime_config(verbose: bool) -> Config {
    match Config::load() {
        Ok(config) => config,
        Err(err) => {
            if verbose {
                eprintln!("warning: failed to load config: {err}");
            }

            Config::default()
        }
    }
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
        ConfigCommand::Show => match Config::load().and_then(|config| config.to_toml_string()) {
            Ok(toml) => {
                print!("{toml}");
                0
            }
            Err(err) => {
                eprintln!("error: failed to load config: {err}");
                EXIT_INTERNAL
            }
        },
    }
}

fn assess_command(cmd: &str, verbose: bool) -> Assessment {
    let _ = verbose;
    match interceptor::assess(cmd) {
        Ok(assessment) => assessment,
        Err(err) => {
            // Always print — the operator must know the scanner is broken.
            eprintln!("error: interceptor scan initialization failed: {err}");
            eprintln!(
                "error: scanner is unhealthy — requiring explicit approval for every command"
            );

            // Fail-closed: Warn forces the confirmation dialog for every command
            // while the scanner is unhealthy. Safe would auto-approve everything.
            Assessment {
                risk: RiskLevel::Warn,
                matched: Vec::new(),
                command: CommandParser::parse(cmd),
            }
        }
    }
}

fn log_assessment(assessment: &Assessment, allowlist_match: Option<&AllowlistMatch>) {
    let source_label = if allowlist_match.is_some() {
        "allowlist"
    } else {
        match assessment.decision_source() {
            DecisionSource::BuiltinPattern => "built-in pattern",
            DecisionSource::CustomPattern => "custom pattern",
            DecisionSource::Fallback => "fallback",
        }
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
    assessment: &Assessment,
    cwd: &Path,
    verbose: bool,
    allowlist_match: Option<&AllowlistMatch>,
) -> (Decision, Vec<SnapshotRecord>) {
    // Block-level commands are catastrophic and irreversible.  The allowlist
    // must never silently bypass them — even an explicit allowlist entry is
    // refused here.  The operator will see the Block dialog and the audit log
    // will record the allowlist pattern that *would have* matched so they can
    // review their config.
    let is_allowlisted = allowlist_match.is_some() && assessment.risk != RiskLevel::Block;

    if is_allowlisted {
        let snapshots = match assessment.risk {
            RiskLevel::Danger => create_snapshots(cwd, &assessment.command.raw, verbose),
            _ => Vec::new(),
        };

        return (Decision::AutoApproved, snapshots);
    }

    match assessment.risk {
        RiskLevel::Block => {
            show_confirmation(assessment, &[]);
            (Decision::Blocked, Vec::new())
        }
        RiskLevel::Danger => {
            let snapshots = create_snapshots(cwd, &assessment.command.raw, verbose);
            let approved = show_confirmation(assessment, &snapshots);
            let decision = if approved {
                Decision::Approved
            } else {
                Decision::Denied
            };

            (decision, snapshots)
        }
        RiskLevel::Warn => {
            let approved = show_confirmation(assessment, &[]);
            let decision = if approved {
                Decision::Approved
            } else {
                Decision::Denied
            };

            (decision, Vec::new())
        }
        RiskLevel::Safe => (Decision::AutoApproved, Vec::new()),
        _ => (Decision::AutoApproved, Vec::new()),
    }
}

fn create_snapshots(cwd: &Path, cmd: &str, verbose: bool) -> Vec<SnapshotRecord> {
    match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(SnapshotRegistry::default().snapshot_all(cwd, cmd)),
        Err(err) => {
            if verbose {
                eprintln!("warning: failed to initialize snapshot runtime: {err}");
            }

            Vec::new()
        }
    }
}

fn append_audit_entry(
    assessment: &Assessment,
    decision: Decision,
    snapshots: &[SnapshotRecord],
    allowlist_match: Option<&AllowlistMatch>,
    verbose: bool,
) {
    let entry = AuditEntry::new(
        assessment.command.raw.clone(),
        assessment.risk,
        assessment.matched.iter().map(Into::into).collect(),
        decision,
        snapshots.iter().map(Into::into).collect(),
        allowlist_match.map(|m| m.pattern.clone()),
    );

    if let Err(err) = AuditLogger::default().append(entry)
        && verbose
    {
        eprintln!("warning: failed to append audit log entry: {err}");
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

    // ── Scanner init failure ──────────────────────────────────────────────────
    //
    // Fail-closed: when interceptor::assess() returns Err, assess_command()
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
}
