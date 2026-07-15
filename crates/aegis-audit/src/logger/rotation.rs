use std::fs;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;

use super::*;
use crate::secure_fs::{create_new, open_read_if_exists, parent_exists_and_is_safe};
use aegis_config::AuditConfig;

#[cfg(test)]
thread_local! {
    static INJECT_GZIP_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn inject_gzip_failure() {
    INJECT_GZIP_FAILURE.with(|flag| flag.set(true));
}

impl AuditRotationPolicy {
    /// Build a rotation policy from the effective audit config, if enabled.
    pub fn from_config(config: &AuditConfig) -> Option<Self> {
        config.rotation_enabled.then_some(Self {
            max_file_size_bytes: config.max_file_size_bytes,
            retention_files: config.retention_files,
            compress_rotated: config.compress_rotated,
        })
    }
}

impl AuditLogger {
    pub(super) fn segments_oldest_to_newest(&self) -> Result<Vec<ArchiveSegment>> {
        let mut segments = self.discover_archives()?;
        segments.sort_by_key(|segment| segment.index);
        segments.reverse();
        if open_read_if_exists(&self.path)?.is_some() {
            segments.push(ArchiveSegment {
                path: self.path.clone(),
                compressed: false,
                index: 0,
            });
        }
        Ok(segments)
    }

    fn discover_archives(&self) -> Result<Vec<ArchiveSegment>> {
        let Some(parent) = self.path.parent() else {
            return Ok(Vec::new());
        };
        let Some(base_name) = self.path.file_name().and_then(|name| name.to_str()) else {
            return Ok(Vec::new());
        };

        let mut segments = Vec::new();
        let prefix = format!("{base_name}.");

        if !parent_exists_and_is_safe(&self.path)? {
            return Ok(segments);
        }

        for entry in fs::read_dir(parent)? {
            let entry = entry?;
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };

            let Some(rest) = file_name.strip_prefix(&prefix) else {
                continue;
            };

            let (index_part, compressed) = match rest.strip_suffix(".gz") {
                Some(index) => (index, true),
                None => (rest, false),
            };

            let Ok(index) = index_part.parse::<usize>() else {
                continue;
            };

            if index == 0 {
                continue;
            }

            let path = entry.path();
            drop(open_read_if_exists(&path)?.ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "audit segment disappeared during discovery: {}",
                        path.display()
                    ),
                )
            })?);
            segments.push(ArchiveSegment {
                path,
                compressed,
                index,
            });
        }

        segments.sort_by(|left, right| {
            left.index
                .cmp(&right.index)
                .then(right.compressed.cmp(&left.compressed))
        });
        segments.dedup_by(|left, right| left.index == right.index);
        Ok(segments)
    }

    pub(super) fn rotate_if_needed(
        &self,
        policy: &AuditRotationPolicy,
        incoming_bytes: u64,
    ) -> Result<()> {
        let Some(file) = open_read_if_exists(&self.path)? else {
            return Ok(());
        };
        let current_size = file.metadata()?.len();
        if current_size.saturating_add(incoming_bytes) <= policy.max_file_size_bytes {
            return Ok(());
        }

        self.rotate(policy)
    }

    fn rotate(&self, policy: &AuditRotationPolicy) -> Result<()> {
        let stale_staging = self.preflight_rotation(policy)?;
        if stale_staging {
            fs::remove_file(self.staging_path())?;
        }
        self.remove_existing_archive(policy.retention_files)?;

        for index in (1..policy.retention_files).rev() {
            if let Some(source) = self.existing_archive_path(index)? {
                let destination = if source
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext == "gz")
                {
                    self.archive_path(index + 1, true)
                } else {
                    self.archive_path(index + 1, false)
                };
                if destination.exists() {
                    fs::remove_file(&destination)?;
                }
                fs::rename(source, destination)?;
            }
        }

        if policy.compress_rotated {
            self.compress_current_to_archive(&self.archive_path(1, true))?;
        } else {
            let destination = self.archive_path(1, false);
            if destination.exists() {
                fs::remove_file(&destination)?;
            }
            fs::rename(&self.path, destination)?;
        }

        Ok(())
    }

    fn preflight_rotation(&self, policy: &AuditRotationPolicy) -> Result<bool> {
        drop(open_read_if_exists(&self.path)?);
        for index in 1..=policy.retention_files {
            for path in [
                self.archive_path(index, false),
                self.archive_path(index, true),
            ] {
                drop(open_read_if_exists(&path)?);
            }
        }
        if policy.compress_rotated {
            Ok(open_read_if_exists(&self.staging_path())?.is_some())
        } else {
            Ok(false)
        }
    }

    fn compress_current_to_archive(&self, destination: &Path) -> Result<()> {
        let staging = self.staging_path();
        let result = (|| {
            let mut source = open_read_if_exists(&self.path)?.ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("active audit log not found: {}", self.path.display()),
                )
            })?;
            let archive = create_new(&staging)?;
            let mut encoder = GzEncoder::new(archive, Compression::default());
            std::io::copy(&mut source, &mut encoder)?;

            #[cfg(test)]
            if INJECT_GZIP_FAILURE.with(|flag| flag.replace(false)) {
                return Err(AuditError::Io(std::io::Error::other(
                    "injected gzip failure",
                )));
            }

            encoder.finish()?;
            fs::rename(&staging, destination)?;
            fs::remove_file(&self.path)?;
            Ok(())
        })();

        if result.is_err() {
            let _ = fs::remove_file(&staging);
        }
        result
    }

    fn remove_existing_archive(&self, index: usize) -> Result<()> {
        for path in [
            self.archive_path(index, false),
            self.archive_path(index, true),
        ] {
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    pub(super) fn existing_archive_path(&self, index: usize) -> Result<Option<PathBuf>> {
        for path in [
            self.archive_path(index, true),
            self.archive_path(index, false),
        ] {
            if open_read_if_exists(&path)?.is_some() {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    fn archive_path(&self, index: usize, compressed: bool) -> PathBuf {
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| {
                if compressed {
                    format!("{name}.{index}.gz")
                } else {
                    format!("{name}.{index}")
                }
            })
            .unwrap_or_else(|| {
                if compressed {
                    format!("audit.jsonl.{index}.gz")
                } else {
                    format!("audit.jsonl.{index}")
                }
            });

        self.path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(file_name)
    }

    fn staging_path(&self) -> PathBuf {
        let mut path = self.archive_path(1, true).into_os_string();
        path.push(".tmp");
        PathBuf::from(path)
    }
}
