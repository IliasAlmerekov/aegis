use super::*;

#[test]
fn query_filters_by_risk() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    for (index, risk) in [
        RiskLevel::Safe,
        RiskLevel::Warn,
        RiskLevel::Danger,
        RiskLevel::Warn,
    ]
    .into_iter()
    .enumerate()
    {
        logger.append(entry(index, risk)).unwrap();
    }

    let entries = logger
        .query(AuditQuery {
            risk: Some(RiskLevel::Warn),
            ..AuditQuery::default()
        })
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert!(
        entries
            .iter()
            .all(|entry| entry.as_base().risk == RiskLevel::Warn)
    );
}

#[test]
fn query_returns_last_n_entries() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    for index in 0..5 {
        logger.append(entry(index, RiskLevel::Warn)).unwrap();
    }

    let entries = logger
        .query(AuditQuery {
            last: Some(2),
            ..AuditQuery::default()
        })
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].as_base().command, "command-3");
    assert_eq!(entries[1].as_base().command, "command-4");
}

#[test]
fn query_returns_last_n_entries_for_matching_risk_only() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    for (index, risk) in [
        RiskLevel::Safe,
        RiskLevel::Warn,
        RiskLevel::Danger,
        RiskLevel::Warn,
        RiskLevel::Danger,
        RiskLevel::Warn,
    ]
    .into_iter()
    .enumerate()
    {
        logger.append(entry(index, risk)).unwrap();
    }

    let entries = logger
        .query(AuditQuery {
            last: Some(2),
            risk: Some(RiskLevel::Warn),
            ..AuditQuery::default()
        })
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].as_base().command, "command-3");
    assert_eq!(entries[1].as_base().command, "command-5");
}

#[test]
fn query_filters_by_decision() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    for index in 0..6 {
        logger.append(entry(index, RiskLevel::Warn)).unwrap();
    }

    let entries = logger
        .query(AuditQuery {
            decision: Some(Decision::Blocked),
            ..AuditQuery::default()
        })
        .unwrap();

    assert!(!entries.is_empty());
    assert!(
        entries
            .iter()
            .all(|entry| entry.as_base().decision == Decision::Blocked)
    );
}

#[test]
fn query_filters_by_command_substring_case_sensitively() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    logger.append(entry(0, RiskLevel::Safe)).unwrap();
    logger.append(entry(1, RiskLevel::Warn)).unwrap();
    logger
        .append({
            let mut e = entry(2, RiskLevel::Warn);
            e.as_base_mut().command = "git stash clear".to_string();
            e
        })
        .unwrap();

    let entries = logger
        .query(AuditQuery {
            command_contains: Some("stash".to_string()),
            ..AuditQuery::default()
        })
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].as_base().command, "git stash clear");

    let no_match = logger
        .query(AuditQuery {
            command_contains: Some("Stash".to_string()),
            ..AuditQuery::default()
        })
        .unwrap();
    assert!(
        no_match.is_empty(),
        "substring filter must be case-sensitive"
    );
}

#[test]
fn query_filters_by_inclusive_time_range() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    for index in 0..4 {
        logger.append(entry(index, RiskLevel::Warn)).unwrap();
    }

    let entries = logger
        .query(AuditQuery {
            since: Some(AuditTimestamp::from_unix_seconds(1_700_000_001).unwrap()),
            until: Some(AuditTimestamp::from_unix_seconds(1_700_000_002).unwrap()),
            ..AuditQuery::default()
        })
        .unwrap();

    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.as_base().command.as_str())
            .collect::<Vec<_>>(),
        vec!["command-1", "command-2"]
    );
}

#[test]
fn query_applies_last_after_other_filters() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    for index in 0..6 {
        logger.append(entry(index, RiskLevel::Warn)).unwrap();
    }

    let entries = logger
        .query(AuditQuery {
            decision: Some(Decision::Denied),
            last: Some(1),
            ..AuditQuery::default()
        })
        .unwrap();

    assert_eq!(entries.len(), 1);
    let base = entries[0].as_base();
    assert_eq!(base.command, "command-5");
    assert_eq!(base.decision, Decision::Denied);
}

#[test]
fn query_last_handles_missing_trailing_newline() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    let lines = [
        entry(0, RiskLevel::Safe),
        entry(1, RiskLevel::Warn),
        entry(2, RiskLevel::Danger),
    ]
    .into_iter()
    .map(|entry| serde_json::to_string(&entry).unwrap())
    .collect::<Vec<_>>()
    .join("\n");

    fs::write(logger.path(), lines).unwrap();

    let entries = logger
        .query(AuditQuery {
            last: Some(2),
            ..AuditQuery::default()
        })
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].as_base().command, "command-1");
    assert_eq!(entries[1].as_base().command, "command-2");
}

#[test]
fn read_last_entry_skips_truncated_final_line() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    let first = serde_json::to_string(&entry(0, RiskLevel::Safe)).unwrap();
    let second = serde_json::to_string(&entry(1, RiskLevel::Warn)).unwrap();
    let contents = format!("{first}\n{second}\n{{\"timestamp\"");

    fs::write(logger.path(), contents).unwrap();

    let last = logger
        .read_last_entry_from_plain_file(logger.path())
        .unwrap()
        .expect("previous valid entry should be returned");

    assert_eq!(last.as_base().command, "command-1");
}

#[cfg(unix)]
#[test]
fn query_rejects_a_symlinked_archive_instead_of_returning_a_partial_view() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    logger.append(entry(1, RiskLevel::Warn)).unwrap();
    let target = dir.path().join("outside-segment");
    let mut bytes = serde_json::to_vec(&entry(0, RiskLevel::Safe)).unwrap();
    bytes.push(b'\n');
    fs::write(&target, bytes).unwrap();
    let archive = dir.path().join("audit.jsonl.1");
    symlink(&target, &archive).unwrap();

    let error = logger.read_all().unwrap_err();

    assert!(matches!(
        error,
        AuditError::InsecureAuditArtifact { path, .. }
            if path == archive.to_string_lossy()
    ));
}

#[cfg(unix)]
#[test]
fn query_rejects_a_broken_symlinked_active_log() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    symlink(dir.path().join("missing-target"), logger.path()).unwrap();

    let error = logger.read_all().unwrap_err();

    assert!(matches!(
        error,
        AuditError::InsecureAuditArtifact { path, .. }
            if path == logger.path().to_string_lossy()
    ));
}

#[cfg(unix)]
#[test]
fn query_tightens_an_owned_existing_archive() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    logger.append(entry(1, RiskLevel::Warn)).unwrap();
    let archive = dir.path().join("audit.jsonl.1");
    let mut bytes = serde_json::to_vec(&entry(0, RiskLevel::Safe)).unwrap();
    bytes.push(b'\n');
    fs::write(&archive, bytes).unwrap();
    fs::set_permissions(&archive, fs::Permissions::from_mode(0o644)).unwrap();

    let entries = logger.read_all().unwrap();

    assert_eq!(entries.len(), 2);
    let mode = fs::metadata(archive).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[cfg(unix)]
#[test]
fn query_rejects_an_unsafe_duplicate_archive_shape() {
    use std::io::Write as _;
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    logger.append(entry(1, RiskLevel::Warn)).unwrap();
    let gzip_archive = dir.path().join("audit.jsonl.1.gz");
    let archive = File::create(&gzip_archive).unwrap();
    let mut encoder = flate2::write::GzEncoder::new(archive, flate2::Compression::default());
    let mut bytes = serde_json::to_vec(&entry(0, RiskLevel::Safe)).unwrap();
    bytes.push(b'\n');
    encoder.write_all(&bytes).unwrap();
    encoder.finish().unwrap();
    let target = dir.path().join("outside-plain");
    fs::write(&target, &bytes).unwrap();
    let plain_archive = dir.path().join("audit.jsonl.1");
    symlink(target, &plain_archive).unwrap();

    let error = logger.read_all().unwrap_err();

    assert!(matches!(
        error,
        AuditError::InsecureAuditArtifact { path, .. }
            if path == plain_archive.to_string_lossy()
    ));
}

#[test]
fn query_of_an_absent_parent_is_empty_without_creating_filesystem_state() {
    let dir = TempDir::new().unwrap();
    let parent = dir.path().join("missing");
    let logger = AuditLogger::new(parent.join("audit.jsonl"));

    let entries = logger.read_all().unwrap();
    let integrity = logger.verify_integrity().unwrap();

    assert!(entries.is_empty());
    assert_eq!(integrity.status, AuditIntegrityStatus::NoIntegrityData);
    assert!(!parent.exists());
}
