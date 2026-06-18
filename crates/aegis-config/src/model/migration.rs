use std::fs;
use std::path::Path;

use crate::error::ConfigError;

use super::AllowlistRule;

type Result<T> = std::result::Result<T, ConfigError>;

/// Find the text bounds of a TOML array assignment `key = [...]`.
///
/// Returns `(start, end)` byte indices in `text` covering the entire
/// `key = [ ... ]` declaration, or `None` if not found.
pub(super) fn find_toml_array_bounds(text: &str, key: &str) -> Option<(usize, usize)> {
    let prefix = format!("{key} = [");
    let start = text.find(&prefix)?;
    let mut depth = 1usize;
    let mut in_string = false;
    let mut in_literal = false;
    let mut escaped = false;

    for (i, ch) in text[start + prefix.len()..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if in_literal {
            if ch == '\'' {
                in_literal = false;
            }
            continue;
        }
        if ch == '\\' && !in_literal {
            escaped = true;
            continue;
        }
        if ch == '"' && !in_string {
            in_string = true;
        } else if ch == '"' && in_string {
            in_string = false;
        } else if ch == '\'' && !in_string {
            in_literal = true;
        } else if !in_string && !in_literal {
            match ch {
                '[' => depth += 1,
                ']' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some((start, start + prefix.len() + i + 1));
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Migrate deprecated `allowlist` syntax in a config file to `[[allow]]`.
///
/// Called after parsing succeeds so the file is known-valid TOML.
/// Replaces `[[allowlist]]` with `[[allow]]` and converts `allowlist = [...]`
/// to equivalent `[[allow]]` tables.  The write is atomic (temp file + rename).
pub(super) fn migrate_deprecated_allowlist_in_file(
    contents: &str,
    path: &Path,
    migrated_rules: &[AllowlistRule],
) -> Result<()> {
    let mut new_contents = contents.to_string();
    let mut migrated = false;

    // 1. Replace deprecated table headers.
    if contents.contains("[[allowlist]]") {
        new_contents = new_contents.replace("[[allowlist]]", "[[allow]]");
        migrated = true;
    }

    // 2. Convert legacy string array to structured tables.
    if contents.contains("allowlist = [")
        && let Some((start, end)) = find_toml_array_bounds(contents, "allowlist")
    {
        let mut replacement = String::new();
        for rule in migrated_rules {
            let body = toml::to_string_pretty(rule).map_err(|error| {
                ConfigError::Config(format!("failed to serialize migrated rule: {error}"))
            })?;
            replacement.push_str(&format!("[[allow]]\n{body}"));
        }
        new_contents.replace_range(start..end, &replacement);
        migrated = true;
    }

    if migrated {
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp_path = path.with_extension(format!("tmp.{pid}.{nanos}"));
        {
            let mut tmp = fs::File::create(&tmp_path)?;
            std::io::Write::write_all(&mut tmp, new_contents.as_bytes())?;
            tmp.sync_all()?;
        }
        fs::rename(&tmp_path, path).inspect_err(|_| {
            let _ = fs::remove_file(&tmp_path);
        })?;
        tracing::info!(
            "Migrated deprecated allowlist syntax to [[allow]] in {}",
            path.display()
        );
    }

    Ok(())
}
