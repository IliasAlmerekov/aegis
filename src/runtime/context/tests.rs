use std::fs;
use std::path::Path;

use super::*;
use crate::config::{CiPolicy, UserPattern};
use crate::decision::{ExecutionTransport, PolicyAction, PolicyRationale};
use crate::explanation::formatter::allowlist_explanation_from;
use crate::explanation::{
    CommandExplanation, ExecutionContextExplanation, ExplainedPatternMatch, PolicyExplanation,
    ScanExplanation,
};
use crate::interceptor::RiskLevel;
use crate::interceptor::patterns::Category;
use tempfile::TempDir;
use time::OffsetDateTime;

fn test_handle() -> Handle {
    // Leak a runtime so the Handle outlives each test.
    // This is fine for unit tests — the OS reclaims it on process exit.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let handle = rt.handle().clone();
    std::mem::forget(rt);
    handle
}

#[test]
fn custom_patterns_are_built_once_into_runtime_scanner() {
    let mut config = AegisConfig::default();
    config.custom_patterns = vec![UserPattern {
        id: "USR-CTX-001".to_string(),
        category: Category::Process,
        risk: RiskLevel::Warn,
        pattern: "echo hello".to_string(),
        description: "custom warning".to_string(),
        safe_alt: None,
        justification: None,
    }];

    let context = RuntimeContext::new(config, test_handle()).unwrap();
    let assessment = context.assess("echo hello");

    assert_eq!(assessment.risk, RiskLevel::Warn);
    assert_eq!(assessment.matched.len(), 1);
    assert_eq!(assessment.matched[0].pattern.id.as_ref(), "USR-CTX-001");
}

#[test]
fn invalid_custom_scanner_aborts_runtime_context_construction() {
    let mut config = AegisConfig::default();
    config.custom_patterns = vec![UserPattern {
        id: "FS-001".to_string(),
        category: Category::Filesystem,
        risk: RiskLevel::Warn,
        pattern: "echo hello".to_string(),
        description: "duplicate id".to_string(),
        safe_alt: None,
        justification: None,
    }];

    let err = match RuntimeContext::new(config, test_handle()) {
        Ok(_) => panic!("invalid custom patterns must abort runtime context construction"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("duplicate pattern id"));
}

#[cfg(not(windows))]
#[test]
fn config_is_shared_across_runtime_dependencies() {
    use crate::config::AllowlistRule;

    let mut config = AegisConfig::default();
    config.allowlist_override_level = AllowlistOverrideLevel::Danger;
    config.allowlist = vec![AllowlistRule {
        pattern: "echo trusted".to_string(),
        cwd: Some(".".to_string()),
        user: None,
        expires_at: None,
        reason: "runtime test".to_string(),
    }];
    config.auto_snapshot_git = false;
    config.auto_snapshot_docker = false;
    config.ci_policy = CiPolicy::Allow;

    let context = RuntimeContext::new(config.clone(), test_handle()).unwrap();

    assert_eq!(context.config().mode, config.mode);
    assert_eq!(context.config().ci_policy, config.ci_policy);
    assert_eq!(
        context.config().strict_allowlist_override,
        AllowlistOverrideLevel::Danger
    );
    assert_eq!(context.config().snapshot_policy, config.snapshot_policy);
    let Some(current_user) = context.current_user() else {
        panic!("test requires a resolvable user");
    };
    let allowlist_ctx =
        AllowlistContext::new("echo trusted", Path::new("."), current_user, now_utc());
    assert_eq!(
        context.allowlist_match(&allowlist_ctx).map(|m| m.pattern),
        Some("echo trusted".to_string())
    );
    assert!(
        context
            .create_snapshots(Path::new("."), "rm -rf /tmp/runtime-context-test", false)
            .is_empty()
    );
    assert_eq!(context.config().ci_policy, CiPolicy::Allow);
    assert_eq!(
        context.config().strict_allowlist_override,
        AllowlistOverrideLevel::Danger
    );
    assert_eq!(context.config().snapshot_policy, config.snapshot_policy);
}

#[test]
fn runtime_context_rejects_expired_allowlist_rules() {
    use crate::config::AllowlistRule;
    use time::{OffsetDateTime, format_description::well_known::Rfc3339};

    let mut config = AegisConfig::default();
    config.allowlist = vec![AllowlistRule {
        pattern: "terraform destroy -target=module.test.*".to_string(),
        cwd: None,
        user: None,
        expires_at: Some(OffsetDateTime::parse("2020-01-01T00:00:00Z", &Rfc3339).unwrap()),
        reason: "expired teardown".to_string(),
    }];

    let err = match RuntimeContext::new(config, test_handle()) {
        Ok(_) => panic!("expired allowlist rules must be rejected before runtime setup"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("expired"));
}

#[test]
fn runtime_context_rejects_unscoped_allowlist_rules() {
    use crate::config::AllowlistRule;

    let mut config = AegisConfig::default();
    config.allowlist = vec![AllowlistRule {
        pattern: "terraform destroy *".to_string(),
        cwd: None,
        user: None,
        expires_at: None,
        reason: "too broad".to_string(),
    }];

    let err = match RuntimeContext::new(config, test_handle()) {
        Ok(_) => panic!("runtime context must reject unscoped allowlist rules"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("must declare cwd or user scope"));
}

#[test]
fn runtime_context_accepts_scoped_allowlist_rules() {
    use crate::config::AllowlistRule;

    let mut config = AegisConfig::default();
    config.allowlist = vec![AllowlistRule {
        pattern: "terraform destroy -target=module.test.*".to_string(),
        cwd: Some("/srv/infra".to_string()),
        user: None,
        expires_at: None,
        reason: "scoped teardown".to_string(),
    }];

    let context = RuntimeContext::new(config, test_handle()).unwrap();
    let allowlist_ctx = AllowlistContext::with_optional_scope(
        "terraform destroy -target=module.test.api",
        Some(Path::new("/srv/infra")),
        context.current_user(),
        now_utc(),
    );

    assert!(context.allowlist_match(&allowlist_ctx).is_some());
}

#[test]
fn runtime_context_accepts_user_scoped_allowlist_rules() {
    use crate::config::AllowlistRule;

    let Some(current_user) = detect_effective_user() else {
        return;
    };
    let mut config = AegisConfig::default();
    config.allowlist = vec![AllowlistRule {
        pattern: "terraform destroy -target=module.test.*".to_string(),
        cwd: None,
        user: Some(current_user.clone()),
        expires_at: None,
        reason: "scoped teardown".to_string(),
    }];

    let context = RuntimeContext::new(config, test_handle()).unwrap();
    let Some(current_user) = context.current_user() else {
        panic!("test requires a resolvable user");
    };
    let allowlist_ctx = AllowlistContext::new(
        "terraform destroy -target=module.test.api",
        Path::new("/srv/infra"),
        current_user,
        now_utc(),
    );

    assert!(context.allowlist_match(&allowlist_ctx).is_some());
}

#[test]
fn unknown_user_does_not_match_user_scoped_allowlist_rule() {
    use crate::config::AllowlistRule;

    let mut config = AegisConfig::default();
    config.allowlist = vec![AllowlistRule {
        pattern: "terraform destroy -target=module.test.*".to_string(),
        cwd: None,
        user: Some("ci".to_string()),
        expires_at: None,
        reason: "user scoped teardown".to_string(),
    }];

    let mut context = RuntimeContext::new(config, test_handle()).unwrap();
    context.current_user = None;

    assert!(
        context
            .allowlist_match_for_command(
                "terraform destroy -target=module.test.api",
                Some(Path::new("/srv/infra")),
            )
            .is_none()
    );
}

#[test]
fn unknown_cwd_does_not_match_cwd_scoped_allowlist_rule() {
    use crate::config::AllowlistRule;

    let mut config = AegisConfig::default();
    config.allowlist = vec![AllowlistRule {
        pattern: "terraform destroy -target=module.test.*".to_string(),
        cwd: Some("/srv/infra".to_string()),
        user: None,
        expires_at: None,
        reason: "scoped teardown".to_string(),
    }];

    let context = RuntimeContext::new(config, test_handle()).unwrap();

    assert!(
        context
            .allowlist_match_for_command("terraform destroy -target=module.test.api", None,)
            .is_none()
    );
}

#[cfg(not(windows))]
#[test]
fn load_for_preserves_project_allowlist_precedence_into_runtime_matching() {
    let workspace = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let global_dir = home.path().join(".config/aegis");
    fs::create_dir_all(&global_dir).unwrap();

    let workspace_cwd = workspace.path().to_string_lossy();
    fs::write(
        global_dir.join("config.toml"),
        format!(
            r#"
[[allow]]
pattern = "terraform destroy -target=module.test.*"
cwd = "{workspace_cwd}"
reason = "global teardown"
expires_at = "2030-01-01T00:00:00Z"
"#
        ),
    )
    .unwrap();
    fs::write(
        workspace.path().join(".aegis.toml"),
        format!(
            r#"
[[allow]]
pattern = "terraform destroy -target=module.test.*"
cwd = "{workspace_cwd}"
reason = "project teardown"
expires_at = "2030-01-01T00:00:00Z"
"#
        ),
    )
    .unwrap();

    let config = AegisConfig::load_for(workspace.path(), Some(home.path())).unwrap();
    let context = RuntimeContext::new(config, test_handle()).unwrap();
    let matched = context
        .allowlist_match_for_command(
            "terraform destroy -target=module.test.api",
            Some(workspace.path()),
        )
        .unwrap();

    assert_eq!(matched.reason, "project teardown");
    assert_eq!(
        matched.source_layer,
        crate::config::ConfigSourceLayer::Project
    );
}

#[test]
fn runtime_context_uses_external_handle_for_snapshots() {
    // Persistent runtime: RuntimeContext must accept an external Handle
    // instead of owning its own Runtime. This proves:
    // 1. RuntimeContext::new accepts a Handle parameter
    // 2. create_snapshots works through the external handle
    // 3. No internal SnapshotRuntime::Ready(Runtime) exists
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let handle = rt.handle().clone();

    let mut config = AegisConfig::default();
    config.auto_snapshot_git = false;
    config.auto_snapshot_docker = false;

    let context = RuntimeContext::new(config, handle).unwrap();
    let snapshots = context.create_snapshots(Path::new("."), "echo test", false);

    // With both snapshot plugins disabled, result is empty — but the call
    // must succeed without panicking (proving the external handle works).
    assert!(snapshots.is_empty());
}

#[test]
fn runtime_context_new_does_not_build_snapshot_registry_eagerly() {
    crate::snapshot::reset_snapshot_registry_build_count_for_tests();

    let mut config = AegisConfig::default();
    config.snapshot_policy = SnapshotPolicy::Selective;
    config.auto_snapshot_git = true;
    config.auto_snapshot_docker = false;

    let _context = RuntimeContext::new(config, test_handle()).unwrap();

    assert_eq!(
        crate::snapshot::snapshot_registry_build_count_for_tests(),
        0
    );
}

#[test]
fn runtime_context_new_requires_handle_parameter() {
    // Verify the two-argument signature is the only way to construct.
    // This test will fail to compile if RuntimeContext::new still accepts
    // only Config (one argument).
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let handle = rt.handle().clone();
    let config = AegisConfig::default();

    // Must compile with two arguments.
    let _context = RuntimeContext::new(config, handle).unwrap();
}

#[test]
fn append_audit_entry_enriches_explanation_with_runtime_outcome() {
    let context = RuntimeContext::new(AegisConfig::default(), test_handle()).unwrap();
    let assessment = context.assess("rm -rf target");
    let snapshots = vec![SnapshotRecord {
        plugin: "git",
        snapshot_id: "snap-1".to_string(),
    }];
    let explanation = CommandExplanation {
        scan: ScanExplanation {
            highest_risk: assessment.risk,
            decision_source: assessment.decision_source(),
            matched_patterns: vec![ExplainedPatternMatch {
                id: "FS-001".to_string(),
                risk: RiskLevel::Danger,
                description: "recursive delete".to_string(),
                matched_text: "rm -rf".to_string(),
                justification: None,
            }],
        },
        policy: PolicyExplanation {
            action: PolicyAction::Prompt,
            rationale: PolicyRationale::RequiresConfirmation,
            requires_confirmation: true,
            snapshots_required: true,
            allowlist_effective: false,
            block_reason: None,
        },
        context: ExecutionContextExplanation {
            mode: context.config().mode,
            transport: ExecutionTransport::Shell,
            ci_detected: false,
            allowlist_match: None,
            applicable_snapshot_plugins: vec!["git".to_string()],
        },
        outcome: None,
    };

    let entry = context.build_audit_entry(
        &assessment,
        Decision::Approved,
        &snapshots,
        &explanation,
        AuditWriteOptions {
            allowlist_match: None,
            allowlist_effective: false,
            ci_detected: false,
            sandbox_status: SandboxStatus::NotConfigured,
        },
    );

    let outcome = entry
        .as_base()
        .explanation
        .as_ref()
        .and_then(|value| value.outcome.as_ref());
    assert_eq!(
        outcome.map(|value| value.decision),
        Some(crate::explanation::ExecutionDecisionExplanation::Approved)
    );
    assert_eq!(
        outcome
            .and_then(|value| value.snapshots.first())
            .map(|value| value.plugin.as_str()),
        Some("git")
    );
}

#[test]
fn append_audit_entry_preserves_allowlist_context_fields() {
    let mut config = AegisConfig::default();
    config.allowlist = vec![crate::config::AllowlistRule {
        pattern: "rm -rf target".to_string(),
        cwd: Some(".".to_string()),
        user: None,
        expires_at: None,
        reason: "approved cleanup".to_string(),
    }];
    let context = RuntimeContext::new(config, test_handle()).unwrap();
    let assessment = context.assess("rm -rf target");
    let allowlist_match =
        context.allowlist_match_for_command("rm -rf target", Some(Path::new(".")));
    let explanation = CommandExplanation {
        scan: ScanExplanation {
            highest_risk: assessment.risk,
            decision_source: assessment.decision_source(),
            matched_patterns: vec![ExplainedPatternMatch {
                id: "FS-001".to_string(),
                risk: RiskLevel::Danger,
                description: "recursive delete".to_string(),
                matched_text: "rm -rf".to_string(),
                justification: None,
            }],
        },
        policy: PolicyExplanation {
            action: PolicyAction::AutoApprove,
            rationale: PolicyRationale::AllowlistOverride,
            requires_confirmation: false,
            snapshots_required: false,
            allowlist_effective: true,
            block_reason: None,
        },
        context: ExecutionContextExplanation {
            mode: context.config().mode,
            transport: ExecutionTransport::Shell,
            ci_detected: false,
            allowlist_match: allowlist_match.as_ref().map(allowlist_explanation_from),
            applicable_snapshot_plugins: Vec::new(),
        },
        outcome: None,
    };

    let entry = context.build_audit_entry(
        &assessment,
        Decision::AutoApproved,
        &[],
        &explanation,
        AuditWriteOptions {
            allowlist_match: allowlist_match.as_ref(),
            allowlist_effective: true,
            ci_detected: false,
            sandbox_status: SandboxStatus::NotConfigured,
        },
    );

    let base = entry.as_base();
    assert_eq!(base.allowlist_pattern.as_deref(), Some("rm -rf target"));
    assert_eq!(base.allowlist_reason.as_deref(), Some("approved cleanup"));
    assert_eq!(base.allowlist_matched, Some(true));
    assert_eq!(base.allowlist_effective, Some(true));
}

fn now_utc() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}
