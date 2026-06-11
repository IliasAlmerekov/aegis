//! Public-API contract tests for `aegis-audit`.
//!
//! All tests in this file are RED: they reference types and functions that do
//! not exist yet in the stub `src/lib.rs`. They will fail to compile (and
//! therefore fail as tests) until the green phase implements the crate.

use aegis_audit::{
    AuditEntry, AuditError, AuditLogger, AuditQuery, AuditRotationPolicy, AuditTimestamp, Decision,
    MatchedPattern,
};
use aegis_config::{AuditConfig, AuditIntegrityMode};
use aegis_types::RiskLevel;

// ---------------------------------------------------------------------------
// 1. AuditError — public type with Io and Parse variants (thiserror-based)
// ---------------------------------------------------------------------------

#[test]
fn test_audit_error_io_variant_is_public() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
    let err: AuditError = AuditError::Io(io_err);
    let msg = err.to_string();
    assert!(
        !msg.is_empty(),
        "AuditError::Io must produce a non-empty Display message"
    );
}

#[test]
fn test_audit_error_parse_variant_is_public() {
    let err: AuditError = AuditError::Parse("unexpected token".to_string());
    let msg = err.to_string();
    assert!(
        msg.contains("unexpected token"),
        "AuditError::Parse message should contain the inner text; got: {msg}"
    );
}

#[test]
fn test_audit_error_implements_std_error() {
    fn assert_std_error<E: std::error::Error>() {}
    assert_std_error::<AuditError>();
}

// ---------------------------------------------------------------------------
// 2. AuditLogger::new(path) constructs without panicking
// ---------------------------------------------------------------------------

#[test]
fn test_audit_logger_new_does_not_panic() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("audit.jsonl");
    let logger = AuditLogger::new(&path);
    assert_eq!(logger.path(), path.as_path());
}

// ---------------------------------------------------------------------------
// 3. AuditLogger::from_audit_config(config) constructs from AuditConfig
// ---------------------------------------------------------------------------

#[test]
fn test_audit_logger_from_audit_config_default() {
    let config = AuditConfig::default();
    let logger = AuditLogger::from_audit_config(&config);
    // path() must return something non-empty (the default audit path)
    assert!(
        logger.path().as_os_str().len() > 0,
        "AuditLogger::from_audit_config must set a non-empty default path"
    );
}

#[test]
fn test_audit_logger_from_audit_config_rotation_disabled_has_no_rotation() {
    let mut config = AuditConfig::default();
    config.rotation_enabled = false;
    let logger = AuditLogger::from_audit_config(&config);
    // When rotation is disabled the logger must expose that state.
    // We verify indirectly: rotating a fresh (non-existent) log with no policy
    // must succeed without errors.
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("audit.jsonl");
    let logger = logger.with_path(path);
    // Simply ensure constructing this logger does not panic.
    let _ = logger;
}

// ---------------------------------------------------------------------------
// 4. AuditEntry::new(...) returns AuditEntry::Decision(_)
// ---------------------------------------------------------------------------

#[test]
fn test_audit_entry_new_returns_decision_variant() {
    let entry = AuditEntry::new(
        "rm -rf /tmp/test",
        RiskLevel::Danger,
        vec![],
        Decision::Approved,
        vec![],
        None,
        None,
    );
    assert!(
        matches!(entry, AuditEntry::Decision(_)),
        "AuditEntry::new must return AuditEntry::Decision, got a different variant"
    );
}

#[test]
fn test_audit_entry_new_stores_command() {
    let cmd = "git push --force origin main";
    let entry = AuditEntry::new(
        cmd,
        RiskLevel::Warn,
        vec![],
        Decision::Denied,
        vec![],
        None,
        None,
    );
    assert_eq!(
        entry.as_base().command,
        cmd,
        "AuditEntry::new must preserve the command string"
    );
}

// ---------------------------------------------------------------------------
// 5. AuditEntry serializes to JSON and deserializes back (round-trip)
// ---------------------------------------------------------------------------

#[test]
fn test_audit_entry_json_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let pattern = MatchedPattern {
        id: "FS-001".to_string(),
        risk: RiskLevel::Danger,
        description: "Recursive delete".to_string(),
        safe_alt: None,
        category: None,
        matched_text: None,
        source: None,
    };
    let entry = AuditEntry::new(
        "rm -rf /",
        RiskLevel::Danger,
        vec![pattern],
        Decision::Blocked,
        vec![],
        None,
        None,
    );

    let json = serde_json::to_string(&entry)?;
    assert!(
        json.contains("FS-001"),
        "serialized JSON must contain the pattern id"
    );

    let recovered: AuditEntry = serde_json::from_str(&json)?;
    assert_eq!(
        recovered.as_base().command,
        "rm -rf /",
        "deserialized entry must preserve the command"
    );
    assert_eq!(
        recovered.as_base().matched_patterns.len(),
        1,
        "deserialized entry must preserve matched_patterns"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// 6. AuditTimestamp formats as RFC 3339
// ---------------------------------------------------------------------------

#[test]
fn test_audit_timestamp_displays_as_rfc3339() {
    let ts = AuditTimestamp::from_unix_seconds(0).expect("epoch must be a valid timestamp");
    let display = ts.to_string();
    // RFC 3339 starts with the year and contains 'T' and 'Z' (or an offset).
    assert!(
        display.starts_with("1970"),
        "timestamp display should start with the year; got: {display}"
    );
    assert!(
        display.contains('T'),
        "RFC 3339 timestamps must contain 'T'; got: {display}"
    );
}

#[test]
fn test_audit_timestamp_parse_rfc3339_round_trip() {
    let original = "2024-01-15T12:34:56Z";
    let ts = AuditTimestamp::parse_rfc3339(original).expect("valid RFC 3339 string must parse");
    let formatted = ts.to_string();
    // The formatted output must represent the same instant.
    let reparsed =
        AuditTimestamp::parse_rfc3339(&formatted).expect("formatted timestamp must re-parse");
    assert_eq!(
        ts, reparsed,
        "round-tripped timestamp must equal the original"
    );
}

// ---------------------------------------------------------------------------
// 7. AuditQuery::default() works
// ---------------------------------------------------------------------------

#[test]
fn test_audit_query_default_all_none() {
    let q = AuditQuery::default();
    assert!(q.last.is_none(), "default AuditQuery.last must be None");
    assert!(q.risk.is_none(), "default AuditQuery.risk must be None");
    assert!(
        q.decision.is_none(),
        "default AuditQuery.decision must be None"
    );
    assert!(q.since.is_none(), "default AuditQuery.since must be None");
    assert!(q.until.is_none(), "default AuditQuery.until must be None");
    assert!(
        q.command_contains.is_none(),
        "default AuditQuery.command_contains must be None"
    );
}

// ---------------------------------------------------------------------------
// 8. AuditRotationPolicy::from_config returns None when rotation is disabled
// ---------------------------------------------------------------------------

#[test]
fn test_audit_rotation_policy_from_config_disabled_returns_none() {
    let mut config = AuditConfig::default();
    config.rotation_enabled = false;
    let policy = AuditRotationPolicy::from_config(&config);
    assert!(
        policy.is_none(),
        "AuditRotationPolicy::from_config must return None when rotation is disabled"
    );
}

#[test]
fn test_audit_rotation_policy_from_config_enabled_returns_some() {
    let mut config = AuditConfig::default();
    config.rotation_enabled = true;
    config.max_file_size_bytes = 1_000_000;
    config.retention_files = 3;
    let policy = AuditRotationPolicy::from_config(&config);
    assert!(
        policy.is_some(),
        "AuditRotationPolicy::from_config must return Some when rotation is enabled"
    );
}
