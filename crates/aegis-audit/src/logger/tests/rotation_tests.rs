use super::*;

#[test]
fn rotation_keeps_archives_and_queries_span_them() {
    let dir = TempDir::new().unwrap();
    let max_bytes = entry_bytes(0, RiskLevel::Warn) as u64 + 1;
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"))
        .with_rotation(rotation_policy(max_bytes, 3, false));

    for index in 0..3 {
        logger.append(entry(index, RiskLevel::Warn)).unwrap();
    }

    assert!(dir.path().join("audit.jsonl.1").exists());
    assert!(dir.path().join("audit.jsonl.2").exists());
    #[cfg(unix)]
    for archive in [
        dir.path().join("audit.jsonl.1"),
        dir.path().join("audit.jsonl.2"),
    ] {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(archive).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    let all = logger.read_all().unwrap();
    assert_eq!(
        all.iter()
            .map(|entry| entry.as_base().command.as_str())
            .collect::<Vec<_>>(),
        vec!["command-0", "command-1", "command-2",]
    );

    let last = logger
        .query(AuditQuery {
            last: Some(2),
            ..AuditQuery::default()
        })
        .unwrap();
    assert_eq!(
        last.iter()
            .map(|entry| entry.as_base().command.as_str())
            .collect::<Vec<_>>(),
        vec!["command-1", "command-2"]
    );
}

#[test]
fn rotation_can_compress_archives_and_still_read_them() {
    let dir = TempDir::new().unwrap();
    let max_bytes = entry_bytes(0, RiskLevel::Warn) as u64 + 1;
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"))
        .with_rotation(rotation_policy(max_bytes, 2, true));

    logger.append(entry(0, RiskLevel::Warn)).unwrap();
    logger.append(entry(1, RiskLevel::Warn)).unwrap();

    let archive_path = dir.path().join("audit.jsonl.1.gz");
    assert!(archive_path.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&archive_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    let mut decompressed = String::new();
    GzDecoder::new(File::open(&archive_path).unwrap())
        .read_to_string(&mut decompressed)
        .unwrap();
    assert!(decompressed.contains("command-0"));

    let all = logger.read_all().unwrap();
    assert_eq!(
        all.iter()
            .map(|entry| entry.as_base().command.as_str())
            .collect::<Vec<_>>(),
        vec!["command-0", "command-1"]
    );
}

#[test]
fn rotation_enforces_retention_limit() {
    let dir = TempDir::new().unwrap();
    let max_bytes = entry_bytes(0, RiskLevel::Warn) as u64 + 1;
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"))
        .with_rotation(rotation_policy(max_bytes, 2, false));

    for index in 0..4 {
        logger.append(entry(index, RiskLevel::Warn)).unwrap();
    }

    assert!(dir.path().join("audit.jsonl.1").exists());
    assert!(dir.path().join("audit.jsonl.2").exists());
    assert!(!dir.path().join("audit.jsonl.3").exists());

    let all = logger.read_all().unwrap();
    assert_eq!(
        all.iter()
            .map(|entry| entry.as_base().command.as_str())
            .collect::<Vec<_>>(),
        vec!["command-1", "command-2", "command-3"]
    );
}

#[cfg(unix)]
#[test]
fn rotation_rejects_an_unsafe_managed_slot_before_mutating_archives() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    AuditLogger::new(&path)
        .append(entry(0, RiskLevel::Warn))
        .unwrap();
    let first = dir.path().join("audit.jsonl.1");
    let second = dir.path().join("audit.jsonl.2");
    let retained = dir.path().join("audit.jsonl.3");
    fs::write(&first, b"first").unwrap();
    let symlink_target = dir.path().join("outside");
    fs::write(&symlink_target, b"outside").unwrap();
    symlink(&symlink_target, &second).unwrap();
    fs::write(&retained, b"retained").unwrap();
    let active_before = fs::read(&path).unwrap();
    let logger = AuditLogger::new(&path).with_rotation(rotation_policy(0, 3, false));

    let error = logger.append(entry(1, RiskLevel::Warn)).unwrap_err();

    assert!(matches!(
        error,
        AuditError::InsecureAuditArtifact { path, .. }
            if path == second.to_string_lossy()
    ));
    assert_eq!(fs::read(&path).unwrap(), active_before);
    assert_eq!(fs::read(first).unwrap(), b"first");
    assert_eq!(fs::read(retained).unwrap(), b"retained");
    assert!(
        fs::symlink_metadata(second)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(symlink_target).unwrap(), b"outside");
}

#[test]
fn gzip_failure_preserves_the_active_log_and_exposes_no_partial_archive() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    AuditLogger::new(&path)
        .append(entry(0, RiskLevel::Warn))
        .unwrap();
    let active_before = fs::read(&path).unwrap();
    let logger = AuditLogger::new(&path).with_rotation(rotation_policy(0, 2, true));
    super::super::rotation::inject_gzip_failure();

    let error = logger.append(entry(1, RiskLevel::Warn)).unwrap_err();

    assert!(matches!(error, AuditError::Io(_)));
    assert_eq!(fs::read(&path).unwrap(), active_before);
    assert!(!dir.path().join("audit.jsonl.1.gz").exists());
    assert!(!dir.path().join("audit.jsonl.1.gz.tmp").exists());
}

#[test]
fn compressed_rotation_recovers_a_safe_stale_staging_artifact() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    AuditLogger::new(&path)
        .append(entry(0, RiskLevel::Warn))
        .unwrap();
    let staging = dir.path().join("audit.jsonl.1.gz.tmp");
    fs::write(&staging, b"stale").unwrap();
    let logger = AuditLogger::new(&path).with_rotation(rotation_policy(0, 2, true));

    logger.append(entry(1, RiskLevel::Warn)).unwrap();

    assert!(!staging.exists());
    assert!(dir.path().join("audit.jsonl.1.gz").exists());
    let entries = logger.read_all().unwrap();
    assert_eq!(entries.len(), 2);
}

#[cfg(unix)]
#[test]
fn unsafe_staging_aborts_compressed_rotation_before_archive_mutation() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("audit.jsonl");
    AuditLogger::new(&path)
        .append(entry(0, RiskLevel::Warn))
        .unwrap();
    let active_before = fs::read(&path).unwrap();
    let first = dir.path().join("audit.jsonl.1.gz");
    fs::write(&first, b"existing archive").unwrap();
    let first_before = fs::read(&first).unwrap();
    let target = dir.path().join("outside-staging");
    fs::write(&target, b"outside").unwrap();
    let staging = dir.path().join("audit.jsonl.1.gz.tmp");
    symlink(&target, &staging).unwrap();
    let logger = AuditLogger::new(&path).with_rotation(rotation_policy(0, 2, true));

    let error = logger.append(entry(1, RiskLevel::Warn)).unwrap_err();

    assert!(matches!(
        error,
        AuditError::InsecureAuditArtifact { path, .. }
            if path == staging.to_string_lossy()
    ));
    assert_eq!(fs::read(path).unwrap(), active_before);
    assert_eq!(fs::read(first).unwrap(), first_before);
    assert!(
        fs::symlink_metadata(staging)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(target).unwrap(), b"outside");
}
