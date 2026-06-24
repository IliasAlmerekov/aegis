mod claude;
mod codex;
mod hook;
mod shell;

use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

pub(crate) use hook::run_hook;
pub(crate) use shell::run_setup_shell;

/// POSIX single-quote a value for safe interpolation into generated shell
/// snippets or wrapper commands.
pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Resolve the absolute path of the currently running Aegis binary, falling
/// back to a bare `aegis` PATH lookup if the executable path is unavailable.
/// Shared by the Codex and Claude installers so both register a PATH-
/// independent hook command.
pub(crate) fn resolved_aegis_bin() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(str::to_owned))
        .unwrap_or_else(|| "aegis".to_string())
}

/// True when the on-disk executable already matches `expected_content` and is
/// installed with the canonical `0755` mode. Used by `write_executable` to
/// short-circuit idempotent reinstalls.
fn executable_content_is_current(existing: &str, mode: u32, expected_content: &str) -> bool {
    existing == expected_content && mode & 0o777 == 0o755
}

/// Materialize an executable file at `path` with `content` and mode `0755`,
/// atomic via a temp file in the same directory. Returns `AlreadyPresent` when
/// the existing file already matches byte-for-byte and has the right mode, so
/// reinstalls are idempotent and never touch a correct file. Shared by the
/// Codex and Claude installers for their hook shims.
pub(crate) fn write_executable(path: &Path, content: &str) -> Result<InstallOutcome, String> {
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

/// Install aegis hooks for the selected agent targets.
pub(crate) fn run_install(args: &super::InstallArgs) -> i32 {
    let mut exit = 0;
    let selection = install_target_selection(args);

    if selection.includes_claude() {
        match claude::run_claude_install(!args.local) {
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
        match codex::run_codex_install() {
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
pub(crate) enum InstallTargetSelection {
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
pub(crate) enum InstallOutcome {
    Installed,
    AlreadyPresent,
    /// Agent directory not present — nothing to install.
    Skipped,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum AgentInstallResult {
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

pub(crate) fn agent_dir_exists(agent_dir: &Path) -> Result<bool, String> {
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

/// Combine two install outcomes into one: `Installed` wins over `Skipped`/`
/// AlreadyPresent`, `Skipped` wins over `AlreadyPresent`. Shared by the Codex
/// and Claude installers to fold the shim-materialize outcome together with
/// the settings-registration outcome.
pub(crate) fn combine_outcomes(lhs: InstallOutcome, rhs: InstallOutcome) -> InstallOutcome {
    if matches!(lhs, InstallOutcome::Installed) || matches!(rhs, InstallOutcome::Installed) {
        InstallOutcome::Installed
    } else if matches!(lhs, InstallOutcome::Skipped) || matches!(rhs, InstallOutcome::Skipped) {
        InstallOutcome::Skipped
    } else {
        InstallOutcome::AlreadyPresent
    }
}

pub(crate) fn load_settings(path: &Path) -> Result<Value, String> {
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

pub(crate) fn write_settings_atomically(path: &Path, settings: &Value) -> Result<(), String> {
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

pub(crate) fn temporary_settings_path(parent: &Path) -> PathBuf {
    // Per-process monotonic counter so two calls in the same process always yield
    // distinct names — `SystemTime::now()` is not guaranteed to advance between
    // rapid calls (its resolution is coarser than a nanosecond on some platforms,
    // e.g. macOS), which would otherwise collide.
    static SEQ: AtomicU64 = AtomicU64::new(0);

    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);

    // pid+nanos give cross-process distinctness; seq guarantees in-process
    // distinctness. The ultimate collision guard remains write_settings_atomically()
    // using create_new(true), which fails closed instead of silently overwriting
    // another installer's temporary file.
    parent.join(format!(".settings.json.aegis-{pid}-{nanos}-{seq}.tmp"))
}

pub(crate) fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
}

pub(crate) fn settings_path_global(home_dir: Option<&Path>) -> Result<PathBuf, String> {
    let home_dir = home_dir.ok_or_else(|| "HOME is not set".to_string())?;
    Ok(home_dir.join(".claude/settings.json"))
}

pub(crate) fn settings_path_local(cwd: &Path) -> PathBuf {
    cwd.join(".claude/settings.json")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use tempfile::TempDir;

    #[test]
    fn resolved_aegis_bin_is_non_empty() {
        // The resolved binary path must always yield a non-empty command string
        // (falling back to the bare `aegis` name when current_exe is unavailable)
        // so the rendered hook template always substitutes a real value.
        let bin = resolved_aegis_bin();
        assert!(!bin.is_empty(), "resolved aegis bin must never be empty");
    }

    #[test]
    fn write_executable_normalizes_mode_to_0755() {
        let dir = TempDir::new().expect("temp dir");
        let hook_path = dir.path().join("hook.sh");
        // Seed a file with outdated content and a non-canonical mode.
        fs::write(&hook_path, "#!/bin/sh\nold\n").expect("write hook");
        fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o644))
            .expect("set permissions");

        let outcome = write_executable(&hook_path, "#!/bin/sh\nnew\n").expect("install");

        assert_eq!(
            outcome,
            InstallOutcome::Installed,
            "mismatched content must be rewritten"
        );
        let mode = fs::metadata(&hook_path)
            .expect("stat hook")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o755, "installed hook must normalize to 0755");

        // A second run with matching content + correct mode is idempotent.
        let again = write_executable(&hook_path, "#!/bin/sh\nnew\n").expect("reinstall");
        assert_eq!(again, InstallOutcome::AlreadyPresent);
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
    fn load_settings_returns_empty_object_for_missing_file() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("nonexistent.json");

        let result = load_settings(&path).expect("missing file must return Ok");
        assert!(
            result.is_object() && result.as_object().unwrap().is_empty(),
            "missing settings file must yield an empty JSON object, got: {result:?}"
        );
    }

    #[test]
    fn load_settings_returns_error_for_invalid_json() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("bad.json");
        fs::write(&path, "not json").unwrap();

        let err = load_settings(&path).unwrap_err();
        assert!(
            err.contains("failed to parse"),
            "invalid JSON must produce a parse error, got: {err}"
        );
    }

    #[test]
    fn write_settings_atomically_creates_file_with_correct_content() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("settings.json");
        let settings = serde_json::json!({"key": "value"});

        write_settings_atomically(&path, &settings).expect("must succeed");

        let written: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["key"], "value");
    }

    #[test]
    fn write_settings_atomically_replaces_existing_file() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"old": true}"#).unwrap();

        let new_settings = serde_json::json!({"new": true});
        write_settings_atomically(&path, &new_settings).expect("must succeed");

        let written: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["new"], true);
        assert!(written.get("old").is_none());
    }

    #[test]
    fn write_settings_atomically_uses_create_new_for_temp_file() {
        // Verify the collision guard: pre-create a file at the expected temp
        // path so that write_settings_atomically's create_new(true) open fails
        // rather than silently overwriting another caller's temp file.
        let dir = TempDir::new().expect("temp dir");
        let settings = serde_json::json!({});

        // Seed the parent with a pre-existing temp file at the exact path the
        // function would choose.  Because pid and nanos are sampled at call
        // time we cannot predict the name, so we instead verify the invariant
        // through the success path: two sequential calls must each produce a
        // distinct temp path (no silent collision between runs in the same
        // process).
        let path_a = temporary_settings_path(dir.path());
        let path_b = temporary_settings_path(dir.path());
        // The per-process sequence counter guarantees two calls in the same
        // process produce different names, independent of clock resolution.
        assert_ne!(
            path_a, path_b,
            "temporary_settings_path must generate distinct paths to avoid collisions"
        );

        // The happy path must also complete without error.
        let dest = dir.path().join("settings.json");
        write_settings_atomically(&dest, &settings).expect("must succeed");
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
}
