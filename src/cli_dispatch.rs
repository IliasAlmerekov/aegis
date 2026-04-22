use tokio::runtime::Handle;

use aegis::planning::prepare_planner;
use aegis::runtime_gate::is_ci_environment;

use crate::cli_commands;
use crate::install;
use crate::shell_compat::ShellLaunchOptions;
use crate::shell_wrapper::run_shell_wrapper;
use crate::{Cli, Commands};

pub(crate) fn run_cli(cli: Cli, runtime: &tokio::runtime::Runtime, handle: Handle) -> i32 {
    let Cli {
        command,
        output,
        verbosity,
        quiet,
        verbose,
        subcommand,
    } = cli;
    let verbosity = crate::OutputVerbosity::from_cli(verbosity, quiet, verbose);

    match subcommand {
        Some(Commands::Watch) => {
            let in_ci = is_ci_environment();
            let prepared = prepare_planner(verbosity.is_verbose(), handle);
            runtime.block_on(aegis::watch::run(&prepared, in_ci))
        }
        Some(Commands::Audit(args)) => cli_commands::handle_audit_command(args),
        Some(Commands::On) => cli_commands::handle_toggle_on_command(),
        Some(Commands::Off) => cli_commands::handle_toggle_off_command(),
        Some(Commands::Status) => cli_commands::handle_toggle_status_command(),
        Some(Commands::Rollback(args)) => cli_commands::handle_rollback_command(args, runtime),
        Some(Commands::Config(args)) => cli_commands::handle_config_command(args),
        Some(Commands::Hook) => install::run_hook(),
        Some(Commands::InstallHooks(args)) => install::run_install(&args),
        None => {
            if let Some(cmd) = command {
                run_shell_wrapper(
                    &cmd,
                    output,
                    verbosity,
                    handle,
                    &ShellLaunchOptions::default(),
                )
            } else {
                0
            }
        }
    }
}
