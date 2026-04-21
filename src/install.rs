use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

const CODEX_PRE_TOOL_USE_HOOK_SH: &str =
    include_str!("../scripts/hooks/codex-pre-tool-use.sh");
const CODEX_SESSION_START_HOOK_SH: &str =
    include_str!("../scripts/hooks/codex-session-start.sh");

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

/// Install aegis hooks for all detected agents (Claude Code + Codex if present).
pub(crate) fn run_install(args: &super::InstallArgs) -> i32 {
    let mut exit = 0;

    match run_install_inner(args.global) {
        Ok(InstallOutcome::Installed) => println!("Claude Code: hook installed"),
        Ok(InstallOutcome::AlreadyPresent) => println!("Claude Code: hook already present, skipping"),
        Ok(InstallOutcome::Skipped) => {}
        Err(err) => {
            eprintln!("error: failed to install Claude Code hook: {err}");
            exit = super::EXIT_INTERNAL;
        }
    }

    match run_codex_install_inner() {
        Ok(InstallOutcome::Installed) => println!("Codex: hooks installed"),
        Ok(InstallOutcome::AlreadyPresent) => println!("Codex: hooks already present, skipping"),
        Ok(InstallOutcome::Skipped) => {}
        Err(err) => {
            eprintln!("error: failed to install Codex hooks: {err}");
            exit = super::EXIT_INTERNAL;
        }
    }

    exit
}

#[derive(Debug)]
enum InstallOutcome {
    Installed,
    AlreadyPresent,
    /// Agent directory not present — nothing to install.
    Skipped,
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

fn run_install_inner(global: bool) -> Result<InstallOutcome, String> {
    let settings_path = if global {
        let home = home_dir();
        settings_path_global(home.as_deref())?
    } else {
        let cwd = env::current_dir()
            .map_err(|err| format!("failed to resolve current directory: {err}"))?;
        settings_path_local(&cwd)
    };

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

fn run_codex_install_inner() -> Result<InstallOutcome, String> {
    let home = home_dir().ok_or_else(|| "HOME is not set".to_string())?;
    let codex_dir = home.join(".codex");

    if !codex_dir.exists() {
        return Ok(InstallOutcome::Skipped);
    }

    let hooks_dir = codex_dir.join("hooks");
    let hooks_json_path = codex_dir.join("hooks.json");

    fs::create_dir_all(&hooks_dir)
        .map_err(|e| format!("failed to create {}: {e}", hooks_dir.display()))?;

    let ptu_dest = hooks_dir.join("aegis-pre-tool-use.sh");
    let session_dest = hooks_dir.join("aegis-session-start.sh");

    write_executable(&ptu_dest, CODEX_PRE_TOOL_USE_HOOK_SH)?;
    write_executable(&session_dest, CODEX_SESSION_START_HOOK_SH)?;

    apply_codex_hooks_json(&hooks_json_path, &ptu_dest, &session_dest)
}

fn write_executable(path: &Path, content: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent", path.display()))?;
    let tmp = temporary_settings_path(parent);
    fs::write(&tmp, content)
        .map_err(|e| format!("failed to write {}: {e}", tmp.display()))?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("failed to chmod {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path)
        .map_err(|e| format!("failed to install {}: {e}", path.display()))?;
    Ok(())
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
    let session_present = codex_hook_present(session_entries, "startup|resume", &session_cmd);
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
    let ptu_present = codex_hook_present(ptu_entries, "Bash", &ptu_cmd);
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

fn codex_hook_present(entries: &[Value], matcher: &str, command: &str) -> bool {
    entries.iter().any(|entry| {
        let Some(obj) = entry.as_object() else {
            return false;
        };
        if obj.get("matcher").and_then(Value::as_str) != Some(matcher) {
            return false;
        }
        obj.get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|hooks| {
                hooks.iter().any(|hook| {
                    let Some(h) = hook.as_object() else {
                        return false;
                    };
                    h.get("type").and_then(Value::as_str) == Some("command")
                        && h.get("command").and_then(Value::as_str) == Some(command)
                })
            })
    })
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

    if pre_tool_use_contains_bash_aegis_hook(pre_tool_use) {
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

fn pre_tool_use_contains_bash_aegis_hook(entries: &[Value]) -> bool {
    entries.iter().any(|entry| {
        let Some(entry) = entry.as_object() else {
            return false;
        };

        if entry.get("matcher").and_then(Value::as_str) != Some("Bash") {
            return false;
        }

        entry
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|hooks| {
                hooks.iter().any(|hook| {
                    let Some(hook) = hook.as_object() else {
                        return false;
                    };

                    hook.get("type").and_then(Value::as_str) == Some("command")
                        && hook.get("command").and_then(Value::as_str) == Some("aegis hook")
                })
            })
    })
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
