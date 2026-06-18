//! Regression test for the Aegis 800-LoC file-size budget (M5.1 quality gate,
//! Task 1).
//!
//! Every Rust source file in the active workspace must stay at or below 800
//! lines. The test walks `CARGO_MANIFEST_DIR` recursively, skips build/cache
//! directories, and asserts that no `.rs` file exceeds the budget. When the
//! budget is violated, ALL offending paths (with their line counts) are
//! collected into a single assertion message so the next worker sees the full
//! list at once.

use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Maximum number of lines permitted in any single Rust source file.
const MAX_LINES: usize = 800;

/// Directory components that must be skipped entirely during the walk.
///
/// These are build output, VCS metadata, or unrelated tooling caches that are
/// not part of the active workspace contract.
const SKIP_DIRS: &[&str] = &[
    "target",
    ".git",
    ".worktrees",
    ".cargo",
    ".claude",
    "node_modules",
];

/// True if any path component of `path` is in the skip-list.
fn is_skipped(path: &Path) -> bool {
    path.components().any(|comp| {
        matches!(comp, std::path::Component::Normal(name) if SKIP_DIRS.iter().any(|s| *s == name))
    })
}

/// Recursively collect every `.rs` file under `dir` whose path is not skipped.
fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        // Skip symlinks (don't follow) — they aren't real source files.
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if path.is_dir() {
            if is_skipped(&path) {
                continue;
            }
            collect_rust_files(&path, out)?;
        } else if path.is_file() {
            if path.extension().and_then(|e| e.to_str()) == Some("rs") && !is_skipped(&path) {
                out.push(path);
            }
        }
    }
    Ok(())
}

/// Count lines in a file using `BufRead::lines().count()` semantics: a final
/// non-empty line without a trailing newline still counts as one line.
fn count_lines(path: &Path) -> std::io::Result<usize> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    Ok(reader.lines().count())
}

#[test]
fn rust_source_files_should_stay_under_800_lines() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let root = PathBuf::from(&manifest_dir);

    let mut rust_files = Vec::new();
    collect_rust_files(&root, &mut rust_files)?;

    let mut offenders: Vec<(PathBuf, usize)> = Vec::new();
    for file in &rust_files {
        let lines = count_lines(file)?;
        if lines > MAX_LINES {
            // Display paths relative to the manifest root for readability.
            let rel = file.strip_prefix(&root).unwrap_or(file);
            offenders.push((rel.to_path_buf(), lines));
        }
    }

    if !offenders.is_empty() {
        offenders.sort_by(|a, b| b.1.cmp(&a.1));
        let mut msg = String::from(
            "The following Rust source files exceed the 800-line budget \
             (M5.1 quality gate, Task 1):\n",
        );
        for (path, lines) in &offenders {
            msg.push_str(&format!("  {} ({} lines)\n", path.display(), lines));
        }
        msg.push_str(&format!(
            "\n{} file(s) over budget. Refactor these files to bring them under \
             {} lines (split modules, extract helpers, move tests to fixtures).",
            offenders.len(),
            MAX_LINES,
        ));
        panic!("{}", msg);
    }

    Ok(())
}
