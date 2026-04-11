use std::collections::HashMap;
use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::config::{DockerScope, DockerScopeMode};
use crate::error::AegisError;
use crate::snapshot::SnapshotPlugin;

type Result<T> = std::result::Result<T, AegisError>;

/// Sentinel returned when there were no running containers at snapshot time.
const NO_CONTAINERS: &str = "none";

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
    pub fn new() -> Self {
        Self::default()
    }

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
        let out = Command::new(&self.docker_bin)
            .args(["inspect", container_id])
            .output()
            .await
            .map_err(|e| AegisError::Snapshot(format!("docker inspect failed: {e}")))?;

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
    fn is_applicable(&self, _cwd: &Path) -> bool {
        let ps_args = self.build_ps_args();
        let output = std::process::Command::new(&self.docker_bin)
            .args(&ps_args)
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
        let ps_out = Command::new(&self.docker_bin)
            .args(&ps_args)
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
            let config = self.inspect_container(container_id).await?;
            let image = format!("aegis-snap-{container_id}-{timestamp}");

            let commit_out = Command::new(&self.docker_bin)
                .args(["commit", container_id, &image])
                .output()
                .await
                .map_err(|e| AegisError::Snapshot(format!("docker commit failed: {e}")))?;

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
            let stop_out = Command::new(&self.docker_bin)
                .args(["stop", &record.container_id])
                .output()
                .await
                .map_err(|e| AegisError::Snapshot(format!("docker stop failed: {e}")))?;

            if !stop_out.status.success() {
                tracing::warn!(
                    container_id = %record.container_id,
                    stderr = %String::from_utf8_lossy(&stop_out.stderr),
                    "docker stop failed — container may already be gone, continuing rollback"
                );
            }

            // Step 2: remove — best-effort; releases the container name for recreation.
            let rm_out = Command::new(&self.docker_bin)
                .args(["rm", &record.container_id])
                .output()
                .await
                .map_err(|e| AegisError::Snapshot(format!("docker rm failed: {e}")))?;

            if !rm_out.status.success() {
                tracing::warn!(
                    container_id = %record.container_id,
                    stderr = %String::from_utf8_lossy(&rm_out.stderr),
                    "docker rm failed — continuing rollback"
                );
            }

            // Step 3: recreate with original host-level config.
            let run_args = Self::build_run_args(&record.image, &record.config);
            let run_out = Command::new(&self.docker_bin)
                .args(&run_args)
                .output()
                .await
                .map_err(|e| AegisError::Snapshot(format!("docker run failed: {e}")))?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command as StdCommand;
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

    #[test]
    fn write_mock_docker_rewrite_keeps_executable_and_updates_contents() {
        let dir = TempDir::new().unwrap();
        let path = write_mock_docker(dir.path(), "sleep 1\n");

        write_mock_docker(dir.path(), "printf 'updated\\n'\n");

        let output = StdCommand::new(&path).output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "updated\n");
    }

    /// Helper that creates a plugin with `All` scope — no filtering.
    /// Use `plugin_with_scope` to test specific scope behaviour.
    fn plugin(bin: &std::path::Path) -> DockerPlugin {
        DockerPlugin {
            docker_bin: bin.to_string_lossy().into_owned(),
            scope: DockerScope {
                mode: DockerScopeMode::All,
                ..DockerScope::default()
            },
        }
    }

    fn plugin_with_scope(bin: &std::path::Path, scope: DockerScope) -> DockerPlugin {
        DockerPlugin {
            docker_bin: bin.to_string_lossy().into_owned(),
            scope,
        }
    }

    /// Minimal `docker inspect` JSON for a container with no special config.
    const MINIMAL_INSPECT: &str = r#"[{"Name":"/test-ctr","Config":{"Labels":{}},"HostConfig":{"Binds":null,"PortBindings":{},"NetworkMode":"bridge","RestartPolicy":{"Name":"no"}}}]"#;

    /// Richer inspect JSON covering ports, binds, network, restart, and labels.
    const RICH_INSPECT: &str = r#"[{"Name":"/web","Config":{"Labels":{"app":"frontend"}},"HostConfig":{"Binds":["/data:/app/data"],"PortBindings":{"80/tcp":[{"HostIp":"","HostPort":"8080"}]},"NetworkMode":"my-net","RestartPolicy":{"Name":"always"}}}]"#;

    /// Build a minimal ContainerRecord JSON string suitable for use as a snapshot_id line.
    fn minimal_record(container_id: &str, image: &str) -> String {
        serde_json::to_string(&ContainerRecord {
            container_id: container_id.to_string(),
            image: image.to_string(),
            config: ContainerConfig {
                name: String::new(),
                binds: vec![],
                port_bindings: vec![],
                labels: HashMap::new(),
                network_mode: "bridge".to_string(),
                restart_policy: "no".to_string(),
            },
        })
        .unwrap()
    }

    // ── is_applicable ──────────────────────────────────────────────────────────

    #[test]
    fn is_applicable_no_docker_cli() {
        let p = DockerPlugin {
            docker_bin: "/nonexistent/bin/docker".to_string(),
            scope: DockerScope::default(),
        };
        assert!(!p.is_applicable(Path::new("/")));
    }

    #[test]
    fn is_applicable_no_running_containers() {
        let dir = TempDir::new().unwrap();
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
        let inspect_json = MINIMAL_INSPECT;
        write_mock_docker(
            dir.path(),
            &format!(
                r#"case "$1" in
  ps)      printf "abc123\ndef456\n"; exit 0 ;;
  inspect) printf '{inspect_json}'; exit 0 ;;
  commit)  printf "sha256:mockhash\n"; exit 0 ;;
  *)       exit 1 ;;
esac"#
            ),
        );
        let id = plugin(&dir.path().join("docker"))
            .snapshot(Path::new("/"), "docker rm -f web")
            .await
            .unwrap();

        assert_eq!(id.lines().count(), 2, "one JSON record per container");
        assert!(id.contains("abc123"), "snapshot_id must reference abc123");
        assert!(id.contains("def456"), "snapshot_id must reference def456");
        assert!(id.contains("aegis-snap-"), "must use aegis-snap- prefix");

        // Each line must be valid JSON with the expected fields.
        for line in id.lines() {
            let rec: ContainerRecord = serde_json::from_str(line)
                .expect("each snapshot_id line must be a valid ContainerRecord");
            assert!(rec.image.starts_with("aegis-snap-"));
        }
    }

    #[tokio::test]
    async fn snapshot_captures_container_metadata_from_inspect() {
        let dir = TempDir::new().unwrap();
        let inspect_json = RICH_INSPECT;
        write_mock_docker(
            dir.path(),
            &format!(
                r#"case "$1" in
  ps)      printf "abc123\n"; exit 0 ;;
  inspect) printf '{inspect_json}'; exit 0 ;;
  commit)  printf "sha256:mockhash\n"; exit 0 ;;
  *)       exit 1 ;;
esac"#
            ),
        );
        let id = plugin(&dir.path().join("docker"))
            .snapshot(Path::new("/"), "docker stop web")
            .await
            .unwrap();

        let rec: ContainerRecord = serde_json::from_str(id.trim()).unwrap();
        assert_eq!(rec.config.name, "web");
        assert_eq!(rec.config.network_mode, "my-net");
        assert_eq!(rec.config.restart_policy, "always");
        assert_eq!(rec.config.binds, vec!["/data:/app/data"]);
        assert!(
            rec.config.port_bindings.iter().any(|p| p.contains("8080")),
            "port binding must reference host port 8080"
        );
        assert_eq!(rec.config.labels.get("app"), Some(&"frontend".to_string()));
    }

    #[tokio::test]
    async fn snapshot_fails_when_inspect_returns_error() {
        let dir = TempDir::new().unwrap();
        write_mock_docker(
            dir.path(),
            r#"case "$1" in
  ps)      printf "abc123\n"; exit 0 ;;
  inspect) printf "Error: no such container\n" >&2; exit 1 ;;
  *)       exit 1 ;;
esac"#,
        );
        let result = plugin(&dir.path().join("docker"))
            .snapshot(Path::new("/"), "rm -rf /")
            .await;
        assert!(result.is_err(), "snapshot must propagate inspect failure");
    }

    #[tokio::test]
    async fn snapshot_fails_when_commit_returns_error() {
        let dir = TempDir::new().unwrap();
        let inspect_json = MINIMAL_INSPECT;
        write_mock_docker(
            dir.path(),
            &format!(
                r#"case "$1" in
  ps)      printf "abc123\n"; exit 0 ;;
  inspect) printf '{inspect_json}'; exit 0 ;;
  commit)  printf "Error: permission denied\n" >&2; exit 1 ;;
  *)       exit 1 ;;
esac"#
            ),
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
    async fn rollback_stops_removes_then_recreates_container() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("calls.log");
        let log_path = log.to_string_lossy().into_owned();

        write_mock_docker(
            dir.path(),
            &format!(
                r#"printf "%s\n" "$*" >> {log_path}
case "$1" in
  stop) exit 0 ;;
  rm)   exit 0 ;;
  run)  printf "newcontainer\n"; exit 0 ;;
  *)    exit 1 ;;
esac"#
            ),
        );

        let snapshot_id = minimal_record("abc123", "aegis-snap-abc123-1700000000");
        plugin(&dir.path().join("docker"))
            .rollback(&snapshot_id)
            .await
            .unwrap();

        let calls = fs::read_to_string(&log).unwrap();
        assert!(calls.contains("stop abc123"), "must call docker stop");
        assert!(
            calls.contains("rm abc123"),
            "must call docker rm to free the name"
        );
        assert!(
            calls.contains("aegis-snap-abc123-1700000000"),
            "must recreate from snapshot image"
        );
    }

    #[tokio::test]
    async fn rollback_uses_captured_name_ports_and_network() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("calls.log");
        let log_path = log.to_string_lossy().into_owned();

        write_mock_docker(
            dir.path(),
            &format!(
                r#"printf "%s\n" "$*" >> {log_path}
case "$1" in
  stop) exit 0 ;;
  rm)   exit 0 ;;
  run)  printf "newcontainer\n"; exit 0 ;;
  *)    exit 1 ;;
esac"#
            ),
        );

        let record = ContainerRecord {
            container_id: "abc123".to_string(),
            image: "aegis-snap-abc123-1700000000".to_string(),
            config: ContainerConfig {
                name: "web".to_string(),
                binds: vec!["/data:/app/data".to_string()],
                port_bindings: vec!["8080:80/tcp".to_string()],
                labels: HashMap::new(),
                network_mode: "my-net".to_string(),
                restart_policy: "always".to_string(),
            },
        };
        let snapshot_id = serde_json::to_string(&record).unwrap();

        plugin(&dir.path().join("docker"))
            .rollback(&snapshot_id)
            .await
            .unwrap();

        let calls = fs::read_to_string(&log).unwrap();
        assert!(calls.contains("--name web"), "must restore container name");
        assert!(
            calls.contains("-p 8080:80/tcp"),
            "must restore port binding"
        );
        assert!(
            calls.contains("-v /data:/app/data"),
            "must restore bind mount"
        );
        assert!(calls.contains("--network my-net"), "must restore network");
        assert!(
            calls.contains("--restart always"),
            "must restore restart policy"
        );
    }

    #[tokio::test]
    async fn rollback_continues_when_stop_fails() {
        let dir = TempDir::new().unwrap();
        write_mock_docker(
            dir.path(),
            r#"case "$1" in
  stop) printf "No such container\n" >&2; exit 1 ;;
  rm)   exit 0 ;;
  run)  printf "newcontainer\n"; exit 0 ;;
  *)    exit 1 ;;
esac"#,
        );
        let snapshot_id = minimal_record("abc123", "aegis-snap-abc123-1700000000");
        // Must succeed despite stop failure.
        plugin(&dir.path().join("docker"))
            .rollback(&snapshot_id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn rollback_continues_when_rm_fails() {
        let dir = TempDir::new().unwrap();
        write_mock_docker(
            dir.path(),
            r#"case "$1" in
  stop) exit 0 ;;
  rm)   printf "No such container\n" >&2; exit 1 ;;
  run)  printf "newcontainer\n"; exit 0 ;;
  *)    exit 1 ;;
esac"#,
        );
        let snapshot_id = minimal_record("abc123", "aegis-snap-abc123-1700000000");
        // Must succeed despite rm failure (container may already be removed).
        plugin(&dir.path().join("docker"))
            .rollback(&snapshot_id)
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
  rm)   exit 0 ;;
  run)  printf "Error: image not found\n" >&2; exit 1 ;;
  *)    exit 1 ;;
esac"#,
        );
        let snapshot_id = minimal_record("abc123", "aegis-snap-abc123-1700000000");
        let result = plugin(&dir.path().join("docker"))
            .rollback(&snapshot_id)
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
  rm)   exit 0 ;;
  run)  printf "newcontainer\n"; exit 0 ;;
  *)    exit 1 ;;
esac"#
            ),
        );

        let r1 = minimal_record("aaa111", "aegis-snap-aaa111-1700000000");
        let r2 = minimal_record("bbb222", "aegis-snap-bbb222-1700000000");
        let snapshot_id = format!("{r1}\n{r2}");

        plugin(&dir.path().join("docker"))
            .rollback(&snapshot_id)
            .await
            .unwrap();

        let calls = fs::read_to_string(&log).unwrap();
        assert!(calls.contains("stop aaa111"));
        assert!(calls.contains("stop bbb222"));
        assert!(calls.contains("rm aaa111"));
        assert!(calls.contains("rm bbb222"));
        assert!(calls.contains("aegis-snap-aaa111-1700000000"));
        assert!(calls.contains("aegis-snap-bbb222-1700000000"));
    }

    #[tokio::test]
    async fn rollback_fails_on_malformed_snapshot_id() {
        let result = DockerPlugin::default().rollback("not-valid-json").await;
        assert!(
            result.is_err(),
            "malformed snapshot_id must return an error"
        );
    }

    // ── build_run_args unit tests ───────────────────────────────────────────────

    #[test]
    fn build_run_args_minimal_config() {
        let cfg = ContainerConfig {
            name: String::new(),
            binds: vec![],
            port_bindings: vec![],
            labels: HashMap::new(),
            network_mode: String::new(),
            restart_policy: "no".to_string(),
        };
        let args = DockerPlugin::build_run_args("my-image", &cfg);
        assert_eq!(args, vec!["run", "-d", "my-image"]);
    }

    #[test]
    fn build_run_args_full_config() {
        let mut labels = HashMap::new();
        labels.insert("env".to_string(), "prod".to_string());

        let cfg = ContainerConfig {
            name: "app".to_string(),
            binds: vec!["/host:/container".to_string()],
            port_bindings: vec!["8080:80/tcp".to_string()],
            labels,
            network_mode: "custom-net".to_string(),
            restart_policy: "always".to_string(),
        };
        let args = DockerPlugin::build_run_args("snap-image", &cfg);

        // Check structural flags are present (order of labels is not guaranteed).
        assert!(args.contains(&"run".to_string()));
        assert!(args.contains(&"-d".to_string()));
        assert!(args.contains(&"--name".to_string()));
        assert!(args.contains(&"app".to_string()));
        assert!(args.contains(&"-v".to_string()));
        assert!(args.contains(&"/host:/container".to_string()));
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"8080:80/tcp".to_string()));
        assert!(args.contains(&"--label".to_string()));
        assert!(args.contains(&"env=prod".to_string()));
        assert!(args.contains(&"--network".to_string()));
        assert!(args.contains(&"custom-net".to_string()));
        assert!(args.contains(&"--restart".to_string()));
        assert!(args.contains(&"always".to_string()));
        assert_eq!(args.last().unwrap(), "snap-image");
    }

    #[test]
    fn build_run_args_skips_no_restart_policy() {
        let cfg = ContainerConfig {
            name: String::new(),
            binds: vec![],
            port_bindings: vec![],
            labels: HashMap::new(),
            network_mode: String::new(),
            restart_policy: "no".to_string(),
        };
        let args = DockerPlugin::build_run_args("img", &cfg);
        assert!(!args.contains(&"--restart".to_string()));
    }

    // ── build_ps_args (scope filtering) ────────────────────────────────────────

    #[test]
    fn build_ps_args_labeled_scope_adds_label_filter() {
        let p = DockerPlugin {
            docker_bin: "docker".to_string(),
            scope: DockerScope::default(), // Labeled, label = "aegis.snapshot"
        };
        let args = p.build_ps_args();
        assert_eq!(
            args,
            vec!["ps", "-q", "--filter", "label=aegis.snapshot=true"],
            "Labeled scope must filter by label"
        );
    }

    #[test]
    fn build_ps_args_all_scope_no_filters() {
        let p = DockerPlugin {
            docker_bin: "docker".to_string(),
            scope: DockerScope {
                mode: DockerScopeMode::All,
                ..DockerScope::default()
            },
        };
        let args = p.build_ps_args();
        assert_eq!(args, vec!["ps", "-q"], "All scope must not add any filters");
    }

    #[test]
    fn build_ps_args_names_scope_adds_name_filters() {
        let p = DockerPlugin {
            docker_bin: "docker".to_string(),
            scope: DockerScope {
                mode: DockerScopeMode::Names,
                name_patterns: vec!["web-.*".to_string(), "api".to_string()],
                ..DockerScope::default()
            },
        };
        let args = p.build_ps_args();
        assert_eq!(
            args,
            vec![
                "ps",
                "-q",
                "--filter",
                "name=web-.*",
                "--filter",
                "name=api"
            ],
            "Names scope must add --filter name=<pat> for each pattern"
        );
    }

    #[test]
    fn build_ps_args_labeled_scope_custom_label() {
        let p = DockerPlugin {
            docker_bin: "docker".to_string(),
            scope: DockerScope {
                mode: DockerScopeMode::Labeled,
                label: "com.myorg.backup".to_string(),
                name_patterns: vec![],
            },
        };
        let args = p.build_ps_args();
        assert_eq!(
            args,
            vec!["ps", "-q", "--filter", "label=com.myorg.backup=true"],
            "Custom label must be used in filter"
        );
    }

    // ── snapshot with scope (integration) ──────────────────────────────────────

    #[tokio::test]
    async fn snapshot_with_labeled_scope_passes_filter_to_docker_ps() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("calls.log");
        let log_path = log.to_string_lossy().into_owned();
        let inspect_json = MINIMAL_INSPECT;

        write_mock_docker(
            dir.path(),
            &format!(
                r#"printf "%s\n" "$*" >> {log_path}
case "$1" in
  ps)      printf "abc123\n"; exit 0 ;;
  inspect) printf '{inspect_json}'; exit 0 ;;
  commit)  printf "sha256:mockhash\n"; exit 0 ;;
  *)       exit 1 ;;
esac"#
            ),
        );

        let scope = DockerScope::default(); // Labeled
        let p = plugin_with_scope(&dir.path().join("docker"), scope);
        let _id = p.snapshot(Path::new("/"), "rm -rf /").await.unwrap();

        let calls = fs::read_to_string(&log).unwrap();
        assert!(
            calls.contains("--filter"),
            "Labeled scope must pass --filter to docker ps, got: {calls}"
        );
        assert!(
            calls.contains("label=aegis.snapshot=true"),
            "Labeled scope must filter by aegis.snapshot label, got: {calls}"
        );
    }

    #[tokio::test]
    async fn snapshot_with_all_scope_does_not_filter() {
        let dir = TempDir::new().unwrap();
        let log = dir.path().join("calls.log");
        let log_path = log.to_string_lossy().into_owned();
        let inspect_json = MINIMAL_INSPECT;

        write_mock_docker(
            dir.path(),
            &format!(
                r#"printf "%s\n" "$*" >> {log_path}
case "$1" in
  ps)      printf "abc123\n"; exit 0 ;;
  inspect) printf '{inspect_json}'; exit 0 ;;
  commit)  printf "sha256:mockhash\n"; exit 0 ;;
  *)       exit 1 ;;
esac"#
            ),
        );

        let scope = DockerScope {
            mode: DockerScopeMode::All,
            ..DockerScope::default()
        };
        let p = plugin_with_scope(&dir.path().join("docker"), scope);
        let _id = p.snapshot(Path::new("/"), "rm -rf /").await.unwrap();

        let calls = fs::read_to_string(&log).unwrap();
        // First line should be just "ps -q" without --filter
        let first_call = calls.lines().next().unwrap();
        assert!(
            !first_call.contains("--filter"),
            "All scope must NOT pass --filter to docker ps, got: {first_call}"
        );
    }
}
