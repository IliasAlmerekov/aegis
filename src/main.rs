use aegis::interceptor;
use clap::{Parser, Subcommand};

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
    Audit,
    /// Manage aegis configuration
    Config,
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
        Some(Commands::Audit) => {
            println!("audit: not yet implemented");
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
