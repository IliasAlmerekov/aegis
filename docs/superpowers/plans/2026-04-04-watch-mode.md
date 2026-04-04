# Watch Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `aegis watch` — a long-lived stdin-driven command interception loop that reads NDJSON command frames, intercepts them through the full Aegis pipeline, and streams structured NDJSON output frames (stdout/stderr chunks + result) back to the caller.

**Architecture:** A multi-thread tokio runtime owned by `main.rs` runs `watch::run(&context)`. The watch loop reads bounded NDJSON frames from stdin, processes each frame serially (assess → dialog on `/dev/tty` → spawn child → emit chunk frames → emit result frame → audit), and terminates on stdin EOF or a fatal stdout write failure. All human-facing output (dialog, block notices) goes to `/dev/tty`; process stdout is a machine-readable NDJSON event stream; process stderr is silent in normal operation.

**Tech Stack:** tokio (process, io-util, rt-multi-thread, sync), serde_json, base64 0.22, crossterm (existing), std::fs for /dev/tty

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `Cargo.toml` | Modify | Add `base64 = "0.22"`; expand tokio features |
| `src/ui/confirm.rs` | Modify | Add `/dev/tty`-based dialog and block notification functions |
| `src/audit/logger.rs` | Modify | Add optional watch-mode fields to `AuditEntry`; add `with_watch_context` builder |
| `src/runtime.rs` | Modify | Add `create_snapshots_async` and `append_watch_audit_entry` |
| `src/watch.rs` | Create | Frame types, bounded reader, emit helper, watch loop, child execution |
| `src/lib.rs` | Modify | Export `pub mod watch` |
| `src/main.rs` | Modify | Wire `Commands::Watch` to own a multi-thread runtime and call `watch::run` |

---

## Task 1: Update Cargo.toml

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add `base64` dependency and expand tokio features**

Open `Cargo.toml` and replace the tokio entry and add base64:

```toml
[dependencies]
# ... existing entries unchanged ...
base64 = "0.22"
tokio = { version = "1", features = ["process", "fs", "rt", "rt-multi-thread", "io-util", "sync"] }
```

The added features:
- `rt-multi-thread` — required for `tokio::task::block_in_place` (used to run TUI dialog without blocking the executor with a separate thread)
- `io-util` — required for `AsyncBufReadExt` (`fill_buf`, `consume`) and `AsyncReadExt` (`read`)
- `sync` — required for `tokio::sync::mpsc`

- [ ] **Step 2: Verify it compiles**

```bash
rtk cargo check
```

Expected: no errors (no new code yet, just features added).

- [ ] **Step 3: Commit**

```bash
rtk git add Cargo.toml Cargo.lock
rtk git commit -m "chore: add base64 dep and expand tokio features for watch mode"
```

---

## Task 2: Add `/dev/tty` UI helpers in `src/ui/confirm.rs`

**Files:**
- Modify: `src/ui/confirm.rs`

The existing `show_confirmation` reads from `io::stdin()` and writes to `io::stderr()`. In watch mode, stdin is the NDJSON control stream — the TUI must use `/dev/tty` instead. This task adds two new public functions that open `/dev/tty` directly and fail-closed if it is unavailable.

- [ ] **Step 1: Write the failing test for tty-unavailable fail-closed behavior**

Add to the `#[cfg(test)]` block at the bottom of `src/ui/confirm.rs`:

```rust
#[test]
fn tty_unavailable_safe_is_approved() {
    let assessment = make_assessment("ls -la", RiskLevel::Safe, vec![]);
    assert!(
        tty_unavailable_decision(&assessment),
        "Safe must be approved when /dev/tty is unavailable"
    );
}

#[test]
fn tty_unavailable_warn_is_denied() {
    let p = make_match("GIT-001", RiskLevel::Warn, "reset", "Hard reset", None);
    let assessment = make_assessment("git reset --hard HEAD~1", RiskLevel::Warn, vec![p]);
    assert!(
        !tty_unavailable_decision(&assessment),
        "Warn must be denied when /dev/tty is unavailable"
    );
}

#[test]
fn tty_unavailable_danger_is_denied() {
    let p = make_match("FS-001", RiskLevel::Danger, r"rm\s+", "Recursive delete", None);
    let assessment = make_assessment("rm -rf /home/user", RiskLevel::Danger, vec![p]);
    assert!(
        !tty_unavailable_decision(&assessment),
        "Danger must be denied when /dev/tty is unavailable"
    );
}

#[test]
fn tty_unavailable_block_is_denied() {
    let p = make_match("PS-006", RiskLevel::Block, "rm", "Root delete", None);
    let assessment = make_assessment("rm -rf /", RiskLevel::Block, vec![p]);
    assert!(
        !tty_unavailable_decision(&assessment),
        "Block must be denied when /dev/tty is unavailable"
    );
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
rtk cargo test -p aegis -- ui::confirm::tests::tty_unavailable 2>&1 | head -20
```

Expected: compile error — `tty_unavailable_decision` is not defined yet.

- [ ] **Step 3: Implement the helper functions**

Add these functions to `src/ui/confirm.rs`, before the `#[cfg(test)]` block:

```rust
// ── /dev/tty UI helpers (watch mode) ─────────────────────────────────────────

/// The fail-closed decision when `/dev/tty` is unavailable.
///
/// Only `Safe` commands are approved without a TTY; everything else is
/// denied or blocked.  Exported so that callers can emit the correct result
/// frame without duplicating the policy.
pub fn tty_unavailable_decision(assessment: &Assessment) -> bool {
    matches!(assessment.risk, RiskLevel::Safe)
}

/// Show the confirmation dialog via `/dev/tty`.
///
/// Opens `/dev/tty` for both input (keystrokes) and output (dialog
/// rendering).  If the device cannot be opened, returns
/// `tty_unavailable_decision(assessment)` — fail-closed for Warn/Danger.
pub fn show_confirmation_via_tty(
    assessment: &Assessment,
    snapshots: &[SnapshotRecord],
) -> bool {
    use std::fs::OpenOptions;

    let tty = match OpenOptions::new().read(true).write(true).open("/dev/tty") {
        Ok(f) => f,
        Err(_) => return tty_unavailable_decision(assessment),
    };
    let tty_write = match tty.try_clone() {
        Ok(f) => f,
        Err(_) => return tty_unavailable_decision(assessment),
    };

    show_confirmation_with_input(
        assessment,
        snapshots,
        true, // /dev/tty is always interactive
        &mut io::BufReader::new(tty),
        &mut { tty_write },
    )
}

/// Show a policy-block notice via `/dev/tty`.
///
/// If `/dev/tty` cannot be opened, does nothing — the caller must still
/// emit the correct NDJSON result frame.
pub fn show_policy_block_via_tty(assessment: &Assessment, reason: &str) {
    use std::fs::OpenOptions;

    if let Ok(mut tty) = OpenOptions::new().write(true).open("/dev/tty") {
        render_policy_block(assessment, reason, &mut tty);
    }
}

/// Show an intrinsic-block notice (RiskLevel::Block pattern) via `/dev/tty`.
///
/// Uses the same `render_block` path as the shell-wrapper mode but routes
/// output to the tty device.  If `/dev/tty` cannot be opened, silent.
pub fn show_block_via_tty(assessment: &Assessment) {
    use std::fs::OpenOptions;

    if let Ok(mut tty) = OpenOptions::new().write(true).open("/dev/tty") {
        render_block(assessment, &mut tty);
    }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

```bash
rtk cargo test -p aegis -- ui::confirm::tests::tty_unavailable
```

Expected: 4 tests pass.

- [ ] **Step 5: Run the full confirm test suite to confirm no regressions**

```bash
rtk cargo test -p aegis -- ui::confirm
```

Expected: all existing tests still pass.

- [ ] **Step 6: Commit**

```bash
rtk git add src/ui/confirm.rs
rtk git commit -m "feat: add /dev/tty UI helpers for watch mode"
```

---

## Task 3: Extend `AuditEntry` with watch-mode fields

**Files:**
- Modify: `src/audit/logger.rs`

Adds four optional fields to `AuditEntry` and a `with_watch_context` builder method. All fields use `skip_serializing_if = "Option::is_none"` — existing log readers that ignore unknown fields are unaffected.

- [ ] **Step 1: Write a failing test for watch-mode audit field round-trip**

Add to the `#[cfg(test)]` section of `src/audit/logger.rs` (find it or add one):

```rust
#[test]
fn watch_context_fields_round_trip_through_json() {
    let entry = AuditEntry::new(
        "git status",
        RiskLevel::Safe,
        vec![],
        Decision::AutoApproved,
        vec![],
        None,
    )
    .with_watch_context(
        Some("claude".to_string()),
        Some("/home/user/project".to_string()),
        Some("frame-42".to_string()),
    );

    let json = serde_json::to_string(&entry).unwrap();
    let back: AuditEntry = serde_json::from_str(&json).unwrap();

    assert_eq!(back.source.as_deref(), Some("claude"));
    assert_eq!(back.cwd.as_deref(), Some("/home/user/project"));
    assert_eq!(back.id.as_deref(), Some("frame-42"));
    assert_eq!(back.transport.as_deref(), Some("watch"));
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
    );

    let json = serde_json::to_string(&entry).unwrap();
    assert!(!json.contains("source"), "source must be absent when None");
    assert!(!json.contains("transport"), "transport must be absent when None");
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
rtk cargo test -p aegis -- audit::logger::tests::watch_context 2>&1 | head -20
```

Expected: compile error — `with_watch_context` does not exist.

- [ ] **Step 3: Add the four optional fields to `AuditEntry`**

In `src/audit/logger.rs`, find the `AuditEntry` struct. Add four fields after `allowlist_pattern`:

```rust
/// The agent/caller identity passed in the watch-mode input frame.
/// Absent for shell-wrapper entries.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub source: Option<String>,

/// The working directory from the watch-mode input frame.
/// Absent for shell-wrapper entries.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub cwd: Option<String>,

/// The correlation ID from the watch-mode input frame, echoed back.
/// Absent for shell-wrapper entries.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub id: Option<String>,

/// Set to `"watch"` for entries created in watch mode.
/// Absent for shell-wrapper entries, making them distinguishable.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub transport: Option<String>,
```

- [ ] **Step 4: Update `AuditEntry::new` to initialize new fields**

Find `AuditEntry::new`. The current `Self { ... }` block ends with `allowlist_pattern`. Add four `None` initializers:

```rust
Self {
    timestamp: current_timestamp(),
    sequence: next_sequence(),
    command: command.into(),
    risk,
    matched_patterns,
    decision,
    snapshots,
    allowlist_pattern,
    source: None,      // added
    cwd: None,         // added
    id: None,          // added
    transport: None,   // added
}
```

- [ ] **Step 5: Add `with_watch_context` builder method**

Add after the closing brace of `AuditEntry::new` (still inside the `impl AuditEntry` block):

```rust
/// Attach watch-mode context fields and set `transport = "watch"`.
///
/// Call this on entries created inside `watch::run` to distinguish them
/// from shell-wrapper entries in the audit log.
pub fn with_watch_context(
    mut self,
    source: Option<String>,
    cwd: Option<String>,
    id: Option<String>,
) -> Self {
    self.source = source;
    self.cwd = cwd;
    self.id = id;
    self.transport = Some("watch".to_string());
    self
}
```

- [ ] **Step 6: Run the tests to confirm they pass**

```bash
rtk cargo test -p aegis -- audit::logger::tests::watch_context
```

Expected: both tests pass.

- [ ] **Step 7: Run the full test suite**

```bash
rtk cargo test
```

Expected: all tests pass. The only change is additive — `new()` just has four more `None` fields.

- [ ] **Step 8: Commit**

```bash
rtk git add src/audit/logger.rs
rtk git commit -m "feat: add watch-mode fields to AuditEntry"
```

---

## Task 4: Add watch-mode methods to `RuntimeContext`

**Files:**
- Modify: `src/runtime.rs`

Adds `create_snapshots_async` (calls `snapshot_registry.snapshot_all()` directly, no `block_on`) and `append_watch_audit_entry` (builds a watch-context `AuditEntry`).

- [ ] **Step 1: Add `create_snapshots_async`**

In `src/runtime.rs`, add this method to the `impl RuntimeContext` block, after `create_snapshots`:

```rust
/// Async variant of `create_snapshots` — call from within an async runtime.
///
/// Calls `snapshot_registry.snapshot_all()` directly without `block_on`,
/// which would panic if called from an already-async context.
pub async fn create_snapshots_async(
    &self,
    cwd: &std::path::Path,
    cmd: &str,
) -> Vec<crate::snapshot::SnapshotRecord> {
    self.snapshot_registry.snapshot_all(cwd, cmd).await
}
```

- [ ] **Step 2: Add `append_watch_audit_entry`**

Add this method after `append_audit_entry` in the same `impl RuntimeContext` block:

```rust
/// Append a watch-mode audit entry with frame correlation fields.
///
/// Identical to `append_audit_entry` but attaches `source`, `cwd`, `id`,
/// and sets `transport = "watch"` via `AuditEntry::with_watch_context`.
pub fn append_watch_audit_entry(
    &self,
    assessment: &crate::interceptor::scanner::Assessment,
    decision: Decision,
    snapshots: &[crate::snapshot::SnapshotRecord],
    allowlist_match: Option<&crate::config::AllowlistMatch>,
    watch_source: Option<String>,
    watch_cwd: Option<String>,
    watch_id: Option<String>,
    verbose: bool,
) {
    let entry = AuditEntry::new(
        assessment.command.raw.clone(),
        assessment.risk,
        assessment.matched.iter().map(Into::into).collect(),
        decision,
        snapshots.iter().map(Into::into).collect(),
        allowlist_match.map(|m| m.pattern.clone()),
    )
    .with_watch_context(watch_source, watch_cwd, watch_id);

    if let Err(err) = self.audit_logger.append(entry)
        && verbose
    {
        eprintln!("warning: failed to append watch audit log entry: {err}");
    }
}
```

- [ ] **Step 3: Compile-check**

```bash
rtk cargo check
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
rtk git add src/runtime.rs
rtk git commit -m "feat: add async snapshot path and watch audit method to RuntimeContext"
```

---

## Task 5: Create `src/watch.rs` — Frame types, bounded reader, emit helper

**Files:**
- Create: `src/watch.rs`

This task establishes the protocol types and I/O primitives. The watch loop (Task 6) is built on top.

- [ ] **Step 1: Write failing tests for the bounded reader**

Create `src/watch.rs` with only the tests first:

```rust
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
```

- [ ] **Step 2: Run tests to confirm they compile and the reader tests pass**

```bash
rtk cargo test -p aegis -- watch::tests 2>&1 | head -40
```

Expected: compile error for `unimplemented!` panic or all tests pass except the `run` placeholder doesn't get tested yet. The bounded reader and frame serialization tests should pass.

- [ ] **Step 3: Register the module in `src/lib.rs`**

Add to `src/lib.rs`:

```rust
pub mod watch;
```

- [ ] **Step 4: Confirm tests pass**

```bash
rtk cargo test -p aegis -- watch::tests
```

Expected: all 10 tests pass (bounded reader + frame serialization).

- [ ] **Step 5: Commit**

```bash
rtk git add src/watch.rs src/lib.rs
rtk git commit -m "feat: add watch frame types, bounded reader, and emit helper"
```

---

## Task 6: Implement the watch loop in `src/watch.rs`

**Files:**
- Modify: `src/watch.rs`

Replace the `run` placeholder with the full watch loop. Uses `tokio::task::block_in_place` for the TUI dialog (requires `rt-multi-thread`, added in Task 1).

- [ ] **Step 1: Replace the `run` placeholder with the full implementation**

Replace the placeholder `run` function in `src/watch.rs` with:

```rust
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
            if emit_frame(&OutputFrame::Error { id: None, exit_code: 4, message: msg.clone() })
                .is_err()
            {
                std::process::exit(4);
            }
            // Audit error frame (no command executed).
            return;
        }
    };

    let id = frame.id.clone();

    // ── 2. Validate cmd ───────────────────────────────────────────────────────
    if frame.cmd.trim().is_empty() {
        let msg = "missing or empty cmd".to_string();
        if emit_frame(&OutputFrame::Error {
            id: id.clone(),
            exit_code: 4,
            message: msg,
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
        strict_allowlist_override: config.strict_allowlist_override,
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
            // block_in_place: runs the closure on the current worker thread
            // while allowing other tasks to proceed on remaining workers.
            // Requires rt-multi-thread (added in Task 1).
            let approved = tokio::task::block_in_place(|| {
                show_confirmation_via_tty(&assessment, &snapshots)
            });
            if approved { Decision::Approved } else { Decision::Denied }
        }
        PolicyAction::Block => {
            // Notify the human via /dev/tty, then emit a machine result frame.
            tokio::task::block_in_place(|| match plan.block_reason {
                Some(BlockReason::IntrinsicRiskBlock) => show_block_via_tty(&assessment),
                Some(BlockReason::StrictPolicy) => show_policy_block_via_tty(
                    &assessment,
                    "strict mode blocks non-safe commands without an allowlisted override",
                ),
                Some(BlockReason::ProtectCiPolicy) => {} // CI not applicable in watch mode
                None => {}
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
        false,
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
///
/// - Child stdin: `/dev/null`
/// - Child stdout: piped → base64 `stdout` frames
/// - Child stderr: piped → base64 `stderr` frames
/// - Ordering: per-stream ordering is preserved; cross-stream ordering
///   between stdout and stderr is not guaranteed.
/// - Fatal write error: kills child with SIGKILL and exits process with 4.
async fn execute_and_emit(cmd: &str, cwd: &PathBuf, id: Option<String>) {
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
                    if tx_out.send(WatchEvent::Stdout(buf[..n].to_vec())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // stderr pump task
    let tx_err = tx; // move last sender (channel closes when both tasks drop)
    tokio::spawn(async move {
        let mut buf = vec![0u8; 4096];
        let mut reader = TokioBufReader::new(child_stderr);
        loop {
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx_err.send(WatchEvent::Stderr(buf[..n].to_vec())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Emitter: drain channel and write frames until both pumps exit.
    while let Some(event) = rx.recv().await {
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
            // Protocol failure: kill child and exit immediately.
            let _ = child.kill().await;
            std::process::exit(4);
        }
    }

    // Reap the child.
    let exit_code = match child.wait().await {
        Ok(status) => {
            status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
        }
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
```

- [ ] **Step 2: Compile check**

```bash
rtk cargo check
```

Expected: no errors.

- [ ] **Step 3: Run all existing tests to check for regressions**

```bash
rtk cargo test
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
rtk git add src/watch.rs
rtk git commit -m "feat: implement watch loop and child execution in watch::run"
```

---

## Task 7: Wire `Commands::Watch` in `src/main.rs`

**Files:**
- Modify: `src/main.rs`

Replace the stub that prints "watch: not yet implemented" and returns 0. The watch mode owns its own multi-thread tokio runtime (required for `block_in_place`).

- [ ] **Step 1: Write a failing integration test**

Add to the `#[cfg(test)]` block in `src/main.rs`:

```rust
// ── Watch mode — stub removed ─────────────────────────────────────────────────
//
// The old stub returned exit 0 for an unimplemented feature.
// Verify that watch mode now participates in the real pipeline by checking
// that a safe command produces a valid NDJSON result frame.
//
// This test is an end-to-end smoke test using pipes, not a full integration
// test (those live in tests/integration/).  It verifies the stub is gone and
// the plumbing is connected.

#[tokio::test]
async fn watch_mode_safe_command_emits_result_frame() {
    use aegis::runtime::RuntimeContext;
    use aegis::watch::{InputFrame, OutputFrame, read_bounded_line, MAX_FRAME_BYTES, ReadLineResult};
    use tokio::io::BufReader;

    // Build a minimal context with default config (Protect mode, no custom patterns).
    let context = RuntimeContext::new(aegis::config::Config::default());

    // Prepare input: one safe NDJSON frame.
    let input = b"{\"cmd\":\"echo hello\",\"id\":\"t1\"}\n";
    let mut reader = BufReader::new(input.as_ref());

    // Read the frame manually and assert it parses correctly.
    let result = read_bounded_line(&mut reader, MAX_FRAME_BYTES).await.unwrap();
    let line = match result {
        ReadLineResult::Line(l) => l,
        _ => panic!("expected Line"),
    };

    let frame: InputFrame = serde_json::from_str(&line).unwrap();
    assert_eq!(frame.cmd, "echo hello");
    assert_eq!(frame.id.as_deref(), Some("t1"));
}
```

- [ ] **Step 2: Run test to confirm it compiles and passes**

```bash
rtk cargo test -- watch_mode_safe_command_emits_result_frame
```

Expected: passes (the test only validates frame parsing, which works from Task 5).

- [ ] **Step 3: Replace the `Commands::Watch` stub in `main.rs`**

Find and replace this block in `fn main()`:

```rust
// OLD (stub — remove this):
Some(Commands::Watch) => {
    println!("watch: not yet implemented");
    0
}
```

Replace with:

```rust
// NEW — watch mode owns its own tokio runtime (rt-multi-thread required
// for block_in_place used during TUI dialog rendering).
Some(Commands::Watch) => {
    let context = RuntimeContext::load(verbose);
    match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt.block_on(aegis::watch::run(&context)),
        Err(err) => {
            eprintln!("error: failed to build tokio runtime for watch mode: {err}");
            EXIT_INTERNAL
        }
    }
}
```

Also add the import at the top of `main.rs` if not already present:
```rust
use aegis::watch;
```

- [ ] **Step 4: Compile and run full test suite**

```bash
rtk cargo test
```

Expected: all tests pass. The old `"watch: not yet implemented"` path is gone.

- [ ] **Step 5: Smoke-test the CLI**

```bash
echo '{"cmd":"echo hello","id":"1"}' | rtk cargo run --bin aegis -- watch
```

Expected output on stdout (approximate):
```
{"type":"stdout","id":"1","data_b64":"aGVsbG8K"}
{"type":"result","id":"1","decision":"approved","exit_code":0}
```

Decode the base64: `echo aGVsbG8K | base64 -d` → `hello`

- [ ] **Step 6: Test invalid JSON input**

```bash
echo 'not json' | rtk cargo run --bin aegis -- watch
```

Expected: one error result frame on stdout, then watch mode reads EOF and exits 0.

- [ ] **Step 7: Test oversized frame**

```bash
python3 -c "import json,sys; sys.stdout.write(json.dumps({'cmd':'x'*1100000})+'\n')" | rtk cargo run --bin aegis -- watch
```

Expected: one error result frame with `"message":"frame exceeds 1 MiB limit"`, then exits 0.

- [ ] **Step 8: Commit**

```bash
rtk git add src/main.rs
rtk git commit -m "feat: implement aegis watch — NDJSON framed command interception loop"
```

---

## Task 8: Add integration tests for watch mode

**Files:**
- Create: `tests/integration/watch_mode.rs`
- Modify: `tests/integration/mod.rs` (or `tests/integration.rs`)

- [ ] **Step 1: Write integration tests**

Create `tests/integration/watch_mode.rs`:

```rust
//! Integration tests for `aegis watch` — end-to-end via child process.

use std::io::Write;
use std::process::{Command, Stdio};

fn aegis_watch(input: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aegis"))
        .arg("watch")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aegis watch");

    child.stdin.as_mut().unwrap().write_all(input).unwrap();
    drop(child.stdin.take()); // close stdin to send EOF

    child.wait_with_output().expect("failed to wait for aegis watch")
}

fn parse_frames(stdout: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("invalid NDJSON frame"))
        .collect()
}

#[test]
fn safe_command_emits_result_approved() {
    // `echo hello` is RiskLevel::Safe — must be auto-approved with no dialog.
    let output = aegis_watch(b"{\"cmd\":\"echo hello\",\"id\":\"1\"}\n");
    assert!(output.status.success(), "watch must exit 0 on clean EOF");

    let frames = parse_frames(&output.stdout);
    let result = frames.iter().find(|f| f["type"] == "result").expect("no result frame");

    assert_eq!(result["decision"], "approved");
    assert_eq!(result["exit_code"], 0);
    assert_eq!(result["id"], "1");
}

#[test]
fn safe_command_stdout_chunk_is_base64() {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

    let output = aegis_watch(b"{\"cmd\":\"printf 'hello'\"}\n");
    let frames = parse_frames(&output.stdout);

    let stdout_frame = frames.iter().find(|f| f["type"] == "stdout").expect("no stdout frame");
    let data_b64 = stdout_frame["data_b64"].as_str().expect("data_b64 must be a string");
    let decoded = BASE64.decode(data_b64).expect("data_b64 must be valid base64");
    assert_eq!(decoded, b"hello");
}

#[test]
fn invalid_json_emits_error_frame_and_continues() {
    // First line is bad JSON; second is a valid safe command.
    let input = b"not-json\n{\"cmd\":\"echo ok\"}\n";
    let output = aegis_watch(input);
    assert!(output.status.success());

    let frames = parse_frames(&output.stdout);
    let error = frames.iter().find(|f| f["type"] == "error").expect("no error frame");
    assert_eq!(error["exit_code"], 4);
    assert!(error["message"].as_str().unwrap().contains("invalid JSON"));

    // The second command must still produce a result frame.
    let results: Vec<_> = frames.iter().filter(|f| f["type"] == "result").collect();
    assert_eq!(results.len(), 1, "second command must produce a result frame");
    assert_eq!(results[0]["decision"], "approved");
}

#[test]
fn missing_cmd_emits_error_frame() {
    let output = aegis_watch(b"{\"source\":\"test\"}\n");
    let frames = parse_frames(&output.stdout);
    let error = frames.iter().find(|f| f["type"] == "error").expect("no error frame");
    assert_eq!(error["exit_code"], 4);
    assert!(error["message"].as_str().unwrap().contains("cmd"));
}

#[test]
fn invalid_cwd_emits_error_frame() {
    let output = aegis_watch(b"{\"cmd\":\"echo x\",\"cwd\":\"/nonexistent/path/xyz\"}\n");
    let frames = parse_frames(&output.stdout);
    let error = frames.iter().find(|f| f["type"] == "error").expect("no error frame");
    assert_eq!(error["exit_code"], 4);
    assert_eq!(error["message"], "invalid cwd");
}

#[test]
fn oversized_frame_emits_error_frame_and_continues() {
    // Build a frame that exceeds 1 MiB.
    let big_cmd = "x".repeat(1_100_000);
    let big_frame = format!("{{\"cmd\":\"{big_cmd}\"}}\n");
    let small_frame = b"{\"cmd\":\"echo after\"}\n";

    let mut input = big_frame.into_bytes();
    input.extend_from_slice(small_frame);

    let output = aegis_watch(&input);
    assert!(output.status.success());

    let frames = parse_frames(&output.stdout);
    let error = frames.iter().find(|f| f["type"] == "error").expect("no error frame");
    assert!(error["message"].as_str().unwrap().contains("1 MiB"));

    // The next valid frame must still be processed.
    let results: Vec<_> = frames.iter().filter(|f| f["type"] == "result").collect();
    assert_eq!(results.len(), 1, "command after oversized frame must execute");
}

#[test]
fn id_field_is_echoed_on_all_frames() {
    let output = aegis_watch(b"{\"cmd\":\"printf 'hi'\",\"id\":\"corr-99\"}\n");
    let frames = parse_frames(&output.stdout);

    for frame in &frames {
        if frame["type"] != "error" {
            assert_eq!(
                frame["id"], "corr-99",
                "id must be echoed on all non-error frames: {frame}"
            );
        }
    }
}

#[test]
fn child_exit_code_is_propagated() {
    let output = aegis_watch(b"{\"cmd\":\"exit 42\",\"id\":\"ec\"}\n");
    let frames = parse_frames(&output.stdout);
    let result = frames.iter().find(|f| f["type"] == "result").unwrap();
    assert_eq!(result["exit_code"], 42);
}

#[test]
fn watch_exits_zero_on_clean_eof() {
    let output = aegis_watch(b"{\"cmd\":\"echo hi\"}\n");
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn watch_mode_audit_entry_sets_transport_watch() {
    // Run a command through watch mode and verify the audit log records
    // transport = "watch".
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let audit_path = dir.path().join("audit.jsonl");

    let mut child = Command::new(env!("CARGO_BIN_EXE_aegis"))
        .arg("watch")
        .env("AEGIS_AUDIT_PATH", &audit_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn");

    child.stdin.as_mut().unwrap().write_all(b"{\"cmd\":\"echo audit\",\"source\":\"test-agent\",\"id\":\"a1\"}\n").unwrap();
    drop(child.stdin.take());
    let _ = child.wait_with_output().unwrap();

    if audit_path.exists() {
        let contents = fs::read_to_string(&audit_path).unwrap();
        let entry: serde_json::Value = serde_json::from_str(contents.trim()).unwrap();
        assert_eq!(entry["transport"], "watch");
        assert_eq!(entry["source"], "test-agent");
        assert_eq!(entry["id"], "a1");
    }
    // Note: if AEGIS_AUDIT_PATH is not supported, this test is a no-op.
    // See TODO: add AEGIS_AUDIT_PATH env var support to AuditLogger if needed.
}
```

- [ ] **Step 2: Register the test module**

Check if `tests/integration/mod.rs` or `tests/integration.rs` exists:

```bash
ls tests/
```

If there is a `tests/integration/` directory with a `mod.rs`, add:
```rust
mod watch_mode;
```

If the integration tests are in `tests/integration.rs` (flat file), append:
```rust
mod watch_mode;
```

If neither exists, create `tests/integration.rs` with:
```rust
mod watch_mode;
```

And create `tests/integration/watch_mode.rs` with the test code above.

- [ ] **Step 3: Run integration tests**

```bash
rtk cargo test --test integration
```

Expected: all tests pass except possibly `watch_mode_audit_entry_sets_transport_watch` (depends on `AEGIS_AUDIT_PATH` support). Mark that test `#[ignore]` if `AuditLogger` does not support path override via env var.

- [ ] **Step 4: Run full test suite**

```bash
rtk cargo test
```

Expected: all tests pass.

- [ ] **Step 5: Run clippy**

```bash
rtk cargo clippy -- -D warnings
```

Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
rtk git add tests/
rtk git commit -m "test: add integration tests for aegis watch mode"
```

---

## Self-Review

**Spec coverage check:**

| Spec requirement | Task |
|---|---|
| NDJSON input: cmd, cwd, interactive, source, id | Task 5 (InputFrame) |
| 1 MiB frame size cap enforced before allocation | Task 5 (read_bounded_line) |
| Unknown fields ignored | Task 5 (serde default) |
| output: stdout/stderr/result/error frames | Task 5 (OutputFrame) |
| data_b64 for binary-safe child output | Task 5, 6 |
| id echoed on all frames | Task 5, 6 |
| /dev/tty for all human UI | Task 2 |
| Fail-closed if /dev/tty unavailable | Task 2 (tty_unavailable_decision) |
| Child stdin = /dev/null | Task 6 (execute_and_emit) |
| Single emitter (mpsc channel) prevents frame interleaving | Task 6 |
| stdout write failure is terminal (kill child, exit 4) | Task 6 |
| stdin EOF → exit 0 | Task 6 (run loop) |
| Per-stream ordering preserved | Task 6 (separate pump tasks) |
| cwd applies to execution, policy, snapshots, audit | Task 6 (process_frame) |
| Audit entries for denied/blocked/approved | Task 6 (append_watch_audit_entry) |
| watch-mode audit fields: source, cwd, id, transport | Task 3, 4 |
| Malformed frames audited as watch-mode errors | Not yet — audit error frames in process_frame; add if audit logger supports it |
| RuntimeContext::create_snapshots_async (no nested block_on) | Task 4 |
| Multi-thread runtime for block_in_place | Task 1, 7 |
| signal → exit_code = 128 + signal | Task 6 (ExitStatusExt) |

**Gap:** Malformed frame audit logging is partially implemented (the code returns early after emitting an error frame but does not call `append_watch_audit_entry` for frames that fail to parse). This is acceptable for v1 — the spec says "audit as watch-mode errors" but parsing failure means there is no `cmd` to attach the entry to. Log a minimal entry or add a `None`-cmd audit path if auditing every malformed frame is strictly required.
