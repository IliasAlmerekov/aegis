use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use time::OffsetDateTime;

use crate::error::AegisError;

type Result<T> = std::result::Result<T, AegisError>;

/// Determine the active config file path for appending an allow rule.
///
/// Checks for a project-level `.aegis.toml` in the current directory first.
/// Falls back to the global config at `~/.config/aegis/config.toml`.
pub fn active_config_path_for_append() -> Option<std::path::PathBuf> {
    let current_dir = std::env::current_dir().ok()?;
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)?;
    let project = current_dir.join(".aegis.toml");
    if project.is_file() {
        return Some(project);
    }
    Some(home.join(".config/aegis").join("config.toml"))
}

/// Escape a string for safe inclusion in a TOML double-quoted value.
fn toml_escape(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() + 2);
    escaped.push('"');
    for c in s.chars() {
        match c {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c => escaped.push(c),
        }
    }
    escaped.push('"');
    escaped
}

/// Append a TOML array-of-tables entry to the config file at `config_path`.
///
/// The write is atomic (temp file + `rename`) so concurrent callers in watch
/// mode cannot corrupt the config.
///
/// If the file does not exist, it is created (including parent directories).
fn append_config_table_entry(
    config_path: &Path,
    header: &str,
    fields: &[(String, String)],
) -> Result<()> {
    let mut content = if config_path.exists() {
        fs::read_to_string(config_path)?
    } else {
        String::new()
    };

    let mut fragment = format!("\n{header}\n");
    for (key, value) in fields {
        fragment.push_str(&format!("{key} = {}\n", toml_escape(value)));
    }
    content.push_str(&fragment);

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let tmp_path = config_path.with_extension(format!("tmp.{pid}.{nanos}"));
    {
        let mut tmp = fs::File::create(&tmp_path)?;
        tmp.write_all(content.as_bytes())?;
        tmp.sync_all()?;
    }
    fs::rename(&tmp_path, config_path)?;

    Ok(())
}

/// Append an `[[allowlist]]` rule to the config file at `config_path`.
pub fn append_allow_rule(config_path: &Path, prefix: &[String], cwd: &Path) -> Result<()> {
    let pattern = prefix.join(" ");
    let reason = allow_reason_for_date(OffsetDateTime::now_utc());
    let cwd_str = cwd.to_string_lossy().into_owned();

    append_config_table_entry(
        config_path,
        "[[allowlist]]",
        &[
            ("pattern".to_string(), pattern),
            ("cwd".to_string(), cwd_str),
            ("reason".to_string(), reason),
        ],
    )
}

/// Append a `[[block]]` rule to the config file at `config_path`.
pub fn append_block_rule(config_path: &Path, prefix: &[String], cwd: &Path) -> Result<()> {
    let pattern = prefix.join(" ");
    let reason = block_reason_for_date(OffsetDateTime::now_utc());
    let cwd_str = cwd.to_string_lossy().into_owned();

    append_config_table_entry(
        config_path,
        "[[block]]",
        &[
            ("pattern".to_string(), pattern),
            ("cwd".to_string(), cwd_str),
            ("reason".to_string(), reason),
        ],
    )
}

/// Build the human-readable reason string for an auto-appended allow rule.
pub(crate) fn allow_reason_for_date(date: OffsetDateTime) -> String {
    let date = date.date();
    format!(
        "Approved by user on {:04}-{:02}-{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    )
}

/// Build the human-readable reason string for an auto-appended block rule.
pub(crate) fn block_reason_for_date(date: OffsetDateTime) -> String {
    let date = date.date();
    format!(
        "Blocked by user on {:04}-{:02}-{:02}",
        date.year(),
        date.month() as u8,
        date.day()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn append_allow_rule_adds_allowlist_entry() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "mode = \"Protect\"\n").unwrap();

        append_allow_rule(&path, &["git".to_string(), "push".to_string()], dir.path()).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert!(
            contents.contains("[[allowlist]]"),
            "must add array-of-tables header; got:\n{contents}"
        );
        assert!(
            contents.contains("pattern = \"git push\""),
            "must add joined pattern string; got:\n{contents}"
        );
        assert!(
            contents.contains("cwd = "),
            "must add cwd scope; got:\n{contents}"
        );
        assert!(
            contents.contains("reason = \"Approved by user on"),
            "must add reason with date; got:\n{contents}"
        );
    }

    #[test]
    fn append_allow_rule_creates_file_if_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");

        append_allow_rule(&path, &["rm".to_string(), "-rf".to_string()], dir.path()).unwrap();

        assert!(path.exists(), "must create missing config file");
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("[[allowlist]]"));
    }

    #[test]
    fn append_allow_rule_preserves_existing_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "mode = \"Protect\"\nauto_snapshot_git = false\n").unwrap();

        append_allow_rule(
            &path,
            &[
                "docker".to_string(),
                "system".to_string(),
                "prune".to_string(),
            ],
            dir.path(),
        )
        .unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert!(
            contents.contains("mode = \"Protect\""),
            "must preserve existing config fields; got:\n{contents}"
        );
        assert!(
            contents.contains("auto_snapshot_git = false"),
            "must preserve existing config fields; got:\n{contents}"
        );
    }

    #[test]
    fn append_allow_rule_uses_today_date_in_reason() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");

        append_allow_rule(
            &path,
            &["git".to_string(), "status".to_string()],
            dir.path(),
        )
        .unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let today = OffsetDateTime::now_utc().date();
        let expected_fragment = format!(
            "{:04}-{:02}-{:02}",
            today.year(),
            today.month() as u8,
            today.day()
        );
        assert!(
            contents.contains(&expected_fragment),
            "reason must contain today's date ({expected_fragment}); got:\n{contents}"
        );
    }

    #[test]
    fn append_allow_rule_appends_to_existing_allowlist_entries() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let existing = r#"mode = "Protect"

[[allowlist]]
pattern = "git status"
cwd = "/srv/infra"
reason = "Approved by user on 2025-01-01"
"#;
        fs::write(&path, existing).unwrap();

        append_allow_rule(&path, &["git".to_string(), "push".to_string()], dir.path()).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let allowlist_count = contents.matches("[[allowlist]]").count();
        assert_eq!(
            allowlist_count, 2,
            "must append a second [[allowlist]] entry; got:\n{contents}"
        );
    }

    #[test]
    fn append_allow_rule_uses_cwd_from_argument() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");

        append_allow_rule(
            &path,
            &["terraform".to_string(), "destroy".to_string()],
            dir.path(),
        )
        .unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let parsed: toml::Value = toml::from_str(&contents).unwrap();
        let cwd = parsed["allowlist"][0]["cwd"].as_str().unwrap();
        assert_eq!(cwd, dir.path().to_string_lossy().as_ref());
    }

    #[test]
    fn append_block_rule_adds_block_entry() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "mode = \"Protect\"\n").unwrap();

        append_block_rule(
            &path,
            &["rm".to_string(), "-rf".to_string(), "/".to_string()],
            dir.path(),
        )
        .unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert!(
            contents.contains("[[block]]"),
            "must add array-of-tables header; got:\n{contents}"
        );
        assert!(
            contents.contains("pattern = \"rm -rf /\""),
            "must add joined pattern string; got:\n{contents}"
        );
        assert!(
            contents.contains("cwd = "),
            "must add cwd scope; got:\n{contents}"
        );
        assert!(
            contents.contains("reason = \"Blocked by user on"),
            "must add reason with date; got:\n{contents}"
        );
    }

    #[test]
    fn append_block_rule_creates_file_if_missing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");

        append_block_rule(&path, &["rm".to_string(), "-rf".to_string()], dir.path()).unwrap();

        assert!(path.exists(), "must create missing config file");
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("[[block]]"));
    }

    #[test]
    fn block_reason_for_date_formats_correctly() {
        let date = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let reason = block_reason_for_date(date);
        assert!(
            reason.contains("2023-11-14"),
            "reason must contain the date in ISO format; got: {reason}"
        );
        assert!(
            reason.starts_with("Blocked by user on "),
            "reason must start with 'Blocked by user on'; got: {reason}"
        );
    }
}
