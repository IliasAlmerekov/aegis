//! Owner-only filesystem creation helpers for snapshot stores and artifacts.

use std::fs::{self, File, OpenOptions};
use std::path::Path;

use crate::error::SnapshotError;

type Result<T> = std::result::Result<T, SnapshotError>;

#[cfg(test)]
thread_local! {
    static INJECT_MODE_HARDEN_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static INJECT_STORE_METADATA_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static INJECTED_EFFECTIVE_UID: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// Creates or hardens the plugin-owned snapshot store before sensitive writes.
pub(crate) fn create_store_dir(plugin: &'static str, path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

        match store_metadata(path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(insecure(plugin, path, "snapshot store is a symlink"));
                }
                if !metadata.is_dir() {
                    return Err(insecure(plugin, path, "snapshot store is not a directory"));
                }
                if metadata.uid() != effective_uid() {
                    return Err(insecure(
                        plugin,
                        path,
                        &format!("directory owned by uid {}", metadata.uid()),
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut builder = fs::DirBuilder::new();
                builder.recursive(true).mode(0o700);
                builder.create(path)?;
            }
            Err(error) => return Err(insecure(plugin, path, &error.to_string())),
        }

        apply_mode(path, 0o700).map_err(|error| insecure(plugin, path, &error.to_string()))?;
        let mode = fs::metadata(path)
            .map_err(|error| insecure(plugin, path, &error.to_string()))?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o700 {
            return Err(insecure(
                plugin,
                path,
                &format!("mode {mode:04o} could not be tightened to 0700"),
            ));
        }
        Ok(())
    }

    #[cfg(not(unix))]
    {
        // ACL handling is deliberately outside Aegis' non-Unix platform scope.
        fs::create_dir_all(path)?;
        let _ = plugin;
        Ok(())
    }
}

#[cfg(unix)]
fn store_metadata(path: &Path) -> std::io::Result<fs::Metadata> {
    #[cfg(test)]
    if INJECT_STORE_METADATA_FAILURE.with(|flag| flag.replace(false)) {
        return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    }

    fs::symlink_metadata(path)
}

#[cfg(unix)]
fn effective_uid() -> u32 {
    #[cfg(test)]
    if let Some(uid) = INJECTED_EFFECTIVE_UID.with(|value| value.replace(None)) {
        return uid;
    }

    // SAFETY: `geteuid` has no preconditions and cannot invalidate Rust memory.
    unsafe { libc::geteuid() }
}

/// Creates one fresh snapshot artifact with owner-readable/writable permissions.
pub(crate) fn create_artifact_file(plugin: &'static str, path: &Path) -> Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        if let Err(error) = harden_artifact_file(path) {
            drop(file);
            let _ = fs::remove_file(path);
            return Err(insecure(plugin, path, &error.to_string()));
        }
        Ok(file)
    }

    #[cfg(not(unix))]
    {
        // ACL handling is deliberately outside Aegis' non-Unix platform scope.
        let _ = plugin;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(Into::into)
    }
}

/// Creates a fresh, owner-only directory for one snapshot bundle.
pub(crate) fn create_artifact_dir(plugin: &'static str, path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(path)?;
        apply_mode(path, 0o700).map_err(|error| insecure(plugin, path, &error.to_string()))?;
        let mode = fs::metadata(path)
            .map_err(|error| insecure(plugin, path, &error.to_string()))?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o700 {
            return Err(insecure(
                plugin,
                path,
                &format!("mode {mode:04o} could not be tightened to 0700"),
            ));
        }
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let _ = plugin;
        fs::create_dir(path)?;
        Ok(())
    }
}

#[cfg(unix)]
fn harden_artifact_file(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    apply_mode(path, 0o600)?;
    let mode = fs::metadata(path)?.permissions().mode() & 0o777;
    if mode == 0o600 {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "mode {mode:04o} could not be tightened to 0600"
        )))
    }
}

#[cfg(unix)]
fn apply_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    #[cfg(test)]
    if INJECT_MODE_HARDEN_FAILURE.with(|flag| flag.replace(false)) {
        return Err(std::io::Error::other("injected mode hardening failure"));
    }

    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(test)]
pub(crate) fn inject_mode_hardening_failure() {
    INJECT_MODE_HARDEN_FAILURE.with(|flag| flag.set(true));
}

#[cfg(test)]
pub(crate) fn inject_store_metadata_failure() {
    INJECT_STORE_METADATA_FAILURE.with(|flag| flag.set(true));
}

#[cfg(test)]
pub(crate) fn inject_effective_uid(uid: u32) {
    INJECTED_EFFECTIVE_UID.with(|value| value.set(Some(uid)));
}

/// Reasserts the owner-only mode after an external tool writes a reserved artifact.
pub(crate) fn harden_existing_artifact(plugin: &'static str, path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        harden_artifact_file(path).map_err(|error| insecure(plugin, path, &error.to_string()))
    }

    #[cfg(not(unix))]
    {
        let _ = (plugin, path);
        Ok(())
    }
}

fn insecure(plugin: &'static str, path: &Path, detail: &str) -> SnapshotError {
    SnapshotError::InsecureSnapshotPermissions {
        plugin: plugin.to_string(),
        path: path.to_string_lossy().to_string(),
        detail: detail.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_a_store_and_a_fresh_artifact() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = temp_dir.path().join("snapshots");
        let artifact = store.join("artifact.dump");

        create_store_dir("test", &store).unwrap();
        let file = create_artifact_file("test", &artifact).unwrap();
        drop(file);

        assert!(store.is_dir());
        assert!(artifact.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn mode_hardening_failure_removes_the_reserved_artifact() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = temp_dir.path().join("snapshots");
        let artifact = store.join("artifact.dump");
        create_store_dir("test", &store).unwrap();
        inject_mode_hardening_failure();

        let error = create_artifact_file("test", &artifact).unwrap_err();

        assert!(matches!(
            error,
            SnapshotError::InsecureSnapshotPermissions { plugin, .. } if plugin == "test"
        ));
        assert!(!artifact.exists());
    }

    #[cfg(not(unix))]
    #[test]
    fn non_unix_fallback_creates_artifacts_without_unix_mode_guarantees() {
        let temp_dir = tempfile::tempdir().unwrap();
        let store = temp_dir.path().join("snapshots");
        let artifact = store.join("artifact.dump");

        create_store_dir("test", &store).unwrap();
        drop(create_artifact_file("test", &artifact).unwrap());

        assert!(artifact.is_file());
    }
}
