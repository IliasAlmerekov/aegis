use std::cell::Cell;
use std::fs;
use std::path::PathBuf;

use serde_json::Value;
use tempfile::TempDir;

use super::{run_watch_plan_with_recovery_prompt, watch_execution_cwd};
use crate::config::AegisConfig;
use crate::decision::ExecutionTransport;
use crate::planning::{
    CwdState, PlanningOutcome, PlanningRequest, PreparedPlanner, prepare_and_plan_async,
};
use crate::runtime::RuntimeContext;
use crate::ui::confirm::RecoveryPromptDecision;
use crate::watch::protocol::InputFrame;

#[test]
fn watch_execution_cwd_returns_resolved_path() {
    let path = PathBuf::from("/srv/project");
    let cwd_state = CwdState::Resolved(path.clone());

    assert_eq!(watch_execution_cwd(&cwd_state), path);
}

#[test]
fn watch_execution_cwd_returns_dot_when_unavailable() {
    let cwd_state = CwdState::Unavailable;

    assert_eq!(watch_execution_cwd(&cwd_state), PathBuf::from("."));
}

fn prepared_with_audit_path(audit_path: PathBuf) -> PreparedPlanner {
    let context = RuntimeContext::new_with_audit_path(
        AegisConfig::default(),
        tokio::runtime::Handle::current(),
        audit_path,
    )
    .unwrap();
    PreparedPlanner::Ready(Box::new(context))
}

async fn effect_opaque_plan(
    prepared: &PreparedPlanner,
    workspace: &TempDir,
) -> (InputFrame, crate::planning::InterceptionPlan) {
    let command = "sh ./run.sh";
    let cwd = workspace.path().to_string_lossy().into_owned();
    let outcome = prepare_and_plan_async(
        prepared,
        PlanningRequest {
            command,
            cwd_state: CwdState::Resolved(workspace.path().to_path_buf()),
            transport: ExecutionTransport::Watch,
            ci_detected: false,
        },
    )
    .await;
    let PlanningOutcome::Planned(plan) = outcome else {
        panic!("expected an interception plan");
    };
    (
        InputFrame {
            cmd: command.to_string(),
            cwd: Some(cwd),
            interactive: None,
            source: Some("test".to_string()),
            id: Some("recovery-test".to_string()),
        },
        plan,
    )
}

fn read_audit_entry(path: &std::path::Path) -> Value {
    let contents = fs::read_to_string(path).unwrap();
    serde_json::from_str(contents.trim()).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn watch_recovery_prompt_deny_prevents_execution_and_audits_degradation() {
    let workspace = TempDir::new().unwrap();
    let audit_dir = TempDir::new().unwrap();
    let audit_path = audit_dir.path().join("audit.jsonl");
    fs::write(workspace.path().join("run.sh"), "printf ran > executed\n").unwrap();
    let prepared = prepared_with_audit_path(audit_path.clone());
    let (frame, plan) = effect_opaque_plan(&prepared, &workspace).await;
    let prompted = Cell::new(false);

    run_watch_plan_with_recovery_prompt(frame, &prepared, plan, false, || {
        prompted.set(true);
        RecoveryPromptDecision::Deny
    })
    .await;

    assert!(prompted.get());
    assert!(!workspace.path().join("executed").exists());
    let entry = read_audit_entry(&audit_path);
    assert_eq!(entry["decision"], "Denied");
    assert_eq!(entry["recovery_degradation"], "no_snapshot_available");
}

#[tokio::test(flavor = "multi_thread")]
async fn watch_recovery_prompt_run_once_executes_and_audits_degradation() {
    let workspace = TempDir::new().unwrap();
    let audit_dir = TempDir::new().unwrap();
    let audit_path = audit_dir.path().join("audit.jsonl");
    fs::write(workspace.path().join("run.sh"), "printf ran > executed\n").unwrap();
    let prepared = prepared_with_audit_path(audit_path.clone());
    let (frame, plan) = effect_opaque_plan(&prepared, &workspace).await;
    let prompted = Cell::new(false);

    run_watch_plan_with_recovery_prompt(frame, &prepared, plan, false, || {
        prompted.set(true);
        RecoveryPromptDecision::RunOnceWithoutRecovery
    })
    .await;

    assert!(prompted.get());
    assert!(workspace.path().join("executed").exists());
    let entry = read_audit_entry(&audit_path);
    assert_eq!(entry["decision"], "Approved");
    assert_eq!(entry["recovery_degradation"], "no_snapshot_available");
}
