use std::path::PathBuf;

use tokio::io::{AsyncReadExt, BufReader as TokioBufReader};
use tokio::sync::mpsc;

use crate::audit::Decision;
use crate::config::amend::{
    AppendOutcome, active_config_path_for_append, append_allow_rule, append_block_rule,
};
use crate::decision::{BlockReason, ExecutionTransport};
use crate::interceptor::parser::{extract_prefix, split_tokens};
use crate::planning::{
    CwdState, ExecutionDisposition, InterceptionPlan, PlanningOutcome, PreparedPlanner,
    SetupFailureKind, SetupFailurePlan, prepare_and_plan,
};
use crate::runtime::{RuntimeContext, WatchAuditContext};
use crate::ui::confirm::{
    PromptDecision, show_block_via_tty, show_confirmation_via_tty_with_decision,
    show_policy_block_via_tty,
};

use super::protocol::{
    InputFrame, MAX_FRAME_BYTES, OutputDecision, OutputFrame, ReadLineResult, emit_frame,
    read_bounded_line,
};

/// mpsc channel capacity for the stdout/stderr pump tasks.
const CHANNEL_CAPACITY: usize = 64;

/// Events sent from stdout/stderr pump tasks to the emitter.
enum WatchEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
}

/// Entry point for `aegis watch`.
///
/// Reads NDJSON command frames from stdin until EOF, processes each one
/// through the full Aegis interception pipeline, and emits NDJSON event
/// frames to stdout.
///
/// Returns the process exit code:
/// - `0` on clean EOF
/// - `4` on fatal stdout write failure (broken control channel)
///
/// Must be called with a multi-thread tokio runtime so that
/// `tokio::task::block_in_place` is available for TUI dialog rendering.
pub async fn run(prepared: &PreparedPlanner, ci_detected: bool) -> i32 {
    // Snapshot toggle state exactly once at the command-boundary gate.
    if !ci_detected {
        match crate::toggle::status() {
            Ok(crate::toggle::ToggleState::Disabled) => return run_disabled().await,
            Ok(crate::toggle::ToggleState::Enabled) => {}
            Err(err) => {
                eprintln!("error: failed to read toggle state: {err}");
                return 4;
            }
        }
    }

    if let PreparedPlanner::SetupFailure(plan) = prepared {
        report_watch_setup_failure(plan);
        return 4;
    }

    let mut reader = TokioBufReader::new(tokio::io::stdin());

    loop {
        match read_bounded_line(&mut reader, MAX_FRAME_BYTES).await {
            Err(e) => {
                eprintln!("aegis: stdin read error: {e}");
                return 4;
            }
            Ok(ReadLineResult::Eof) => return 0,
            Ok(ReadLineResult::Oversized) => {
                if emit_frame(&OutputFrame::Error {
                    id: None,
                    exit_code: 4,
                    message: "frame exceeds 1 MiB limit".to_string(),
                })
                .is_err()
                {
                    std::process::exit(4);
                }
                // Not audited — no parseable command. Continue loop.
            }
            Ok(ReadLineResult::Line(line)) => {
                if line.trim().is_empty() {
                    continue; // skip blank separator lines
                }
                process_frame(line, prepared, ci_detected).await;
            }
        }
    }
}

/// Entry point for disabled watch passthrough mode.
///
/// Frames are still parsed and cwd-validated, but they bypass planning,
/// prompting, snapshots, and audit writes before executing and streaming the
/// child command output.
pub async fn run_disabled() -> i32 {
    let mut reader = TokioBufReader::new(tokio::io::stdin());

    loop {
        match read_bounded_line(&mut reader, MAX_FRAME_BYTES).await {
            Err(e) => {
                eprintln!("aegis: stdin read error: {e}");
                return 4;
            }
            Ok(ReadLineResult::Eof) => return 0,
            Ok(ReadLineResult::Oversized) => {
                if emit_frame(&OutputFrame::Error {
                    id: None,
                    exit_code: 4,
                    message: "frame exceeds 1 MiB limit".to_string(),
                })
                .is_err()
                {
                    std::process::exit(4);
                }
            }
            Ok(ReadLineResult::Line(line)) => {
                if line.trim().is_empty() {
                    continue;
                }
                process_disabled_frame(line).await;
            }
        }
    }
}

/// Process a single input line as a watch-mode frame.
async fn process_frame(line: String, prepared: &PreparedPlanner, ci_detected: bool) {
    // ── 1. Parse JSON ─────────────────────────────────────────────────────────
    let frame: InputFrame = match serde_json::from_str(&line) {
        Ok(f) => f,
        Err(e) => {
            let msg = format!("invalid JSON: {e}");
            if emit_frame(&OutputFrame::Error {
                id: None,
                exit_code: 4,
                message: msg,
            })
            .is_err()
            {
                std::process::exit(4);
            }
            return;
        }
    };

    let id = frame.id.clone();

    // ── 2. Validate cmd ───────────────────────────────────────────────────────
    if frame.cmd.trim().is_empty() {
        if emit_frame(&OutputFrame::Error {
            id: id.clone(),
            exit_code: 4,
            message: "missing or empty cmd".to_string(),
        })
        .is_err()
        {
            std::process::exit(4);
        }
        return;
    }

    // ── 3. Validate and resolve cwd ───────────────────────────────────────────
    let cwd_state = if let Some(ref cwd_str) = frame.cwd {
        let path = PathBuf::from(cwd_str);
        if !path.is_dir() {
            if emit_frame(&OutputFrame::Error {
                id: id.clone(),
                exit_code: 4,
                message: "invalid cwd".to_string(),
            })
            .is_err()
            {
                std::process::exit(4);
            }
            return;
        }
        CwdState::Resolved(path)
    } else {
        match std::env::current_dir() {
            Ok(path) => CwdState::Resolved(path),
            Err(_) => CwdState::Unavailable,
        }
    };
    let outcome = prepare_and_plan(
        prepared,
        crate::planning::PlanningRequest {
            command: &frame.cmd,
            cwd_state,
            transport: ExecutionTransport::Watch,
            ci_detected,
        },
    );

    match outcome {
        PlanningOutcome::SetupFailure(plan) => {
            report_watch_setup_failure(&plan);
            std::process::exit(4);
        }
        PlanningOutcome::Planned(plan) => run_watch_plan(frame, prepared, plan, ci_detected).await,
    }
}

async fn process_disabled_frame(line: String) {
    let frame: InputFrame = match serde_json::from_str(&line) {
        Ok(f) => f,
        Err(e) => {
            let msg = format!("invalid JSON: {e}");
            if emit_frame(&OutputFrame::Error {
                id: None,
                exit_code: 4,
                message: msg,
            })
            .is_err()
            {
                std::process::exit(4);
            }
            return;
        }
    };

    let id = frame.id.clone();

    if frame.cmd.trim().is_empty() {
        if emit_frame(&OutputFrame::Error {
            id,
            exit_code: 4,
            message: "missing or empty cmd".to_string(),
        })
        .is_err()
        {
            std::process::exit(4);
        }
        return;
    }

    let cwd = match resolve_frame_cwd(&frame, &id) {
        Ok(path) => path,
        Err(()) => return,
    };

    execute_and_emit(&frame.cmd, &cwd, id).await;
}

async fn run_watch_plan(
    frame: InputFrame,
    prepared: &PreparedPlanner,
    plan: InterceptionPlan,
    ci_detected: bool,
) {
    let id = frame.id.clone();
    let context = runtime_context(prepared);
    let cwd = watch_execution_cwd(plan.decision_context().cwd_state());
    let snapshots = create_watch_snapshots(context, &plan, cwd.as_path()).await;

    let runtime_decision = match plan.execution_disposition() {
        ExecutionDisposition::Execute => Decision::AutoApproved,
        ExecutionDisposition::RequiresApproval => {
            let decision = tokio::task::block_in_place(|| {
                show_confirmation_via_tty_with_decision(
                    plan.assessment(),
                    plan.explanation(),
                    &snapshots,
                )
            });
            if decision == PromptDecision::ApproveAlways {
                if let Some(config_path) = active_config_path_for_append() {
                    let tokens = split_tokens(&frame.cmd);
                    let prefix = extract_prefix(&tokens);
                    match append_allow_rule(&config_path, &prefix, &cwd) {
                        Ok(AppendOutcome::Conflict {
                            pattern,
                            existing_location,
                        }) => {
                            let location = match existing_location {
                                crate::config::allowlist::ConfigSourceLayer::Project => "project",
                                crate::config::allowlist::ConfigSourceLayer::Global => "global",
                            };
                            eprintln!(
                                "warning: conflicting rule for '{pattern}' already exists in {location} config"
                            );
                        }
                        Ok(AppendOutcome::SkippedDuplicate | AppendOutcome::Appended) => {}
                        Err(err) => eprintln!("error: failed to append allow rule: {err}"),
                    }
                } else {
                    eprintln!("warning: cannot persist allow rule: no config file found");
                }
            }
            if decision == PromptDecision::DenyAlways {
                if let Some(config_path) = active_config_path_for_append() {
                    let tokens = split_tokens(&frame.cmd);
                    let prefix = extract_prefix(&tokens);
                    match append_block_rule(&config_path, &prefix, &cwd) {
                        Ok(AppendOutcome::Conflict {
                            pattern,
                            existing_location,
                        }) => {
                            let location = match existing_location {
                                crate::config::allowlist::ConfigSourceLayer::Project => "project",
                                crate::config::allowlist::ConfigSourceLayer::Global => "global",
                            };
                            eprintln!(
                                "warning: conflicting rule for '{pattern}' already exists in {location} config"
                            );
                        }
                        Ok(AppendOutcome::SkippedDuplicate | AppendOutcome::Appended) => {}
                        Err(err) => eprintln!("error: failed to append block rule: {err}"),
                    }
                } else {
                    eprintln!("warning: cannot persist block rule: no config file found");
                }
            }
            if matches!(
                decision,
                PromptDecision::Approve | PromptDecision::ApproveAlways
            ) {
                Decision::Approved
            } else {
                Decision::Denied
            }
        }
        ExecutionDisposition::Block => {
            tokio::task::block_in_place(|| match plan.policy_decision().block_reason() {
                Some(BlockReason::IntrinsicRiskBlock) => {
                    show_block_via_tty(plan.assessment(), plan.explanation())
                }
                Some(BlockReason::StrictPolicy) => {
                    show_policy_block_via_tty(plan.assessment(), plan.explanation())
                }
                Some(BlockReason::ProtectCiPolicy) => {
                    show_policy_block_via_tty(plan.assessment(), plan.explanation())
                }
                Some(BlockReason::BlocklistOverride) => {
                    show_policy_block_via_tty(plan.assessment(), plan.explanation())
                }
                None => {}
            });
            Decision::Blocked
        }
    };

    if let Err(err) = context.append_watch_audit_entry(
        plan.assessment(),
        runtime_decision,
        &snapshots,
        plan.explanation(),
        WatchAuditContext {
            allowlist_match: plan.decision_context().allowlist_match(),
            allowlist_effective: plan.policy_decision().allowlist_effective,
            ci_detected,
            source: frame.source.clone(),
            cwd: frame.cwd.clone(),
            id: id.clone(),
        },
    ) {
        if emit_frame(&OutputFrame::Error {
            id: id.clone(),
            exit_code: 4,
            message: format!("audit log write failed: {err}"),
        })
        .is_err()
        {
            std::process::exit(4);
        }
        return;
    }

    match runtime_decision {
        Decision::Denied => {
            if emit_frame(&OutputFrame::Result {
                id,
                decision: OutputDecision::Denied,
                exit_code: 2,
            })
            .is_err()
            {
                std::process::exit(4);
            }
        }
        Decision::Blocked => {
            if emit_frame(&OutputFrame::Result {
                id,
                decision: OutputDecision::Blocked,
                exit_code: 3,
            })
            .is_err()
            {
                std::process::exit(4);
            }
        }
        Decision::Approved | Decision::AutoApproved => {
            execute_and_emit(&frame.cmd, &cwd, id).await;
        }
    }
}

fn runtime_context(prepared: &PreparedPlanner) -> &RuntimeContext {
    match prepared {
        PreparedPlanner::Ready(context) => context,
        PreparedPlanner::SetupFailure(_) => unreachable!("watch run handles setup failure first"),
    }
}

fn resolve_frame_cwd(frame: &InputFrame, id: &Option<String>) -> Result<PathBuf, ()> {
    if let Some(ref cwd_str) = frame.cwd {
        let path = PathBuf::from(cwd_str);
        if !path.is_dir() {
            if emit_frame(&OutputFrame::Error {
                id: id.clone(),
                exit_code: 4,
                message: "invalid cwd".to_string(),
            })
            .is_err()
            {
                std::process::exit(4);
            }
            return Err(());
        }
        return Ok(path);
    }

    Ok(match std::env::current_dir() {
        Ok(path) => path,
        Err(_) => PathBuf::from("."),
    })
}

fn watch_execution_cwd(cwd_state: &CwdState) -> PathBuf {
    match cwd_state {
        CwdState::Resolved(path) => path.clone(),
        CwdState::Unavailable => PathBuf::from("."),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::watch_execution_cwd;
    use crate::planning::CwdState;

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
}

async fn create_watch_snapshots(
    context: &RuntimeContext,
    plan: &InterceptionPlan,
    cwd: &std::path::Path,
) -> Vec<crate::snapshot::SnapshotRecord> {
    if matches!(
        plan.snapshot_plan(),
        crate::planning::SnapshotPlan::NotRequired
    ) {
        return Vec::new();
    }

    context
        .create_snapshots_async(cwd, &plan.assessment().command.raw)
        .await
}

fn report_watch_setup_failure(plan: &SetupFailurePlan) {
    eprintln!("{}", plan.user_message());
    if matches!(plan.kind(), SetupFailureKind::InvalidConfig) {
        eprintln!("error: Fix or remove the invalid config file and try again.");
    }
}

/// Spawn the child command, stream its output as NDJSON frames, and emit
/// a final result frame.
async fn execute_and_emit(cmd: &str, cwd: &std::path::Path, id: Option<String>) {
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    use tokio::process::Command;

    let shell = std::env::var_os("AEGIS_REAL_SHELL")
        .or_else(|| std::env::var_os("SHELL"))
        .unwrap_or_else(|| "/bin/sh".into());

    let mut child = match Command::new(&shell)
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            if emit_frame(&OutputFrame::Error {
                id,
                exit_code: 4,
                message: format!("failed to spawn child: {e}"),
            })
            .is_err()
            {
                std::process::exit(4);
            }
            return;
        }
    };

    let child_stdout = child.stdout.take().expect("stdout piped");
    let child_stderr = child.stderr.take().expect("stderr piped");

    let (tx, mut rx) = mpsc::channel::<WatchEvent>(CHANNEL_CAPACITY);

    // stdout pump task
    let tx_out = tx.clone();
    tokio::spawn(async move {
        let mut buf = vec![0u8; 4096];
        let mut reader = TokioBufReader::new(child_stdout);
        loop {
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx_out
                        .send(WatchEvent::Stdout(buf[..n].to_vec()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });

    // stderr pump task — move last sender so channel closes when both tasks drop
    let tx_err = tx;
    tokio::spawn(async move {
        let mut buf = vec![0u8; 4096];
        let mut reader = TokioBufReader::new(child_stderr);
        loop {
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx_err
                        .send(WatchEvent::Stderr(buf[..n].to_vec()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });

    // Drain channel and write frames until both pumps exit.
    while let Some(event) = rx.recv().await {
        use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
        let frame = match event {
            WatchEvent::Stdout(data) => OutputFrame::Stdout {
                id: id.clone(),
                data_b64: BASE64.encode(&data),
            },
            WatchEvent::Stderr(data) => OutputFrame::Stderr {
                id: id.clone(),
                data_b64: BASE64.encode(&data),
            },
        };
        if emit_frame(&frame).is_err() {
            let _ = child.kill().await;
            std::process::exit(4);
        }
    }

    // Reap the child.
    let exit_code = match child.wait().await {
        Ok(status) => status.code().unwrap_or_else(|| {
            #[cfg(unix)]
            {
                128 + status.signal().unwrap_or(0)
            }
            #[cfg(not(unix))]
            {
                128
            }
        }),
        Err(_) => 4,
    };

    if emit_frame(&OutputFrame::Result {
        id,
        decision: OutputDecision::Approved,
        exit_code,
    })
    .is_err()
    {
        std::process::exit(4);
    }
}
