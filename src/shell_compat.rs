use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clap::Parser;

use crate::{Cli, EXIT_INTERNAL};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ShellLaunchOptions {
    pub(crate) login: bool,
    pub(crate) interactive: bool,
    pub(crate) positional_args: Vec<OsString>,
}

impl ShellLaunchOptions {
    pub(crate) fn command_flag(&self, shell: &Path) -> String {
        let mut flag = String::from("-");
        if self.login && shell_supports_login_flag(shell) {
            flag.push('l');
        }
        if self.interactive {
            flag.push('i');
        }
        flag.push('c');
        flag
    }

    pub(crate) fn session_flags(&self, shell: &Path) -> Vec<&'static str> {
        let mut flags = Vec::new();
        if self.login && shell_supports_login_flag(shell) {
            flags.push("-l");
        }
        if self.interactive {
            flags.push("-i");
        }
        flags
    }
}

pub(crate) enum InvocationMode {
    Cli(Cli),
    ShellCompatCommand {
        command: String,
        launch: ShellLaunchOptions,
    },
    ShellCompatSession {
        launch: ShellLaunchOptions,
    },
}

pub(crate) fn parse_invocation_mode() -> Result<InvocationMode, String> {
    let argv: Vec<OsString> = env::args_os().collect();

    if let Some(invocation) = parse_shell_compat_invocation(&argv[1..])? {
        return Ok(invocation);
    }

    match Cli::try_parse_from(argv) {
        Ok(cli) => Ok(InvocationMode::Cli(cli)),
        Err(err) => err.exit(),
    }
}

pub(crate) fn parse_shell_compat_invocation(
    args: &[OsString],
) -> Result<Option<InvocationMode>, String> {
    if args.is_empty() || !starts_with_shell_compat_flags(args[0].as_os_str()) {
        return Ok(None);
    }

    let mut launch = ShellLaunchOptions::default();
    let mut index = 0;

    while index < args.len() {
        let Some(arg) = args[index].to_str() else {
            return Ok(None);
        };

        match arg {
            "--login" => {
                launch.login = true;
                index += 1;
            }
            "-l" => {
                launch.login = true;
                index += 1;
            }
            "-i" => {
                launch.interactive = true;
                index += 1;
            }
            "-c" => {
                let command = parse_shell_compat_command(args, index + 1)?;
                launch.positional_args = args[index + 2..].to_vec();
                return Ok(Some(InvocationMode::ShellCompatCommand { command, launch }));
            }
            _ if arg.starts_with('-') && !arg.starts_with("--") => {
                let Some(bundle) = arg.strip_prefix('-') else {
                    return Ok(None);
                };

                let mut command_flag = false;
                for flag in bundle.chars() {
                    match flag {
                        'l' => launch.login = true,
                        'i' => launch.interactive = true,
                        'c' => {
                            command_flag = true;
                        }
                        _ => return Ok(None),
                    }
                }

                if command_flag {
                    let command = parse_shell_compat_command(args, index + 1)?;
                    launch.positional_args = args[index + 2..].to_vec();
                    return Ok(Some(InvocationMode::ShellCompatCommand { command, launch }));
                }

                index += 1;
            }
            _ => {
                launch.positional_args = args[index..].to_vec();
                return Ok(Some(InvocationMode::ShellCompatSession { launch }));
            }
        }
    }

    Ok(Some(InvocationMode::ShellCompatSession { launch }))
}

pub(crate) fn starts_with_shell_compat_flags(arg: &OsStr) -> bool {
    let Some(text) = arg.to_str() else {
        return false;
    };

    if matches!(text, "--login" | "-l" | "-i") {
        return true;
    }

    let Some(bundle) = text.strip_prefix('-') else {
        return false;
    };

    !bundle.is_empty()
        && !bundle.starts_with('-')
        && bundle.len() > 1
        && bundle.chars().all(|flag| matches!(flag, 'l' | 'i' | 'c'))
        && bundle.contains('c')
}

pub(crate) fn parse_shell_compat_command(
    args: &[OsString],
    index: usize,
) -> Result<String, String> {
    let Some(command) = args.get(index) else {
        return Err("shell compatibility mode requires a command after -c".to_string());
    };

    command
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "shell compatibility mode only supports UTF-8 command strings".to_string())
}

pub(crate) fn exec_command(cmd: &str, launch: &ShellLaunchOptions) -> i32 {
    let shell = resolve_shell();

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        let mut command = Command::new(&shell);
        command
            .arg(launch.command_flag(&shell))
            .arg(cmd)
            .args(&launch.positional_args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let err = command.exec();

        eprintln!("error: failed to exec shell {}: {err}", shell.display());
        EXIT_INTERNAL
    }

    #[cfg(not(unix))]
    {
        let mut command = Command::new(&shell);
        command
            .arg(launch.command_flag(&shell))
            .arg(cmd)
            .args(&launch.positional_args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        match command.status() {
            Ok(status) => status.code().unwrap_or(EXIT_INTERNAL),
            Err(err) => {
                eprintln!("error: failed to spawn shell {}: {err}", shell.display());
                EXIT_INTERNAL
            }
        }
    }
}

pub(crate) fn exec_shell_session(launch: &ShellLaunchOptions) -> i32 {
    let shell = resolve_shell();

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        let mut command = Command::new(&shell);
        command
            .args(launch.session_flags(&shell))
            .args(&launch.positional_args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let err = command.exec();

        eprintln!("error: failed to exec shell {}: {err}", shell.display());
        EXIT_INTERNAL
    }

    #[cfg(not(unix))]
    {
        let mut command = Command::new(&shell);
        command
            .args(launch.session_flags(&shell))
            .args(&launch.positional_args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        match command.status() {
            Ok(status) => status.code().unwrap_or(EXIT_INTERNAL),
            Err(err) => {
                eprintln!("error: failed to spawn shell {}: {err}", shell.display());
                EXIT_INTERNAL
            }
        }
    }
}

pub(crate) fn shell_supports_login_flag(shell: &Path) -> bool {
    shell
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| !matches!(name, "sh" | "dash"))
        .unwrap_or(true)
}

pub(crate) fn resolve_shell() -> PathBuf {
    let aegis_real_shell = env::var_os("AEGIS_REAL_SHELL");
    let shell_env = env::var_os("SHELL");
    let current_exe = env::current_exe().ok();
    resolve_shell_inner(
        aegis_real_shell.as_deref(),
        shell_env.as_deref(),
        current_exe.as_deref(),
    )
}

pub(crate) fn resolve_shell_inner(
    aegis_real_shell: Option<&OsStr>,
    shell_env: Option<&OsStr>,
    current_exe: Option<&Path>,
) -> PathBuf {
    if let Some(shell) = aegis_real_shell.filter(|s| !s.is_empty()) {
        return PathBuf::from(shell);
    }

    if let Some(shell) = shell_env.filter(|s| !s.is_empty()) {
        let shell_path = PathBuf::from(shell);
        if !same_file(&shell_path, current_exe) {
            return shell_path;
        }
    }

    PathBuf::from("/bin/sh")
}

pub(crate) fn same_file(path: &Path, other: Option<&Path>) -> bool {
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
