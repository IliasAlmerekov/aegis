// watch mode: NDJSON framed stdin loop

use std::io::Write;
use std::path::PathBuf;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader as TokioBufReader};
use tokio::sync::mpsc;

use crate::audit::Decision;
use crate::config::AllowlistMatch;
use crate::decision::{BlockReason, DecisionInput, DecisionPlan, PolicyAction, evaluate_policy};
use crate::interceptor::RiskLevel;
use crate::runtime::RuntimeContext;
use crate::ui::confirm::{
    show_block_via_tty, show_confirmation_via_tty, show_policy_block_via_tty,
    tty_unavailable_decision,
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
    let line =
        serde_json::to_string(frame).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    lock.write_all(line.as_bytes())?;
    lock.write_all(b"\n")?;
    lock.flush()
}

// Placeholder for Task 6 — the watch loop lives here.
pub async fn run(_context: &RuntimeContext) -> i32 {
    unimplemented!("watch loop implemented in Task 6")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
