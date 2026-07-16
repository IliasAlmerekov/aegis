//! Owner-only filesystem helpers for audit directories and artifacts.

use std::fs::{self, File, OpenOptions};
use std::path::Path;

#[cfg(unix)]
use std::path::PathBuf;

use crate::error::AuditError;

type Result<T> = std::result::Result<T, AuditError>;

pub(crate) fn create_parent_directories(path: &Path) -> Result<()> {
    let parent = immediate_parent(path);

    #[cfg(unix)]
    {
        match fs::symlink_metadata(parent) {
            Ok(metadata) => {
                validate_parent(parent, &metadata)?;
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(insecure(parent, &error.to_string())),
        }

        let mut missing = missing_parent_components(parent)?;
        missing.reverse();
        for directory in missing {
            create_missing_directory(&directory)?;
        }
    }

    #[cfg(not(unix))]
    fs::create_dir_all(parent)?;

    Ok(())
}

#[cfg(unix)]
fn create_missing_directory(directory: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    let created = match builder.create(directory) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(error) => return Err(AuditError::Io(error)),
    };

    let metadata =
        fs::symlink_metadata(directory).map_err(|error| insecure(directory, &error.to_string()))?;
    validate_parent(directory, &metadata)?;

    if created {
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| insecure(directory, &error.to_string()))?;
    }

    let metadata =
        fs::symlink_metadata(directory).map_err(|error| insecure(directory, &error.to_string()))?;
    validate_parent(directory, &metadata)?;
    let mode = metadata.permissions().mode() & 0o777;
    if metadata.uid() != effective_uid() || mode != 0o700 {
        return Err(insecure(
            directory,
            &format!(
                "audit directory is not owner-only (uid {}, mode {mode:04o})",
                metadata.uid()
            ),
        ));
    }

    Ok(())
}

pub(crate) fn parent_exists_and_is_safe(path: &Path) -> Result<bool> {
    let parent = immediate_parent(path);

    #[cfg(unix)]
    match fs::symlink_metadata(parent) {
        Ok(metadata) => {
            validate_parent(parent, &metadata)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(insecure(parent, &error.to_string())),
    }

    #[cfg(not(unix))]
    {
        Ok(parent.exists())
    }
}

fn immediate_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

#[cfg(unix)]
fn missing_parent_components(parent: &Path) -> Result<Vec<PathBuf>> {
    let mut missing = Vec::new();
    let mut current = parent;
    loop {
        match fs::symlink_metadata(current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    let followed = fs::metadata(current)
                        .map_err(|error| insecure(current, &error.to_string()))?;
                    if !followed.is_dir() {
                        return Err(insecure(current, "audit path ancestor is not a directory"));
                    }
                } else if !metadata.is_dir() {
                    return Err(insecure(current, "audit path ancestor is not a directory"));
                }
                return Ok(missing);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                current = current.parent().unwrap_or_else(|| Path::new("."));
            }
            Err(error) => return Err(insecure(current, &error.to_string())),
        }
    }
}

#[cfg(unix)]
fn validate_parent(path: &Path, metadata: &fs::Metadata) -> Result<()> {
    if metadata.file_type().is_symlink() {
        return Err(insecure(path, "immediate audit parent is a symlink"));
    }
    if !metadata.is_dir() {
        return Err(insecure(path, "immediate audit parent is not a directory"));
    }
    Ok(())
}

pub(crate) fn open_append(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }

    open_and_harden(path, &options)
}

pub(crate) fn open_lock(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }

    open_and_harden(path, &options)
}

pub(crate) fn open_read_if_exists(path: &Path) -> Result<Option<File>> {
    let mut options = OpenOptions::new();
    options.read(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }

    match open_and_harden(path, &options) {
        Ok(file) => Ok(Some(file)),
        Err(AuditError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn create_new(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }

    open_and_harden(path, &options)
}

fn open_and_harden(path: &Path, options: &OpenOptions) -> Result<File> {
    #[cfg(unix)]
    reject_obvious_unsafe_target(path)?;

    let file = options.open(path).map_err(|error| {
        #[cfg(unix)]
        if error.raw_os_error() == Some(libc::ELOOP) {
            return insecure(path, "audit artifact is a symlink");
        }
        AuditError::Io(error)
    })?;

    #[cfg(unix)]
    harden_artifact(path, &file)?;

    Ok(file)
}

#[cfg(unix)]
fn reject_obvious_unsafe_target(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;

    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(insecure(path, "audit artifact is a symlink"))
        }
        Ok(metadata) if !metadata.file_type().is_file() => {
            Err(insecure(path, "audit artifact is not a regular file"))
        }
        Ok(metadata) if metadata.uid() != effective_uid() => Err(insecure(
            path,
            &format!("artifact owned by uid {}", metadata.uid()),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AuditError::Io(error)),
    }
}

#[cfg(unix)]
fn harden_artifact(path: &Path, file: &File) -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let metadata = file
        .metadata()
        .map_err(|error| insecure(path, &error.to_string()))?;
    let expected_uid = effective_uid();
    validate_artifact_metadata(
        metadata.file_type().is_file(),
        metadata.uid(),
        metadata.permissions().mode() & 0o777,
        expected_uid,
        false,
    )
    .map_err(|detail| insecure(path, &detail))?;

    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|error| insecure(path, &error.to_string()))?;
    let hardened = file
        .metadata()
        .map_err(|error| insecure(path, &error.to_string()))?;
    let mode = hardened.permissions().mode() & 0o777;
    validate_artifact_metadata(
        hardened.file_type().is_file(),
        hardened.uid(),
        mode,
        expected_uid,
        true,
    )
    .map_err(|detail| insecure(path, &detail))?;
    Ok(())
}

#[cfg(unix)]
fn validate_artifact_metadata(
    is_regular: bool,
    uid: u32,
    mode: u32,
    expected_uid: u32,
    require_owner_only: bool,
) -> std::result::Result<(), String> {
    if !is_regular {
        return Err("audit artifact is not a regular file".to_string());
    }
    if uid != expected_uid {
        return Err(format!("artifact owned by uid {uid}"));
    }
    if require_owner_only && mode != 0o600 {
        return Err(format!(
            "artifact could not be tightened to mode 0600 (mode {mode:04o})"
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn effective_uid() -> u32 {
    // SAFETY: `geteuid` has no preconditions and cannot invalidate Rust memory.
    unsafe { libc::geteuid() }
}

#[cfg(unix)]
fn insecure(path: &Path, detail: &str) -> AuditError {
    AuditError::InsecureAuditArtifact {
        path: path.to_string_lossy().to_string(),
        detail: detail.to_string(),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn concurrent_directory_creation_accepts_an_existing_safe_directory() {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

        let temporary = tempfile::TempDir::new().unwrap();
        let directory = temporary.path().join("audit");
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder.create(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();

        create_missing_directory(&directory).unwrap();
    }

    #[test]
    fn concurrent_directory_creation_rejects_an_existing_symlink() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::TempDir::new().unwrap();
        let target = temporary.path().join("target");
        let directory = temporary.path().join("audit");
        fs::create_dir(&target).unwrap();
        symlink(&target, &directory).unwrap();

        let error = create_missing_directory(&directory).unwrap_err();
        assert!(error.to_string().contains("symlink"));
    }

    #[test]
    fn metadata_policy_rejects_another_owner_even_for_root() {
        let error = validate_artifact_metadata(true, 1000, 0o600, 0, false).unwrap_err();

        assert!(error.contains("uid 1000"));
    }
}
