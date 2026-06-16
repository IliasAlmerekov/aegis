use super::*;

#[test]
fn append_and_read_back_pruned_entry() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    let pruned = AuditEntry::new(
        "aegis prune snap-abc123",
        RiskLevel::Safe,
        Vec::new(),
        Decision::Pruned,
        Vec::new(),
        None,
        None,
    );

    logger.append(pruned.clone()).unwrap();

    let read_back = logger.read_all().unwrap();
    assert_eq!(
        read_back.len(),
        1,
        "prune record must be appended exactly once"
    );
    let base = read_back[0].as_base();
    assert_eq!(base.decision, Decision::Pruned);
    assert!(
        base.command.contains("snap-abc123"),
        "command must name the pruned snapshot: {}",
        base.command
    );
}

#[test]
fn pruned_entry_participates_in_integrity_chain() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    let first = AuditEntry::new(
        "rm -rf src",
        RiskLevel::Danger,
        Vec::new(),
        Decision::Approved,
        Vec::new(),
        None,
        None,
    );
    logger.append(first).unwrap();

    let pruned = AuditEntry::new(
        "aegis prune snap-abc123",
        RiskLevel::Safe,
        Vec::new(),
        Decision::Pruned,
        Vec::new(),
        None,
        None,
    );
    logger.append(pruned).unwrap();

    let report = logger.verify_integrity().unwrap();
    assert_eq!(
        report.status,
        AuditIntegrityStatus::Verified,
        "audit chain must verify after a Pruned entry: {}",
        report.message
    );
}
