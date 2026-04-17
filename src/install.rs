use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

/// Run the Claude Code `PreToolUse` hook and rewrite unwrapped Bash commands
/// through `aegis --command`.
pub(crate) fn run_hook() -> i32 {
    if let Some(output) = hook_response_from_stdin() {
        println!("{output}");
    }

    0
}

/// Install the aegis Claude Code hook into the requested settings file.
pub(crate) fn run_install(args: &super::InstallArgs) -> i32 {
    match run_install_inner(args.global) {
        Ok(InstallOutcome::Installed) => {
            println!("Claude Code: hook installed");
            0
        }
        Ok(InstallOutcome::AlreadyPresent) => {
            println!("Claude Code: hook already present, skipping");
            0
        }
        Err(err) => {
            eprintln!("error: failed to install Claude Code hook: {err}");
            super::EXIT_INTERNAL
        }
    }
}

enum InstallOutcome {
    Installed,
    AlreadyPresent,
}

fn hook_response_from_stdin() -> Option<Value> {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return None;
    }

    hook_response_value(&input)
}

fn hook_response_value(input: &str) -> Option<Value> {
    let input: Value = serde_json::from_str(input).ok()?;
    let tool_input = input.get("tool_input")?.as_object()?;
    let command = tool_input.get("command")?.as_str()?;

    if command.starts_with("aegis") {
        return None;
    }

    let mut updated_input = tool_input.clone();
    updated_input.insert(
        "command".to_string(),
        Value::String(format!("aegis --command {}", shell_quote(command))),
    );

    Some(serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "permissionDecisionReason": "aegis intercept",
            "updatedInput": updated_input,
        }
    }))
}

fn shell_quote(command: &str) -> String {
    format!("'{}'", command.replace('\'', "'\\''"))
}

fn run_install_inner(global: bool) -> Result<InstallOutcome, String> {
    let cwd =
        env::current_dir().map_err(|err| format!("failed to resolve current directory: {err}"))?;
    let home = home_dir();
    let settings_path = settings_path(global, &cwd, home.as_deref())?;

    let mut settings = load_settings(&settings_path)?;
    let outcome = apply_installation(&mut settings)?;
    if matches!(outcome, InstallOutcome::Installed) {
        write_settings_atomically(&settings_path, &settings)?;
    }

    Ok(outcome)
}

fn load_settings(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }

    let raw = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;

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

    if pre_tool_use_contains_aegis_hook(pre_tool_use) {
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

fn pre_tool_use_contains_aegis_hook(entries: &[Value]) -> bool {
    entries.iter().any(|entry| {
        entry
            .as_object()
            .and_then(|object| object.get("hooks"))
            .and_then(Value::as_array)
            .is_some_and(|hooks| {
                hooks.iter().any(|hook| {
                    hook.as_object()
                        .and_then(|object| object.get("command"))
                        .and_then(Value::as_str)
                        == Some("aegis hook")
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

    let write_result = (|| -> Result<(), String> {
        serde_json::to_writer_pretty(&mut temp, settings)
            .map_err(|err| format!("failed to serialize JSON for {}: {err}", path.display()))?;
        temp.write_all(b"\n")
            .map_err(|err| format!("failed to finish writing {}: {err}", path.display()))?;
        temp.sync_all()
            .map_err(|err| format!("failed to flush {}: {err}", temp_path.display()))?;
        fs::rename(&temp_path, path)
            .map_err(|err| format!("failed to replace {}: {err}", path.display()))?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    write_result?;

    Ok(())
}

fn temporary_settings_path(parent: &Path) -> PathBuf {
    let pid = process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

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

fn settings_path(global: bool, cwd: &Path, home_dir: Option<&Path>) -> Result<PathBuf, String> {
    if global {
        let home_dir = home_dir.ok_or_else(|| "HOME is not set".to_string())?;
        Ok(home_dir.join(".claude/settings.json"))
    } else {
        Ok(cwd.join(".claude/settings.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    #[test]
    fn hook_rewrites_plain_command_with_shell_quote() {
        let output =
            hook_response_value(r#"{"tool_input":{"command":"git commit -m 'fix: hello'"}}"#)
                .expect("expected rewrite output");
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
        assert!(
            hook_response_value(r#"{"tool_input":{"command":"aegis --command 'rm -rf /tmp'"}}"#)
                .is_none()
        );
    }

    #[test]
    fn hook_skips_missing_command_field() {
        assert!(hook_response_value(r#"{"tool_input":{}}"#).is_none());
    }

    #[test]
    fn install_settings_path_uses_local_cwd_by_default() {
        let cwd = TempDir::new().expect("temp dir");
        let home = TempDir::new().expect("home dir");

        let path = settings_path(false, cwd.path(), Some(home.path())).expect("local path");
        assert_eq!(path, cwd.path().join(".claude/settings.json"));
    }

    #[test]
    fn install_settings_path_uses_home_for_global() {
        let cwd = TempDir::new().expect("temp dir");
        let home = TempDir::new().expect("home dir");

        let path = settings_path(true, cwd.path(), Some(home.path())).expect("global path");
        assert_eq!(path, home.path().join(".claude/settings.json"));
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
