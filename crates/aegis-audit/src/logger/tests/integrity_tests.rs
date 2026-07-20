use super::*;

#[test]
fn append_with_chain_sha256_populates_hash_fields() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"))
        .with_integrity_mode(AuditIntegrityMode::ChainSha256);

    logger.append(entry(0, RiskLevel::Safe)).unwrap();
    logger.append(entry(1, RiskLevel::Warn)).unwrap();

    let entries = logger.read_all().unwrap();
    let b0 = entries[0].as_base();
    let b1 = entries[1].as_base();
    assert_eq!(b0.chain_alg.as_deref(), Some("sha256"));
    assert!(b0.entry_hash.is_some());
    assert!(b0.prev_hash.is_none());
    assert_eq!(b1.chain_alg.as_deref(), Some("sha256"));
    assert_eq!(b1.prev_hash, b0.entry_hash);
}

#[test]
fn append_normalizes_legacy_fields_only_once() {
    let source = include_str!("../writer.rs");
    let append_start = source
        .find("pub fn append(&self, entry: AuditEntry) -> Result<()> {")
        .expect("append function must exist");
    let append_source = &source[append_start..];
    let next_fn = append_source
        .find("\n    pub(super) fn lock_path(&self) -> PathBuf {")
        .expect("append must be followed by read_all");
    let append_body = &append_source[..next_fn];

    let normalize_calls = append_body.matches("normalize_legacy_fields()").count();
    assert_eq!(
        normalize_calls, 1,
        "append must normalize legacy fields exactly once to avoid hidden repeat transforms"
    );
}

#[test]
fn compute_entry_hash_changes_when_explanation_changes() {
    let base_entry =
        entry(0, RiskLevel::Danger).with_explanation(explanation_with_match_text("rm -rf"));
    let changed_entry =
        entry(0, RiskLevel::Danger).with_explanation(explanation_with_match_text("rm -fr"));

    let entry_hash = compute_entry_hash(&base_entry, None).unwrap();
    let changed_hash = compute_entry_hash(&changed_entry, None).unwrap();

    assert_ne!(entry_hash, changed_hash);
}

#[test]
fn integrity_payload_omits_explanation_key_when_absent() {
    let entry = entry(0, RiskLevel::Warn);
    let base = entry.as_base();
    let payload = AuditIntegrityPayload {
        timestamp: base.timestamp,
        sequence: base.sequence,
        command: &base.command,
        risk: base.risk,
        matched_patterns: &base.matched_patterns,
        pattern_ids: &base.pattern_ids,
        decision: base.decision,
        snapshots: &base.snapshots,
        explanation: None,
        mode: base.mode,
        ci_detected: base.ci_detected,
        allowlist_matched: base.allowlist_matched,
        allowlist_effective: base.allowlist_effective,
        chain_alg: CHAIN_ALG_SHA256,
        prev_hash: None,
        allowlist_pattern: base.allowlist_pattern.as_deref(),
        allowlist_reason: base.allowlist_reason.as_deref(),
        source: None,
        cwd: None,
        id: None,
        transport: None,
        basis: None,
        analysis: None,
    };

    let json = serde_json::to_value(&payload).unwrap();

    assert!(json.get("explanation").is_none());
}

#[test]
fn verify_integrity_reports_no_data_for_legacy_entries() {
    let report = verify_integrity_entries(&[entry(0, RiskLevel::Safe), entry(1, RiskLevel::Warn)]);

    assert_eq!(report.status, AuditIntegrityStatus::NoIntegrityData);
}

#[test]
fn verify_integrity_detects_reordered_entries() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"))
        .with_integrity_mode(AuditIntegrityMode::ChainSha256);

    logger.append(entry(0, RiskLevel::Safe)).unwrap();
    logger.append(entry(1, RiskLevel::Warn)).unwrap();
    logger.append(entry(2, RiskLevel::Danger)).unwrap();

    let mut entries = logger.read_all().unwrap();
    entries.swap(1, 2);

    let report = verify_integrity_entries(&entries);
    assert_eq!(report.status, AuditIntegrityStatus::Corrupt);
    assert!(report.message.contains("chain link mismatch"));
}

#[cfg(unix)]
#[test]
fn verify_integrity_rejects_a_symlinked_gzip_segment() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    logger.append(entry(1, RiskLevel::Warn)).unwrap();
    let target = dir.path().join("outside-gzip");
    fs::write(&target, b"not relevant").unwrap();
    let archive = dir.path().join("audit.jsonl.1.gz");
    symlink(&target, &archive).unwrap();

    let error = logger.verify_integrity().unwrap_err();

    assert!(matches!(
        error,
        AuditError::InsecureAuditArtifact { path, .. }
            if path == archive.to_string_lossy()
    ));
}
