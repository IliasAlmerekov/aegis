// watch mode: NDJSON framed stdin loop

use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader as TokioBufReader};
use tokio::sync::mpsc;

use crate::audit::Decision;
use crate::decision::{BlockReason, DecisionInput, PolicyAction, evaluate_policy};
use crate::runtime::RuntimeContext;
use crate::ui::confirm::{
    show_block_via_tty, show_confirmation_via_tty, show_policy_block_via_tty,
};

// ── Constants ─────────────────────────────────────────────────────────────────

/// Maximum bytes per input frame (1 MiB). Enforced before allocation.
pub const MAX_FRAME_BYTES: usize = 1 << 20;

/// mpsc channel capacity for the stdout/stderr pump tasks.
const CHANNEL_CAPACITY: usize = 64;

// ── Input frame ───────────────────────────────────────────────────────────────

/// One NDJSON command frame read from process stdin.
#[derive(Debug, Deserialize)]
pub struct InputFrame {
    pub cmd: String,
    pub cwd: Option<String>,
    /// Reserved — ignored in v1.
    pub interactive: Option<bool>,
    pub source: Option<String>,
    pub id: Option<String>,
}

// ── Output frames ─────────────────────────────────────────────────────────────

/// The `decision` field in a result or error output frame.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OutputDecision {
    Approved,
    Denied,
    Blocked,
    Error,
}

/// One NDJSON frame written to process stdout.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OutputFrame {
    Stdout {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        data_b64: String,
    },
    Stderr {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        data_b64: String,
    },
    Result {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        decision: OutputDecision,
        exit_code: i32,
    },
    Error {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        exit_code: i32,
        message: String,
    },
}

// ── Internal channel events ───────────────────────────────────────────────────

/// Events sent from stdout/stderr pump tasks to the emitter.
enum WatchEvent {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
}

// ── Bounded line reader ───────────────────────────────────────────────────────

/// Result of reading one line from the bounded frame reader.
pub enum ReadLineResult {
    /// A complete line with the trailing `\n` (and optional `\r`) stripped.
    Line(String),
    /// The line exceeded `max_bytes`; the rest of it has been consumed.
    Oversized,
    /// stdin reached EOF with no more data.
    Eof,
}

/// Read one newline-terminated line from `reader`, enforcing `max_bytes`.
///
/// The byte cap is enforced *before* allocation — the internal buffer never
/// grows beyond `max_bytes + 1`.  When a line would exceed the limit, the
/// remainder is drained so the next call can read cleanly.
///
/// Returns `Err` only for I/O errors or non-UTF-8 content.
pub async fn read_bounded_line<R>(
    reader: &mut TokioBufReader<R>,
    max_bytes: usize,
) -> std::io::Result<ReadLineResult>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut buf: Vec<u8> = Vec::new();

    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if buf.is_empty() {
                return Ok(ReadLineResult::Eof);
            }
            // Last line with no trailing newline.
            return to_utf8_line(buf);
        }

        let newline_pos = available.iter().position(|&b| b == b'\n');
        let chunk_len = newline_pos.map_or(available.len(), |p| p + 1);
        let is_end = newline_pos.is_some();

        if buf.len() + chunk_len > max_bytes {
            // Frame too large — consume this chunk, then drain to end of line.
            reader.consume(chunk_len);
            if !is_end {
                drain_to_newline(reader).await?;
            }
            return Ok(ReadLineResult::Oversized);
        }

        buf.extend_from_slice(&available[..chunk_len]);
        reader.consume(chunk_len);

        if is_end {
            // Strip trailing \n and optional \r.
            if buf.last() == Some(&b'\n') {
                buf.pop();
            }
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            return to_utf8_line(buf);
        }
    }
}

fn to_utf8_line(buf: Vec<u8>) -> std::io::Result<ReadLineResult> {
    String::from_utf8(buf)
        .map(ReadLineResult::Line)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Consume bytes from `reader` until a `\n` is found or EOF.
async fn drain_to_newline<R>(reader: &mut TokioBufReader<R>) -> std::io::Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
{
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(());
        }
        if let Some(p) = available.iter().position(|&b| b == b'\n') {
            reader.consume(p + 1);
            return Ok(());
        }
        let len = available.len();
        reader.consume(len);
    }
}

// ── Frame emitter ─────────────────────────────────────────────────────────────

/// Write one NDJSON frame to process stdout.
///
/// Returns `Err` if the write fails — the caller must treat this as terminal
/// (broken control channel) and call `std::process::exit(4)`.
pub fn emit_frame(frame: &OutputFrame) -> std::io::Result<()> {
    let line = serde_json::to_string(frame).map_err(std::io::Error::other)?;
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    lock.write_all(line.as_bytes())?;
    lock.write_all(b"\n")?;
    lock.flush()
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
pub async fn run(context: &RuntimeContext) -> i32 {
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
                process_frame(line, context).await;
            }
        }
    }
}

/// Process a single input line as a watch-mode frame.
async fn process_frame(line: String, context: &RuntimeContext) {
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
    let cwd = if let Some(ref cwd_str) = frame.cwd {
        let p = PathBuf::from(cwd_str);
        if !p.is_dir() {
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
        p
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };

    // ── 4. Assess ─────────────────────────────────────────────────────────────
    let assessment = context.assess(&frame.cmd);
    let allowlist_match = context.allowlist_match(&frame.cmd);

    // ── 5. Evaluate policy ────────────────────────────────────────────────────
    let config = context.config();
    let plan = evaluate_policy(DecisionInput {
        mode: config.mode,
        risk: assessment.risk,
        in_ci: false, // CI env detection is irrelevant in watch mode
        ci_policy: config.ci_policy,
        allowlist_match: allowlist_match.is_some(),
        allowlist_override_level: config.allowlist_override_level,
    });

    // ── 6. Snapshots ──────────────────────────────────────────────────────────
    let snapshots = if plan.should_snapshot {
        context.create_snapshots_async(&cwd, &frame.cmd).await
    } else {
        Vec::new()
    };

    // ── 7. Dialog / decision (blocking — uses /dev/tty) ──────────────────────
    let decision = match plan.action {
        PolicyAction::AutoApprove => Decision::AutoApproved,
        PolicyAction::Prompt => {
            let approved =
                tokio::task::block_in_place(|| show_confirmation_via_tty(&assessment, &snapshots));
            if approved {
                Decision::Approved
            } else {
                Decision::Denied
            }
        }
        PolicyAction::Block => {
            tokio::task::block_in_place(|| match plan.block_reason {
                Some(BlockReason::IntrinsicRiskBlock) => show_block_via_tty(&assessment),
                Some(BlockReason::StrictPolicy) => show_policy_block_via_tty(
                    &assessment,
                    "strict mode blocks non-safe commands unless the allowlist \
                     override level permits it",
                ),
                Some(BlockReason::ProtectCiPolicy) | None => {}
            });
            Decision::Blocked
        }
    };

    // ── 8. Audit ──────────────────────────────────────────────────────────────
    context.append_watch_audit_entry(
        &assessment,
        decision,
        &snapshots,
        allowlist_match.as_ref(),
        frame.source.clone(),
        frame.cwd.clone(),
        id.clone(),
        true,
    );

    // ── 9. Emit result or execute ─────────────────────────────────────────────
    match decision {
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

/// Spawn the child command, stream its output as NDJSON frames, and emit
/// a final result frame.
async fn execute_and_emit(cmd: &str, cwd: &std::path::Path, id: Option<String>) {
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
        Ok(status) => status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)),
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    // ── Bounded reader ────────────────────────────────────────────────────────

    async fn read_line(input: &[u8]) -> std::io::Result<ReadLineResult> {
        let mut reader = TokioBufReader::new(input);
        read_bounded_line(&mut reader, MAX_FRAME_BYTES).await
    }

    async fn read_line_with_limit(input: &[u8], limit: usize) -> std::io::Result<ReadLineResult> {
        let mut reader = TokioBufReader::new(input);
        read_bounded_line(&mut reader, limit).await
    }

    #[tokio::test]
    async fn read_line_basic() {
        let result = read_line(b"{\"cmd\":\"ls\"}\n").await.unwrap();
        match result {
            ReadLineResult::Line(s) => assert_eq!(s, "{\"cmd\":\"ls\"}"),
            _ => panic!("expected Line"),
        }
    }

    #[tokio::test]
    async fn read_line_eof_returns_eof() {
        let result = read_line(b"").await.unwrap();
        assert!(matches!(result, ReadLineResult::Eof));
    }

    #[tokio::test]
    async fn read_line_no_trailing_newline_returns_line() {
        let result = read_line(b"{\"cmd\":\"ls\"}").await.unwrap();
        match result {
            ReadLineResult::Line(s) => assert_eq!(s, "{\"cmd\":\"ls\"}"),
            _ => panic!("expected Line"),
        }
    }

    #[tokio::test]
    async fn read_line_oversized_returns_oversized() {
        // limit = 5 bytes; input is 7 bytes before \n
        let result = read_line_with_limit(b"1234567\n", 5).await.unwrap();
        assert!(matches!(result, ReadLineResult::Oversized));
    }

    #[tokio::test]
    async fn read_line_oversized_then_next_line_ok() {
        // First line is oversized; second line must still be readable.
        let input = b"1234567\nnext\n";
        let mut reader = TokioBufReader::new(input.as_ref());
        let first = read_bounded_line(&mut reader, 5).await.unwrap();
        assert!(matches!(first, ReadLineResult::Oversized));
        let second = read_bounded_line(&mut reader, 5).await.unwrap();
        match second {
            ReadLineResult::Line(s) => assert_eq!(s, "next"),
            _ => panic!("expected Line for second frame"),
        }
    }

    #[tokio::test]
    async fn read_line_strips_crlf() {
        let result = read_line(b"{\"cmd\":\"ls\"}\r\n").await.unwrap();
        match result {
            ReadLineResult::Line(s) => assert_eq!(s, "{\"cmd\":\"ls\"}"),
            _ => panic!("expected Line"),
        }
    }

    // ── Frame emit ────────────────────────────────────────────────────────────

    #[test]
    fn output_frame_result_serializes_correctly() {
        let frame = OutputFrame::Result {
            id: Some("42".to_string()),
            decision: OutputDecision::Approved,
            exit_code: 0,
        };
        let json = serde_json::to_string(&frame).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "result");
        assert_eq!(v["id"], "42");
        assert_eq!(v["decision"], "approved");
        assert_eq!(v["exit_code"], 0);
    }

    #[test]
    fn output_frame_result_omits_id_when_none() {
        let frame = OutputFrame::Result {
            id: None,
            decision: OutputDecision::Denied,
            exit_code: 2,
        };
        let json = serde_json::to_string(&frame).unwrap();
        assert!(!json.contains("\"id\""), "id must be absent when None");
    }

    #[test]
    fn output_frame_stdout_uses_base64() {
        let data = b"\xff\xfe"; // non-UTF-8 bytes
        let frame = OutputFrame::Stdout {
            id: None,
            data_b64: BASE64.encode(data),
        };
        let json = serde_json::to_string(&frame).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "stdout");
        let decoded = BASE64.decode(v["data_b64"].as_str().unwrap()).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn output_frame_error_serializes_correctly() {
        let frame = OutputFrame::Error {
            id: Some("bad".to_string()),
            exit_code: 4,
            message: "invalid JSON".to_string(),
        };
        let json = serde_json::to_string(&frame).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["type"], "error");
        assert_eq!(v["exit_code"], 4);
        assert_eq!(v["message"], "invalid JSON");
    }
}
