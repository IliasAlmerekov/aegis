// Clap parse tests for `aegis setup-shell`. Kept in a dedicated module (rather
// than `main_tests.rs`) so the main test file stays under the 800-line budget.
// `Cli`, `Commands`, and `SetupShellArgs` are crate-root items, so a same-crate
// child module can reach them via `crate::`.

use clap::Parser;

use crate::{Cli, Commands};

#[test]
fn parse_setup_shell_command_defaults_to_install_mode() {
    let cli = Cli::try_parse_from(["aegis", "setup-shell"]).unwrap();

    match cli.subcommand {
        Some(Commands::SetupShell(args)) => {
            assert!(!args.remove);
            assert_eq!(args.shell, None);
            assert_eq!(args.rc_file, None);
            assert_eq!(args.aegis_bin, None);
        }
        _ => panic!("expected setup-shell subcommand"),
    }
}

#[test]
fn parse_setup_shell_remove_command() {
    let cli = Cli::try_parse_from(["aegis", "setup-shell", "--remove"]).unwrap();

    match cli.subcommand {
        Some(Commands::SetupShell(args)) => assert!(args.remove),
        _ => panic!("expected setup-shell subcommand"),
    }
}

#[test]
fn parse_setup_shell_overrides() {
    let cli = Cli::try_parse_from([
        "aegis",
        "setup-shell",
        "--shell",
        "/bin/zsh",
        "--rc-file",
        "/tmp/aegis-test-zshrc",
        "--aegis-bin",
        "/usr/local/bin/aegis",
    ])
    .unwrap();

    match cli.subcommand {
        Some(Commands::SetupShell(args)) => {
            assert_eq!(
                args.shell.as_deref(),
                Some(std::path::Path::new("/bin/zsh"))
            );
            assert_eq!(
                args.rc_file.as_deref(),
                Some(std::path::Path::new("/tmp/aegis-test-zshrc"))
            );
            assert_eq!(
                args.aegis_bin.as_deref(),
                Some(std::path::Path::new("/usr/local/bin/aegis"))
            );
        }
        _ => panic!("expected setup-shell subcommand"),
    }
}
