use std::fs;
use std::path::Path;

use serde_json::Value;

use super::{
    AgentInstallResult, InstallOutcome, combine_outcomes, load_settings, resolved_aegis_bin,
    shell_quote, write_executable, write_settings_atomically,
};

const CLAUDE_PRE_TOOL_USE_HOOK_SH: &str = include_str!("../../scripts/hooks/claude-code.sh");

pub(crate) fn run_claude_install(global: bool) -> AgentInstallResult {
    AgentInstallResult::from_result(run_install_inner(global))
}

fn run_install_inner(global: bool) -> Result<InstallOutcome, String> {
    if global {
        let home = super::home_dir();
        return run_global_claude_install_at_home(home.as_deref());
    }

    let cwd = std::env::current_dir()
        .map_err(|err| format!("failed to resolve current directory: {err}"))?;
    let settings_path = super::settings_path_local(&cwd);
    run_install_at_path(&settings_path)
}

fn run_global_claude_install_at_home(home_dir: Option<&Path>) -> Result<InstallOutcome, String> {
    let settings_path = super::settings_path_global(home_dir)?;
    let claude_dir = settings_path.parent().ok_or_else(|| {
        format!(
            "{} does not have a parent directory",
            settings_path.display()
        )
    })?;

    if !super::agent_dir_exists(claude_dir)? {
        return Ok(InstallOutcome::Skipped);
    }

    run_install_at_path(&settings_path)
}

fn run_install_at_path(settings_path: &Path) -> Result<InstallOutcome, String> {
    let mut settings = load_settings(settings_path)?;

    // The shim lives next to the settings file in `<settings_dir>/hooks/`.
    // Deriving the dir from the settings path keeps global and `--local`
    // installs on a single code path.
    let settings_dir = settings_path.parent().ok_or_else(|| {
        format!(
            "{} does not have a parent directory",
            settings_path.display()
        )
    })?;
    let hooks_dir = settings_dir.join("hooks");
    fs::create_dir_all(&hooks_dir)
        .map_err(|err| format!("failed to create {}: {err}", hooks_dir.display()))?;

    let shim_path = hooks_dir.join("aegis-pre-tool-use.sh");
    let shim_outcome = write_executable(&shim_path, &render_claude_pre_tool_use_hook())?;

    // Resolve to an absolute path so the registered command is PATH-independent
    // even when install ran from a relative cwd (e.g. a project-local install).
    let hook_command = std::path::absolute(&shim_path)
        .map_err(|err| format!("failed to resolve absolute hook path: {err}"))?
        .to_str()
        .ok_or_else(|| "hook path is not valid UTF-8".to_string())?
        .to_owned();

    let settings_outcome = apply_installation(&mut settings, &hook_command)?;
    if matches!(settings_outcome, InstallOutcome::Installed) {
        write_settings_atomically(settings_path, &settings)?;
    }

    Ok(combine_outcomes(shim_outcome, settings_outcome))
}

/// Materialize the Claude PreToolUse hook with `__AEGIS_BIN__` replaced by an
/// absolute, shell-quoted path to the Aegis binary. Mirrors the Codex renderer
/// so both shims stay byte-identical except for the header comment.
fn render_claude_pre_tool_use_hook() -> String {
    CLAUDE_PRE_TOOL_USE_HOOK_SH.replace("__AEGIS_BIN__", &shell_quote(&resolved_aegis_bin()))
}

fn apply_installation(settings: &mut Value, hook_command: &str) -> Result<InstallOutcome, String> {
    let root = settings
        .as_object_mut()
        .ok_or_else(|| "settings.json must contain a top-level JSON object".to_string())?;

    let hooks = root
        .entry("hooks".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| "settings.hooks must be a JSON object".to_string())?;

    let pre_tool_use = hooks
        .entry("PreToolUse".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let pre_tool_use = pre_tool_use
        .as_array_mut()
        .ok_or_else(|| "settings.hooks.PreToolUse must be a JSON array".to_string())?;

    if pre_tool_use_contains_bash_aegis_hook(pre_tool_use, hook_command)? {
        return Ok(InstallOutcome::AlreadyPresent);
    }

    pre_tool_use.push(serde_json::json!({
        "matcher": "Bash",
        "hooks": [
            {
                "type": "command",
                "command": hook_command
            }
        ]
    }));

    Ok(InstallOutcome::Installed)
}

fn pre_tool_use_contains_bash_aegis_hook(
    entries: &[Value],
    expected_command: &str,
) -> Result<bool, String> {
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

            if hook_type == "command" && hook_command == expected_command {
                found = true;
            }
        }
    }

    Ok(found)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use tempfile::TempDir;

    /// A fixed absolute command used by the JSON-only `apply_installation`
    /// tests so they can exercise registration without touching the filesystem.
    const TEST_HOOK_COMMAND: &str = "/tmp/aegis-hooks/aegis-pre-tool-use.sh";

    #[test]
    fn render_claude_pre_tool_use_hook_substitutes_absolute_binary_path() {
        let rendered = render_claude_pre_tool_use_hook();

        assert!(
            !rendered.contains("__AEGIS_BIN__"),
            "placeholder must be substituted at install time, got:\n{rendered}"
        );
        let expected = format!("AEGIS_BIN={}", shell_quote(&resolved_aegis_bin()));
        assert!(
            rendered.contains(&expected),
            "rendered hook must assign the shell-quoted absolute aegis path, got:\n{rendered}"
        );
        // The transparent-rewrite shim must not reintroduce jq/python3 parsing.
        assert!(!rendered.contains("python3 -"));
        assert!(!rendered.contains("jq -"));
        assert!(rendered.contains("exec \"${AEGIS_BIN}\" hook"));
    }

    #[test]
    fn claude_install_materializes_pre_tool_use_shim() {
        let dir = TempDir::new().expect("temp dir");
        let settings_dir = dir.path().join(".claude");
        fs::create_dir_all(&settings_dir).expect("create settings dir");
        let settings_path = settings_dir.join("settings.json");
        fs::write(&settings_path, "{}\n").expect("seed settings file");

        let outcome = run_install_at_path(&settings_path).expect("install");
        assert!(matches!(outcome, InstallOutcome::Installed));

        let shim = settings_dir.join("hooks").join("aegis-pre-tool-use.sh");
        assert!(shim.exists(), "shim must be materialized at the hooks dir");
        let content = fs::read_to_string(&shim).expect("read shim");
        assert!(
            !content.contains("__AEGIS_BIN__"),
            "placeholder must be substituted in the materialized shim"
        );
        let mode = fs::metadata(&shim)
            .expect("stat shim")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o755, "materialized shim must be executable");
    }

    #[test]
    fn claude_install_registers_absolute_hook_command() {
        let dir = TempDir::new().expect("temp dir");
        let settings_dir = dir.path().join(".claude");
        fs::create_dir_all(&settings_dir).expect("create settings dir");
        let settings_path = settings_dir.join("settings.json");
        fs::write(&settings_path, "{}\n").expect("seed settings file");

        run_install_at_path(&settings_path).expect("install");

        let written = fs::read_to_string(&settings_path).expect("read settings");
        let parsed: Value = serde_json::from_str(&written).expect("parse settings");
        let command = parsed["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .expect("command string");
        assert_ne!(
            command,
            "aegis hook",
            "must not register the PATH-dependent bare command"
        );
        let expected_shim = settings_dir.join("hooks").join("aegis-pre-tool-use.sh");
        assert_eq!(
            command,
            expected_shim.display().to_string(),
            "must register the absolute shim path"
        );
        assert!(
            command.starts_with('/'),
            "registered command must be absolute"
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
        let expected_shim = settings_dir.join("hooks").join("aegis-pre-tool-use.sh");
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
                                    "command": expected_shim.display().to_string()
                                }
                            ]
                        }
                    ]
                }
            })
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
    fn local_install_can_bootstrap_project_settings_when_missing() {
        let project = TempDir::new().expect("project dir");
        let settings_path = super::super::settings_path_local(project.path());

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

        let outcome = apply_installation(&mut settings, TEST_HOOK_COMMAND).expect("first install");
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
                        "command": TEST_HOOK_COMMAND
                    }
                ]
            })
        );

        let outcome = apply_installation(&mut settings, TEST_HOOK_COMMAND).expect("second install");
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

        let outcome = apply_installation(&mut settings, TEST_HOOK_COMMAND).expect("install");
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

        let outcome = apply_installation(&mut settings, TEST_HOOK_COMMAND).expect("install");
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
                                    "command": TEST_HOOK_COMMAND
                                }
                            ]
                        }
                    ]
                }
            })
        );
    }
}
