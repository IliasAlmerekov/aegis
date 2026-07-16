use std::cell::RefCell;

use aegis_types::SandboxStatus;

use super::*;

#[test]
fn optional_unavailability_is_audited_then_warned_before_execution() {
    let events = RefCell::new(Vec::new());

    let exit_code = complete_shell_execution(
        Decision::AutoApproved,
        || Ok(((), SandboxStatus::Unavailable)),
        |decision, status| {
            events
                .borrow_mut()
                .push(format!("audit:{decision:?}:{status:?}"));
            Ok::<_, String>(())
        },
        |message| events.borrow_mut().push(format!("warning:{message}")),
        |()| {
            events.borrow_mut().push("execute".to_string());
            0
        },
        |message| events.borrow_mut().push(format!("error:{message}")),
    );

    assert_eq!(exit_code, 0);
    assert_eq!(
        events.into_inner(),
        vec![
            "audit:AutoApproved:Unavailable".to_string(),
            format!("warning:{}", aegis::runtime::SANDBOX_UNAVAILABLE_MESSAGE),
            "execute".to_string(),
        ]
    );
}

#[test]
fn required_unavailability_is_audited_as_blocked_and_never_executes() {
    let events = RefCell::new(Vec::new());

    let exit_code = complete_shell_execution(
        Decision::Approved,
        || Err::<((), SandboxStatus), _>(aegis_sandbox::SandboxError::Required),
        |decision, status| {
            events
                .borrow_mut()
                .push(format!("audit:{decision:?}:{status:?}"));
            Ok::<_, String>(())
        },
        |message| events.borrow_mut().push(format!("warning:{message}")),
        |()| {
            events.borrow_mut().push("execute".to_string());
            0
        },
        |message| events.borrow_mut().push(format!("error:{message}")),
    );

    assert_eq!(exit_code, EXIT_BLOCKED);
    assert_eq!(
        events.into_inner(),
        vec![
            "audit:Blocked:Unavailable".to_string(),
            format!(
                "error:{}",
                aegis::runtime::SANDBOX_REQUIRED_UNAVAILABLE_MESSAGE
            ),
        ]
    );
}

#[test]
fn setup_failure_is_audited_as_not_attempted_and_never_executes() {
    let events = RefCell::new(Vec::new());

    let exit_code = complete_shell_execution(
        Decision::Approved,
        || {
            Err::<((), SandboxStatus), _>(aegis_sandbox::SandboxError::SetupFailed(
                "invalid profile".to_string(),
            ))
        },
        |decision, status| {
            events
                .borrow_mut()
                .push(format!("audit:{decision:?}:{status:?}"));
            Ok::<_, String>(())
        },
        |message| events.borrow_mut().push(format!("warning:{message}")),
        |()| {
            events.borrow_mut().push("execute".to_string());
            0
        },
        |message| events.borrow_mut().push(format!("error:{message}")),
    );

    assert_eq!(exit_code, EXIT_INTERNAL);
    assert_eq!(
            events.into_inner(),
            vec![
                "audit:Blocked:NotAttempted".to_string(),
                "error:Sandbox setup failed; command not executed: sandbox setup failed: invalid profile"
                    .to_string(),
            ]
        );
}

#[test]
fn audit_failure_prevents_optional_warning_and_execution() {
    let events = RefCell::new(Vec::new());

    let exit_code = complete_shell_execution(
        Decision::AutoApproved,
        || Ok(((), SandboxStatus::Unavailable)),
        |_decision, _status| Err::<(), _>("permission denied".to_string()),
        |message| events.borrow_mut().push(format!("warning:{message}")),
        |()| {
            events.borrow_mut().push("execute".to_string());
            0
        },
        |message| events.borrow_mut().push(format!("error:{message}")),
    );

    assert_eq!(exit_code, EXIT_INTERNAL);
    assert_eq!(
        events.into_inner(),
        vec!["error:failed to write audit log: permission denied".to_string()]
    );
}

#[test]
fn unconfigured_sandbox_is_silent() {
    let events = RefCell::new(Vec::new());

    let exit_code = complete_shell_execution(
        Decision::AutoApproved,
        || Ok(((), SandboxStatus::NotConfigured)),
        |_decision, _status| Ok::<_, String>(()),
        |message| events.borrow_mut().push(format!("warning:{message}")),
        |()| {
            events.borrow_mut().push("execute".to_string());
            0
        },
        |message| events.borrow_mut().push(format!("error:{message}")),
    );

    assert_eq!(exit_code, 0);
    assert_eq!(events.into_inner(), vec!["execute".to_string()]);
}
