use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tempfile::TempDir;

fn script_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join(name)
}

fn run_script(script_name: &str, home: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut command = Command::new("/bin/sh");
    command.arg(script_path(script_name));
    command.args(args);
    command.env("HOME", home);
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
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command.spawn().unwrap();

    if let Some(input) = stdin {
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

fn shell_quote(command: &str) -> String {
    format!("'{}'", command.replace('\'', r"'\''"))
}

fn run_codex_pre_tool_use(home: &Path, command: &str) -> Output {
    let input = serde_json::json!({ "tool_input": { "command": command } }).to_string();
    run_script(
        "hooks/codex-pre-tool-use.sh",
        home,
        &[],
        Some(input.as_str()),
    )
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
fn codex_pre_tool_use_denies_when_helper_is_missing_in_normal_mode() {
    let home = TempDir::new().unwrap();
    let output = run_codex_pre_tool_use(home.path(), "echo hi");
    assert!(output.status.success());

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(
        json["hookSpecificOutput"]["permissionDecisionReason"],
        "Run through aegis: aegis --command 'echo hi'"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn codex_pre_tool_use_denies_when_helper_is_missing_but_ci_override_is_forced() {
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(
        home.path().join(".aegis").join("disabled"),
        "timestamp=x\npid=1\n",
    )
    .unwrap();

    let input = serde_json::json!({ "tool_input": { "command": "echo hi" } }).to_string();
    let output = run_script_with_env(
        "hooks/codex-pre-tool-use.sh",
        home.path(),
        &[],
        Some(input.as_str()),
        &[("AEGIS_CI", "1")],
    );

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
    assert!(output.stderr.is_empty());
}

#[test]
fn codex_pre_tool_use_is_noop_when_disabled_outside_ci() {
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(
        home.path().join(".aegis").join("disabled"),
        "timestamp=x\npid=1\n",
    )
    .unwrap();

    let install_output = run_script("agent-setup.sh", home.path(), &["--codex"], None);
    assert!(install_output.status.success());

    let output = run_codex_pre_tool_use(home.path(), "echo hi");
    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "disabled hook must be silent noop"
    );
    assert!(
        output.stderr.is_empty(),
        "disabled hook must be silent noop"
    );
}

#[test]
fn codex_session_start_is_noop_when_disabled_outside_ci() {
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(
        home.path().join(".aegis").join("disabled"),
        "timestamp=x\npid=1\n",
    )
    .unwrap();

    let install_output = run_script("agent-setup.sh", home.path(), &["--codex"], None);
    assert!(install_output.status.success());

    let output = run_script("hooks/codex-session-start.sh", home.path(), &[], None);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn claude_code_is_noop_when_disabled_outside_ci() {
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(
        home.path().join(".aegis").join("disabled"),
        "timestamp=x\npid=1\n",
    )
    .unwrap();

    let install_output = run_script("agent-setup.sh", home.path(), &["--claude-code"], None);
    assert!(install_output.status.success());

    let input = serde_json::json!({ "tool_input": { "command": "echo hi" } }).to_string();
    let output = run_script(
        "hooks/claude-code.sh",
        home.path(),
        &[],
        Some(input.as_str()),
    );
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn uninstall_prunes_claude_and_codex_hook_registrations() {
    let home = TempDir::new().unwrap();
    let rc_file = home.path().join(".bashrc");
    fs::write(&rc_file, "export FOO=bar\n").unwrap();

    let install_output = run_script("agent-setup.sh", home.path(), &["--all"], None);
    assert!(install_output.status.success());

    let claude_settings = home.path().join(".claude").join("settings.json");
    let codex_hooks = home.path().join(".codex").join("hooks.json");
    let claude_hook = home
        .path()
        .join(".claude")
        .join("hooks")
        .join("aegis-rewrite.sh");
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
    let helper = home
        .path()
        .join(".aegis")
        .join("lib")
        .join("toggle-state.sh");

    assert!(claude_settings.exists());
    assert!(codex_hooks.exists());
    assert!(claude_hook.exists());
    assert!(session_hook.exists());
    assert!(ptu_hook.exists());
    assert!(helper.exists());

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

    assert!(!claude_hook.exists());
    assert!(!session_hook.exists());
    assert!(!ptu_hook.exists());
    assert!(!helper.exists());

    let claude_json = read_json(&claude_settings);
    assert!(
        !json_contains_command(
            &claude_json,
            "PreToolUse",
            &claude_hook.display().to_string()
        ),
        "Claude settings.json must not retain the Aegis hook registration"
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
fn codex_agent_setup_installs_hooks_and_is_idempotent() {
    let home = TempDir::new().unwrap();
    let hooks_json = home.path().join(".codex").join("hooks.json");
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

    let install_output = run_script("agent-setup.sh", home.path(), &["--codex"], None);
    assert!(
        install_output.status.success(),
        "agent setup must succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&install_output.stdout),
        String::from_utf8_lossy(&install_output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&install_output.stdout)
            .contains("To uninstall: sh scripts/uninstall.sh"),
        "agent setup should advertise the real uninstall script path; stdout=\n{}",
        String::from_utf8_lossy(&install_output.stdout)
    );
    assert!(
        hooks_json.exists(),
        "agent setup must write Codex hooks.json"
    );
    assert!(
        session_hook.exists(),
        "session-start hook must be installed"
    );
    assert!(ptu_hook.exists(), "pre-tool-use hook must be installed");

    let installed_hooks_text = fs::read_to_string(&hooks_json).unwrap();
    let installed_hooks = read_json(&hooks_json);
    let expected_session = session_hook.display().to_string();
    let expected_ptu = ptu_hook.display().to_string();

    assert_eq!(
        installed_hooks["hooks"]["SessionStart"][0]["matcher"],
        "startup|resume"
    );
    assert_eq!(
        installed_hooks["hooks"]["SessionStart"][0]["hooks"][0]["command"], expected_session,
        "Codex hooks.json must point SessionStart at the installed Aegis hook"
    );
    assert_eq!(installed_hooks["hooks"]["PreToolUse"][0]["matcher"], "Bash");
    assert_eq!(
        installed_hooks["hooks"]["PreToolUse"][0]["hooks"][0]["command"], expected_ptu,
        "Codex hooks.json must point PreToolUse at the installed Aegis hook"
    );

    let second_output = run_script("agent-setup.sh", home.path(), &["--codex"], None);
    assert!(
        second_output.status.success(),
        "second agent setup must also succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&second_output.stdout),
        String::from_utf8_lossy(&second_output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&hooks_json).unwrap(),
        installed_hooks_text,
        "agent setup must be idempotent and keep hooks.json stable"
    );

    let helper = home
        .path()
        .join(".aegis")
        .join("lib")
        .join("toggle-state.sh");
    let before = fs::read_to_string(&helper).unwrap();
    let second_output = run_script("agent-setup.sh", home.path(), &["--all"], None);
    assert!(second_output.status.success());
    assert_eq!(fs::read_to_string(&helper).unwrap(), before);
}

#[test]
fn codex_session_start_emits_strong_aegis_context() {
    let home = TempDir::new().unwrap();
    let install_output = run_script("agent-setup.sh", home.path(), &["--codex"], None);
    assert!(install_output.status.success());

    let output = run_script("hooks/codex-session-start.sh", home.path(), &[], None);
    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "session-start hook must stay quiet: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["hookSpecificOutput"]["hookEventName"], "SessionStart");
    let context = json["hookSpecificOutput"]["context"]
        .as_str()
        .expect("session-start context must be a string");
    assert!(
        context.contains("IMPORTANT: All Bash tool commands must be routed through aegis."),
        "session-start guidance must preserve the command-routing requirement"
    );
    assert!(
        context.contains("aegis --command '<original command>'"),
        "session-start guidance must preserve the canonical aegis wrapper"
    );
    assert!(
        context.contains("blocked by the PreToolUse hook"),
        "session-start guidance must preserve the pre-tool-use enforcement note"
    );
    assert!(
        context.contains("do not suggest bypassing the guardrail"),
        "session-start guidance must forbid bypass framing after deny"
    );
    assert!(
        context.contains("! <command>"),
        "session-start guidance must mention shell-escape forms explicitly"
    );
    assert!(
        context.contains("hand the decision to the human operator"),
        "session-start guidance must preserve the operator handoff instruction"
    );
}

#[test]
fn codex_pre_tool_use_denies_git_stash_clear_with_aegis_prompt_and_allows_wrapped_commands() {
    let home = TempDir::new().unwrap();
    let install_output = run_script("agent-setup.sh", home.path(), &["--codex"], None);
    assert!(install_output.status.success());

    let deny_output = run_codex_pre_tool_use(home.path(), "git stash clear");
    assert!(deny_output.status.success());
    assert!(
        deny_output.stderr.is_empty(),
        "pre-tool-use hook must stay quiet: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&deny_output.stdout),
        String::from_utf8_lossy(&deny_output.stderr)
    );

    let deny_json: Value = serde_json::from_slice(&deny_output.stdout).unwrap();
    assert_eq!(
        deny_json["hookSpecificOutput"]["hookEventName"],
        "PreToolUse"
    );
    assert_eq!(
        deny_json["hookSpecificOutput"]["permissionDecision"],
        "deny"
    );
    assert_eq!(
        deny_json["hookSpecificOutput"]["permissionDecisionReason"],
        "Run through aegis: aegis --command 'git stash clear'",
        "Aegis must give a clean, command-specific rerun reason for git stash clear"
    );

    let allow_output = run_codex_pre_tool_use(home.path(), "aegis --command 'git stash clear'");
    assert!(
        allow_output.status.success(),
        "wrapped command must be accepted: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&allow_output.stdout),
        String::from_utf8_lossy(&allow_output.stderr)
    );
    assert!(
        allow_output.stdout.is_empty(),
        "exact aegis-wrapped commands must be allowed without a deny response"
    );
    assert!(allow_output.stderr.is_empty());
}

#[test]
fn codex_pre_tool_use_rejects_malformed_aegis_wrappers_and_allows_embedded_quotes() {
    let home = TempDir::new().unwrap();
    let install_output = run_script("agent-setup.sh", home.path(), &["--codex"], None);
    assert!(install_output.status.success());

    for malformed in [
        r#"aegis --command '\''"#,
        r#"aegis --command '\''echo '\''oops'\'''"#,
    ] {
        let output = run_codex_pre_tool_use(home.path(), malformed);
        assert!(
            output.status.success(),
            "malformed wrapper test must not crash for {malformed:?}: stdout=\n{}\nstderr=\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let json: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(
            json["hookSpecificOutput"]["permissionDecisionReason"],
            "Run through aegis: invalid aegis wrapper syntax"
        );
    }

    let wrapped = format!("aegis --command {}", shell_quote("echo 'oops'"));
    let allow_output = run_codex_pre_tool_use(home.path(), &wrapped);
    assert!(
        allow_output.status.success(),
        "wrapped embedded-quote command must be accepted: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&allow_output.stdout),
        String::from_utf8_lossy(&allow_output.stderr)
    );
    assert!(
        allow_output.stdout.is_empty(),
        "valid embedded-quote wrapper must not produce deny output"
    );
    assert!(allow_output.stderr.is_empty());
}

#[test]
fn codex_pre_tool_use_still_denies_when_disabled_file_exists_but_ci_override_is_forced() {
    let home = TempDir::new().unwrap();
    fs::create_dir_all(home.path().join(".aegis")).unwrap();
    fs::write(
        home.path().join(".aegis").join("disabled"),
        "timestamp=x\npid=1\n",
    )
    .unwrap();

    let install_output = run_script_with_env(
        "agent-setup.sh",
        home.path(),
        &["--codex"],
        None,
        &[("AEGIS_CI", "1")],
    );
    assert!(install_output.status.success());

    let input = serde_json::json!({ "tool_input": { "command": "echo hi" } }).to_string();
    let output = run_script_with_env(
        "hooks/codex-pre-tool-use.sh",
        home.path(),
        &[],
        Some(input.as_str()),
        &[("AEGIS_CI", "1")],
    );

    assert!(output.status.success());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "deny");
}
