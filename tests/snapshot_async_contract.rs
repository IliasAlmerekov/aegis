use std::fs;
use std::path::Path;

const SNAPSHOT_PLUGINS: &[(&str, &str)] = &[
    ("crates/aegis-snapshot/src/git.rs", "GitPlugin"),
    ("crates/aegis-snapshot/src/docker/mod.rs", "DockerPlugin"),
    (
        "crates/aegis-snapshot/src/postgres/mod.rs",
        "PostgresPlugin",
    ),
    ("crates/aegis-snapshot/src/mysql/mod.rs", "MysqlPlugin"),
    ("crates/aegis-snapshot/src/sqlite.rs", "SqlitePlugin"),
    (
        "crates/aegis-snapshot/src/supabase/runtime/mod.rs",
        "SupabasePlugin",
    ),
];

const RETRY_HELPERS: &[(&str, &str)] = &[
    (
        "crates/aegis-snapshot/src/docker/mod.rs",
        "async fn sleep_docker_busy_retry_delay",
    ),
    (
        "crates/aegis-snapshot/src/postgres/mod.rs",
        "async fn output_with_busy_retry",
    ),
    (
        "crates/aegis-snapshot/src/mysql/mod.rs",
        "async fn spawn_with_busy_retry",
    ),
];

#[test]
fn test_snapshot_plugins_do_not_block_tokio_runtime_contract()
-> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut failures = Vec::new();

    for (relative_path, plugin_type) in SNAPSHOT_PLUGINS {
        let source = fs::read_to_string(manifest_dir.join(relative_path))?;
        let uncommented_source = strip_line_comments(&source);
        let impl_body = extract_braced_body(
            &uncommented_source,
            &format!("impl SnapshotPlugin for {plugin_type}"),
        )?;
        let is_applicable_body = extract_braced_body(&impl_body, "async fn is_applicable")?;

        for forbidden in [
            "std::process::Command",
            "std::fs::",
            "fs::metadata",
            ".is_file(",
            ".exists(",
            "std::thread::sleep",
            "tokio::task::spawn_blocking",
            "spawn_blocking(",
        ] {
            if is_applicable_body.contains(forbidden) {
                failures.push(format!(
                    "{relative_path}::{plugin_type}::is_applicable contains blocking API {forbidden:?}"
                ));
            }
        }

        let has_sync_binary_lookup = uncommented_source.contains("fn binary_available(")
            && !uncommented_source.contains("async fn binary_available(");
        if has_sync_binary_lookup && is_applicable_body.contains("binary_available(") {
            failures.push(format!(
                "{relative_path}::{plugin_type}::is_applicable calls synchronous binary_available"
            ));
        }
    }

    for (relative_path, helper_name) in RETRY_HELPERS {
        let source = fs::read_to_string(manifest_dir.join(relative_path))?;
        let helper_body = extract_braced_body(&strip_line_comments(&source), helper_name)?;

        if !helper_body.contains("tokio::time::sleep") {
            failures.push(format!(
                "{relative_path}::{helper_name} does not use tokio::time::sleep for retry delay"
            ));
        }
        for forbidden in [
            "std::thread::sleep",
            "tokio::task::spawn_blocking",
            "spawn_blocking(",
        ] {
            if helper_body.contains(forbidden) {
                failures.push(format!(
                    "{relative_path}::{helper_name} contains blocking retry workaround {forbidden:?}"
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "snapshot async contract violations:\n{}",
        failures.join("\n")
    );

    Ok(())
}

fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| line.split_once("//").map_or(line, |(code, _comment)| code))
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_braced_body(source: &str, needle: &str) -> Result<String, Box<dyn std::error::Error>> {
    let start = source
        .find(needle)
        .ok_or_else(|| format!("could not find {needle:?}"))?;
    let after_needle = &source[start..];
    let open_relative = after_needle
        .find('{')
        .ok_or_else(|| format!("could not find opening brace for {needle:?}"))?;
    let open = start + open_relative;

    let mut depth = 0usize;
    for (index, ch) in source[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let close = open + index;
                    return Ok(source[open + 1..close].to_string());
                }
            }
            _ => {}
        }
    }

    Err(format!("could not find closing brace for {needle:?}").into())
}
