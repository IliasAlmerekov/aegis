//! Iteration 3 slice 4 — the parent language-worker client.
//!
//! Hybrid test strategy (confirmed up front): real subprocess for the modes
//! the real worker or a substitute binary can produce (clean round-trip,
//! crash/non-zero exit, stdout noise), and `tokio::io::duplex` mocks for the
//! modes the well-behaved worker never produces (timeout, duplicate response,
//! out-of-order response, unexpected id, partial prior results on early EOF).

use std::io;
use std::pin::Pin;
use std::process::Stdio;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use aegis::analysis::{RequestKind, TargetRequest, TargetResult, Worker, WorkerError, analyze};
use aegis_language::SourceLanguage;
use aegis_language::protocol::{self, MAX_SOURCE_BYTES, Response};
use aegis_types::DegradationReason;

/// One Parse request with the given id.
fn req(id: u32, language: SourceLanguage, source: &[u8]) -> TargetRequest {
    TargetRequest {
        request_id: id,
        language,
        source: source.to_vec(),
        kind: RequestKind::Parse,
    }
}

/// A response frame bytes for `id` carrying `resp`.
fn resp_bytes(id: u32, resp: &Response) -> Vec<u8> {
    protocol::encode_response(id, resp).expect("test response encodes")
}

/// Spawn an arbitrary command with piped stdio and return its pipes.
fn spawn_piped(cmd: &mut Command) -> (Child, ChildStdin, ChildStdout) {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("command must spawn");
    let stdin = child.stdin.take().expect("stdin piped");
    let stdout = child.stdout.take().expect("stdout piped");
    (child, stdin, stdout)
}

/// Every `WorkerError` variant must degrade as `WorkerFailure`
/// (ADR-022 §2, §5). `WorkerError` is `Clone`, so this checks the real variant
/// (including `Io`), not a stand-in.
fn degrades_as_worker_failure(err: &WorkerError) -> bool {
    DegradationReason::from(err.clone()) == DegradationReason::WorkerFailure
}

// ── Real subprocess: clean round-trip ──────────────────────────────────────

#[tokio::test]
async fn analyze_round_trips_a_clean_parse_request_via_real_subprocess() {
    let mut worker = Worker::spawn(Some(env!("CARGO_BIN_EXE_aegis")))
        .await
        .expect("spawning the real worker must succeed");

    let results = worker
        .analyze(
            vec![req(1, SourceLanguage::Python, b"print(1)")],
            Duration::from_secs(2),
        )
        .await;
    assert_eq!(results.len(), 1);
    assert!(
        matches!(
            results[0],
            TargetResult::Responded(Response::Parsed { error_count: 0 })
        ),
        "clean Python source must round-trip: {results:?}"
    );

    let code = worker.shutdown().await.expect("shutdown must succeed");
    assert_eq!(code, Some(0), "a clean session must exit 0");
}

// ── L7: Worker::analyze closes stdin and reaps the child ───────────────────

#[tokio::test]
async fn worker_analyze_reaps_the_child_so_exit_code_is_available_immediately() {
    // ADR-022 §2: the worker is ephemeral. Worker::analyze must close stdin
    // (so the worker sees end-of-session and exits) and wait for the child to
    // be reaped — observable as a populated exit_code right after analyze,
    // without a separate shutdown call.
    let mut worker = Worker::spawn(Some(env!("CARGO_BIN_EXE_aegis")))
        .await
        .expect("spawning the real worker must succeed");

    let results = worker
        .analyze(
            vec![req(1, SourceLanguage::Python, b"print(1)")],
            Duration::from_secs(2),
        )
        .await;
    assert!(
        matches!(
            results[0],
            TargetResult::Responded(Response::Parsed { error_count: 0 })
        ),
        "clean source must round-trip: {results:?}"
    );
    // analyze reaped the child (closed stdin → worker exited → wait returned).
    assert_eq!(
        worker.exit_code(),
        Some(0),
        "Worker::analyze must close stdin and reap the child"
    );
}

// ── L8: a worker that responds fully then exits non-zero is not success ──────

/// Render `bytes` as a POSIX `printf` octal-escape string, so a substitute
/// shell command can emit exact binary response frames on stdout.
fn printf_octal(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 4);
    for b in bytes {
        s.push_str(&format!("\\{b:03o}"));
    }
    s
}

#[tokio::test]
async fn worker_analyze_degrades_when_the_worker_responds_fully_then_exits_nonzero() {
    // The substitute consumes stdin (so the parent's write succeeds and the
    // race is closed), emits one valid Parsed response for request 1, then
    // exits 3. The parent receives the response, then waits and sees the
    // non-zero exit — the whole session must degrade as NonZeroExit, not be
    // reported as success.
    let frame = resp_bytes(1, &Response::Parsed { error_count: 0 });
    let script = format!(
        "cat >/dev/null 2>&1; printf '{}'; exit 3",
        printf_octal(&frame)
    );
    let mut cmd = Command::new("sh");
    cmd.args(["-c", &script]);
    let mut worker =
        Worker::spawn_command(cmd).expect("spawning the substitute worker must succeed");

    let results = worker
        .analyze(
            vec![req(1, SourceLanguage::Python, b"print(1)")],
            Duration::from_secs(2),
        )
        .await;
    assert_eq!(results.len(), 1);
    match &results[0] {
        TargetResult::Failed(WorkerError::NonZeroExit { code }) => {
            assert_eq!(*code, Some(3), "the substitute's exit-3 must be reported");
        }
        other => panic!(
            "expected Failed(NonZeroExit {{ code: Some(3) }}) — a non-zero exit after \
             responding must not be silent success, got {other:?}"
        ),
    }
}

#[tokio::test]
async fn worker_deadline_covers_a_blocked_request_write() {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "sleep 5"]);
    let mut worker =
        Worker::spawn_command(cmd).expect("spawning the substitute worker must succeed");
    let source = vec![b'x'; MAX_SOURCE_BYTES];
    let started = Instant::now();

    let results = worker
        .analyze(
            vec![req(1, SourceLanguage::Python, &source)],
            Duration::from_millis(50),
        )
        .await;

    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(matches!(
        results.as_slice(),
        [TargetResult::Failed(WorkerError::Timeout)]
    ));
}

#[tokio::test]
async fn worker_deadline_covers_reap_after_a_valid_response() {
    let frame = resp_bytes(1, &Response::Parsed { error_count: 0 });
    let script = format!(
        "cat >/dev/null 2>&1; printf '{}'; sleep 5",
        printf_octal(&frame)
    );
    let mut cmd = Command::new("sh");
    cmd.args(["-c", &script]);
    let mut worker =
        Worker::spawn_command(cmd).expect("spawning the substitute worker must succeed");
    let started = Instant::now();

    let results = worker
        .analyze(
            vec![req(1, SourceLanguage::Python, b"print(1)")],
            Duration::from_millis(50),
        )
        .await;

    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(matches!(
        results.as_slice(),
        [TargetResult::Failed(WorkerError::Timeout)]
    ));
}

// ── Real subprocess: crash / non-zero exit ──────────────────────────────────

#[tokio::test]
async fn analyze_reports_a_worker_failure_when_the_worker_exits_nonzero_without_responding() {
    // Reads and discards stdin, then exits 3 without writing any response.
    // The parent's write succeeds; the read then hits EOF → a worker failure.
    let (_child, mut stdin, mut stdout) =
        spawn_piped(Command::new("sh").args(["-c", "cat >/dev/null 2>&1; exit 3"]));

    let results = analyze(
        &mut stdout,
        &mut stdin,
        vec![req(1, SourceLanguage::Python, b"print(1)")],
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(results.len(), 1);
    match &results[0] {
        TargetResult::Failed(err) => assert!(
            degrades_as_worker_failure(err),
            "a non-zero worker exit must degrade as WorkerFailure: {err}"
        ),
        other => panic!("expected a failure, got {other:?}"),
    }
}

// ── Real subprocess: stdout noise ───────────────────────────────────────────

#[tokio::test]
async fn analyze_reports_protocol_noise_when_stdout_is_not_a_frame() {
    // Writes 16 non-magic bytes to stdout (≥ HEADER_LEN so the decoder reaches
    // the magic check), then consumes stdin so the parent's write succeeds.
    let (_child, mut stdin, mut stdout) = spawn_piped(
        Command::new("sh").args(["-c", "printf 'AAAAAAAAAAAAAAAA'; cat >/dev/null 2>&1"]),
    );

    let results = analyze(
        &mut stdout,
        &mut stdin,
        vec![req(1, SourceLanguage::Python, b"print(1)")],
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(results.len(), 1);
    match &results[0] {
        TargetResult::Failed(WorkerError::ProtocolNoise(_)) => {}
        other => panic!("expected ProtocolNoise, got {other:?}"),
    }
}

// ── Duplex mock: timeout ────────────────────────────────────────────────────

#[tokio::test]
async fn analyze_times_out_when_the_worker_never_responds() {
    // The server end is held open but never writes a response, so the client's
    // read hangs until the deadline elapses.
    let (mut reader, _server_writer) = tokio::io::duplex(1024);
    let mut sink = tokio::io::sink();

    let results = analyze(
        &mut reader,
        &mut sink,
        vec![req(1, SourceLanguage::Python, b"print(1)")],
        Duration::from_millis(50),
    )
    .await;
    assert_eq!(results.len(), 1);
    match &results[0] {
        TargetResult::Failed(WorkerError::Timeout) => {}
        other => panic!("expected Timeout, got {other:?}"),
    }
}

// ── Duplex mock: duplicate response ─────────────────────────────────────────

#[tokio::test]
async fn analyze_detects_a_duplicate_response() {
    // Two requests [1, 2]; the server emits resp(1) twice, then resp(2).
    let (mut reader, mut server) = tokio::io::duplex(8192);
    server
        .write_all(&resp_bytes(1, &Response::Parsed { error_count: 0 }))
        .await
        .unwrap();
    server
        .write_all(&resp_bytes(1, &Response::Parsed { error_count: 0 }))
        .await
        .unwrap();
    server
        .write_all(&resp_bytes(2, &Response::Parsed { error_count: 0 }))
        .await
        .unwrap();
    drop(server);
    let mut sink = tokio::io::sink();

    let results = analyze(
        &mut reader,
        &mut sink,
        vec![
            req(1, SourceLanguage::Python, b"a = 1"),
            req(2, SourceLanguage::Python, b"b = 2"),
        ],
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(results.len(), 2);
    assert!(
        matches!(
            results[0],
            TargetResult::Responded(Response::Parsed { error_count: 0 })
        ),
        "the first response is retained: {results:?}"
    );
    match &results[1] {
        TargetResult::Failed(WorkerError::DuplicateResponse { request_id: 1 }) => {}
        other => panic!("expected DuplicateResponse {{ request_id: 1 }}, got {other:?}"),
    }
}

// ── Duplex mock: out-of-order response ──────────────────────────────────────

#[tokio::test]
async fn analyze_detects_an_out_of_order_response() {
    // Two requests [1, 2]; the server emits resp(2) before resp(1).
    let (mut reader, mut server) = tokio::io::duplex(8192);
    server
        .write_all(&resp_bytes(2, &Response::Parsed { error_count: 0 }))
        .await
        .unwrap();
    server
        .write_all(&resp_bytes(1, &Response::Parsed { error_count: 0 }))
        .await
        .unwrap();
    drop(server);
    let mut sink = tokio::io::sink();

    let results = analyze(
        &mut reader,
        &mut sink,
        vec![
            req(1, SourceLanguage::Python, b"a = 1"),
            req(2, SourceLanguage::Python, b"b = 2"),
        ],
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(results.len(), 2);
    match &results[0] {
        TargetResult::Failed(WorkerError::OutOfOrder {
            expected: 1,
            got: 2,
        }) => {}
        other => panic!("expected OutOfOrder {{ expected: 1, got: 2 }}, got {other:?}"),
    }
    assert!(
        matches!(results[1], TargetResult::Failed(_)),
        "the not-yet-responded target is also failed: {results:?}"
    );
}

// ── Duplex mock: unexpected response id ─────────────────────────────────────

#[tokio::test]
async fn analyze_detects_a_response_for_an_id_that_was_never_sent() {
    let (mut reader, mut server) = tokio::io::duplex(8192);
    server
        .write_all(&resp_bytes(99, &Response::Parsed { error_count: 0 }))
        .await
        .unwrap();
    drop(server);
    let mut sink = tokio::io::sink();

    let results = analyze(
        &mut reader,
        &mut sink,
        vec![req(1, SourceLanguage::Python, b"a = 1")],
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(results.len(), 1);
    match &results[0] {
        TargetResult::Failed(WorkerError::UnexpectedResponse { request_id: 99 }) => {}
        other => panic!("expected UnexpectedResponse {{ request_id: 99 }}, got {other:?}"),
    }
}

// ── Duplex mock: partial prior results on early EOF ─────────────────────────

#[tokio::test]
async fn analyze_retains_prior_results_when_a_later_target_hits_eof() {
    // Three requests [1, 2, 3]; the server emits resp(1) then closes.
    let (mut reader, mut server) = tokio::io::duplex(8192);
    server
        .write_all(&resp_bytes(1, &Response::Parsed { error_count: 0 }))
        .await
        .unwrap();
    drop(server);
    let mut sink = tokio::io::sink();

    let results = analyze(
        &mut reader,
        &mut sink,
        vec![
            req(1, SourceLanguage::Python, b"a = 1"),
            req(2, SourceLanguage::Python, b"b = 2"),
            req(3, SourceLanguage::Python, b"c = 3"),
        ],
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(results.len(), 3);
    assert!(
        matches!(
            results[0],
            TargetResult::Responded(Response::Parsed { error_count: 0 })
        ),
        "the first response is retained even though the session later failed: {results:?}"
    );
    assert!(
        matches!(results[1], TargetResult::Failed(WorkerError::Closed)),
        "the second target must fail Closed: {results:?}"
    );
    assert!(
        matches!(results[2], TargetResult::Failed(WorkerError::Closed)),
        "the third target must fail Closed: {results:?}"
    );
}

// ── Duplex mock: clean multi-target round-trip (no subprocess) ──────────────

#[tokio::test]
async fn analyze_correlates_a_clean_multi_target_session_by_request_id() {
    let (mut reader, mut server) = tokio::io::duplex(8192);
    server
        .write_all(&resp_bytes(10, &Response::Parsed { error_count: 0 }))
        .await
        .unwrap();
    server
        .write_all(&resp_bytes(11, &Response::Parsed { error_count: 1 }))
        .await
        .unwrap();
    drop(server);
    let mut sink = tokio::io::sink();

    let results = analyze(
        &mut reader,
        &mut sink,
        vec![
            req(10, SourceLanguage::Python, b"a = 1"),
            req(11, SourceLanguage::Python, b"def f("),
        ],
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(results.len(), 2);
    assert!(matches!(
        results[0],
        TargetResult::Responded(Response::Parsed { error_count: 0 })
    ));
    assert!(matches!(
        results[1],
        TargetResult::Responded(Response::Parsed { error_count: 1 })
    ));
}

// ── L1: a flush failure must surface as Io, not masquerade as Timeout ───────

/// A writer whose `write_all` succeeds but `flush` always fails, to pin the
/// flush-error propagation path in isolation from the write path.
struct FlushErrorWriter;

impl AsyncWrite for FlushErrorWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "flush failed (reader gone)",
        )))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn analyze_surfaces_a_flush_failure_as_io_not_timeout() {
    // The writer accepts the request bytes (write succeeds) but the flush
    // fails. Previously `let _ = writer.flush()` dropped this error and the
    // read loop would later time out, masking the real cause as Timeout.
    // Now the flush error ends the session as Io for every target.
    let mut reader = tokio::io::empty(); // no responses will ever arrive
    let mut writer = FlushErrorWriter;

    let results = analyze(
        &mut reader,
        &mut writer,
        vec![req(1, SourceLanguage::Python, b"print(1)")],
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(results.len(), 1);
    match &results[0] {
        TargetResult::Failed(WorkerError::Io(_)) => {}
        other => panic!(
            "expected Failed(Io) from the flush error, got {other:?} \
             (Timeout would mean the flush error was dropped again)"
        ),
    }
}

// ── L2 (client side): an oversized request degrades rather than panicking ──

#[tokio::test]
async fn analyze_degrades_an_oversized_request_without_panicking() {
    // A source above MAX_SOURCE_BYTES cannot be encoded; the client must
    // degrade the target (no panic from the encoder).
    let mut reader = tokio::io::empty();
    let mut sink = tokio::io::sink();

    let results = analyze(
        &mut reader,
        &mut sink,
        vec![req(
            1,
            SourceLanguage::Python,
            &vec![b'x'; MAX_SOURCE_BYTES + 1],
        )],
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(results.len(), 1);
    assert!(
        matches!(results[0], TargetResult::Failed(_)),
        "an oversized request must degrade, not panic: {results:?}"
    );
}
