//! Async, catch-only script-file source reader (ADR-022 §6, L1 Iteration 4).
//!
//! Reads a local regular file through the caller's own permissions, within a
//! bounded budget, and records only the resulting bytes' hash — never
//! persisting the source itself. A successful read never claims that the
//! interpreter will execute these exact bytes later (TOCTOU remains an
//! accepted residual risk per ADR-022 §6): Effect-opaque execution and
//! Required recovery are never waived by a successful or failed read.
//!
//! This module performs a pre-open `symlink_metadata` check (rejecting
//! symlinks, FIFOs, sockets, devices, and directories without following them)
//! and a post-open re-check on the opened file handle, but does not use a
//! platform-specific `O_NOFOLLOW` open (that would require a new dependency
//! outside this project's approved list) — a symlink swap between the two
//! checks is the documented residual race, which is why a successful read
//! only ever adds evidence and never removes the Effect-opaque backstop.

use std::path::Path;

use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

/// A successfully read script-file source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadSource {
    /// The decoded UTF-8 source text (BOM stripped, if present).
    ///
    /// In-memory only — callers must not persist this into audit JSONL or any
    /// other durable surface (ADR-022 §10); only [`ReadSource::source_hash`]
    /// and the byte length are metadata-safe to persist.
    pub source: String,
    /// The exact byte length of the original file content (before BOM strip).
    pub byte_len: u64,
    /// A hex SHA-256 digest of the original file bytes (before BOM strip).
    pub source_hash: String,
}

/// Why a script-file read did not produce a [`ReadSource`].
///
/// Every variant is a catch-only outcome: none of them are ever surfaced as a
/// hard error to the caller's shell — the caller maps each to typed Analysis
/// degradation instead (ADR-022 §4/§6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceReadError {
    /// The path does not exist.
    NotFound,
    /// The path exists but is not a regular file (symlink, FIFO, socket,
    /// device, or directory).
    NotRegularFile,
    /// The caller's permissions do not allow reading the path.
    PermissionDenied,
    /// The file exceeds `limit_bytes`.
    TooLarge {
        /// The file's actual size in bytes.
        size: u64,
        /// The budget that was exceeded.
        limit: u64,
    },
    /// The file content is not valid UTF-8.
    InvalidUtf8,
    /// Any other I/O failure, carried as a display string (no `std::io::Error`
    /// in the public type, since it is not `PartialEq`/`Eq`).
    Io(String),
}

const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Read `path` as a script-file analysis source, bounded by `limit_bytes`.
///
/// Rejects symlinks, FIFOs, sockets, devices, and directories without
/// following them. Never reads more than `limit_bytes` + 1 bytes off disk (so
/// an oversized file is detected without paying for a full read).
pub async fn read_script_file(
    path: &Path,
    limit_bytes: u64,
) -> Result<ReadSource, SourceReadError> {
    let pre_metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(map_open_error)?;
    if !pre_metadata.is_file() {
        return Err(SourceReadError::NotRegularFile);
    }

    let file = tokio::fs::File::open(path).await.map_err(map_open_error)?;
    let post_metadata = file.metadata().await.map_err(map_open_error)?;
    if !post_metadata.is_file() {
        return Err(SourceReadError::NotRegularFile);
    }

    let size = post_metadata.len();
    if size > limit_bytes {
        return Err(SourceReadError::TooLarge {
            size,
            limit: limit_bytes,
        });
    }

    // Read one byte beyond the limit so a file that grew after the metadata
    // check (but is still reported within budget) is still caught as
    // oversized rather than silently truncated.
    let mut bytes = Vec::with_capacity(size.min(limit_bytes) as usize + 1);
    file.take(limit_bytes + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|err| SourceReadError::Io(err.to_string()))?;
    if bytes.len() as u64 > limit_bytes {
        return Err(SourceReadError::TooLarge {
            size: bytes.len() as u64,
            limit: limit_bytes,
        });
    }

    let byte_len = bytes.len() as u64;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let source_hash = format!("{:x}", hasher.finalize());

    let content = bytes.strip_prefix(&UTF8_BOM).unwrap_or(bytes.as_slice());
    let source = String::from_utf8(content.to_vec()).map_err(|_| SourceReadError::InvalidUtf8)?;

    Ok(ReadSource {
        source,
        byte_len,
        source_hash,
    })
}

fn map_open_error(err: std::io::Error) -> SourceReadError {
    match err.kind() {
        std::io::ErrorKind::NotFound => SourceReadError::NotFound,
        std::io::ErrorKind::PermissionDenied => SourceReadError::PermissionDenied,
        _ => SourceReadError::Io(err.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn regular_file_under_limit_is_read_and_hashed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("script.py");
        std::fs::write(&path, "print(1)\n").unwrap();

        let result = read_script_file(&path, 1024).await.unwrap();

        assert_eq!(result.source, "print(1)\n");
        assert_eq!(result.byte_len, 9);
        // Known-good SHA-256 of "print(1)\n", computed independently via
        // `printf 'print(1)\n' | sha256sum`.
        assert_eq!(
            result.source_hash,
            "cc42155088fca5730758db72b2a5bca33112a941dfaa2d43098ec422ce4ea213"
        );
    }

    #[tokio::test]
    async fn missing_file_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.py");

        assert_eq!(
            read_script_file(&path, 1024).await.unwrap_err(),
            SourceReadError::NotFound
        );
    }

    #[tokio::test]
    async fn directory_is_rejected_as_not_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(
            read_script_file(dir.path(), 1024).await.unwrap_err(),
            SourceReadError::NotRegularFile
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn symlink_is_rejected_without_following() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.py");
        std::fs::write(&target, "print(1)\n").unwrap();
        let link = dir.path().join("link.py");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert_eq!(
            read_script_file(&link, 1024).await.unwrap_err(),
            SourceReadError::NotRegularFile
        );
    }

    #[tokio::test]
    async fn oversized_file_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.py");
        std::fs::write(&path, "x".repeat(2048)).unwrap();

        assert_eq!(
            read_script_file(&path, 1024).await.unwrap_err(),
            SourceReadError::TooLarge {
                size: 2048,
                limit: 1024,
            }
        );
    }

    #[tokio::test]
    async fn invalid_utf8_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.py");
        std::fs::write(&path, [0xFF, 0xFE, 0x00]).unwrap();

        assert_eq!(
            read_script_file(&path, 1024).await.unwrap_err(),
            SourceReadError::InvalidUtf8
        );
    }

    #[tokio::test]
    async fn utf8_bom_is_stripped_from_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bom.py");
        let mut bytes = UTF8_BOM.to_vec();
        bytes.extend_from_slice(b"print(1)\n");
        std::fs::write(&path, &bytes).unwrap();

        let result = read_script_file(&path, 1024).await.unwrap();

        assert_eq!(result.source, "print(1)\n");
        // byte_len records the original on-disk size, BOM included.
        assert_eq!(result.byte_len, bytes.len() as u64);
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn permission_denied_file_is_rejected() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.py");
        std::fs::write(&path, "print(1)\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

        let result = read_script_file(&path, 1024).await;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        // Running as root (common in containers) bypasses the permission bit
        // entirely, so only assert the error when the read actually failed.
        if let Err(err) = result {
            assert_eq!(err, SourceReadError::PermissionDenied);
        }
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn unix_socket_is_rejected_as_not_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sock");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();

        assert_eq!(
            read_script_file(&path, 1024).await.unwrap_err(),
            SourceReadError::NotRegularFile
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn fifo_is_rejected_as_not_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fifo");
        let status = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .expect("mkfifo must be available in the test environment");
        assert!(status.success());

        assert_eq!(
            read_script_file(&path, 1024).await.unwrap_err(),
            SourceReadError::NotRegularFile
        );
    }
}
