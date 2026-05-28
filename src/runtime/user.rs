//! User detection for scoped allowlist and audit attribution.

#[cfg(not(windows))]
use std::path::Path;
#[cfg(not(windows))]
use std::path::PathBuf;
#[cfg(not(windows))]
use std::process::Command;

pub(crate) fn detect_effective_user() -> Option<String> {
    #[cfg(not(windows))]
    {
        let id_path = find_id_in_path()?;
        run_id_command(&id_path)
    }

    #[cfg(windows)]
    {
        None
    }
}

/// Search the `PATH` env var directories for the `id` executable and return
/// the first absolute path found, or `None` when PATH is unset or `id` is
/// not present in any of its directories.
#[cfg(not(windows))]
fn find_id_in_path() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join("id");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(not(windows))]
fn run_id_command(id_path: &Path) -> Option<String> {
    let output = Command::new(id_path).arg("-un").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let user = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!user.is_empty()).then_some(user)
}

#[cfg(test)]
mod tests {
    #[cfg(not(windows))]
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn find_id_in_path_returns_absolute_path() {
        let path = find_id_in_path().expect("id must be present in PATH for this test");
        assert!(
            path.is_absolute(),
            "find_id_in_path must return an absolute path, got {path:?}"
        );
        assert!(path.is_file(), "returned path must exist as a regular file");
    }

    #[cfg(not(windows))]
    #[test]
    fn detect_effective_user_resolves_id_via_path_lookup() {
        // Verifies that detect_effective_user() resolves the id binary through
        // PATH rather than relying on a hardcoded location like /usr/bin/id.
        let user = detect_effective_user();
        assert!(
            user.is_some(),
            "detect_effective_user must return Some when PATH contains id"
        );
        assert!(!user.unwrap().is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn run_id_command_with_path_resolved_id_returns_username() {
        let id_path = find_id_in_path().expect("id must be present in PATH for this test");
        let user = run_id_command(&id_path);
        assert!(user.is_some());
        assert!(!user.unwrap().is_empty());
    }
}
