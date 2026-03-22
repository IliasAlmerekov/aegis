//! Integration tests for [`DockerPlugin`] that exercise a **real Docker daemon**.
//!
//! # Why these exist
//!
//! Unit tests in `src/snapshot/docker.rs` use mock shell scripts in place of the
//! Docker CLI. That is fast and deterministic but it cannot catch:
//!
//! - Differences in `docker inspect` JSON schema across Docker versions.
//! - Whether `docker commit` genuinely preserves filesystem state.
//! - Whether a container recreated from the snapshot image actually starts.
//! - Whether the name / label / restart-policy flags are accepted by the real CLI.
//!
//! These tests fill that gap.
//!
//! # How to run
//!
//! The tests are **skipped by default**. To opt in, set the environment variable
//! and make sure the Docker daemon is running:
//!
//! ```bash
//! AEGIS_DOCKER_TESTS=1 cargo test --test docker_integration
//! ```
//!
//! In CI, add `AEGIS_DOCKER_TESTS: "1"` to the job environment and include a
//! `docker` service or DinD setup.
//!
//! # Assumptions
//!
//! - `docker` is on `PATH`.
//! - The `alpine` image is available (or can be pulled).
//! - Tests are not run in parallel with other processes that create containers
//!   named `aegis-itest-*` (the unique name prefix used here).

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use aegis::snapshot::{DockerPlugin, SnapshotPlugin};

// ─── guard ───────────────────────────────────────────────────────────────────

/// Returns `true` when the test opt-in variable is set and the Docker daemon
/// responds to `docker ps -q`.
fn docker_available() -> bool {
    if std::env::var("AEGIS_DOCKER_TESTS").is_err() {
        return false;
    }
    std::process::Command::new("docker")
        .args(["ps", "-q"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Macro that skips the calling test with a printed reason when Docker is not
/// available, instead of panicking. Using a macro keeps the call site clean and
/// avoids needing `return` from inside an async block.
macro_rules! require_docker {
    () => {
        if !docker_available() {
            eprintln!(
                "skipping: set AEGIS_DOCKER_TESTS=1 and start the Docker daemon to run this test"
            );
            return;
        }
    };
}

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Generate a unique container name that will not collide with other tests or
/// with containers already on the host.
fn unique_name(label: &str) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("aegis-itest-{label}-{ts}")
}

/// RAII guard that force-removes a container by **name** when dropped.
///
/// Using the name (not the ID) means the guard cleans up the container even
/// after rollback has recreated it under the same name.
struct ContainerGuard(String);

impl Drop for ContainerGuard {
    fn drop(&mut self) {
        // Best-effort: stop then remove. Ignore errors — the container may
        // already be gone, or stop may time out; either way we want rm.
        let _ = std::process::Command::new("docker")
            .args(["stop", "-t", "1", &self.0])
            .output();
        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", &self.0])
            .output();
    }
}

/// Start a detached `alpine sleep 3600` container and return its full container
/// ID. Panics if the container cannot be started.
async fn start_alpine(name: &str, extra_args: &[&str]) -> String {
    let mut args = vec!["run", "-d", "--name", name];
    args.extend_from_slice(extra_args);
    args.extend(["alpine", "sleep", "3600"]);

    let out = tokio::process::Command::new("docker")
        .args(&args)
        .output()
        .await
        .expect("failed to run docker run");

    assert!(
        out.status.success(),
        "docker run failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Run a command inside a container and return whether it succeeded.
async fn exec_in(container: &str, cmd: &str) -> bool {
    tokio::process::Command::new("docker")
        .args(["exec", container, "sh", "-c", cmd])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Retrieve a single field from `docker inspect` using a Go template.
async fn inspect_field(container: &str, template: &str) -> String {
    let out = tokio::process::Command::new("docker")
        .args(["inspect", "--format", template, container])
        .output()
        .await
        .expect("docker inspect failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

// ─── tests ────────────────────────────────────────────────────────────────────

/// `is_applicable` returns `true` when at least one container is running.
#[tokio::test]
async fn is_applicable_true_when_container_running() {
    require_docker!();

    let name = unique_name("applicable");
    let _guard = ContainerGuard(name.clone());
    start_alpine(&name, &[]).await;

    assert!(
        DockerPlugin::new().is_applicable(Path::new("/")),
        "is_applicable must return true when a container is running"
    );
}

/// `snapshot` calls `docker inspect` against a real container and successfully
/// parses the JSON. This catches schema differences across Docker versions.
///
/// Also verifies that the snapshot_id is well-formed (parseable JSON per line).
#[tokio::test]
async fn snapshot_parses_real_inspect_output() {
    require_docker!();

    let name = unique_name("inspect");
    let _guard = ContainerGuard(name.clone());
    start_alpine(&name, &[]).await;

    let plugin = DockerPlugin::new();
    let snapshot_id = plugin
        .snapshot(Path::new("/"), "docker rm -f test")
        .await
        .expect("snapshot must succeed against a real container");

    assert_ne!(snapshot_id, "none", "snapshot_id must not be the sentinel");

    // Every line must be valid JSON containing the expected keys.
    for line in snapshot_id.lines() {
        let v: serde_json::Value =
            serde_json::from_str(line).expect("each snapshot_id line must be valid JSON");
        assert!(
            v["container_id"].is_string(),
            "record must have container_id"
        );
        assert!(v["image"].is_string(), "record must have image");
        assert!(v["config"].is_object(), "record must have config object");
        assert!(
            v["config"]["network_mode"].is_string(),
            "config must have network_mode"
        );
        assert!(
            v["config"]["restart_policy"].is_string(),
            "config must have restart_policy"
        );
    }
}

/// After snapshot → modify filesystem → rollback, the modification must be
/// absent in the recreated container.
///
/// This is the core correctness test: it proves that `docker commit` genuinely
/// captures the filesystem state *before* the modification, and that `rollback`
/// starts a new container from that pre-modification image.
#[tokio::test]
async fn snapshot_rollback_reverts_filesystem_change() {
    require_docker!();

    let name = unique_name("lifecycle");
    let _guard = ContainerGuard(name.clone());

    start_alpine(&name, &[]).await;

    let plugin = DockerPlugin::new();

    // Take snapshot before the modification.
    let snapshot_id = plugin
        .snapshot(Path::new("/"), "docker rm -f test")
        .await
        .expect("snapshot must succeed");

    // Modify: write a marker file that should not exist after rollback.
    assert!(
        exec_in(&name, "echo aegis_marker > /aegis_marker_file").await,
        "docker exec must succeed"
    );
    assert!(
        exec_in(&name, "test -f /aegis_marker_file").await,
        "marker must exist before rollback"
    );

    // Roll back: stops + removes the container, recreates from snapshot image.
    plugin
        .rollback(&snapshot_id)
        .await
        .expect("rollback must succeed");

    // The container is now the rolled-back one, accessible by its original name.
    assert!(
        !exec_in(&name, "test -f /aegis_marker_file").await,
        "marker file must NOT exist in the rolled-back container"
    );
}

/// After rollback the recreated container must have the same name as the
/// original. This verifies that `--name` is captured in the snapshot and
/// replayed in the `docker run` invocation.
#[tokio::test]
async fn snapshot_rollback_preserves_container_name() {
    require_docker!();

    let name = unique_name("named");
    let _guard = ContainerGuard(name.clone());

    start_alpine(&name, &[]).await;

    let plugin = DockerPlugin::new();
    let snapshot_id = plugin
        .snapshot(Path::new("/"), "docker stop test")
        .await
        .expect("snapshot must succeed");

    plugin
        .rollback(&snapshot_id)
        .await
        .expect("rollback must succeed");

    // `docker inspect --format '{{.Name}}'` returns `/name`.
    let actual_name = inspect_field(&name, "{{.Name}}").await;
    assert_eq!(
        actual_name,
        format!("/{name}"),
        "rolled-back container must have the original name"
    );
}

/// After rollback the recreated container must carry the same user-defined
/// labels. This verifies that the `--label` flags are captured and replayed.
#[tokio::test]
async fn snapshot_rollback_preserves_labels() {
    require_docker!();

    let name = unique_name("labeled");
    let _guard = ContainerGuard(name.clone());

    start_alpine(&name, &["--label", "aegis-test=true", "--label", "env=ci"]).await;

    let plugin = DockerPlugin::new();
    let snapshot_id = plugin
        .snapshot(Path::new("/"), "docker stop test")
        .await
        .expect("snapshot must succeed");

    plugin
        .rollback(&snapshot_id)
        .await
        .expect("rollback must succeed");

    let label_val = inspect_field(&name, "{{index .Config.Labels \"aegis-test\"}}").await;
    assert_eq!(
        label_val, "true",
        "rolled-back container must carry the original label"
    );

    let env_label = inspect_field(&name, "{{index .Config.Labels \"env\"}}").await;
    assert_eq!(
        env_label, "ci",
        "rolled-back container must carry all original labels"
    );
}

/// After rollback the recreated container must have the same restart policy.
#[tokio::test]
async fn snapshot_rollback_preserves_restart_policy() {
    require_docker!();

    let name = unique_name("restart");
    let _guard = ContainerGuard(name.clone());

    start_alpine(&name, &["--restart", "on-failure"]).await;

    let plugin = DockerPlugin::new();
    let snapshot_id = plugin
        .snapshot(Path::new("/"), "docker stop test")
        .await
        .expect("snapshot must succeed");

    plugin
        .rollback(&snapshot_id)
        .await
        .expect("rollback must succeed");

    let policy = inspect_field(&name, "{{.HostConfig.RestartPolicy.Name}}").await;
    assert_eq!(
        policy, "on-failure",
        "rolled-back container must have the original restart policy"
    );
}

/// Rollback must succeed even when the original container has already been
/// removed before rollback runs. The `docker stop` and `docker rm` steps are
/// best-effort; only `docker run` failing is a hard error.
#[tokio::test]
async fn rollback_succeeds_when_original_container_already_removed() {
    require_docker!();

    let name = unique_name("preremoved");
    let _guard = ContainerGuard(name.clone());

    let container_id = start_alpine(&name, &[]).await;

    let plugin = DockerPlugin::new();
    let snapshot_id = plugin
        .snapshot(Path::new("/"), "docker rm -f test")
        .await
        .expect("snapshot must succeed");

    // Manually remove the container before calling rollback.
    let rm_out = tokio::process::Command::new("docker")
        .args(["rm", "-f", &container_id])
        .output()
        .await
        .unwrap();
    assert!(rm_out.status.success(), "pre-removal must succeed");

    // rollback must still succeed: stop/rm are best-effort, run should work.
    plugin
        .rollback(&snapshot_id)
        .await
        .expect("rollback must succeed even when original container was already removed");

    // The container is accessible by name again.
    let running = inspect_field(&name, "{{.State.Running}}").await;
    assert_eq!(running, "true", "rolled-back container must be running");
}

/// A port binding specified at container start must be captured by snapshot
/// and replayed on rollback.
///
/// Uses `127.0.0.1` to avoid binding on `0.0.0.0`. If the port is already in
/// use, the test will fail with a Docker error about port allocation — that is
/// an environment issue, not a bug in the plugin.
#[tokio::test]
async fn snapshot_rollback_preserves_port_binding() {
    require_docker!();

    let name = unique_name("ported");
    let _guard = ContainerGuard(name.clone());

    // Use a high, unlikely-to-conflict port. Binding to 127.0.0.1 only.
    start_alpine(&name, &["-p", "127.0.0.1:41980:80"]).await;

    let plugin = DockerPlugin::new();
    let snapshot_id = plugin
        .snapshot(Path::new("/"), "docker stop test")
        .await
        .expect("snapshot must succeed");

    plugin
        .rollback(&snapshot_id)
        .await
        .expect("rollback must succeed");

    // docker inspect: PortBindings["80/tcp"][0].HostPort
    let host_port = inspect_field(
        &name,
        "{{(index (index .HostConfig.PortBindings \"80/tcp\") 0).HostPort}}",
    )
    .await;
    assert_eq!(
        host_port, "41980",
        "rolled-back container must have the original port binding"
    );
}
