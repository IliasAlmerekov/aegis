use super::*;

#[tokio::test]
async fn is_applicable_no_docker_cli() {
    let p = DockerPlugin {
        docker_bin: "/nonexistent/bin/docker".to_string(),
        scope: DockerScope::default(),
    };
    assert!(!p.is_applicable(Path::new("/")).await);
}

#[tokio::test]
async fn is_applicable_no_running_containers() {
    let dir = TempDir::new().unwrap();
    write_mock_docker(
        dir.path(),
        r#"case "$1" in
  ps) exit 0 ;;
  *) exit 1 ;;
esac"#,
    );
    assert!(
        !plugin(&dir.path().join("docker"))
            .is_applicable(Path::new("/"))
            .await
    );
}

#[tokio::test]
async fn is_applicable_with_running_containers() {
    let dir = TempDir::new().unwrap();
    write_mock_docker(
        dir.path(),
        r#"case "$1" in
  ps) printf "abc123\n"; exit 0 ;;
  *) exit 1 ;;
esac"#,
    );
    assert!(
        plugin(&dir.path().join("docker"))
            .is_applicable(Path::new("/"))
            .await
    );
}

#[tokio::test]
async fn is_applicable_docker_not_running() {
    let dir = TempDir::new().unwrap();
    write_mock_docker(
        dir.path(),
        r#"case "$1" in
  ps) echo "Cannot connect to the Docker daemon" >&2; exit 1 ;;
  *) exit 1 ;;
esac"#,
    );
    assert!(
        !plugin(&dir.path().join("docker"))
            .is_applicable(Path::new("/"))
            .await
    );
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
