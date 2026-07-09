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

fn run_claude_code_hook(home: &Path, command: &str) -> Output {
    let input = serde_json::json!({ "tool_input": { "command": command } }).to_string();
    run_script("hooks/claude-code.sh", home, &[], Some(input.as_str()))
}

#[test]
fn codex_pre_tool_use_rewrites_when_helper_is_missing_in_normal_mode() {
    let home = TempDir::new().unwrap();
    let output = run_codex_pre_tool_use(home.path(), "echo hi");
    assert!(output.status.success());

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "allow");
    assert_eq!(
        json["hookSpecificOutput"]["updatedInput"]["command"],
        "aegis --command 'echo hi'"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn codex_pre_tool_use_rewrites_when_helper_is_missing_but_ci_override_is_forced() {
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
    assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "allow");
    assert_eq!(
        json["hookSpecificOutput"]["updatedInput"]["command"],
        "aegis --command 'echo hi'"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn codex_pre_tool_use_is_noop_when_disabled_outside_ci() {
    let home = TempDir::new().unwrap();
    prepare_agent_dirs(home.path(), false, true);
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
    prepare_agent_dirs(home.path(), false, true);
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
    prepare_agent_dirs(home.path(), true, false);
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
fn claude_code_hook_rewrites_unwrapped_bash_command() {
    // The Claude PreToolUse shim must delegate to `aegis hook` (via AEGIS_BIN)
    // and emit the transparent allow+updatedInput rewrite, identical to Codex.
    // The legacy jq-based script checked `command -v aegis` on PATH and emitted
    // a stderr warning instead of JSON when aegis was not on PATH, so this test
    // is red against the legacy script and green against the jq-free shim.
    let home = TempDir::new().unwrap();
    let output = run_claude_code_hook(home.path(), "git status");
    assert!(
        output.status.success(),
        "claude hook must succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "allow");
    assert_eq!(
        json["hookSpecificOutput"]["updatedInput"]["command"],
        "aegis --command 'git status'"
    );
    assert!(
        output.stderr.is_empty(),
        "claude hook must stay quiet: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn codex_agent_setup_installs_hooks_and_is_idempotent() {
    let home = TempDir::new().unwrap();
    prepare_agent_dirs(home.path(), false, true);
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

    let before = fs::read_to_string(&session_hook).unwrap();
    let second_output = run_script("agent-setup.sh", home.path(), &["--all"], None);
    assert!(second_output.status.success());
    assert_eq!(fs::read_to_string(&session_hook).unwrap(), before);
}

#[test]
fn codex_session_start_emits_additional_context() {
    let home = TempDir::new().unwrap();
    prepare_agent_dirs(home.path(), false, true);
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
    // Codex expects SessionStart context under `additionalContext`; the legacy
    // `context` field was the invalid-output root cause and must be gone.
    assert!(
        json["hookSpecificOutput"].get("context").is_none(),
        "legacy `context` field must be absent"
    );
    let context = json["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("session-start additionalContext must be a string");
    assert!(
        context.contains("IMPORTANT: All Bash tool commands must be routed through aegis."),
        "session-start guidance must preserve the command-routing requirement"
    );
    assert!(
        context.contains("aegis --command '<original command>'"),
        "session-start guidance must preserve the canonical aegis wrapper"
    );
    assert!(
        context.contains("transparently rewrites"),
        "session-start guidance must describe transparent PreToolUse rewriting"
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
fn codex_pre_tool_use_rewrites_git_stash_clear_and_passes_through_wrapped_commands() {
    let home = TempDir::new().unwrap();
    prepare_agent_dirs(home.path(), false, true);
    let install_output = run_script("agent-setup.sh", home.path(), &["--codex"], None);
    assert!(install_output.status.success());

    let rewrite_output = run_codex_pre_tool_use(home.path(), "git stash clear");
    assert!(rewrite_output.status.success());
    assert!(
        rewrite_output.stderr.is_empty(),
        "pre-tool-use hook must stay quiet: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&rewrite_output.stdout),
        String::from_utf8_lossy(&rewrite_output.stderr)
    );

    let rewrite_json: Value = serde_json::from_slice(&rewrite_output.stdout).unwrap();
    assert_eq!(
        rewrite_json["hookSpecificOutput"]["hookEventName"],
        "PreToolUse"
    );
    assert_eq!(
        rewrite_json["hookSpecificOutput"]["permissionDecision"],
        "allow"
    );
    assert_eq!(
        rewrite_json["hookSpecificOutput"]["updatedInput"]["command"],
        "aegis --command 'git stash clear'",
        "Aegis must transparently rewrite the command through the wrapper"
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
        "canonical aegis-wrapped commands must pass through without a rewrite response"
    );
    assert!(allow_output.stderr.is_empty());
}

#[test]
fn codex_pre_tool_use_rejects_malformed_aegis_wrappers_and_allows_embedded_quotes() {
    let home = TempDir::new().unwrap();
    prepare_agent_dirs(home.path(), false, true);
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
        assert!(
            json["hookSpecificOutput"]["permissionDecisionReason"]
                .as_str()
                .unwrap()
                .contains("invalid aegis wrapper syntax"),
            "malformed wrapper must be denied with a clear reason"
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
        "valid embedded-quote wrapper must pass through without a rewrite response"
    );
    assert!(allow_output.stderr.is_empty());
}

#[test]
fn codex_pre_tool_use_still_rewrites_when_disabled_file_exists_but_ci_override_is_forced() {
    let home = TempDir::new().unwrap();
    prepare_agent_dirs(home.path(), false, true);
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
    assert_eq!(json["hookSpecificOutput"]["permissionDecision"], "allow");
    assert_eq!(
        json["hookSpecificOutput"]["updatedInput"]["command"],
        "aegis --command 'echo hi'"
    );
}

// H4 regression: both hooks must fail CLOSED (deny JSON, exit 0) when AEGIS_BIN
// points at a non-existent path.  Pre-fix behavior: exec failed → exit 127, empty
// stdout → the agent treated this as a non-blocking error and the original
// unwrapped command ran unscanned (fail-open).

#[test]
fn test_claude_code_hook_fails_closed_when_aegis_bin_missing() {
    let home = TempDir::new().unwrap();
    // Enforcement must be active: no disabled file, no CI vars (run_script_with_env
    // already strips the CI vars before applying our overrides).
    let missing_bin = home.path().join("no-such-aegis").display().to_string();
    let stdin_json =
        serde_json::json!({ "tool_input": { "command": "rm -rf /tmp/x" } }).to_string();

    let output = run_script_with_env(
        "hooks/claude-code.sh",
        home.path(),
        &[],
        Some(stdin_json.as_str()),
        &[("AEGIS_BIN", missing_bin.as_str())],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "hook must exit 0 (not 127) when AEGIS_BIN is missing; stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect(
        "hook must emit valid JSON on stdout when AEGIS_BIN is missing; stdout was empty or unparseable",
    );

    assert_eq!(
        json["hookSpecificOutput"]["permissionDecision"], "deny",
        "hook must deny (fail closed) when AEGIS_BIN is missing; json=\n{json}"
    );

    let reason = json["reason"]
        .as_str()
        .expect("top-level `reason` field must be a non-empty string");
    assert!(
        !reason.is_empty(),
        "top-level `reason` must be non-empty; json=\n{json}"
    );

    assert_ne!(
        json["hookSpecificOutput"]["permissionDecision"], "allow",
        "hook must NOT allow when AEGIS_BIN is missing"
    );

    assert!(
        json["hookSpecificOutput"].get("updatedInput").is_none(),
        "hook must NOT emit updatedInput when failing closed; json=\n{json}"
    );
}

#[test]
fn test_codex_pre_tool_use_hook_fails_closed_when_aegis_bin_missing() {
    let home = TempDir::new().unwrap();
    // Enforcement must be active: no disabled file, no CI vars.
    let missing_bin = home.path().join("no-such-aegis").display().to_string();
    let stdin_json =
        serde_json::json!({ "tool_input": { "command": "rm -rf /tmp/x" } }).to_string();

    let output = run_script_with_env(
        "hooks/codex-pre-tool-use.sh",
        home.path(),
        &[],
        Some(stdin_json.as_str()),
        &[("AEGIS_BIN", missing_bin.as_str())],
    );

    assert_eq!(
        output.status.code(),
        Some(0),
        "hook must exit 0 (not 127) when AEGIS_BIN is missing; stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect(
        "hook must emit valid JSON on stdout when AEGIS_BIN is missing; stdout was empty or unparseable",
    );

    assert_eq!(
        json["hookSpecificOutput"]["permissionDecision"], "deny",
        "hook must deny (fail closed) when AEGIS_BIN is missing; json=\n{json}"
    );

    let reason = json["reason"]
        .as_str()
        .expect("top-level `reason` field must be a non-empty string");
    assert!(
        !reason.is_empty(),
        "top-level `reason` must be non-empty; json=\n{json}"
    );

    assert_ne!(
        json["hookSpecificOutput"]["permissionDecision"], "allow",
        "hook must NOT allow when AEGIS_BIN is missing"
    );

    assert!(
        json["hookSpecificOutput"].get("updatedInput").is_none(),
        "hook must NOT emit updatedInput when failing closed; json=\n{json}"
    );
}
