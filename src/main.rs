mod audit;
mod config;
mod error;
mod interceptor;
mod snapshot;
mod ui;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "aegis", version, about = "A terminal proxy that intercepts AI agent commands")]
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
    let cli = Cli::parse();

    match cli.subcommand {
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
            if let Some(cmd) = cli.command {
                println!("intercepting: {cmd}");
            }
        }
    }
}
