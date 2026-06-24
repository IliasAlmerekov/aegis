use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::{
    AgentInstallResult, InstallOutcome, agent_dir_exists, shell_quote, write_settings_atomically,
};

const CODEX_PRE_TOOL_USE_HOOK_SH: &str = include_str!("../../scripts/hooks/codex-pre-tool-use.sh");
const CODEX_SESSION_START_HOOK_SH: &str =
    include_str!("../../scripts/hooks/codex-session-start.sh");

pub(crate) fn run_codex_install() -> AgentInstallResult {
    AgentInstallResult::from_result(run_codex_install_inner())
}

fn run_codex_install_inner() -> Result<InstallOutcome, String> {
    let home = super::home_dir().ok_or_else(|| "HOME is not set".to_string())?;
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
    let config_outcome = apply_codex_config_toml(&codex_dir.join("config.toml"))?;

    Ok(combine_outcomes(
        combine_outcomes(hooks_outcome, hooks_json_outcome),
        config_outcome,
    ))
}

fn materialize_codex_hooks(codex_dir: &Path) -> Result<InstallOutcome, String> {
    let hooks_dir = codex_dir.join("hooks");
    fs::create_dir_all(&hooks_dir)
        .map_err(|e| format!("failed to create {}: {e}", hooks_dir.display()))?;

    let ptu_outcome = write_executable(
        &hooks_dir.join("aegis-pre-tool-use.sh"),
        &render_pre_tool_use_hook(),
    )?;
    let session_outcome = write_executable(
        &hooks_dir.join("aegis-session-start.sh"),
        CODEX_SESSION_START_HOOK_SH,
    )?;

    Ok(combine_outcomes(ptu_outcome, session_outcome))
}

/// Resolve the absolute path of the currently running Aegis binary, falling
/// back to a bare `aegis` PATH lookup if the executable path is unavailable.
fn resolved_aegis_bin() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(str::to_owned))
        .unwrap_or_else(|| "aegis".to_string())
}

/// Materialize the Codex PreToolUse hook with `__AEGIS_BIN__` replaced by an
/// absolute, shell-quoted path to the Aegis binary. This keeps the hook working
/// when Codex runs it with a minimal PATH; an explicit `AEGIS_BIN` in the
/// environment still overrides the templated default.
fn render_pre_tool_use_hook() -> String {
    CODEX_PRE_TOOL_USE_HOOK_SH.replace("__AEGIS_BIN__", &shell_quote(&resolved_aegis_bin()))
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

    let mut root = super::load_settings(hooks_json)?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "hooks.json must be a JSON object".to_string())?;

    let hooks = obj
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()))
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

fn apply_codex_config_toml(config_path: &Path) -> Result<InstallOutcome, String> {
    let mut config = load_codex_config_toml(config_path)?;
    let root = config
        .as_table_mut()
        .ok_or_else(|| "config.toml must contain a top-level TOML table".to_string())?;

    let features = root
        .entry("features".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
        .as_table_mut()
        .ok_or_else(|| "config.toml features must be a TOML table".to_string())?;

    let removed_legacy_hooks_flag = features.remove("codex_hooks").is_some();
    let hooks_was_enabled = features
        .get("hooks")
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);

    features.insert("hooks".to_string(), toml::Value::Boolean(true));

    if hooks_was_enabled && !removed_legacy_hooks_flag {
        return Ok(InstallOutcome::AlreadyPresent);
    }

    write_toml_atomically(config_path, &config)?;
    Ok(InstallOutcome::Installed)
}

fn load_codex_config_toml(path: &Path) -> Result<toml::Value, String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(toml::Value::Table(toml::map::Map::new()));
        }
        Err(err) => return Err(format!("failed to read {}: {err}", path.display())),
    };

    if raw.trim().is_empty() {
        return Ok(toml::Value::Table(toml::map::Map::new()));
    }

    let value: toml::Value = toml::from_str(&raw)
        .map_err(|err| format!("failed to parse {} as TOML: {err}", path.display()))?;

    if value.is_table() {
        Ok(value)
    } else {
        Err(format!(
            "{} must contain a top-level TOML table",
            path.display()
        ))
    }
}

fn write_toml_atomically(path: &Path, value: &toml::Value) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} does not have a parent directory", path.display()))?;

    fs::create_dir_all(parent)
        .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;

    let rendered = toml::to_string_pretty(value)
        .map_err(|err| format!("failed to serialize TOML for {}: {err}", path.display()))?;

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

        temp.write_all(rendered.as_bytes())
            .map_err(|err| format!("failed to write {}: {err}", temp_path.display()))?;
        temp.sync_all()
            .map_err(|err| format!("failed to flush {}: {err}", temp_path.display()))?;
    }

    fs::rename(&temp_path, path)
        .map_err(|err| format!("failed to replace {}: {err}", path.display()))?;

    Ok(())
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

fn temporary_settings_path(parent: &Path) -> PathBuf {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();

    parent.join(format!(".settings.json.aegis-{pid}-{nanos}.tmp"))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use tempfile::TempDir;

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
    fn render_pre_tool_use_hook_substitutes_absolute_binary_path() {
        let rendered = render_pre_tool_use_hook();

        assert!(
            !rendered.contains("__AEGIS_BIN__"),
            "placeholder must be substituted at install time"
        );
        let expected = format!("AEGIS_BIN={}", shell_quote(&resolved_aegis_bin()));
        assert!(
            rendered.contains(&expected),
            "rendered hook must assign the shell-quoted absolute aegis path, got:\n{rendered}"
        );
        // The transparent-rewrite hook must not reintroduce jq/python3 parsing.
        assert!(!rendered.contains("python3 -"));
        assert!(!rendered.contains("jq -"));
        assert!(rendered.contains("exec \"${AEGIS_BIN}\" hook"));
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
    fn codex_install_creates_supported_config_toml() {
        let home = TempDir::new().expect("home dir");
        let codex_dir = home.path().join(".codex");
        fs::create_dir_all(&codex_dir).expect("create codex dir");

        let outcome = run_codex_install_at_dir(&codex_dir).expect("install");

        assert!(matches!(outcome, InstallOutcome::Installed));
        let config_path = codex_dir.join("config.toml");
        let config = fs::read_to_string(&config_path).expect("read config.toml");
        let parsed: toml::Value = toml::from_str(&config).expect("config.toml parses");
        assert_eq!(parsed["features"]["hooks"].as_bool(), Some(true));
        assert!(parsed["features"].get("codex_hooks").is_none());
        assert!(parsed.get("profiles").is_none());
    }

    #[test]
    fn codex_install_repairs_legacy_config_toml_without_dropping_unrelated_settings() {
        let home = TempDir::new().expect("home dir");
        let codex_dir = home.path().join(".codex");
        fs::create_dir_all(&codex_dir).expect("create codex dir");
        fs::write(
            codex_dir.join("config.toml"),
            r#"
approval_policy = "on-request"

[features]
multi_agent = true
codex_hooks = true

[profiles.strict]
sandbox_mode = "read-only"
"#,
        )
        .expect("write legacy config.toml");

        let outcome = run_codex_install_at_dir(&codex_dir).expect("install");

        assert!(matches!(outcome, InstallOutcome::Installed));
        let config = fs::read_to_string(codex_dir.join("config.toml")).expect("read config.toml");
        let parsed: toml::Value = toml::from_str(&config).expect("config.toml parses");
        assert_eq!(parsed["approval_policy"].as_str(), Some("on-request"));
        assert_eq!(parsed["features"]["multi_agent"].as_bool(), Some(true));
        assert_eq!(parsed["features"]["hooks"].as_bool(), Some(true));
        assert!(parsed["features"].get("codex_hooks").is_none());
        assert_eq!(
            parsed["profiles"]["strict"]["sandbox_mode"].as_str(),
            Some("read-only")
        );
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
}
