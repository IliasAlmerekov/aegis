//! Docker snapshot provider.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Output;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use aegis_config::{DockerScope, DockerScopeMode};

use crate::SnapshotPlugin;
use crate::error::SnapshotError;

type Result<T> = std::result::Result<T, SnapshotError>;

/// Sentinel returned when there were no running containers at snapshot time.
const NO_CONTAINERS: &str = "none";
const EXECUTABLE_BUSY_ERRNO: i32 = 26;
const DOCKER_BUSY_RETRY_ATTEMPTS: usize = 40;
const DOCKER_BUSY_RETRY_DELAY_MS: u64 = 25;

/// Snapshot plugin that commits and saves Docker container state.
pub struct DockerPlugin {
    /// Path to the docker executable. Defaults to `"docker"` (resolved from PATH).
    docker_bin: String,
    /// Scoping rules controlling which containers are eligible for snapshot.
    scope: DockerScope,
}

impl Default for DockerPlugin {
    fn default() -> Self {
        Self {
            docker_bin: "docker".to_string(),
            scope: DockerScope::default(),
        }
    }
}

impl DockerPlugin {
    /// Create a `DockerPlugin` with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the container scoping rules for this plugin instance.
    pub fn with_scope(mut self, scope: DockerScope) -> Self {
        self.scope = scope;
        self
    }

    fn build_ps_args(&self) -> Vec<String> {
        let mut args = vec!["ps".to_string(), "-q".to_string()];
        match self.scope.mode {
            DockerScopeMode::Labeled => {
                args.push("--filter".to_string());
                args.push(format!("label={}=true", self.scope.label));
            }
            DockerScopeMode::All => { /* no filters */ }
            DockerScopeMode::Names => {
                for pat in &self.scope.name_patterns {
                    args.push("--filter".to_string());
                    args.push(format!("name={pat}"));
                }
            }
        }
        args
    }

    async fn run_docker_output<I, S>(&self, args: I, context: &str) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let args: Vec<OsString> = args
            .into_iter()
            .map(|arg| arg.as_ref().to_os_string())
            .collect();

        let mut attempt = 0usize;
        loop {
            match Command::new(&self.docker_bin).args(&args).output().await {
                Ok(output) => return Ok(output),
                Err(error)
                    if is_executable_busy(&error) && attempt < DOCKER_BUSY_RETRY_ATTEMPTS =>
                {
                    attempt += 1;
                    tracing::warn!(
                        docker_bin = %self.docker_bin,
                        context,
                        attempt,
                        "docker binary busy during command launch, retrying"
                    );
                    sleep_docker_busy_retry_delay().await;
                }
                Err(error) => {
                    return Err(SnapshotError::Snapshot(format!("{context}: {error}")));
                }
            }
        }
    }
}

fn is_executable_busy(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(EXECUTABLE_BUSY_ERRNO)
}

pub(crate) async fn sleep_docker_busy_retry_delay() {
    tokio::time::sleep(Duration::from_millis(DOCKER_BUSY_RETRY_DELAY_MS)).await;
}

/// Host-level configuration captured from a running container before snapshot.
#[derive(Debug, Serialize, Deserialize)]
struct ContainerConfig {
    name: String,
    binds: Vec<String>,
    port_bindings: Vec<String>,
    labels: HashMap<String, String>,
    network_mode: String,
    restart_policy: String,
}

/// One record in the snapshot_id string — one entry per snapshotted container.
#[derive(Debug, Serialize, Deserialize)]
struct ContainerRecord {
    container_id: String,
    image: String,
    config: ContainerConfig,
}

impl DockerPlugin {
    async fn inspect_container(&self, container_id: &str) -> Result<ContainerConfig> {
        let out = self
            .run_docker_output(["inspect", container_id], "docker inspect failed")
            .await?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(SnapshotError::Snapshot(format!(
                "docker inspect {container_id} failed: {stderr}"
            )));
        }

        let json: serde_json::Value = serde_json::from_slice(&out.stdout)
            .map_err(|e| SnapshotError::Snapshot(format!("failed to parse docker inspect: {e}")))?;

        let c = &json[0];

        let name = c["Name"]
            .as_str()
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string();

        let binds: Vec<String> = c["HostConfig"]["Binds"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let mut port_bindings = Vec::new();
        if let Some(obj) = c["HostConfig"]["PortBindings"].as_object() {
            for (container_port, bindings) in obj {
                if let Some(arr) = bindings.as_array() {
                    for b in arr {
                        let host_port = b["HostPort"].as_str().unwrap_or("").trim();
                        if host_port.is_empty() {
                            continue;
                        }
                        let host_ip = b["HostIp"].as_str().unwrap_or("").trim();
                        let spec = if host_ip.is_empty() {
                            format!("{host_port}:{container_port}")
                        } else {
                            format!("{host_ip}:{host_port}:{container_port}")
                        };
                        port_bindings.push(spec);
                    }
                }
            }
        }

        let labels: HashMap<String, String> = c["Config"]["Labels"]
            .as_object()
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let network_mode = c["HostConfig"]["NetworkMode"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let restart_policy = c["HostConfig"]["RestartPolicy"]["Name"]
            .as_str()
            .unwrap_or("no")
            .to_string();

        Ok(ContainerConfig {
            name,
            binds,
            port_bindings,
            labels,
            network_mode,
            restart_policy,
        })
    }

    fn build_run_args(image: &str, cfg: &ContainerConfig) -> Vec<String> {
        let mut args = vec!["run".to_string(), "-d".to_string()];

        if !cfg.name.is_empty() {
            args.extend(["--name".to_string(), cfg.name.clone()]);
        }

        for bind in &cfg.binds {
            args.extend(["-v".to_string(), bind.clone()]);
        }

        for p in &cfg.port_bindings {
            args.extend(["-p".to_string(), p.clone()]);
        }

        for (k, v) in &cfg.labels {
            args.extend(["--label".to_string(), format!("{k}={v}")]);
        }

        if !cfg.network_mode.is_empty() {
            args.extend(["--network".to_string(), cfg.network_mode.clone()]);
        }

        if cfg.restart_policy != "no" && !cfg.restart_policy.is_empty() {
            args.extend(["--restart".to_string(), cfg.restart_policy.clone()]);
        }

        args.push(image.to_string());
        args
    }
}

#[async_trait]
impl SnapshotPlugin for DockerPlugin {
    fn name(&self) -> &'static str {
        "docker"
    }

    async fn is_applicable(&self, _cwd: &Path) -> bool {
        let ps_args = self.build_ps_args();
        match self.run_docker_output(&ps_args, "docker ps").await {
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

    async fn snapshot(&self, _cwd: &Path, _cmd: &str) -> Result<String> {
        let ps_args = self.build_ps_args();
        let ps_out = self
            .run_docker_output(&ps_args, "failed to run docker ps")
            .await?;

        if !ps_out.status.success() {
            let stderr = String::from_utf8_lossy(&ps_out.stderr);
            return Err(SnapshotError::Snapshot(format!(
                "docker ps failed: {stderr}"
            )));
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
            let config = self.inspect_container(container_id).await?;
            let image = format!("aegis-snap-{container_id}-{timestamp}");

            let commit_out = self
                .run_docker_output(["commit", container_id, &image], "docker commit failed")
                .await?;

            if !commit_out.status.success() {
                let stderr = String::from_utf8_lossy(&commit_out.stderr);
                return Err(SnapshotError::Snapshot(format!(
                    "docker commit {container_id} failed: {stderr}"
                )));
            }

            tracing::info!(container_id, %image, "docker snapshot created");
            let record = ContainerRecord {
                container_id: container_id.to_string(),
                image,
                config,
            };
            records.push(serde_json::to_string(&record).map_err(|e| {
                SnapshotError::Snapshot(format!("failed to serialize snapshot record: {e}"))
            })?);
        }

        Ok(records.join("\n"))
    }

    async fn rollback(&self, snapshot_id: &str) -> Result<()> {
        if snapshot_id == NO_CONTAINERS {
            tracing::info!("docker snapshot had no containers, nothing to roll back");
            return Ok(());
        }

        for line in snapshot_id.lines() {
            let record: ContainerRecord = serde_json::from_str(line).map_err(|e| {
                SnapshotError::Snapshot(format!("malformed docker snapshot record: {e}"))
            })?;

            let stop_out = self
                .run_docker_output(["stop", &record.container_id], "docker stop failed")
                .await?;

            if !stop_out.status.success() {
                tracing::warn!(
                    container_id = %record.container_id,
                    stderr = %String::from_utf8_lossy(&stop_out.stderr),
                    "docker stop failed — container may already be gone, continuing rollback"
                );
            }

            let rm_out = self
                .run_docker_output(["rm", &record.container_id], "docker rm failed")
                .await?;

            if !rm_out.status.success() {
                tracing::warn!(
                    container_id = %record.container_id,
                    stderr = %String::from_utf8_lossy(&rm_out.stderr),
                    "docker rm failed — continuing rollback"
                );
            }

            let run_args = Self::build_run_args(&record.image, &record.config);
            let run_out = self
                .run_docker_output(&run_args, "docker run failed")
                .await?;

            if !run_out.status.success() {
                let stderr = String::from_utf8_lossy(&run_out.stderr);
                return Err(SnapshotError::Snapshot(format!(
                    "docker run {} failed: {stderr}",
                    record.image
                )));
            }

            tracing::info!(
                container_id = %record.container_id,
                image = %record.image,
                "docker container rolled back"
            );
        }

        Ok(())
    }

    async fn delete(&self, snapshot_id: &str) -> Result<()> {
        if snapshot_id == NO_CONTAINERS {
            tracing::info!("docker snapshot had no containers, nothing to delete");
            return Ok(());
        }

        for line in snapshot_id.lines() {
            let record: ContainerRecord = serde_json::from_str(line).map_err(|e| {
                SnapshotError::Snapshot(format!("malformed docker snapshot record: {e}"))
            })?;

            let rmi_out = self
                .run_docker_output(["rmi", &record.image], "docker rmi failed")
                .await?;

            if rmi_out.status.success() {
                tracing::info!(image = %record.image, "docker snapshot image deleted");
                continue;
            }

            let stderr = String::from_utf8_lossy(&rmi_out.stderr).to_string();
            if stderr.contains("No such image")
                || stderr.contains("no such image")
                || stderr.to_lowercase().contains("not found")
            {
                tracing::info!(image = %record.image, "docker image already removed");
                continue;
            }

            return Err(SnapshotError::DeleteFailed {
                plugin: "docker".to_string(),
                snapshot_id: snapshot_id.to_string(),
                source: format!("docker rmi failed: {stderr}"),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests;
