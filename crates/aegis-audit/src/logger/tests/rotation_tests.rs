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
