use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Output;
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::config::{DockerScope, DockerScopeMode};
use crate::error::AegisError;
use crate::snapshot::SnapshotPlugin;

type Result<T> = std::result::Result<T, AegisError>;

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

    /// Build the argument list for `docker ps -q` honouring the configured scope.
    ///
    /// - `Labeled` → `["ps", "-q", "--filter", "label=<key>=true"]`
    /// - `All`     → `["ps", "-q"]`
    /// - `Names`   → `["ps", "-q", "--filter", "name=<pat>", ...]`
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
                    return Err(AegisError::Snapshot(format!("{context}: {error}")));
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
///
/// These fields are **not** preserved by `docker commit` (which saves only filesystem
/// layers) but are required to recreate a container with the same runtime behaviour.
#[derive(Debug, Serialize, Deserialize)]
struct ContainerConfig {
    /// Container name without the leading `/`.
    name: String,
    /// Bind mounts, e.g. `["/host/path:/container/path:ro"]`.
    /// Named volumes are recorded by mount spec but their data is not captured.
    binds: Vec<String>,
    /// Port mappings as `"[host_ip:]host_port:container_port/proto"` strings.
    port_bindings: Vec<String>,
    /// User-defined labels.
    labels: HashMap<String, String>,
    /// Network mode, e.g. `"bridge"`, `"host"`, or a custom named network.
    network_mode: String,
    /// Restart policy name, e.g. `"no"`, `"always"`, `"on-failure"`.
    restart_policy: String,
}

/// One record in the snapshot_id string — one entry per snapshotted container.
/// The snapshot_id is a newline-separated sequence of these JSON objects.
#[derive(Debug, Serialize, Deserialize)]
struct ContainerRecord {
    /// Short container ID as returned by `docker ps -q`.
    container_id: String,
    /// Name of the committed snapshot image (`aegis-snap-<id>-<ts>`).
    image: String,
    /// Host-level config captured before the commit via `docker inspect`.
    config: ContainerConfig,
}

impl DockerPlugin {
    /// Run `docker inspect <container_id>` and extract the fields needed for rollback.
    async fn inspect_container(&self, container_id: &str) -> Result<ContainerConfig> {
        let out = self
            .run_docker_output(["inspect", container_id], "docker inspect failed")
            .await?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(AegisError::Snapshot(format!(
                "docker inspect {container_id} failed: {stderr}"
            )));
        }

        let json: serde_json::Value = serde_json::from_slice(&out.stdout)
            .map_err(|e| AegisError::Snapshot(format!("failed to parse docker inspect: {e}")))?;

        // `docker inspect` always returns an array, even for a single container.
        let c = &json[0];

        let name = c["Name"]
            .as_str()
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string();

        // Binds can be null when there are no bind mounts.
        let binds: Vec<String> = c["HostConfig"]["Binds"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        // PortBindings: { "80/tcp": [{ "HostIp": "", "HostPort": "8080" }], ... }
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

    /// Build the `docker run` argument list to recreate a container from a snapshot image.
    ///
    /// Env vars, CMD, and ENTRYPOINT are already baked into the committed image by
    /// `docker commit` — only host-level config (name, ports, volumes, network, restart,
    /// labels) needs to be replayed here.
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

    /// Returns `true` when Docker CLI is reachable and at least one container is running.
    ///
    /// Uses `docker ps -q`: exits 0 with output when containers are running,
    /// exits 0 with no output when docker is up but idle,
    /// exits non-zero or errors when docker is unavailable.
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

    /// Capture each running container's filesystem state and host-level configuration.
    ///
    /// For each container:
    /// 1. `docker inspect` — records name, bind mounts, port bindings, network, restart
    ///    policy, and labels.
    /// 2. `docker commit` — saves the filesystem diff as `aegis-snap-<id>-<ts>`.
    ///
    /// # Snapshot format
    ///
    /// Returns a newline-separated list of JSON objects, one per container:
    /// ```json
    /// {"container_id":"abc123","image":"aegis-snap-abc123-1700000000","config":{...}}
    /// ```
    ///
    /// # Limitations
    ///
    /// - **Named volume data**: the volume *association* is recorded so the mount spec is
    ///   replayed on rollback, but the volume data itself is not captured.
    /// - **Removed networks**: if a custom network referenced in the config no longer exists
    ///   when rollback runs, `docker run` will fail with a descriptive error from Docker.
    /// - **Env / CMD / ENTRYPOINT**: these are baked into the committed image by
    ///   `docker commit` and do not need to be replayed separately.
    async fn snapshot(&self, _cwd: &Path, _cmd: &str) -> Result<String> {
        let ps_args = self.build_ps_args();
        let ps_out = self
            .run_docker_output(&ps_args, "failed to run docker ps")
            .await?;

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
            let config = self.inspect_container(container_id).await?;
            let image = format!("aegis-snap-{container_id}-{timestamp}");

            let commit_out = self
                .run_docker_output(["commit", container_id, &image], "docker commit failed")
                .await?;

            if !commit_out.status.success() {
                let stderr = String::from_utf8_lossy(&commit_out.stderr);
                return Err(AegisError::Snapshot(format!(
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
                AegisError::Snapshot(format!("failed to serialize snapshot record: {e}"))
            })?);
        }

        Ok(records.join("\n"))
    }

    /// Restore each container from its snapshot image and captured configuration.
    ///
    /// For each record:
    /// 1. `docker stop <id>` — best-effort; the container may already be gone.
    /// 2. `docker rm <id>` — best-effort; frees the original name so it can be reused.
    /// 3. `docker run -d [original flags] <snapshot-image>` — recreates the container
    ///    with its original name, port bindings, volumes, network, restart policy, and labels.
    async fn rollback(&self, snapshot_id: &str) -> Result<()> {
        if snapshot_id == NO_CONTAINERS {
            tracing::info!("docker snapshot had no containers, nothing to roll back");
            return Ok(());
        }

        for line in snapshot_id.lines() {
            let record: ContainerRecord = serde_json::from_str(line).map_err(|e| {
                AegisError::Snapshot(format!("malformed docker snapshot record: {e}"))
            })?;

            // Step 1: stop — best-effort.
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

            // Step 2: remove — best-effort; releases the container name for recreation.
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

            // Step 3: recreate with original host-level config.
            let run_args = Self::build_run_args(&record.image, &record.config);
            let run_out = self
                .run_docker_output(&run_args, "docker run failed")
                .await?;

            if !run_out.status.success() {
                let stderr = String::from_utf8_lossy(&run_out.stderr);
                return Err(AegisError::Snapshot(format!(
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
}

#[cfg(all(test, unix))]
mod tests;
