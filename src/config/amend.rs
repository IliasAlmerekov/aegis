use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use time::OffsetDateTime;

use crate::config::AegisConfig;
use crate::config::allowlist::ConfigSourceLayer;
use crate::error::AegisError;

type Result<T> = std::result::Result<T, AegisError>;

/// Which table an append targets.
enum TableKind {
    Allowlist,
    Blocklist,
}

/// Outcome of appending an allowlist or blocklist rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendOutcome {
    /// The rule was appended to the config file.
    Appended,
    /// An exact duplicate was already present in the same table; append was skipped.
    SkippedDuplicate,
    /// A conflicting rule exists in the opposite table.
    Conflict {
        /// The pattern that caused the conflict.
        pattern: String,
        /// Config layer where the existing conflicting rule was found.
        existing_location: ConfigSourceLayer,
    },
}

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

/// Derive the config source layer from the path used for appending.
fn config_layer_from_path(config_path: &Path) -> ConfigSourceLayer {
    if config_path.file_name().is_some_and(|n| n == ".aegis.toml") {
        ConfigSourceLayer::Project
    } else {
        ConfigSourceLayer::Global
    }
}

/// Check whether appending a rule to `target_table` would create a duplicate
/// or conflict against the parsed `config`.
///
/// Returns `Some(AppendOutcome)` when the caller should return early
/// (`SkippedDuplicate` or `Conflict`), or `None` when the append may proceed.
///
/// Duplicate detection requires `user.is_none()` because only auto-written
/// rules (no explicit user scope) are subject to deduplication.
/// Conflict detection ignores `user` entirely: a rule with the same pattern
/// and cwd in the opposite table is always reported as a conflict regardless
/// of user scope.
fn check_dedup_and_conflict(
    config: &AegisConfig,
    pattern: &str,
    cwd: &str,
    target_table: TableKind,
    location: ConfigSourceLayer,
) -> Option<AppendOutcome> {
    match target_table {
        TableKind::Allowlist => {
            if config
                .allowlist
                .iter()
                .any(|r| r.pattern == pattern && r.cwd.as_deref() == Some(cwd) && r.user.is_none())
            {
                return Some(AppendOutcome::SkippedDuplicate);
            }
            if config
                .blocklist
                .iter()
                .any(|r| r.pattern == pattern && r.cwd.as_deref() == Some(cwd))
            {
                return Some(AppendOutcome::Conflict {
                    pattern: pattern.to_string(),
                    existing_location: location,
                });
            }
        }
        TableKind::Blocklist => {
            if config
                .blocklist
                .iter()
                .any(|r| r.pattern == pattern && r.cwd.as_deref() == Some(cwd) && r.user.is_none())
            {
                return Some(AppendOutcome::SkippedDuplicate);
            }
            if config
                .allowlist
                .iter()
                .any(|r| r.pattern == pattern && r.cwd.as_deref() == Some(cwd))
            {
                return Some(AppendOutcome::Conflict {
                    pattern: pattern.to_string(),
                    existing_location: location,
                });
            }
        }
    }
    None
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
/// If `existing_content` is `Some`, it is used directly (avoids a second read
/// when the caller has already parsed the file for deduplication).
/// If the file does not exist, it is created (including parent directories).
fn append_config_table_entry(
    config_path: &Path,
    existing_content: Option<&str>,
    header: &str,
    fields: &[(String, String)],
) -> Result<()> {
    let mut content = if let Some(c) = existing_content {
        c.to_string()
    } else if config_path.exists() {
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
pub fn append_allow_rule(
    config_path: &Path,
    prefix: &[String],
    cwd: &Path,
) -> Result<AppendOutcome> {
    let pattern = prefix.join(" ");
    let cwd_str = cwd.to_string_lossy().into_owned();
    let mut existing_content: Option<String> = None;

    if config_path.exists() {
        let contents = fs::read_to_string(config_path)?;
        let config: AegisConfig = toml::from_str(&contents)
            .map_err(|e| AegisError::Config(format!("failed to parse config: {e}")))?;

        if let Some(outcome) = check_dedup_and_conflict(
            &config,
            &pattern,
            &cwd_str,
            TableKind::Allowlist,
            config_layer_from_path(config_path),
        ) {
            return Ok(outcome);
        }

        existing_content = Some(contents);
    }

    let reason = allow_reason_for_date(OffsetDateTime::now_utc());

    append_config_table_entry(
        config_path,
        existing_content.as_deref(),
        "[[allowlist]]",
        &[
            ("pattern".to_string(), pattern),
            ("cwd".to_string(), cwd_str),
            ("reason".to_string(), reason),
        ],
    )?;

    Ok(AppendOutcome::Appended)
}

/// Append a `[[block]]` rule to the config file at `config_path`.
pub fn append_block_rule(
    config_path: &Path,
    prefix: &[String],
    cwd: &Path,
) -> Result<AppendOutcome> {
    let pattern = prefix.join(" ");
    let cwd_str = cwd.to_string_lossy().into_owned();
    let mut existing_content: Option<String> = None;

    if config_path.exists() {
        let contents = fs::read_to_string(config_path)?;
        let config: AegisConfig = toml::from_str(&contents)
            .map_err(|e| AegisError::Config(format!("failed to parse config: {e}")))?;

        if let Some(outcome) = check_dedup_and_conflict(
            &config,
            &pattern,
            &cwd_str,
            TableKind::Blocklist,
            config_layer_from_path(config_path),
        ) {
            return Ok(outcome);
        }

        existing_content = Some(contents);
    }

    let reason = block_reason_for_date(OffsetDateTime::now_utc());

    append_config_table_entry(
        config_path,
        existing_content.as_deref(),
        "[[block]]",
        &[
            ("pattern".to_string(), pattern),
            ("cwd".to_string(), cwd_str),
            ("reason".to_string(), reason),
        ],
    )?;

    Ok(AppendOutcome::Appended)
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

    #[test]
    fn append_allow_rule_skips_exact_duplicate_in_allowlist() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let existing = format!(
            r#"[[allowlist]]
pattern = "git push"
cwd = "{}"
reason = "Approved by user on 2025-01-01"
"#,
            dir.path().to_string_lossy()
        );
        fs::write(&path, &existing).unwrap();

        let outcome =
            append_allow_rule(&path, &["git".to_string(), "push".to_string()], dir.path()).unwrap();
        assert_eq!(
            outcome,
            AppendOutcome::SkippedDuplicate,
            "must skip exact duplicate in allowlist"
        );

        let contents = fs::read_to_string(&path).unwrap();
        let allowlist_count = contents.matches("[[allowlist]]").count();
        assert_eq!(
            allowlist_count, 1,
            "must not append duplicate; got:\n{contents}"
        );
    }

    #[test]
    fn append_allow_rule_warns_when_same_pattern_exists_in_blocklist() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let existing = format!(
            r#"[[block]]
pattern = "git push"
cwd = "{}"
reason = "Blocked by user on 2025-01-01"
"#,
            dir.path().to_string_lossy()
        );
        fs::write(&path, &existing).unwrap();

        let outcome =
            append_allow_rule(&path, &["git".to_string(), "push".to_string()], dir.path()).unwrap();
        assert!(
            matches!(
                outcome,
                AppendOutcome::Conflict {
                    pattern: ref p,
                    ..
                } if p == "git push"
            ),
            "must return conflict warning; got: {outcome:?}"
        );

        let contents = fs::read_to_string(&path).unwrap();
        let allowlist_count = contents.matches("[[allowlist]]").count();
        assert_eq!(
            allowlist_count, 0,
            "must not append on conflict; got:\n{contents}"
        );
    }

    #[test]
    fn append_block_rule_skips_exact_duplicate_in_blocklist() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let existing = format!(
            r#"[[block]]
pattern = "rm -rf /"
cwd = "{}"
reason = "Blocked by user on 2025-01-01"
"#,
            dir.path().to_string_lossy()
        );
        fs::write(&path, &existing).unwrap();

        let outcome = append_block_rule(
            &path,
            &["rm".to_string(), "-rf".to_string(), "/".to_string()],
            dir.path(),
        )
        .unwrap();
        assert_eq!(
            outcome,
            AppendOutcome::SkippedDuplicate,
            "must skip exact duplicate in blocklist"
        );

        let contents = fs::read_to_string(&path).unwrap();
        let block_count = contents.matches("[[block]]").count();
        assert_eq!(
            block_count, 1,
            "must not append duplicate; got:\n{contents}"
        );
    }

    #[test]
    fn append_block_rule_warns_when_same_pattern_exists_in_allowlist() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let existing = format!(
            r#"[[allowlist]]
pattern = "rm -rf /"
cwd = "{}"
reason = "Approved by user on 2025-01-01"
"#,
            dir.path().to_string_lossy()
        );
        fs::write(&path, &existing).unwrap();

        let outcome = append_block_rule(
            &path,
            &["rm".to_string(), "-rf".to_string(), "/".to_string()],
            dir.path(),
        )
        .unwrap();
        assert!(
            matches!(
                outcome,
                AppendOutcome::Conflict {
                    pattern: ref p,
                    ..
                } if p == "rm -rf /"
            ),
            "must return conflict warning; got: {outcome:?}"
        );

        let contents = fs::read_to_string(&path).unwrap();
        let block_count = contents.matches("[[block]]").count();
        assert_eq!(
            block_count, 0,
            "must not append on conflict; got:\n{contents}"
        );
    }

    #[test]
    fn append_non_conflicting_rules_still_appends_normally() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let existing = format!(
            r#"[[allowlist]]
pattern = "git status"
cwd = "{}"
reason = "Approved by user on 2025-01-01"

[[block]]
pattern = "rm -rf /"
cwd = "{}"
reason = "Blocked by user on 2025-01-01"
"#,
            dir.path().to_string_lossy(),
            dir.path().to_string_lossy()
        );
        fs::write(&path, &existing).unwrap();

        let outcome =
            append_allow_rule(&path, &["git".to_string(), "push".to_string()], dir.path()).unwrap();
        assert_eq!(
            outcome,
            AppendOutcome::Appended,
            "must append non-conflicting rule; got: {outcome:?}"
        );

        let contents = fs::read_to_string(&path).unwrap();
        let allowlist_count = contents.matches("[[allowlist]]").count();
        assert_eq!(
            allowlist_count, 2,
            "must append non-conflicting rule; got:\n{contents}"
        );
    }

    #[test]
    fn conflict_location_reports_project() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".aegis.toml");
        let existing = format!(
            r#"[[block]]
pattern = "git push"
cwd = "{}"
reason = "Blocked by user on 2025-01-01"
"#,
            dir.path().to_string_lossy()
        );
        fs::write(&path, &existing).unwrap();

        let outcome =
            append_allow_rule(&path, &["git".to_string(), "push".to_string()], dir.path()).unwrap();
        assert!(
            matches!(
                outcome,
                AppendOutcome::Conflict {
                    existing_location: ConfigSourceLayer::Project,
                    ..
                }
            ),
            "must report project location; got: {outcome:?}"
        );
    }

    #[test]
    fn conflict_location_reports_global() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let existing = format!(
            r#"[[block]]
pattern = "git push"
cwd = "{}"
reason = "Blocked by user on 2025-01-01"
"#,
            dir.path().to_string_lossy()
        );
        fs::write(&path, &existing).unwrap();

        let outcome =
            append_allow_rule(&path, &["git".to_string(), "push".to_string()], dir.path()).unwrap();
        assert!(
            matches!(
                outcome,
                AppendOutcome::Conflict {
                    existing_location: ConfigSourceLayer::Global,
                    ..
                }
            ),
            "must report global location; got: {outcome:?}"
        );
    }

    #[test]
    fn same_pattern_different_cwd_is_not_duplicate() {
        let dir_a = TempDir::new().unwrap();
        let dir_b = TempDir::new().unwrap();
        let path = dir_a.path().join("config.toml");
        let existing = format!(
            r#"[[allowlist]]
pattern = "git push"
cwd = "{}"
reason = "Approved by user on 2025-01-01"
"#,
            dir_a.path().to_string_lossy()
        );
        fs::write(&path, &existing).unwrap();

        let outcome = append_allow_rule(
            &path,
            &["git".to_string(), "push".to_string()],
            dir_b.path(),
        )
        .unwrap();
        assert_eq!(
            outcome,
            AppendOutcome::Appended,
            "same pattern with different cwd must NOT be treated as duplicate; got: {outcome:?}"
        );

        let contents = fs::read_to_string(&path).unwrap();
        let allowlist_count = contents.matches("[[allowlist]]").count();
        assert_eq!(
            allowlist_count, 2,
            "must append a second [[allowlist]] entry for different cwd; got:\n{contents}"
        );
    }
}
