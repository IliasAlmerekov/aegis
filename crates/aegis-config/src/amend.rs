//! Helpers for appending rules to config files interactively.

use std::fs;
use std::path::Path;

use time::OffsetDateTime;

use crate::AegisConfig;
use crate::allowlist::ConfigSourceLayer;
use crate::error::ConfigError;

type Result<T> = std::result::Result<T, ConfigError>;

mod formatting;
mod validation;

use formatting::{allow_reason_for_date, append_config_table_entry, block_reason_for_date};
use validation::{check_dedup_and_conflict, config_layer_from_path};

/// Which table an append targets.
#[derive(Clone, Copy)]
enum TableKind {
    Allow,
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

fn append_rule(
    config_path: &Path,
    prefix: &[String],
    cwd: &Path,
    kind: TableKind,
) -> Result<AppendOutcome> {
    let pattern = prefix.join(" ");
    let cwd_str = cwd.to_string_lossy().into_owned();
    let mut existing_content: Option<String> = None;

    if config_path.exists() {
        let contents = fs::read_to_string(config_path)?;
        let config: AegisConfig = toml::from_str(&contents)
            .map_err(|e| ConfigError::Config(format!("failed to parse config: {e}")))?;

        if let Some(outcome) = check_dedup_and_conflict(
            &config,
            &pattern,
            &cwd_str,
            kind,
            config_layer_from_path(config_path),
        ) {
            return Ok(outcome);
        }

        existing_content = Some(contents);
    }

    let (header, reason) = match kind {
        TableKind::Allow => (
            "[[allow]]",
            allow_reason_for_date(OffsetDateTime::now_utc()),
        ),
        TableKind::Blocklist => (
            "[[block]]",
            block_reason_for_date(OffsetDateTime::now_utc()),
        ),
    };

    append_config_table_entry(
        config_path,
        existing_content.as_deref(),
        header,
        &[
            ("pattern".to_string(), pattern),
            ("cwd".to_string(), cwd_str),
            ("reason".to_string(), reason),
        ],
    )?;

    Ok(AppendOutcome::Appended)
}

/// Append an `[[allow]]` rule to the config file at `config_path`.
pub fn append_allow_rule(
    config_path: &Path,
    prefix: &[String],
    cwd: &Path,
) -> Result<AppendOutcome> {
    append_rule(config_path, prefix, cwd, TableKind::Allow)
}

/// Append a `[[block]]` rule to the config file at `config_path`.
pub fn append_block_rule(
    config_path: &Path,
    prefix: &[String],
    cwd: &Path,
) -> Result<AppendOutcome> {
    append_rule(config_path, prefix, cwd, TableKind::Blocklist)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    type TomlTables<'a> = Vec<(&'a str, Vec<(&'a str, &'a str, &'a str)>)>;

    #[test]
    fn append_allow_rule_adds_allow_header() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "mode = \"Protect\"\n").unwrap();

        append_allow_rule(&path, &["git".to_string(), "push".to_string()], dir.path()).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert!(
            contents.contains("[[allow]]"),
            "must add array-of-tables header; got:\n{contents}"
        );
    }

    #[test]
    fn append_allow_rule_adds_pattern() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "mode = \"Protect\"\n").unwrap();

        append_allow_rule(&path, &["git".to_string(), "push".to_string()], dir.path()).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert!(
            contents.contains("pattern = \"git push\""),
            "must add joined pattern string; got:\n{contents}"
        );
    }

    #[test]
    fn append_allow_rule_adds_cwd() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "mode = \"Protect\"\n").unwrap();

        append_allow_rule(&path, &["git".to_string(), "push".to_string()], dir.path()).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert!(
            contents.contains("cwd = "),
            "must add cwd scope; got:\n{contents}"
        );
    }

    #[test]
    fn append_allow_rule_adds_reason() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "mode = \"Protect\"\n").unwrap();

        append_allow_rule(&path, &["git".to_string(), "push".to_string()], dir.path()).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
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
        assert!(contents.contains("[[allow]]"));
    }

    #[test]
    fn append_allow_rule_preserves_mode() {
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
            "must preserve existing mode; got:\n{contents}"
        );
    }

    #[test]
    fn append_allow_rule_preserves_auto_snapshot_git() {
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
            contents.contains("auto_snapshot_git = false"),
            "must preserve existing auto_snapshot_git; got:\n{contents}"
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

[[allow]]
pattern = "git status"
cwd = "/srv/infra"
reason = "Approved by user on 2025-01-01"
"#;
        fs::write(&path, existing).unwrap();

        append_allow_rule(&path, &["git".to_string(), "push".to_string()], dir.path()).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let allowlist_count = contents.matches("[[allow]]").count();
        assert_eq!(
            allowlist_count, 2,
            "must append a second [[allow]] entry; got:\n{contents}"
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
        let cwd = parsed["allow"][0]["cwd"].as_str().unwrap();
        assert_eq!(cwd, dir.path().to_string_lossy().as_ref());
    }

    #[test]
    fn append_block_rule_adds_block_header() {
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
    }

    #[test]
    fn append_block_rule_adds_pattern() {
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
            contents.contains("pattern = \"rm -rf /\""),
            "must add joined pattern string; got:\n{contents}"
        );
    }

    #[test]
    fn append_block_rule_adds_cwd() {
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
            contents.contains("cwd = "),
            "must add cwd scope; got:\n{contents}"
        );
    }

    #[test]
    fn append_block_rule_adds_reason() {
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

    fn make_toml_config(tables: TomlTables<'_>) -> String {
        let mut doc = toml::value::Table::new();
        for (table_name, entries) in tables {
            let array: Vec<toml::Value> = entries
                .into_iter()
                .map(|(pattern, cwd, reason)| {
                    let mut map = toml::value::Table::new();
                    map.insert(
                        "pattern".to_string(),
                        toml::Value::String(pattern.to_string()),
                    );
                    map.insert("cwd".to_string(), toml::Value::String(cwd.to_string()));
                    map.insert(
                        "reason".to_string(),
                        toml::Value::String(reason.to_string()),
                    );
                    toml::Value::Table(map)
                })
                .collect();
            doc.insert(table_name.to_string(), toml::Value::Array(array));
        }
        toml::to_string(&doc).unwrap()
    }

    #[test]
    fn append_allow_rule_skips_exact_duplicate_in_allowlist() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let existing = make_toml_config(vec![(
            "allow",
            vec![(
                "git push",
                &dir.path().to_string_lossy(),
                "Approved by user on 2025-01-01",
            )],
        )]);
        fs::write(&path, &existing).unwrap();

        let outcome =
            append_allow_rule(&path, &["git".to_string(), "push".to_string()], dir.path()).unwrap();
        assert_eq!(
            outcome,
            AppendOutcome::SkippedDuplicate,
            "must skip exact duplicate in allowlist"
        );

        let contents = fs::read_to_string(&path).unwrap();
        let allowlist_count = contents.matches("[[allow]]").count();
        assert_eq!(
            allowlist_count, 1,
            "must not append duplicate; got:\n{contents}"
        );
    }

    #[test]
    fn append_allow_rule_warns_when_same_pattern_exists_in_blocklist() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let existing = make_toml_config(vec![(
            "block",
            vec![(
                "git push",
                &dir.path().to_string_lossy(),
                "Blocked by user on 2025-01-01",
            )],
        )]);
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
        let allowlist_count = contents.matches("[[allow]]").count();
        assert_eq!(
            allowlist_count, 0,
            "must not append on conflict; got:\n{contents}"
        );
    }

    #[test]
    fn append_block_rule_skips_exact_duplicate_in_blocklist() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let existing = make_toml_config(vec![(
            "block",
            vec![(
                "rm -rf /",
                &dir.path().to_string_lossy(),
                "Blocked by user on 2025-01-01",
            )],
        )]);
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
        let existing = make_toml_config(vec![(
            "allow",
            vec![(
                "rm -rf /",
                &dir.path().to_string_lossy(),
                "Approved by user on 2025-01-01",
            )],
        )]);
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
        let cwd = dir.path().to_string_lossy().to_string();
        let existing = make_toml_config(vec![
            (
                "allow",
                vec![("git status", &cwd, "Approved by user on 2025-01-01")],
            ),
            (
                "block",
                vec![("rm -rf /", &cwd, "Blocked by user on 2025-01-01")],
            ),
        ]);
        fs::write(&path, &existing).unwrap();

        let outcome =
            append_allow_rule(&path, &["git".to_string(), "push".to_string()], dir.path()).unwrap();
        assert_eq!(
            outcome,
            AppendOutcome::Appended,
            "must append non-conflicting rule; got: {outcome:?}"
        );

        let contents = fs::read_to_string(&path).unwrap();
        let allowlist_count = contents.matches("[[allow]]").count();
        assert_eq!(
            allowlist_count, 2,
            "must append non-conflicting rule; got:\n{contents}"
        );
    }

    #[test]
    fn conflict_location_reports_project() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".aegis.toml");
        let existing = make_toml_config(vec![(
            "block",
            vec![(
                "git push",
                &dir.path().to_string_lossy(),
                "Blocked by user on 2025-01-01",
            )],
        )]);
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
        let existing = make_toml_config(vec![(
            "block",
            vec![(
                "git push",
                &dir.path().to_string_lossy(),
                "Blocked by user on 2025-01-01",
            )],
        )]);
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
        let existing = make_toml_config(vec![(
            "allow",
            vec![(
                "git push",
                &dir_a.path().to_string_lossy(),
                "Approved by user on 2025-01-01",
            )],
        )]);
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
        let allowlist_count = contents.matches("[[allow]]").count();
        assert_eq!(
            allowlist_count, 2,
            "must append a second [[allow]] entry for different cwd; got:\n{contents}"
        );
    }
}
