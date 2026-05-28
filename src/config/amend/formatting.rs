//! TOML formatting and atomic config file append helpers.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use time::OffsetDateTime;

use crate::error::AegisError;

type Result<T> = std::result::Result<T, AegisError>;

/// Escape a string for safe inclusion in a TOML double-quoted value.
pub(super) fn toml_escape(s: &str) -> String {
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
pub(super) fn append_config_table_entry(
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
    fs::rename(&tmp_path, config_path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp_path);
    })?;

    Ok(())
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
