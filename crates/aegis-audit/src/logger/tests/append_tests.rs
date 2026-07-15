use super::*;

#[test]
fn append_and_read_back_five_entries_field_by_field() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    let written = vec![
        entry(0, RiskLevel::Safe),
        entry(1, RiskLevel::Warn),
        entry(2, RiskLevel::Danger),
        entry(3, RiskLevel::Block),
        entry(4, RiskLevel::Warn),
    ];

    for entry in &written {
        logger.append(entry.clone()).unwrap();
    }

    let read_back = logger.read_all().unwrap();
    assert_eq!(read_back.len(), 5);

    for (expected, actual) in written.iter().zip(read_back.iter()) {
        let exp = expected.as_base();
        let act = actual.as_base();
        assert_eq!(act.timestamp, exp.timestamp);
        assert_eq!(act.command, exp.command);
        assert_eq!(act.risk, exp.risk);
        assert_eq!(act.decision, exp.decision);
        assert_eq!(act.matched_patterns.len(), exp.matched_patterns.len());
        assert_eq!(act.snapshots.len(), exp.snapshots.len());

        for (ep, ap) in exp.matched_patterns.iter().zip(act.matched_patterns.iter()) {
            assert_eq!(ap.id, ep.id);
            assert_eq!(ap.risk, ep.risk);
            assert_eq!(ap.description, ep.description);
            assert_eq!(ap.safe_alt, ep.safe_alt);
        }

        for (es, as_) in exp.snapshots.iter().zip(act.snapshots.iter()) {
            assert_eq!(as_.plugin, es.plugin);
            assert_eq!(as_.snapshot_id, es.snapshot_id);
        }
    }
}

#[test]
fn append_serializes_rfc3339_timestamp_and_sequence() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    logger.append(entry(0, RiskLevel::Safe)).unwrap();

    let written = fs::read_to_string(logger.path()).unwrap();
    let json: serde_json::Value = serde_json::from_str(written.trim()).unwrap();

    assert_eq!(json["timestamp"], "2023-11-14T22:13:20Z");
    assert_eq!(json["sequence"], 1);
}

#[test]
fn append_serializes_pattern_ids_and_allowlist_flags() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    logger.append(entry(0, RiskLevel::Warn)).unwrap();

    let written = fs::read_to_string(logger.path()).unwrap();
    let json: serde_json::Value = serde_json::from_str(written.trim()).unwrap();

    assert_eq!(json["pattern_ids"], serde_json::json!(["PAT-000"]));
    assert_eq!(json["allowlist_matched"], serde_json::json!(false));
    assert_eq!(json["allowlist_effective"], serde_json::json!(false));
}

#[test]
fn new_does_not_create_files_or_directories() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("nested/audit.jsonl"));

    assert!(!logger.path().exists());
    assert!(!logger.lock_path().exists());
}

#[test]
fn append_creates_parent_and_writes_entry_without_prebuilt_helpers() {
    let temp = tempfile::TempDir::new().unwrap();
    let log_path = temp.path().join("nested/audit.jsonl");
    let logger = AuditLogger::new(&log_path);

    let entry = AuditEntry::new(
        "echo hello".to_string(),
        RiskLevel::Safe,
        Vec::new(),
        Decision::Approved,
        Vec::new(),
        None,
        None,
    );

    logger.append(entry).unwrap();

    assert!(log_path.exists());
    let contents = std::fs::read_to_string(log_path).unwrap();
    assert!(contents.contains("\"command\":\"echo hello\""));
}

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

#[cfg(unix)]
#[test]
fn append_creates_owner_only_audit_directories_and_artifacts() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let first_directory = dir.path().join("first");
    let second_directory = first_directory.join("second");
    let logger = AuditLogger::new(second_directory.join("audit.jsonl"));

    logger.append(entry(0, RiskLevel::Safe)).unwrap();

    for directory in [&first_directory, &second_directory] {
        let mode = fs::metadata(directory).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "{} must be owner-only", directory.display());
    }
    for artifact in [logger.path(), logger.lock_path().as_path()] {
        let mode = fs::metadata(artifact).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{} must be owner-only", artifact.display());
    }
}

#[cfg(unix)]
#[test]
fn append_tightens_owned_existing_audit_artifacts() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    fs::write(logger.path(), []).unwrap();
    fs::write(logger.lock_path(), []).unwrap();
    fs::set_permissions(logger.path(), fs::Permissions::from_mode(0o644)).unwrap();
    fs::set_permissions(logger.lock_path(), fs::Permissions::from_mode(0o644)).unwrap();

    logger.append(entry(0, RiskLevel::Safe)).unwrap();

    for artifact in [logger.path(), logger.lock_path().as_path()] {
        let mode = fs::metadata(artifact).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{} must be tightened", artifact.display());
    }
}

#[cfg(unix)]
#[test]
fn append_rejects_a_symlinked_active_log_without_touching_its_target() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let target = dir.path().join("target");
    fs::write(&target, b"sentinel").unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    symlink(&target, logger.path()).unwrap();

    let error = logger.append(entry(0, RiskLevel::Safe)).unwrap_err();

    assert!(matches!(
        error,
        AuditError::InsecureAuditArtifact { path, .. }
            if path == logger.path().to_string_lossy()
    ));
    assert_eq!(fs::read(target).unwrap(), b"sentinel");
}

#[cfg(unix)]
#[test]
fn append_rejects_a_symlinked_immediate_parent() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let target = dir.path().join("target-directory");
    fs::create_dir(&target).unwrap();
    let parent = dir.path().join("audit-directory");
    symlink(&target, &parent).unwrap();
    let logger = AuditLogger::new(parent.join("audit.jsonl"));

    let error = logger.append(entry(0, RiskLevel::Safe)).unwrap_err();

    assert!(matches!(
        error,
        AuditError::InsecureAuditArtifact { path, .. }
            if path == parent.to_string_lossy()
    ));
    assert!(fs::read_dir(target).unwrap().next().is_none());
}

#[cfg(unix)]
#[test]
fn append_rejects_a_non_regular_active_log_with_a_typed_error() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    fs::create_dir(logger.path()).unwrap();

    let error = logger.append(entry(0, RiskLevel::Safe)).unwrap_err();

    assert!(matches!(
        error,
        AuditError::InsecureAuditArtifact { path, .. }
            if path == logger.path().to_string_lossy()
    ));
}

#[cfg(unix)]
#[test]
fn append_leaves_a_preexisting_parent_mode_unchanged() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let parent = dir.path().join("caller-owned");
    fs::create_dir(&parent).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o750)).unwrap();
    let logger = AuditLogger::new(parent.join("audit.jsonl"));

    logger.append(entry(0, RiskLevel::Safe)).unwrap();

    let mode = fs::metadata(parent).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o750);
}

#[cfg(unix)]
#[test]
fn append_rejects_a_symlinked_lock_without_touching_its_target() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));
    let target = dir.path().join("lock-target");
    fs::write(&target, b"sentinel").unwrap();
    symlink(&target, logger.lock_path()).unwrap();

    let error = logger.append(entry(0, RiskLevel::Safe)).unwrap_err();

    assert!(matches!(
        error,
        AuditError::InsecureAuditArtifact { path, .. }
            if path == logger.lock_path().to_string_lossy()
    ));
    assert_eq!(fs::read(target).unwrap(), b"sentinel");
}

#[test]
fn append_creates_companion_lock_file() {
    let dir = TempDir::new().unwrap();
    let logger = AuditLogger::new(dir.path().join("audit.jsonl"));

    logger.append(entry(0, RiskLevel::Safe)).unwrap();

    assert!(
        dir.path().join("audit.jsonl.lock").exists(),
        "append path must create a companion lock file"
    );
}
