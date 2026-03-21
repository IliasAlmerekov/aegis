use std::env;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};

use aegis::audit::{AuditEntry, AuditLogger, Decision};
use aegis::config::Config;
use aegis::interceptor;
use aegis::interceptor::RiskLevel;
use aegis::interceptor::parser::Parser as CommandParser;
use aegis::interceptor::scanner::Assessment;
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
                    1
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
    let assessment = assess_command(cmd, verbose);

    if verbose {
        log_assessment(&assessment);
    }

    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (decision, snapshots) = decide_command(&assessment, &cwd, verbose);

    append_audit_entry(&assessment, decision, &snapshots, verbose);

    match decision {
        Decision::Approved | Decision::AutoApproved => exec_command(cmd, verbose),
        Decision::Denied | Decision::Blocked => 1,
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
                    1
                }
            },
            Err(err) => {
                eprintln!("error: failed to resolve current directory: {err}");
                1
            }
        },
        ConfigCommand::Show => match Config::load().and_then(|config| config.to_toml_string()) {
            Ok(toml) => {
                print!("{toml}");
                0
            }
            Err(err) => {
                eprintln!("error: failed to load config: {err}");
                1
            }
        },
    }
}

fn assess_command(cmd: &str, verbose: bool) -> Assessment {
    match interceptor::assess(cmd) {
        Ok(assessment) => assessment,
        Err(err) => {
            if verbose {
                eprintln!("warning: interceptor scan initialization failed: {err}");
            }

            Assessment {
                risk: RiskLevel::Safe,
                matched: Vec::new(),
                command: CommandParser::parse(cmd),
            }
        }
    }
}

fn log_assessment(assessment: &Assessment) {
    eprintln!(
        "scan: risk={:?}, executable={}, matched={}",
        assessment.risk,
        assessment.command.executable.as_deref().unwrap_or("<none>"),
        assessment.matched.len()
    );

    for pattern in &assessment.matched {
        eprintln!(
            "match: id={}, category={:?}, risk={:?}, description={}",
            pattern.id, pattern.category, pattern.risk, pattern.description
        );

        if let Some(safe_alt) = &pattern.safe_alt {
            eprintln!("safe alternative: {safe_alt}");
        }
    }
}

fn decide_command(
    assessment: &Assessment,
    cwd: &Path,
    verbose: bool,
) -> (Decision, Vec<SnapshotRecord>) {
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
    verbose: bool,
) {
    let entry = AuditEntry::new(
        assessment.command.raw.clone(),
        assessment.risk,
        assessment
            .matched
            .iter()
            .map(|pattern| pattern.as_ref().into())
            .collect(),
        decision,
        snapshots.iter().map(Into::into).collect(),
    );

    if let Err(err) = AuditLogger::default().append(entry)
        && verbose
    {
        eprintln!("warning: failed to append audit log entry: {err}");
    }
}

fn exec_command(cmd: &str, verbose: bool) -> i32 {
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

        if verbose {
            eprintln!("error: failed to exec shell {}: {err}", shell.display());
        }

        1
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
            Ok(status) => status.code().unwrap_or(1),
            Err(err) => {
                if verbose {
                    eprintln!("error: failed to spawn shell {}: {err}", shell.display());
                }

                1
            }
        }
    }
}

fn resolve_shell() -> PathBuf {
    if let Some(shell) = env::var_os("AEGIS_REAL_SHELL").filter(|shell| !shell.is_empty()) {
        return PathBuf::from(shell);
    }

    let current_exe = env::current_exe().ok();
    if let Some(shell) = env::var_os("SHELL").filter(|shell| !shell.is_empty()) {
        let shell_path = PathBuf::from(shell);
        if !same_file(&shell_path, current_exe.as_deref()) {
            return shell_path;
        }
    }

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
