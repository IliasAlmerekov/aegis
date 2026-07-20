use tokio::runtime::Handle;

use aegis::planning::prepare_planner;
use aegis::runtime_gate::is_ci_environment;

use crate::cli_commands;
use crate::install;
use crate::shell_compat::ShellLaunchOptions;
use crate::shell_wrapper::run_shell_wrapper;
use crate::{Cli, Commands};

/// Run the undocumented internal language-worker mode (ADR-022 §2).
///
/// The process delegates immediately to [`aegis_language::worker::run`] over
/// stdin/stdout: it reads length-bounded request frames, parses the supplied
/// source bytes with the pinned Tree-sitter grammars, and writes one response
/// frame per request. There is no Tokio runtime, no filesystem access, no
/// subprocess, and no shell handling here — the worker is a minimal,
/// synchronous, parse-only process. Business logic stays in `aegis-language`;
/// this function only wires stdio and maps the worker's stop reason to an exit
/// code.
///
/// A clean session end ([`aegis_language::RunOutcome::EndOfInput`] or
/// [`aegis_language::RunOutcome::MaxRequestsReached`]) exits 0. Any worker
/// failure (malformed frame, truncated trailing frame, read/write error) exits
/// non-zero so the parent client can detect abnormal termination; the parent
/// also detects this via EOF and timeout, so the exit code is a hint, not the
/// primary signal.
pub(crate) fn run_internal_language_worker() -> i32 {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let outcome = aegis_language::worker::run(stdin.lock(), stdout.lock());
    match outcome {
        aegis_language::RunOutcome::EndOfInput | aegis_language::RunOutcome::MaxRequestsReached => {
            0
        }
        _ => crate::EXIT_INTERNAL,
    }
}

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
        Some(Commands::Snapshot(args)) => cli_commands::handle_snapshot_command(args, runtime),
        Some(Commands::Config(args)) => cli_commands::handle_config_command(args),
        Some(Commands::Hook) => install::run_hook(),
        Some(Commands::InstallHooks(args)) => install::run_install(&args),
        Some(Commands::SetupShell(args)) => install::run_setup_shell(&args),
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
