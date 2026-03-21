use aegis::audit::AuditLogger;
use aegis::interceptor;
use aegis::interceptor::RiskLevel;
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
    Config,
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

fn main() {
    let Cli {
        command,
        verbose,
        subcommand,
    } = Cli::parse();

    match subcommand {
        Some(Commands::Watch) => {
            println!("watch: not yet implemented");
        }
        Some(Commands::Audit(args)) => {
            let logger = AuditLogger::default();
            match logger.query(args.last, args.risk) {
                Ok(entries) => {
                    print!("{}", AuditLogger::format_entries(&entries));
                }
                Err(err) => {
                    eprintln!("error: failed to read audit log: {err}");
                    std::process::exit(1);
                }
            }
        }
        Some(Commands::Config) => {
            println!("config: not yet implemented");
        }
        None => {
            if let Some(cmd) = command {
                match interceptor::assess(&cmd) {
                    Ok(assessment) if verbose => {
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
                    Ok(_) => {}
                    Err(err) => {
                        eprintln!("warning: interceptor scan initialization failed: {err}");
                    }
                }

                println!("intercepting: {cmd}");
            }
        }
    }
}

fn parse_risk_level(value: &str) -> Result<RiskLevel, String> {
    value.parse()
}
