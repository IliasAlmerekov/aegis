use super::*;

#[tokio::test]
async fn delete_removes_image_when_docker_rmi_succeeds() {
    let dir = TempDir::new().unwrap();
    let log = dir.path().join("calls.log");
    let log_path = log.to_string_lossy().into_owned();

    write_mock_docker(
        dir.path(),
        &format!(
            r#"printf "%s\n" "$*" >> {log_path}
case "$1" in
  rmi) exit 0 ;;
  *)   exit 1 ;;
esac"#
        ),
    );

    let record = minimal_record("abc123", "aegis-snap-abc123-1700000000");
    plugin(&dir.path().join("docker"))
        .delete(&record)
        .await
        .unwrap();

    let calls = fs::read_to_string(&log).unwrap();
    assert!(
        calls.contains("rmi aegis-snap-abc123-1700000000"),
        "must call docker rmi"
    );
}

#[tokio::test]
async fn delete_is_idempotent_when_image_is_already_missing() {
    let dir = TempDir::new().unwrap();
    write_mock_docker(
        dir.path(),
        r#"case "$1" in
  rmi) printf "Error: No such image: aegis-snap-abc123-1700000000\n" >&2; exit 1 ;;
  *)   exit 1 ;;
esac"#,
    );

    let record = minimal_record("abc123", "aegis-snap-abc123-1700000000");
    plugin(&dir.path().join("docker"))
        .delete(&record)
        .await
        .unwrap();
}

#[tokio::test]
async fn delete_noop_for_no_containers_sentinel() {
    DockerPlugin::default().delete(NO_CONTAINERS).await.unwrap();
}

#[tokio::test]
async fn delete_fails_on_malformed_snapshot_id() {
    let result = DockerPlugin::default().delete("not-valid-json").await;
    assert!(
        result.is_err(),
        "malformed snapshot_id must return an error: {result:?}"
    );
}
