use super::*;

#[tokio::test]
async fn rollback_retries_when_docker_binary_is_temporarily_busy() {
    let dir = TempDir::new().unwrap();
    let docker_bin = write_mock_docker(
        dir.path(),
        r#"case "$1" in
  stop) exit 0 ;;
  rm)   exit 0 ;;
  run)  printf "newcontainer\n"; exit 0 ;;
  *)    exit 1 ;;
esac"#,
    );
    let quoted_path = single_quote_for_shell(&docker_bin);
    let mut holder = StdCommand::new("/bin/sh")
        .arg("-c")
        .arg(format!("exec 3>>'{quoted_path}'; sleep 0.3"))
        .spawn()
        .unwrap();

    std::thread::sleep(std::time::Duration::from_millis(25));

    let snapshot_id = minimal_record("abc123", "aegis-snap-abc123-1700000000");
    let result = plugin(&docker_bin).rollback(&snapshot_id).await;

    let _ = holder.wait();
    assert!(
        result.is_ok(),
        "rollback should retry transient ETXTBSY from mock docker binary: {result:?}"
    );
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

#[tokio::test]
async fn rollback_fails_when_image_is_missing_from_runtime() {
    let dir = TempDir::new().unwrap();
    write_mock_docker(
        dir.path(),
        r#"case "$1" in
  stop) exit 0 ;;
  rm)   exit 0 ;;
  run)  printf "Error: No such image\n" >&2; exit 1 ;;
  *)    exit 1 ;;
esac"#,
    );

    let record = minimal_record("abc123", "missing-image");
    let result = plugin(&dir.path().join("docker")).rollback(&record).await;

    assert!(result.is_err(), "rollback should fail on missing image");
    if let Err(err) = result {
        match err {
            AegisError::Snapshot(msg) => {
                assert!(msg.contains("docker run missing-image failed"));
                assert!(msg.contains("No such image"));
            }
            other => panic!("expected snapshot error, got: {other:?}"),
        }
    }
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

// ── async-safety regression tests ─────────────────────────────────────────

/// Checks counter before awaiting bg task to prove concurrent progress.
///
/// With blocking `is_applicable` on a `current_thread` runtime:
///   - The single Tokio thread is held for ~50ms by the blocking `std::process::Command`
///   - The bg task cannot be scheduled during that time
///   - `counter_after` == 0  → assertion fails (as desired for a red test)
///
/// After the fix (async `is_applicable` with `tokio::process::Command`):
///   - The Tokio thread is yielded at each `.await` point
///   - The bg task wakes after 10ms and increments the counter
///   - `counter_after` == 1  → assertion passes
#[tokio::test(flavor = "current_thread")]
async fn is_applicable_does_not_block_tokio_runtime_v2() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let dir = TempDir::new().unwrap();
    write_mock_docker(
        dir.path(),
        r#"case "$1" in
  ps) sleep 0.05; printf "abc123\n"; exit 0 ;;
  *) exit 1 ;;
esac"#,
    );

    let p = plugin(&dir.path().join("docker"));
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_bg = Arc::clone(&counter);

    let bg = tokio::task::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        counter_bg.fetch_add(1, Ordering::SeqCst);
    });

    // Async call — yields the Tokio thread while waiting for docker ps.
    let _ = p.is_applicable(Path::new("/")).await;

    // Read counter *before* awaiting bg. With blocking is_applicable the bg task
    // was never polled while is_applicable ran, so counter is still 0 here.
    let counter_after = counter.load(Ordering::SeqCst);

    bg.await.unwrap();

    // This assertion FAILS with the current blocking implementation:
    // counter_after == 0, not 1.
    assert_eq!(
        counter_after, 1,
        "is_applicable blocked the Tokio thread — background task could not progress \
         (counter={counter_after}, expected 1). Fix: make is_applicable async and use \
         tokio::process::Command."
    );
}

/// Verifies that `sleep_docker_busy_retry_delay` yields the current-thread runtime
/// so that other tasks can make progress during the delay.
///
/// The retry delay must use `tokio::time::sleep`, which is cheaper than
/// dispatching a blocking sleep to a helper thread and still yields the
/// runtime while the delay is pending.
///
/// This is a behavioral contract test: it asserts the function MUST yield the runtime
/// and will catch any regression that changes it to a blocking call (e.g., replacing
/// the body with a bare `thread::sleep` call).
#[tokio::test(flavor = "current_thread")]
async fn sleep_docker_busy_retry_delay_yields_to_runtime() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let flag = Arc::new(AtomicBool::new(false));
    let flag_bg = Arc::clone(&flag);

    // Background task sets the flag after 5ms.
    let bg = tokio::task::spawn(async move {
        tokio::time::sleep(Duration::from_millis(5)).await;
        flag_bg.store(true, Ordering::SeqCst);
    });

    // sleep_docker_busy_retry_delay sleeps for DOCKER_BUSY_RETRY_DELAY_MS (25ms).
    // A properly yielding implementation allows the bg task (5ms) to
    // complete before this returns.
    sleep_docker_busy_retry_delay().await;

    let flag_value = flag.load(Ordering::SeqCst);

    bg.await.unwrap();

    assert!(
        flag_value,
        "sleep_docker_busy_retry_delay did not yield to the runtime — \
         background task flag was not set (expected true, got false). \
         This would be a regression; the function must not block the Tokio thread."
    );
}
