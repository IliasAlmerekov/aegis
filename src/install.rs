use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

const CODEX_PRE_TOOL_USE_HOOK_SH: &str = include_str!("../scripts/hooks/codex-pre-tool-use.sh");
const CODEX_SESSION_START_HOOK_SH: &str = include_str!("../scripts/hooks/codex-session-start.sh");

/// Run the Claude Code `PreToolUse` hook and rewrite unwrapped Bash commands
/// through `aegis --command`.
pub(crate) fn run_hook() -> i32 {
    match hook_response_from_stdin() {
        HookOutcome::Allow(output) | HookOutcome::Deny(output) => {
            println!("{output}");
        }
        HookOutcome::Noop => {}
    }

    0
}

/// Install aegis hooks for the selected agent targets.
pub(crate) fn run_install(args: &super::InstallArgs) -> i32 {
    let mut exit = 0;
    let selection = install_target_selection(args);

    if selection.includes_claude() {
        match run_claude_install(!args.local) {
            AgentInstallResult::Installed => println!("Claude Code: hook installed"),
            AgentInstallResult::AlreadyPresent => {
                println!("Claude Code: hook already present, skipping")
            }
            AgentInstallResult::Skipped => {
                println!("Claude Code: skipped (agent directory not present)")
            }
            AgentInstallResult::Error(err) => {
                eprintln!("error: failed to install Claude Code hook: {err}");
                exit = super::EXIT_INTERNAL;
            }
        }
    }

    if selection.includes_codex() {
        match run_codex_install() {
            AgentInstallResult::Installed => println!("Codex: hooks installed"),
            AgentInstallResult::AlreadyPresent => {
                println!("Codex: hooks already present, skipping")
            }
            AgentInstallResult::Skipped => println!("Codex: skipped (agent directory not present)"),
            AgentInstallResult::Error(err) => {
                eprintln!("error: failed to install Codex hooks: {err}");
                exit = super::EXIT_INTERNAL;
            }
        }
    }

    exit
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InstallTargetSelection {
    All,
    ClaudeCode,
    Codex,
}

impl InstallTargetSelection {
    fn includes_claude(self) -> bool {
        matches!(self, Self::All | Self::ClaudeCode)
    }

    fn includes_codex(self) -> bool {
        matches!(self, Self::All | Self::Codex)
    }
}

fn install_target_selection(args: &super::InstallArgs) -> InstallTargetSelection {
    // Legacy `aegis install` behavior installs both agents, so no explicit
    // target flag falls back to `All`.
    if args.all || (!args.claude_code && !args.codex) {
        InstallTargetSelection::All
    } else if args.claude_code {
        InstallTargetSelection::ClaudeCode
    } else {
        InstallTargetSelection::Codex
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InstallOutcome {
    Installed,
    AlreadyPresent,
    /// Agent directory not present — nothing to install.
    Skipped,
}

#[derive(Debug, Eq, PartialEq)]
enum AgentInstallResult {
    Installed,
    AlreadyPresent,
    Skipped,
    Error(String),
}

impl AgentInstallResult {
    fn from_result(result: Result<InstallOutcome, String>) -> Self {
        match result {
            Ok(InstallOutcome::Installed) => Self::Installed,
            Ok(InstallOutcome::AlreadyPresent) => Self::AlreadyPresent,
            Ok(InstallOutcome::Skipped) => Self::Skipped,
            Err(err) => Self::Error(err),
        }
    }
}

#[derive(Debug)]
enum HookOutcome {
    Allow(Value),
    Deny(Value),
    Noop,
}

fn hook_response_from_stdin() -> HookOutcome {
    let mut input = String::new();
    if let Err(err) = std::io::stdin().read_to_string(&mut input) {
        return HookOutcome::Deny(hook_deny_output(format!(
            "aegis could not read hook input: {err}"
        )));
    }

    hook_response_value(&input)
}

fn hook_response_value(input: &str) -> HookOutcome {
    let input: Value = match serde_json::from_str(input) {
        Ok(value) => value,
        Err(err) => {
            return HookOutcome::Deny(hook_deny_output(format!("invalid hook input: {err}")));
        }
    };

    let Some(root) = input.as_object() else {
        return HookOutcome::Deny(hook_deny_output(
            "invalid hook input: expected a JSON object".to_string(),
        ));
    };

    let Some(tool_input) = root.get("tool_input") else {
        return HookOutcome::Deny(hook_deny_output(
            "invalid hook input: missing tool_input".to_string(),
        ));
    };

    let Some(tool_input) = tool_input.as_object() else {
        return HookOutcome::Deny(hook_deny_output(
            "invalid hook input: tool_input must be a JSON object".to_string(),
        ));
    };

    let Some(command_value) = tool_input.get("command") else {
        return HookOutcome::Noop;
    };

    let Some(command) = command_value.as_str() else {
        return HookOutcome::Deny(hook_deny_output(
            "invalid hook input: tool_input.command must be a string".to_string(),
        ));
    };

    if is_already_wrapped(command) {
        return HookOutcome::Noop;
    }

    let mut updated_input = tool_input.clone();
    updated_input.insert(
        "command".to_string(),
        Value::String(format!("aegis --command {}", shell_quote(command))),
    );

    HookOutcome::Allow(serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "permissionDecisionReason": "aegis intercept",
            "updatedInput": updated_input,
        }
    }))
}

fn hook_deny_output(reason: String) -> Value {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    })
}

fn is_already_wrapped(command: &str) -> bool {
    command
        .strip_prefix("aegis")
        .is_some_and(|rest| rest.is_empty() || rest.chars().next().is_some_and(char::is_whitespace))
}

fn shell_quote(command: &str) -> String {
    format!("'{}'", command.replace('\'', "'\\''"))
}

fn run_claude_install(global: bool) -> AgentInstallResult {
    AgentInstallResult::from_result(run_install_inner(global))
}

fn run_install_inner(global: bool) -> Result<InstallOutcome, String> {
    if global {
        let home = home_dir();
        return run_global_claude_install_at_home(home.as_deref());
    }

    let cwd =
        env::current_dir().map_err(|err| format!("failed to resolve current directory: {err}"))?;
    let settings_path = settings_path_local(&cwd);
    run_install_at_path(&settings_path)
}

fn run_global_claude_install_at_home(home_dir: Option<&Path>) -> Result<InstallOutcome, String> {
    let settings_path = settings_path_global(home_dir)?;
    let claude_dir = settings_path.parent().ok_or_else(|| {
        format!(
            "{} does not have a parent directory",
            settings_path.display()
        )
    })?;

    if !agent_dir_exists(claude_dir)? {
        return Ok(InstallOutcome::Skipped);
    }

    run_install_at_path(&settings_path)
}

fn run_install_at_path(settings_path: &Path) -> Result<InstallOutcome, String> {
    let mut settings = load_settings(settings_path)?;
    let outcome = apply_installation(&mut settings)?;
    if matches!(outcome, InstallOutcome::Installed) {
        write_settings_atomically(settings_path, &settings)?;
    }

    Ok(outcome)
}

// ── Codex installation ────────────────────────────────────────────────────────

fn run_codex_install() -> AgentInstallResult {
    AgentInstallResult::from_result(run_codex_install_inner())
}

fn run_codex_install_inner() -> Result<InstallOutcome, String> {
    let home = home_dir().ok_or_else(|| "HOME is not set".to_string())?;
    run_codex_install_at_dir(&home.join(".codex"))
}

fn run_codex_install_at_dir(codex_dir: &Path) -> Result<InstallOutcome, String> {
    if !agent_dir_exists(codex_dir)? {
        return Ok(InstallOutcome::Skipped);
    }

    let hooks_outcome = materialize_codex_hooks(codex_dir)?;
    let hooks_dir = codex_dir.join("hooks");
    let hooks_json_outcome = apply_codex_hooks_json(
        &codex_dir.join("hooks.json"),
        &hooks_dir.join("aegis-pre-tool-use.sh"),
        &hooks_dir.join("aegis-session-start.sh"),
    )?;

    Ok(combine_outcomes(hooks_outcome, hooks_json_outcome))
}

fn materialize_codex_hooks(codex_dir: &Path) -> Result<InstallOutcome, String> {
    let hooks_dir = codex_dir.join("hooks");
    fs::create_dir_all(&hooks_dir)
        .map_err(|e| format!("failed to create {}: {e}", hooks_dir.display()))?;

    let ptu_outcome = write_executable(
        &hooks_dir.join("aegis-pre-tool-use.sh"),
        CODEX_PRE_TOOL_USE_HOOK_SH,
    )?;
    let session_outcome = write_executable(
        &hooks_dir.join("aegis-session-start.sh"),
        CODEX_SESSION_START_HOOK_SH,
    )?;

    Ok(combine_outcomes(ptu_outcome, session_outcome))
}

fn write_executable(path: &Path, content: &str) -> Result<InstallOutcome, String> {
    use std::os::unix::fs::PermissionsExt;

    match fs::read_to_string(path) {
        Ok(existing) => {
            let metadata = fs::metadata(path)
                .map_err(|err| format!("failed to stat {}: {err}", path.display()))?;
            if executable_content_is_current(&existing, metadata.permissions().mode(), content) {
                return Ok(InstallOutcome::AlreadyPresent);
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(format!("failed to read {}: {err}", path.display())),
    }

    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent", path.display()))?;
    let tmp = temporary_settings_path(parent);
    fs::write(&tmp, content).map_err(|e| format!("failed to write {}: {e}", tmp.display()))?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("failed to chmod {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("failed to install {}: {e}", path.display()))?;
    Ok(InstallOutcome::Installed)
}

fn apply_codex_hooks_json(
    hooks_json: &Path,
    ptu_dest: &Path,
    session_dest: &Path,
) -> Result<InstallOutcome, String> {
    let ptu_cmd = ptu_dest
        .to_str()
        .ok_or_else(|| "pre-tool-use hook path is not valid UTF-8".to_string())?
        .to_owned();
    let session_cmd = session_dest
        .to_str()
        .ok_or_else(|| "session-start hook path is not valid UTF-8".to_string())?
        .to_owned();

    let mut root = load_settings(hooks_json)?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "hooks.json must be a JSON object".to_string())?;

    let hooks = obj
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| "hooks.hooks must be a JSON object".to_string())?;

    let session_entries = hooks
        .entry("SessionStart".to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| "hooks.hooks.SessionStart must be an array".to_string())?;
    let session_present = codex_hook_present(
        session_entries,
        "startup|resume",
        &session_cmd,
        "hooks.hooks.SessionStart",
    )?;
    if !session_present {
        session_entries.push(serde_json::json!({
            "matcher": "startup|resume",
            "hooks": [{ "type": "command", "command": session_cmd }]
        }));
    }

    let ptu_entries = hooks
        .entry("PreToolUse".to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| "hooks.hooks.PreToolUse must be an array".to_string())?;
    let ptu_present = codex_hook_present(ptu_entries, "Bash", &ptu_cmd, "hooks.hooks.PreToolUse")?;
    if !ptu_present {
        ptu_entries.push(serde_json::json!({
            "matcher": "Bash",
            "hooks": [{ "type": "command", "command": ptu_cmd }]
        }));
    }

    if session_present && ptu_present {
        return Ok(InstallOutcome::AlreadyPresent);
    }

    write_settings_atomically(hooks_json, &root)?;
    Ok(InstallOutcome::Installed)
}

fn executable_content_is_current(existing: &str, mode: u32, expected_content: &str) -> bool {
    existing == expected_content && mode & 0o777 == 0o755
}

fn codex_hook_present(
    entries: &[Value],
    matcher: &str,
    command: &str,
    location: &str,
) -> Result<bool, String> {
    let mut found = false;

    for entry in entries {
        let obj = entry
            .as_object()
            .ok_or_else(|| format!("{location} entries must contain objects"))?;
        let entry_matcher = obj
            .get("matcher")
            .ok_or_else(|| format!("{location} entries must contain matcher"))?
            .as_str()
            .ok_or_else(|| format!("{location} entry matcher must be a string"))?;

        if entry_matcher != matcher {
            continue;
        }

        let hooks = obj
            .get("hooks")
            .ok_or_else(|| format!("{location} matching entry must contain hooks"))?
            .as_array()
            .ok_or_else(|| format!("{location} matching entry hooks must be an array"))?;

        for hook in hooks {
            let hook = hook
                .as_object()
                .ok_or_else(|| format!("{location} matching entry hooks must contain objects"))?;
            let hook_type = hook
                .get("type")
                .ok_or_else(|| format!("{location} matching entry hook must contain type"))?
                .as_str()
                .ok_or_else(|| format!("{location} matching entry hook type must be a string"))?;
            let hook_command = hook
                .get("command")
                .ok_or_else(|| format!("{location} matching entry hook must contain command"))?
                .as_str()
                .ok_or_else(|| {
                    format!("{location} matching entry hook command must be a string")
                })?;

            if hook_type == "command" && hook_command == command {
                found = true;
            }
        }
    }

    Ok(found)
}

fn combine_outcomes(lhs: InstallOutcome, rhs: InstallOutcome) -> InstallOutcome {
    if matches!(lhs, InstallOutcome::Installed) || matches!(rhs, InstallOutcome::Installed) {
        InstallOutcome::Installed
    } else if matches!(lhs, InstallOutcome::Skipped) || matches!(rhs, InstallOutcome::Skipped) {
        InstallOutcome::Skipped
    } else {
        InstallOutcome::AlreadyPresent
    }
}

fn agent_dir_exists(agent_dir: &Path) -> Result<bool, String> {
    if !agent_dir.exists() {
        return Ok(false);
    }

    if agent_dir.is_dir() {
        return Ok(true);
    }

    Err(format!(
        "{} exists but is not a directory",
        agent_dir.display()
    ))
}

fn load_settings(path: &Path) -> Result<Value, String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Value::Object(Map::new()));
        }
        Err(err) => return Err(format!("failed to read {}: {err}", path.display())),
    };

    if raw.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }

    let value: Value = serde_json::from_str(&raw)
        .map_err(|err| format!("failed to parse {} as JSON: {err}", path.display()))?;

    if value.is_object() {
        Ok(value)
    } else {
        Err(format!(
            "{} must contain a top-level JSON object",
            path.display()
        ))
    }
}

fn apply_installation(settings: &mut Value) -> Result<InstallOutcome, String> {
    let root = settings
        .as_object_mut()
        .ok_or_else(|| "settings.json must contain a top-level JSON object".to_string())?;

    let hooks = root
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| "settings.hooks must be a JSON object".to_string())?;

    let pre_tool_use = hooks
        .entry("PreToolUse".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let pre_tool_use = pre_tool_use
        .as_array_mut()
        .ok_or_else(|| "settings.hooks.PreToolUse must be a JSON array".to_string())?;

    if pre_tool_use_contains_bash_aegis_hook(pre_tool_use)? {
        return Ok(InstallOutcome::AlreadyPresent);
    }

    pre_tool_use.push(serde_json::json!({
        "matcher": "Bash",
        "hooks": [
            {
                "type": "command",
                "command": "aegis hook"
            }
        ]
    }));

    Ok(InstallOutcome::Installed)
}

fn pre_tool_use_contains_bash_aegis_hook(entries: &[Value]) -> Result<bool, String> {
    let mut found = false;

    for entry in entries {
        let entry = entry
            .as_object()
            .ok_or_else(|| "settings.hooks.PreToolUse entries must contain objects".to_string())?;
        let matcher = entry
            .get("matcher")
            .ok_or_else(|| "settings.hooks.PreToolUse entries must contain matcher".to_string())?
            .as_str()
            .ok_or_else(|| {
                "settings.hooks.PreToolUse entry matcher must be a string".to_string()
            })?;

        if matcher != "Bash" {
            continue;
        }

        let hooks = entry
            .get("hooks")
            .ok_or_else(|| {
                "settings.hooks.PreToolUse matching Bash entry must contain hooks".to_string()
            })?
            .as_array()
            .ok_or_else(|| {
                "settings.hooks.PreToolUse matching Bash entry hooks must be an array".to_string()
            })?;

        for hook in hooks {
            let hook = hook.as_object().ok_or_else(|| {
                "settings.hooks.PreToolUse matching Bash entry hooks must contain objects"
                    .to_string()
            })?;

            let hook_type = hook
                .get("type")
                .ok_or_else(|| {
                    "settings.hooks.PreToolUse matching Bash hook must contain type".to_string()
                })?
                .as_str()
                .ok_or_else(|| {
                    "settings.hooks.PreToolUse matching Bash hook type must be a string".to_string()
                })?;
            let hook_command = hook
                .get("command")
                .ok_or_else(|| {
                    "settings.hooks.PreToolUse matching Bash hook must contain command".to_string()
                })?
                .as_str()
                .ok_or_else(|| {
                    "settings.hooks.PreToolUse matching Bash hook command must be a string"
                        .to_string()
                })?;

            if hook_type == "command" && hook_command == "aegis hook" {
                found = true;
            }
        }
    }

    Ok(found)
}

fn write_settings_atomically(path: &Path, settings: &Value) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} does not have a parent directory", path.display()))?;

    fs::create_dir_all(parent)
        .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;

    let temp_path = temporary_settings_path(parent);
    {
        let mut temp = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|err| {
                format!(
                    "failed to create temporary file {}: {err}",
                    temp_path.display()
                )
            })?;

        serde_json::to_writer_pretty(&mut temp, settings)
            .map_err(|err| format!("failed to serialize JSON for {}: {err}", path.display()))?;
        temp.write_all(b"\n")
            .map_err(|err| format!("failed to finish writing {}: {err}", path.display()))?;
        temp.sync_all()
            .map_err(|err| format!("failed to flush {}: {err}", temp_path.display()))?;
    }

    fs::rename(&temp_path, path)
        .map_err(|err| format!("failed to replace {}: {err}", path.display()))?;

    Ok(())
}

fn temporary_settings_path(parent: &Path) -> PathBuf {
    let pid = process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    // This name does not try to provide strong entropy on its own; the collision guard is
    // write_settings_atomically() using create_new(true), which fails closed instead of
    // silently overwriting another installer's temporary file.
    parent.join(format!(".settings.json.aegis-{pid}-{nanos}.tmp"))
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

fn settings_path_global(home_dir: Option<&Path>) -> Result<PathBuf, String> {
    let home_dir = home_dir.ok_or_else(|| "HOME is not set".to_string())?;
    Ok(home_dir.join(".claude/settings.json"))
}

fn settings_path_local(cwd: &Path) -> PathBuf {
    cwd.join(".claude/settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    #[test]
    fn hook_rewrites_plain_command_with_shell_quote() {
        let output =
            match hook_response_value(r#"{"tool_input":{"command":"git commit -m 'fix: hello'"}}"#)
            {
                HookOutcome::Allow(output) => output,
                other => panic!("expected rewrite output, got {other:?}"),
            };
        let rewritten = format!(
            "aegis --command {}",
            shell_quote("git commit -m 'fix: hello'")
        );

        let expected = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "allow",
                "permissionDecisionReason": "aegis intercept",
                "updatedInput": {
                    "command": rewritten
                }
            }
        });

        assert_eq!(output, expected);
    }

    #[test]
    fn hook_skips_already_wrapped_command() {
        assert!(matches!(
            hook_response_value(r#"{"tool_input":{"command":"aegis --command 'rm -rf /tmp'"}}"#),
            HookOutcome::Noop
        ));
    }

    #[test]
    fn hook_skips_missing_command_field() {
        assert!(matches!(
            hook_response_value(r#"{"tool_input":{}}"#),
            HookOutcome::Noop
        ));
    }

    #[test]
    fn hook_rejects_malformed_json_input() {
        assert!(matches!(
            hook_response_value(
                r#"{"tool_input":{"command":#),
            HookOutcome::Deny(_)
        ));
    }

    #[test]
    fn hook_rejects_non_object_tool_input() {
        assert!(matches!(
            hook_response_value(r#"{"tool_input":"rm -rf /"}"#
            ),
            HookOutcome::Deny(_)
        ));
    }

    #[test]
    fn hook_does_not_skip_aegisctl_commands() {
        assert!(matches!(
            hook_response_value(r#"{"tool_input":{"command":"aegisctl status"}}"#),
            HookOutcome::Allow(_)
        ));
    }

    #[test]
    fn install_settings_path_uses_local_cwd_by_default() {
        let cwd = TempDir::new().expect("temp dir");

        let path = settings_path_local(cwd.path());
        assert_eq!(path, cwd.path().join(".claude/settings.json"));
    }

    #[test]
    fn install_settings_path_uses_home_for_global() {
        let home = TempDir::new().expect("home dir");

        let path = settings_path_global(Some(home.path())).expect("global path");
        assert_eq!(path, home.path().join(".claude/settings.json"));
    }

    #[test]
    fn load_settings_does_not_preflight_with_exists_check() {
        let source = include_str!("install.rs");
        let start = source
            .find("fn load_settings(path: &Path) -> Result<Value, String> {")
            .expect("load_settings function must exist");
        let load_settings_source = &source[start..];
        let next_fn = load_settings_source
            .find(
                "\nfn apply_installation(settings: &mut Value) -> Result<InstallOutcome, String> {",
            )
            .expect("load_settings must be followed by apply_installation");
        let load_settings_body = &load_settings_source[..next_fn];

        assert!(
            !load_settings_body.contains("path.exists()"),
            "load_settings must not preflight with exists(); handle NotFound from read_to_string to avoid TOCTOU"
        );
    }

    #[test]
    fn temporary_settings_path_documents_create_new_collision_guard() {
        let source = include_str!("install.rs");
        let start = source
            .find("fn temporary_settings_path(parent: &Path) -> PathBuf {")
            .expect("temporary_settings_path function must exist");
        let temp_path_source = &source[start..];
        let next_fn = temp_path_source
            .find("\nfn home_dir() -> Option<PathBuf> {")
            .expect("temporary_settings_path must be followed by home_dir");
        let temp_path_body = &temp_path_source[..next_fn];

        assert!(
            temp_path_body.contains("create_new") && temp_path_body.contains("collision guard"),
            "temporary_settings_path must document that write_settings_atomically relies on create_new as the collision guard"
        );
    }

    #[test]
    fn install_round_trip_writes_settings_file_atomically() {
        let dir = TempDir::new().expect("temp dir");
        let settings_dir = dir.path().join(".claude");
        fs::create_dir_all(&settings_dir).expect("create settings dir");
        let settings_path = settings_dir.join("settings.json");
        fs::write(&settings_path, "{}\n").expect("seed settings file");

        let outcome = run_install_at_path(&settings_path).expect("install");
        assert!(matches!(outcome, InstallOutcome::Installed));

        let written = fs::read_to_string(&settings_path).expect("read settings");
        let parsed: Value = serde_json::from_str(&written).expect("parse settings");
        assert_eq!(
            parsed,
            serde_json::json!({
                "hooks": {
                    "PreToolUse": [
                        {
                            "matcher": "Bash",
                            "hooks": [
                                {
                                    "type": "command",
                                    "command": "aegis hook"
                                }
                            ]
                        }
                    ]
                }
            })
        );
    }

    #[test]
    fn install_target_selection_defaults_to_all_when_no_target_flags_are_set() {
        let args = crate::InstallArgs {
            local: false,
            all: false,
            claude_code: false,
            codex: false,
        };

        assert_eq!(install_target_selection(&args), InstallTargetSelection::All);
    }

    #[test]
    fn install_target_selection_honors_explicit_target_flags() {
        let claude_only = crate::InstallArgs {
            local: false,
            all: false,
            claude_code: true,
            codex: false,
        };
        assert_eq!(
            install_target_selection(&claude_only),
            InstallTargetSelection::ClaudeCode
        );

        let codex_only = crate::InstallArgs {
            local: false,
            all: false,
            claude_code: false,
            codex: true,
        };
        assert_eq!(
            install_target_selection(&codex_only),
            InstallTargetSelection::Codex
        );

        let all_targets = crate::InstallArgs {
            local: false,
            all: true,
            claude_code: false,
            codex: true,
        };
        assert_eq!(
            install_target_selection(&all_targets),
            InstallTargetSelection::All
        );
    }

    #[test]
    fn global_claude_install_skips_when_agent_dir_is_missing() {
        let home = TempDir::new().expect("home dir");

        let outcome = run_global_claude_install_at_home(Some(home.path())).expect("install");

        assert!(matches!(outcome, InstallOutcome::Skipped));
        assert!(!home.path().join(".claude/settings.json").exists());
    }

    #[test]
    fn global_claude_install_errors_on_malformed_settings_json() {
        let home = TempDir::new().expect("home dir");
        let claude_dir = home.path().join(".claude");
        fs::create_dir_all(&claude_dir).expect("create claude dir");
        fs::write(claude_dir.join("settings.json"), "{not valid json").expect("write settings");

        let err = run_global_claude_install_at_home(Some(home.path()))
            .expect_err("malformed settings should error");

        assert!(err.contains(".claude/settings.json"));
    }

    #[test]
    fn global_claude_install_errors_on_malformed_nested_bash_hook_entry() {
        let home = TempDir::new().expect("home dir");
        let claude_dir = home.path().join(".claude");
        fs::create_dir_all(&claude_dir).expect("create claude dir");
        fs::write(
            claude_dir.join("settings.json"),
            serde_json::json!({
                "hooks": {
                    "PreToolUse": [
                        {
                            "matcher": "Bash",
                            "hooks": "not-an-array"
                        }
                    ]
                }
            })
            .to_string(),
        )
        .expect("write settings");

        let err = run_global_claude_install_at_home(Some(home.path()))
            .expect_err("malformed nested bash hook should error");

        assert!(err.contains("settings.hooks.PreToolUse"));
    }

    #[test]
    fn global_claude_install_errors_on_non_object_pre_tool_use_member() {
        let home = TempDir::new().expect("home dir");
        let claude_dir = home.path().join(".claude");
        fs::create_dir_all(&claude_dir).expect("create claude dir");
        fs::write(
            claude_dir.join("settings.json"),
            serde_json::json!({
                "hooks": {
                    "PreToolUse": ["bad-entry"]
                }
            })
            .to_string(),
        )
        .expect("write settings");

        let err = run_global_claude_install_at_home(Some(home.path()))
            .expect_err("non-object pre-tool-use member should error");

        assert!(err.contains("settings.hooks.PreToolUse"));
    }

    #[test]
    fn global_claude_install_errors_on_non_string_bash_matcher() {
        let home = TempDir::new().expect("home dir");
        let claude_dir = home.path().join(".claude");
        fs::create_dir_all(&claude_dir).expect("create claude dir");
        fs::write(
            claude_dir.join("settings.json"),
            serde_json::json!({
                "hooks": {
                    "PreToolUse": [
                        {
                            "matcher": 7,
                            "hooks": []
                        }
                    ]
                }
            })
            .to_string(),
        )
        .expect("write settings");

        let err = run_global_claude_install_at_home(Some(home.path()))
            .expect_err("non-string matcher should error");

        assert!(err.contains("settings.hooks.PreToolUse"));
    }

    #[test]
    fn codex_install_errors_on_malformed_hooks_json() {
        let home = TempDir::new().expect("home dir");
        let codex_dir = home.path().join(".codex");
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        fs::write(codex_dir.join("hooks.json"), "{not valid json").expect("write hooks.json");

        let err =
            run_codex_install_at_dir(&codex_dir).expect_err("malformed hooks.json should error");

        assert!(err.contains("hooks.json"));
    }

    #[test]
    fn codex_install_errors_on_malformed_nested_session_start_hook_entry() {
        let home = TempDir::new().expect("home dir");
        let codex_dir = home.path().join(".codex");
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        fs::write(
            codex_dir.join("hooks.json"),
            serde_json::json!({
                "hooks": {
                    "SessionStart": [
                        {
                            "matcher": "startup|resume",
                            "hooks": "not-an-array"
                        }
                    ]
                }
            })
            .to_string(),
        )
        .expect("write hooks.json");

        let err = run_codex_install_at_dir(&codex_dir)
            .expect_err("malformed nested session start hook should error");

        assert!(err.contains("hooks.hooks.SessionStart"));
    }

    #[test]
    fn codex_install_errors_on_non_object_session_start_member() {
        let home = TempDir::new().expect("home dir");
        let codex_dir = home.path().join(".codex");
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        fs::write(
            codex_dir.join("hooks.json"),
            serde_json::json!({
                "hooks": {
                    "SessionStart": [42]
                }
            })
            .to_string(),
        )
        .expect("write hooks.json");

        let err = run_codex_install_at_dir(&codex_dir)
            .expect_err("non-object session-start member should error");

        assert!(err.contains("hooks.hooks.SessionStart"));
    }

    #[test]
    fn codex_install_errors_on_non_string_session_start_matcher() {
        let home = TempDir::new().expect("home dir");
        let codex_dir = home.path().join(".codex");
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        fs::write(
            codex_dir.join("hooks.json"),
            serde_json::json!({
                "hooks": {
                    "SessionStart": [
                        {
                            "matcher": 42,
                            "hooks": []
                        }
                    ]
                }
            })
            .to_string(),
        )
        .expect("write hooks.json");

        let err = run_codex_install_at_dir(&codex_dir)
            .expect_err("non-string session-start matcher should error");

        assert!(err.contains("hooks.hooks.SessionStart"));
    }

    #[test]
    fn codex_install_errors_on_non_string_pre_tool_use_matcher() {
        let home = TempDir::new().expect("home dir");
        let codex_dir = home.path().join(".codex");
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        fs::write(
            codex_dir.join("hooks.json"),
            serde_json::json!({
                "hooks": {
                    "PreToolUse": [
                        {
                            "matcher": false,
                            "hooks": []
                        }
                    ]
                }
            })
            .to_string(),
        )
        .expect("write hooks.json");

        let err = run_codex_install_at_dir(&codex_dir)
            .expect_err("non-string pre-tool-use matcher should error");

        assert!(err.contains("hooks.hooks.PreToolUse"));
    }

    #[test]
    fn codex_install_skips_when_agent_dir_is_missing() {
        let home = TempDir::new().expect("home dir");
        let codex_dir = home.path().join(".codex");

        let outcome = run_codex_install_at_dir(&codex_dir).expect("install");

        assert!(matches!(outcome, InstallOutcome::Skipped));
        assert!(!codex_dir.join("hooks.json").exists());
    }

    #[test]
    fn codex_install_is_idempotent_without_duplicate_registrations() {
        let home = TempDir::new().expect("home dir");
        let codex_dir = home.path().join(".codex");
        fs::create_dir_all(&codex_dir).expect("create codex dir");

        let first = run_codex_install_at_dir(&codex_dir).expect("first install");
        assert!(matches!(first, InstallOutcome::Installed));

        let second = run_codex_install_at_dir(&codex_dir).expect("second install");
        assert!(matches!(second, InstallOutcome::AlreadyPresent));

        let hooks: Value = serde_json::from_str(
            &fs::read_to_string(codex_dir.join("hooks.json")).expect("read hooks.json"),
        )
        .expect("parse hooks.json");

        let session_entries = hooks["hooks"]["SessionStart"]
            .as_array()
            .expect("SessionStart array");
        assert_eq!(session_entries.len(), 1);

        let pre_tool_use_entries = hooks["hooks"]["PreToolUse"]
            .as_array()
            .expect("PreToolUse array");
        assert_eq!(pre_tool_use_entries.len(), 1);
    }

    #[test]
    fn write_executable_repairs_missing_owner_execute_bit() {
        let dir = TempDir::new().expect("temp dir");
        let hook_path = dir.path().join("hook.sh");
        fs::write(&hook_path, CODEX_PRE_TOOL_USE_HOOK_SH).expect("write hook");
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o455))
            .expect("set permissions");

        let outcome = write_executable(&hook_path, CODEX_PRE_TOOL_USE_HOOK_SH).expect("install");

        assert_eq!(
            outcome,
            InstallOutcome::Installed,
            "matching content with missing owner execute should be repaired"
        );
        let mode = fs::metadata(&hook_path)
            .expect("stat hook")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o755, "installed hook should normalize to 0755");
    }

    #[test]
    fn local_install_can_bootstrap_project_settings_when_missing() {
        let project = TempDir::new().expect("project dir");
        let settings_path = settings_path_local(project.path());

        let outcome = run_install_at_path(&settings_path).expect("install");

        assert!(matches!(outcome, InstallOutcome::Installed));
        assert!(settings_path.exists());
    }

    #[test]
    fn install_is_idempotent_and_preserves_existing_entries() {
        let mut settings = serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "echo keep"
                            }
                        ]
                    }
                ]
            }
        });

        let outcome = apply_installation(&mut settings).expect("first install");
        assert!(matches!(outcome, InstallOutcome::Installed));

        let pre_tool_use = settings["hooks"]["PreToolUse"]
            .as_array()
            .expect("PreToolUse array");
        assert_eq!(pre_tool_use.len(), 2);
        assert_eq!(
            pre_tool_use[1],
            serde_json::json!({
                "matcher": "Bash",
                "hooks": [
                    {
                        "type": "command",
                        "command": "aegis hook"
                    }
                ]
            })
        );

        let outcome = apply_installation(&mut settings).expect("second install");
        assert!(matches!(outcome, InstallOutcome::AlreadyPresent));
        assert_eq!(
            settings["hooks"]["PreToolUse"]
                .as_array()
                .expect("PreToolUse array")
                .len(),
            2
        );
    }

    #[test]
    fn install_ignores_non_bash_hook_with_aegis_command() {
        let mut settings = serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Git",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "aegis hook"
                            }
                        ]
                    }
                ]
            }
        });

        let outcome = apply_installation(&mut settings).expect("install");
        assert!(matches!(outcome, InstallOutcome::Installed));
        assert_eq!(
            settings["hooks"]["PreToolUse"]
                .as_array()
                .expect("PreToolUse array")
                .len(),
            2
        );
    }

    #[test]
    fn install_adds_hooks_tree_when_missing() {
        let mut settings = serde_json::json!({});

        let outcome = apply_installation(&mut settings).expect("install");
        assert!(matches!(outcome, InstallOutcome::Installed));
        assert_eq!(
            settings,
            serde_json::json!({
                "hooks": {
                    "PreToolUse": [
                        {
                            "matcher": "Bash",
                            "hooks": [
                                {
                                    "type": "command",
                                    "command": "aegis hook"
                                }
                            ]
                        }
                    ]
                }
            })
        );
    }
}
