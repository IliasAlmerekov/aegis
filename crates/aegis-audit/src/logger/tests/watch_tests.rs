use super::*;

#[test]
fn watch_context_fields_round_trip_through_json() {
    let entry = AuditEntry::new(
        "git status",
        RiskLevel::Safe,
        vec![],
        Decision::AutoApproved,
        vec![],
        None,
        None,
    )
    .with_watch_context(
        Some("claude".to_string()),
        Some("/home/user/project".to_string()),
        Some("frame-42".to_string()),
    );

    let json = serde_json::to_string(&entry).unwrap();
    let back: AuditEntry = serde_json::from_str(&json).unwrap();

    let watch = match &back {
        AuditEntry::Watch(w) => w,
        _ => panic!("expected Watch variant after round-trip"),
    };
    assert_eq!(watch.source.as_deref(), Some("claude"));
    assert_eq!(watch.cwd.as_deref(), Some("/home/user/project"));
    assert_eq!(watch.id.as_deref(), Some("frame-42"));
    assert!(json.contains(r#""transport":"watch""#));
}

#[test]
fn watch_context_fields_absent_when_not_set() {
    let entry = AuditEntry::new(
        "ls",
        RiskLevel::Safe,
        vec![],
        Decision::AutoApproved,
        vec![],
        None,
        None,
    );

    let json = serde_json::to_string(&entry).unwrap();
    assert!(!json.contains("source"), "source must be absent when None");
    assert!(
        !json.contains("transport"),
        "transport must be absent when None"
    );
}
