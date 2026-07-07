// Integration tests for install/uninstall flows, split from agent_hooks.rs to
// keep both files within the 800-line budget (M5.1 quality gate, Task 1).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tempfile::TempDir;

fn script_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join(name)
}

fn aegis_test_binary() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_aegis")
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("CARGO_BIN_EXE_aegis is not set for agent hook tests"))
}

fn prepare_agent_dirs(home: &Path, claude: bool, codex: bool) {
    if claude {
        fs::create_dir_all(home.join(".claude")).unwrap();
    }

    if codex {
        fs::create_dir_all(home.join(".codex")).unwrap();
    }
}

fn run_script(script_name: &str, home: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut command = Command::new("/bin/sh");
    command.arg(script_path(script_name));
    command.args(args);
    command.env("HOME", home);
    command.env("AEGIS_BIN", aegis_test_binary());
    for key in [
        "AEGIS_CI",
        "CI",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "CIRCLECI",
        "BUILDKITE",
        "TRAVIS",
        "TF_BUILD",
        "JENKINS_URL",
    ] {
        command.env_remove(key);
    }
    command.stdin(std::process::Stdio::piped());
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    let mut child = command.spawn().unwrap();

    if let Some(input) = stdin {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }

    child.wait_with_output().unwrap()
}

fn run_script_with_env(
    script_name: &str,
    home: &Path,
    args: &[&str],
    stdin: Option<&str>,
    envs: &[(&str, &str)],
) -> Output {
    let mut command = Command::new("/bin/sh");
    command.arg(script_path(script_name));
    command.args(args);
    command.env("HOME", home);
    command.env("AEGIS_BIN", aegis_test_binary());
    for key in [
        "AEGIS_CI",
        "CI",
        "GITHUB_ACTIONS",
        "GITLAB_CI",
        "CIRCLECI",
        "BUILDKITE",
        "TRAVIS",
        "TF_BUILD",
        "JENKINS_URL",
    ] {
        command.env_remove(key);
    }
    for (key, value) in envs {
        command.env(key, value);
    }
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command.spawn().unwrap();

    if let Some(input) = stdin {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }

    child.wait_with_output().unwrap()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn json_contains_command(json: &Value, section: &str, command: &str) -> bool {
    json["hooks"][section].as_array().is_some_and(|entries| {
        entries.iter().any(|entry| {
            entry["hooks"]
                .as_array()
                .is_some_and(|hooks| hooks.iter().any(|hook| hook["command"] == command))
        })
    })
}

#[test]
fn uninstall_prunes_claude_and_codex_hook_registrations() {
    let home = TempDir::new().unwrap();
    prepare_agent_dirs(home.path(), true, true);
    let rc_file = home.path().join(".bashrc");
    fs::write(&rc_file, "export FOO=bar\n").unwrap();

    // Seed unrelated user content in Claude settings.json so we can assert it
    // survives uninstall alongside the aegis migration/prune.
    let claude_settings = home.path().join(".claude").join("settings.json");
    fs::write(
        &claude_settings,
        serde_json::json!({
            "theme": "dark",
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            { "type": "command", "command": "echo user-keep" }
                        ]
                    }
                ]
            }
        })
        .to_string(),
    )
    .unwrap();

    let install_output = run_script("agent-setup.sh", home.path(), &["--all"], None);
    assert!(install_output.status.success());

    let codex_hooks = home.path().join(".codex").join("hooks.json");
    let session_hook = home
        .path()
        .join(".codex")
        .join("hooks")
        .join("aegis-session-start.sh");
    let ptu_hook = home
        .path()
        .join(".codex")
        .join("hooks")
        .join("aegis-pre-tool-use.sh");
    let claude_shim = home
        .path()
        .join(".claude")
        .join("hooks")
        .join("aegis-pre-tool-use.sh");

    assert!(claude_settings.exists());
    assert!(codex_hooks.exists());
    assert!(session_hook.exists());
    assert!(ptu_hook.exists());
    assert!(
        claude_shim.exists(),
        "Claude shim must be materialized by install"
    );

    let claude_json = read_json(&claude_settings);
    let claude_shim_command = claude_shim.display().to_string();
    assert!(
        json_contains_command(&claude_json, "PreToolUse", &claude_shim_command),
        "Claude settings.json must register the absolute shim path before uninstall"
    );
    assert!(
        json_contains_command(&claude_json, "PreToolUse", "echo user-keep"),
        "Claude install must preserve unrelated user Bash hooks"
    );
    assert!(
        !json_contains_command(&claude_json, "PreToolUse", "aegis hook"),
        "Claude install must register the absolute shim, not the legacy bare command"
    );

    let rc_file_str = rc_file.display().to_string();
    let fake_bindir = home.path().join("bin");
    fs::create_dir_all(&fake_bindir).unwrap();
    let bindir_str = fake_bindir.display().to_string();
    let uninstall_output = run_script_with_env(
        "uninstall.sh",
        home.path(),
        &[],
        None,
        &[
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("SHELL", "/bin/bash"),
            ("AEGIS_BINDIR", &bindir_str),
        ],
    );
    assert!(
        uninstall_output.status.success(),
        "uninstall must succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&uninstall_output.stdout),
        String::from_utf8_lossy(&uninstall_output.stderr)
    );

    assert!(!session_hook.exists());
    assert!(!ptu_hook.exists());
    assert!(
        !claude_shim.exists(),
        "uninstall must remove the absolute Claude hook shim"
    );

    let claude_json = read_json(&claude_settings);
    assert!(
        !json_contains_command(&claude_json, "PreToolUse", &claude_shim_command),
        "Claude settings.json must not retain the absolute shim registration"
    );
    assert!(
        !json_contains_command(&claude_json, "PreToolUse", "aegis hook"),
        "Claude settings.json must not retain the legacy bare aegis hook registration"
    );
    assert!(
        json_contains_command(&claude_json, "PreToolUse", "echo user-keep"),
        "uninstall must preserve unrelated user Bash hooks"
    );
    assert_eq!(
        claude_json["theme"], "dark",
        "uninstall must preserve unrelated top-level user settings"
    );

    let codex_session_command = session_hook.display().to_string();
    let codex_ptu_command = ptu_hook.display().to_string();
    let codex_json = read_json(&codex_hooks);
    assert!(
        !json_contains_command(&codex_json, "SessionStart", &codex_session_command),
        "Codex hooks.json must not retain the SessionStart registration"
    );
    assert!(
        !json_contains_command(&codex_json, "PreToolUse", &codex_ptu_command),
        "Codex hooks.json must not retain the PreToolUse registration"
    );

    assert_eq!(fs::read_to_string(&rc_file).unwrap(), "export FOO=bar\n");
}

#[test]
fn claude_install_migrates_legacy_aegis_hook_registration_to_absolute_shim() {
    // End-to-end migration seam through the public binary surface: seed a real
    // legacy bare `aegis hook` Bash registration alongside an unrelated user hook,
    // run `aegis install-hooks --claude-code`, and assert the legacy command is
    // migrated to the absolute shim, the shim is materialized on disk, and the
    // user hook survives. The JSON-only `apply_installation` unit tests cover the
    // prune logic with a fake command; this closes the seam between that logic
    // and a real filesystem install.
    let home = TempDir::new().unwrap();
    prepare_agent_dirs(home.path(), true, false);
    let claude_settings = home.path().join(".claude").join("settings.json");
    fs::write(
        &claude_settings,
        serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [{ "type": "command", "command": "aegis hook" }]
                    },
                    {
                        "matcher": "Bash",
                        "hooks": [{ "type": "command", "command": "echo user-keep" }]
                    }
                ]
            }
        })
        .to_string(),
    )
    .unwrap();

    let install_output = run_script("agent-setup.sh", home.path(), &["--claude-code"], None);
    assert!(
        install_output.status.success(),
        "claude install must succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&install_output.stdout),
        String::from_utf8_lossy(&install_output.stderr)
    );

    let claude_shim = home
        .path()
        .join(".claude")
        .join("hooks")
        .join("aegis-pre-tool-use.sh");
    assert!(
        claude_shim.exists(),
        "absolute shim must be materialized on disk"
    );

    let claude_json = read_json(&claude_settings);
    let shim_command = claude_shim.display().to_string();
    assert!(
        json_contains_command(&claude_json, "PreToolUse", &shim_command),
        "claude settings must register the absolute shim path; settings=\n{claude_json}"
    );
    assert!(
        !json_contains_command(&claude_json, "PreToolUse", "aegis hook"),
        "legacy bare `aegis hook` registration must be migrated away; settings=\n{claude_json}"
    );
    assert!(
        json_contains_command(&claude_json, "PreToolUse", "echo user-keep"),
        "unrelated user hook must survive the migration; settings=\n{claude_json}"
    );
}

#[test]
fn agent_setup_wrapper_delegates_to_binary_install_hooks_command() {
    let home = TempDir::new().unwrap();
    let fake_bin_dir = home.path().join("bin");
    let fake_aegis = fake_bin_dir.join("aegis");
    let args_log = home.path().join("agent-setup-args.log");

    fs::create_dir_all(&fake_bin_dir).unwrap();
    fs::write(
        &fake_aegis,
        format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" > '{}'\nprintf 'delegated from wrapper\\n'\n",
            args_log.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&fake_aegis).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake_aegis, permissions).unwrap();
    }

    let fake_aegis_str = fake_aegis.display().to_string();
    let output = run_script_with_env(
        "agent-setup.sh",
        home.path(),
        &["--codex"],
        None,
        &[("AEGIS_BIN", &fake_aegis_str)],
    );

    assert!(
        output.status.success(),
        "wrapper must delegate successfully: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&args_log).unwrap(),
        "install-hooks --codex\n",
        "compatibility wrapper must forward its supported flags to aegis install-hooks"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "delegated from wrapper\n"
    );
    assert!(output.stderr.is_empty());
}
