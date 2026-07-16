//! Sandbox lifecycle for Watch commands.

use std::future::Future;

use aegis_types::{RecoveryDegradation, SandboxStatus};

use crate::audit::Decision;
use crate::planning::InterceptionPlan;
use crate::runtime::{RuntimeContext, WatchAuditContext};
use crate::snapshot::SnapshotRecord;

use super::protocol::{InputFrame, OutputDecision, OutputFrame, emit_frame};

pub(super) struct WatchExecution<'a> {
    pub(super) frame: &'a InputFrame,
    pub(super) context: &'a RuntimeContext,
    pub(super) plan: &'a InterceptionPlan,
    pub(super) ci_detected: bool,
    pub(super) cwd: &'a std::path::Path,
    pub(super) snapshots: &'a [SnapshotRecord],
    pub(super) recovery_degradation: Option<RecoveryDegradation>,
}

#[derive(Debug, PartialEq, Eq)]
enum WatchSandboxEvent {
    Warning,
    RequiredBlocked,
    SetupFailed(String),
}

async fn complete_watch_sandbox_lifecycle<C, AuditError, SpawnFuture>(
    decision: Decision,
    preparation: Result<(C, SandboxStatus), aegis_sandbox::SandboxError>,
    mut append_audit: impl FnMut(Decision, SandboxStatus) -> Result<(), AuditError>,
    mut emit_event: impl FnMut(WatchSandboxEvent),
    spawn: impl FnOnce(C) -> SpawnFuture,
    report_audit_error: impl FnOnce(&AuditError),
) where
    SpawnFuture: Future<Output = ()>,
{
    match preparation {
        Ok((command, status)) => {
            if let Err(err) = append_audit(decision, status) {
                report_audit_error(&err);
                return;
            }
            if status == SandboxStatus::Unavailable {
                emit_event(WatchSandboxEvent::Warning);
            }
            spawn(command).await;
        }
        Err(aegis_sandbox::SandboxError::Required) => {
            if let Err(err) = append_audit(Decision::Blocked, SandboxStatus::Unavailable) {
                report_audit_error(&err);
                return;
            }
            emit_event(WatchSandboxEvent::RequiredBlocked);
        }
        Err(err) => {
            if let Err(audit_err) = append_audit(Decision::Blocked, SandboxStatus::NotAttempted) {
                report_audit_error(&audit_err);
                return;
            }
            emit_event(WatchSandboxEvent::SetupFailed(err.to_string()));
        }
    }
}

pub(super) async fn complete_watch_approved_execution(
    execution: WatchExecution<'_>,
    decision: Decision,
) {
    let preparation = prepare_watch_command(
        &execution.frame.cmd,
        execution.context.config().sandbox.as_ref(),
    )
    .await;
    let event_id = execution.frame.id.clone();
    let spawn_id = execution.frame.id.clone();
    let audit_id = execution.frame.id.clone();
    let cwd = execution.cwd;

    complete_watch_sandbox_lifecycle(
        decision,
        preparation,
        |final_decision, sandbox_status| {
            append_watch_execution_audit(&execution, final_decision, sandbox_status)
        },
        |event| emit_watch_sandbox_event(event, &event_id),
        |command| super::runner::execute_prepared_and_emit(command, cwd, spawn_id),
        |err| emit_watch_audit_error(&audit_id, err),
    )
    .await;
}

fn emit_watch_sandbox_event(event: WatchSandboxEvent, id: &Option<String>) {
    let result = match event {
        WatchSandboxEvent::Warning => emit_frame(&OutputFrame::Warning {
            id: id.clone(),
            code: crate::runtime::SANDBOX_UNAVAILABLE_CODE,
            sandbox_status: SandboxStatus::Unavailable,
            message: crate::runtime::SANDBOX_UNAVAILABLE_MESSAGE,
        }),
        WatchSandboxEvent::RequiredBlocked => emit_frame(&OutputFrame::SandboxResult {
            id: id.clone(),
            decision: OutputDecision::Blocked,
            exit_code: 3,
            code: crate::runtime::SANDBOX_REQUIRED_UNAVAILABLE_CODE,
            sandbox_status: SandboxStatus::Unavailable,
            message: crate::runtime::SANDBOX_REQUIRED_UNAVAILABLE_MESSAGE,
        }),
        WatchSandboxEvent::SetupFailed(err) => emit_frame(&OutputFrame::Error {
            id: id.clone(),
            exit_code: 4,
            message: format!("Sandbox setup failed; command not executed: {err}"),
        }),
    };
    if result.is_err() {
        std::process::exit(4);
    }
}

pub(super) fn append_watch_execution_audit(
    execution: &WatchExecution<'_>,
    decision: Decision,
    sandbox_status: SandboxStatus,
) -> Result<(), crate::error::AegisError> {
    let watch = WatchAuditContext {
        allowlist_match: execution.plan.decision_context().allowlist_match(),
        allowlist_effective: execution.plan.policy_decision().allowlist_effective,
        ci_detected: execution.ci_detected,
        sandbox_status,
        source: execution.frame.source.clone(),
        cwd: execution.frame.cwd.clone(),
        id: execution.frame.id.clone(),
    };
    match execution.recovery_degradation {
        Some(degradation) => execution
            .context
            .append_watch_audit_entry_with_recovery_degradation(
                execution.plan.assessment(),
                decision,
                execution.snapshots,
                execution.plan.explanation(),
                watch,
                degradation,
            ),
        None => execution.context.append_watch_audit_entry(
            execution.plan.assessment(),
            decision,
            execution.snapshots,
            execution.plan.explanation(),
            watch,
        ),
    }
}

pub(super) fn emit_watch_audit_error(id: &Option<String>, err: &crate::error::AegisError) {
    if emit_frame(&OutputFrame::Error {
        id: id.clone(),
        exit_code: 4,
        message: format!("audit log write failed: {err}"),
    })
    .is_err()
    {
        std::process::exit(4);
    }
}

pub(super) fn sandbox_status_before_preparation(context: &RuntimeContext) -> SandboxStatus {
    if context.config().sandbox.is_some() {
        SandboxStatus::NotAttempted
    } else {
        SandboxStatus::NotConfigured
    }
}

pub(super) async fn prepare_watch_command(
    cmd: &str,
    sandbox: Option<&aegis_sandbox::SandboxConfig>,
) -> Result<(std::process::Command, SandboxStatus), aegis_sandbox::SandboxError> {
    let cmd = cmd.to_owned();
    let sandbox = sandbox.cloned();
    tokio::task::spawn_blocking(move || prepare_watch_command_blocking(&cmd, sandbox.as_ref()))
        .await
        .map_err(|err| {
            aegis_sandbox::SandboxError::Execution(format!(
                "Watch Sandbox preparation task failed: {err}"
            ))
        })?
}

fn prepare_watch_command_blocking(
    cmd: &str,
    sandbox: Option<&aegis_sandbox::SandboxConfig>,
) -> Result<(std::process::Command, SandboxStatus), aegis_sandbox::SandboxError> {
    let shell = std::env::var_os("AEGIS_REAL_SHELL")
        .or_else(|| std::env::var_os("SHELL"))
        .unwrap_or_else(|| "/bin/sh".into());
    let args = [
        std::ffi::OsString::from("-c"),
        std::ffi::OsString::from(cmd),
    ];

    if let Some(config) = sandbox {
        let prepared = aegis_sandbox::prepare_for_spawn(config, shell.as_os_str(), &args)?;
        return Ok((prepared.command, prepared.status));
    }

    let mut command = std::process::Command::new(shell);
    command.args(args);
    Ok((command, SandboxStatus::NotConfigured))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[tokio::test]
    async fn optional_unavailability_audits_then_warns_then_spawns() {
        let events = RefCell::new(Vec::new());

        complete_watch_sandbox_lifecycle(
            Decision::AutoApproved,
            Ok(((), SandboxStatus::Unavailable)),
            |decision, status| {
                events
                    .borrow_mut()
                    .push(format!("audit:{decision:?}:{status:?}"));
                Ok::<_, String>(())
            },
            |event| events.borrow_mut().push(format!("event:{event:?}")),
            |()| async {
                events.borrow_mut().push("spawn".to_string());
            },
            |err| events.borrow_mut().push(format!("audit-error:{err}")),
        )
        .await;

        assert_eq!(
            events.into_inner(),
            ["audit:AutoApproved:Unavailable", "event:Warning", "spawn",]
        );
    }

    #[tokio::test]
    async fn required_unavailability_audits_block_and_never_spawns() {
        let events = RefCell::new(Vec::new());

        complete_watch_sandbox_lifecycle(
            Decision::Approved,
            Err::<((), SandboxStatus), _>(aegis_sandbox::SandboxError::Required),
            |decision, status| {
                events
                    .borrow_mut()
                    .push(format!("audit:{decision:?}:{status:?}"));
                Ok::<_, String>(())
            },
            |event| events.borrow_mut().push(format!("event:{event:?}")),
            |()| async {
                events.borrow_mut().push("spawn".to_string());
            },
            |err| events.borrow_mut().push(format!("audit-error:{err}")),
        )
        .await;

        assert_eq!(
            events.into_inner(),
            ["audit:Blocked:Unavailable", "event:RequiredBlocked"]
        );
    }

    #[tokio::test]
    async fn setup_failure_audits_not_attempted_and_never_spawns() {
        let events = RefCell::new(Vec::new());

        complete_watch_sandbox_lifecycle(
            Decision::Approved,
            Err::<((), SandboxStatus), _>(aegis_sandbox::SandboxError::SetupFailed(
                "invalid profile".to_string(),
            )),
            |decision, status| {
                events
                    .borrow_mut()
                    .push(format!("audit:{decision:?}:{status:?}"));
                Ok::<_, String>(())
            },
            |event| events.borrow_mut().push(format!("event:{event:?}")),
            |()| async {
                events.borrow_mut().push("spawn".to_string());
            },
            |err| events.borrow_mut().push(format!("audit-error:{err}")),
        )
        .await;

        assert_eq!(
            events.into_inner(),
            [
                "audit:Blocked:NotAttempted",
                "event:SetupFailed(\"sandbox setup failed: invalid profile\")",
            ]
        );
    }

    #[tokio::test]
    async fn audit_failure_prevents_warning_and_spawn() {
        let events = RefCell::new(Vec::new());

        complete_watch_sandbox_lifecycle(
            Decision::AutoApproved,
            Ok(((), SandboxStatus::Unavailable)),
            |_decision, _status| Err::<(), _>("permission denied".to_string()),
            |event| events.borrow_mut().push(format!("event:{event:?}")),
            |()| async {
                events.borrow_mut().push("spawn".to_string());
            },
            |err| events.borrow_mut().push(format!("audit-error:{err}")),
        )
        .await;

        assert_eq!(events.into_inner(), ["audit-error:permission denied"]);
    }

    #[tokio::test]
    async fn unconfigured_sandbox_audits_and_spawns_without_warning() {
        let events = RefCell::new(Vec::new());

        complete_watch_sandbox_lifecycle(
            Decision::AutoApproved,
            Ok(((), SandboxStatus::NotConfigured)),
            |decision, status| {
                events
                    .borrow_mut()
                    .push(format!("audit:{decision:?}:{status:?}"));
                Ok::<_, String>(())
            },
            |event| events.borrow_mut().push(format!("event:{event:?}")),
            |()| async {
                events.borrow_mut().push("spawn".to_string());
            },
            |err| events.borrow_mut().push(format!("audit-error:{err}")),
        )
        .await;

        assert_eq!(
            events.into_inner(),
            ["audit:AutoApproved:NotConfigured", "spawn"]
        );
    }
}
