use std::path::Path;

use async_trait::async_trait;
use tokio::process::Command;

use crate::error::AegisError;
use crate::snapshot::SnapshotPlugin;

type Result<T> = std::result::Result<T, AegisError>;

/// Sentinel returned when there were no running containers at snapshot time.
const NO_CONTAINERS: &str = "none";

pub struct DockerPlugin {
    /// Path to the docker executable. Defaults to `"docker"` (resolved from PATH).
    docker_bin: String,
}

impl Default for DockerPlugin {
    fn default() -> Self {
        Self {
            docker_bin: "docker".to_string(),
        }
    }
}

impl DockerPlugin {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl SnapshotPlugin for DockerPlugin {
    fn name(&self) -> &'static str {
        "docker"
    }

    /// Returns `true` when Docker CLI is reachable and at least one container is running.
    ///
    /// Uses `docker ps -q`: exits 0 with output when containers are running,
    /// exits 0 with no output when docker is up but idle,
    /// exits non-zero or errors when docker is unavailable.
    fn is_applicable(&self, _cwd: &Path) -> bool {
        let output = std::process::Command::new(&self.docker_bin)
            .args(["ps", "-q"])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                !String::from_utf8_lossy(&out.stdout).trim().is_empty()
            }
            Ok(_) => {
                tracing::warn!("docker ps failed — Docker may not be running");
                false
            }
            Err(e) => {
                tracing::warn!(error = %e, "docker CLI not found — skipping Docker plugin");
                false
            }
        }
    }

    /// Commit each running container as `aegis-snap-<container_id>-<timestamp>`.
    ///
    /// The returned `snapshot_id` is newline-separated `<container_id>:<image_name>` pairs,
    /// one per container, so `rollback` can restore each one independently.
    async fn snapshot(&self, _cwd: &Path, _cmd: &str) -> Result<String> {
        let ps_out = Command::new(&self.docker_bin)
            .args(["ps", "-q"])
            .output()
            .await
            .map_err(|e| AegisError::Snapshot(format!("failed to run docker ps: {e}")))?;

        if !ps_out.status.success() {
            let stderr = String::from_utf8_lossy(&ps_out.stderr);
            return Err(AegisError::Snapshot(format!("docker ps failed: {stderr}")));
        }

        let stdout = String::from_utf8_lossy(&ps_out.stdout);
        let container_ids: Vec<&str> = stdout
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();

        if container_ids.is_empty() {
            tracing::info!("no running containers to snapshot");
            return Ok(NO_CONTAINERS.to_string());
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut records = Vec::with_capacity(container_ids.len());
        for container_id in container_ids {
            let image_name = format!("aegis-snap-{container_id}-{timestamp}");

            let commit_out = Command::new(&self.docker_bin)
                .args(["commit", container_id, &image_name])
                .output()
                .await
                .map_err(|e| AegisError::Snapshot(format!("docker commit failed: {e}")))?;

            if !commit_out.status.success() {
                let stderr = String::from_utf8_lossy(&commit_out.stderr);
                return Err(AegisError::Snapshot(format!(
                    "docker commit {container_id} failed: {stderr}"
                )));
            }

            tracing::info!(container_id, %image_name, "docker snapshot created");
            records.push(format!("{container_id}:{image_name}"));
        }

        Ok(records.join("\n"))
    }

    /// Restore each container from its committed snapshot image.
    ///
    /// Stops the original container (best-effort; it may already be gone),
    /// then starts a new detached container from the snapshot image.
    async fn rollback(&self, snapshot_id: &str) -> Result<()> {
        if snapshot_id == NO_CONTAINERS {
            tracing::info!("docker snapshot had no containers, nothing to roll back");
            return Ok(());
        }

        for record in snapshot_id.lines() {
            let (container_id, image_name) = record.split_once(':').ok_or_else(|| {
                AegisError::Snapshot(format!("malformed docker snapshot record: {record:?}"))
            })?;

            // Stop the original container — best-effort; log warning on failure.
            let stop_out = Command::new(&self.docker_bin)
                .args(["stop", container_id])
                .output()
                .await
                .map_err(|e| AegisError::Snapshot(format!("docker stop failed: {e}")))?;

            if !stop_out.status.success() {
                let stderr = String::from_utf8_lossy(&stop_out.stderr);
                tracing::warn!(
                    container_id,
                    %stderr,
                    "docker stop failed — container may already be gone, continuing rollback"
                );
            }

            // Start a new container from the snapshot image.
            let run_out = Command::new(&self.docker_bin)
                .args(["run", "-d", image_name])
                .output()
                .await
                .map_err(|e| AegisError::Snapshot(format!("docker run failed: {e}")))?;

            if !run_out.status.success() {
                let stderr = String::from_utf8_lossy(&run_out.stderr);
                return Err(AegisError::Snapshot(format!(
                    "docker run {image_name} failed: {stderr}"
                )));
            }

            tracing::info!(container_id, image_name, "docker snapshot rolled back");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Write a shell script to `dir/docker` and make it executable.
    fn write_mock_docker(dir: &std::path::Path, script: &str) -> std::path::PathBuf {
        let path = dir.join("docker");
        fs::write(&path, format!("#!/bin/sh\n{script}")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    fn plugin(bin: &std::path::Path) -> DockerPlugin {
        DockerPlugin {
            docker_bin: bin.to_string_lossy().into_owned(),
        }
    }

    // ── is_applicable ──────────────────────────────────────────────────────────

    #[test]
    fn is_applicable_no_docker_cli() {
        let p = DockerPlugin {
            docker_bin: "/nonexistent/bin/docker".to_string(),
        };
        assert!(!p.is_applicable(Path::new("/")));
    }

    #[test]
    fn is_applicable_no_running_containers() {
        let dir = TempDir::new().unwrap();
        // docker ps exits 0 but prints nothing — no running containers
        write_mock_docker(
            dir.path(),
            r#"case "$1" in
  ps) exit 0 ;;
  *) exit 1 ;;
esac"#,
        );
        assert!(!plugin(&dir.path().join("docker")).is_applicable(Path::new("/")));
    }

    #[test]
    fn is_applicable_with_running_containers() {
        let dir = TempDir::new().unwrap();
        write_mock_docker(
            dir.path(),
            r#"case "$1" in
  ps) printf "abc123\n"; exit 0 ;;
  *) exit 1 ;;
esac"#,
        );
        assert!(plugin(&dir.path().join("docker")).is_applicable(Path::new("/")));
    }

    #[test]
    fn is_applicable_docker_not_running() {
        let dir = TempDir::new().unwrap();
        // docker ps exits non-zero — daemon not running
        write_mock_docker(
            dir.path(),
            r#"case "$1" in
  ps) echo "Cannot connect to the Docker daemon" >&2; exit 1 ;;
  *) exit 1 ;;
esac"#,
        );
        assert!(!plugin(&dir.path().join("docker")).is_applicable(Path::new("/")));
    }

    // ── snapshot ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn snapshot_returns_sentinel_when_no_containers() {
        let dir = TempDir::new().unwrap();
        write_mock_docker(
            dir.path(),
            r#"case "$1" in
  ps) exit 0 ;;
  *) exit 1 ;;
esac"#,
        );
        let id = plugin(&dir.path().join("docker"))
            .snapshot(Path::new("/"), "rm -rf /")
            .await
            .unwrap();
        assert_eq!(id, NO_CONTAINERS);
    }

    #[tokio::test]
    async fn snapshot_commits_each_running_container() {
        let dir = TempDir::new().unwrap();
        write_mock_docker(
            dir.path(),
            r#"case "$1" in
  ps) printf "abc123\ndef456\n"; exit 0 ;;
  commit) printf "sha256:mockhash\n"; exit 0 ;;
  *) exit 1 ;;
esac"#,
        );
        let id = plugin(&dir.path().join("docker"))
            .snapshot(Path::new("/"), "docker rm -f web")
            .await
            .unwrap();

        assert!(id.contains("abc123"), "snapshot_id must reference abc123");
        assert!(id.contains("def456"), "snapshot_id must reference def456");
        assert!(
            id.contains("aegis-snap-"),
            "snapshot_id must use aegis-snap- prefix"
        );
        // Two records, one per line
        assert_eq!(id.lines().count(), 2);
    }

    #[tokio::test]
    async fn snapshot_fails_when_commit_returns_error() {
        let dir = TempDir::new().unwrap();
        write_mock_docker(
            dir.path(),
            r#"case "$1" in
  ps) printf "abc123\n"; exit 0 ;;
  commit) printf "Error: permission denied\n" >&2; exit 1 ;;
  *) exit 1 ;;
esac"#,
        );
        let result = plugin(&dir.path().join("docker"))
            .snapshot(Path::new("/"), "rm -rf /")
            .await;
        assert!(result.is_err(), "snapshot must propagate commit failure");
    }

    // ── rollback ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn rollback_noop_for_no_containers_sentinel() {
        // Must succeed without touching any docker binary.
        DockerPlugin::default()
            .rollback(NO_CONTAINERS)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn rollback_stops_then_restarts_each_container() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("calls.log");
        let log_path = log.to_string_lossy().into_owned();

        write_mock_docker(
            dir.path(),
            &format!(
                r#"printf "%s\n" "$*" >> {log_path}
case "$1" in
  stop) exit 0 ;;
  run)  printf "newcontainer\n"; exit 0 ;;
  *)    exit 1 ;;
esac"#
            ),
        );

        plugin(&dir.path().join("docker"))
            .rollback("abc123:aegis-snap-abc123-1700000000")
            .await
            .unwrap();

        let calls = fs::read_to_string(&log).unwrap();
        assert!(calls.contains("stop abc123"), "must call docker stop");
        assert!(
            calls.contains("run -d aegis-snap-abc123"),
            "must call docker run"
        );
    }

    #[tokio::test]
    async fn rollback_continues_when_stop_fails() {
        // A container that is already gone should not abort rollback.
        let dir = TempDir::new().unwrap();
        write_mock_docker(
            dir.path(),
            r#"case "$1" in
  stop) printf "No such container\n" >&2; exit 1 ;;
  run)  printf "newcontainer\n"; exit 0 ;;
  *)    exit 1 ;;
esac"#,
        );
        // Must succeed despite stop failure.
        plugin(&dir.path().join("docker"))
            .rollback("abc123:aegis-snap-abc123-1700000000")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn rollback_fails_when_run_fails() {
        let dir = TempDir::new().unwrap();
        write_mock_docker(
            dir.path(),
            r#"case "$1" in
  stop) exit 0 ;;
  run)  printf "Error: image not found\n" >&2; exit 1 ;;
  *)    exit 1 ;;
esac"#,
        );
        let result = plugin(&dir.path().join("docker"))
            .rollback("abc123:aegis-snap-abc123-1700000000")
            .await;
        assert!(result.is_err(), "rollback must propagate run failure");
    }

    #[tokio::test]
    async fn rollback_restores_multiple_containers() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("calls.log");
        let log_path = log.to_string_lossy().into_owned();

        write_mock_docker(
            dir.path(),
            &format!(
                r#"printf "%s\n" "$*" >> {log_path}
case "$1" in
  stop) exit 0 ;;
  run)  printf "newcontainer\n"; exit 0 ;;
  *)    exit 1 ;;
esac"#
            ),
        );

        plugin(&dir.path().join("docker"))
            .rollback("aaa111:aegis-snap-aaa111-1700000000\nbbb222:aegis-snap-bbb222-1700000000")
            .await
            .unwrap();

        let calls = fs::read_to_string(&log).unwrap();
        assert!(calls.contains("stop aaa111"));
        assert!(calls.contains("stop bbb222"));
        assert!(calls.contains("run -d aegis-snap-aaa111"));
        assert!(calls.contains("run -d aegis-snap-bbb222"));
    }

    #[tokio::test]
    async fn rollback_fails_on_malformed_snapshot_id() {
        let result = DockerPlugin::default().rollback("no-colon-here").await;
        assert!(
            result.is_err(),
            "malformed snapshot_id must return an error"
        );
    }
}
