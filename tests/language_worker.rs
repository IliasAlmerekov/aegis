//! Iteration 3 — the ephemeral language worker as a real subprocess.
//!
//! These tests spawn the official `aegis` binary with the undocumented
//! `--internal-language-worker` flag, which makes the process delegate
//! immediately to `aegis_language::worker::run` over stdin/stdout (ADR-022 §2).
//! They pin the subprocess-level invariants the parent client (Iteration 3
//! slice 4) relies on:
//!
//! - a clean Parse request round-trips over real pipes;
//! - the worker writes *only* frame bytes to stdout — no noise that would
//!   corrupt the framed stream;
//! - the worker exits cleanly when the parent closes stdin (end of session);
//! - a malformed frame stops the worker with a non-zero exit, and only the
//!   responses served before the malformed frame are present on stdout.
//!
//! Crash/hang/timeout handling and prior-result retention are parent-client
//! concerns and are covered by the slice-4 client tests, not here.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

use aegis_language::SourceLanguage;
use aegis_language::protocol::{self, DecodedFrame, Request, Response};

/// Path to the built `aegis` binary under test.
fn aegis_bin() -> String {
    env!("CARGO_BIN_EXE_aegis").to_owned()
}

/// Spawn the worker subprocess with piped stdin/stdout.
fn spawn_worker() -> std::process::Child {
    Command::new(aegis_bin())
        .arg("--internal-language-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the aegis binary must be buildable and spawnable")
}

/// Encode a sequence of requests into one byte buffer.
fn encode_requests(requests: &[(u32, Request)]) -> Vec<u8> {
    let mut buf = Vec::new();
    for (id, req) in requests {
        buf.extend_from_slice(&protocol::encode_request(*id, req).expect("test source encodes"));
    }
    buf
}

/// Decode every response frame from `buf`, returning them in order. Asserts
/// that the buffer is consumed exactly with no trailing bytes — i.e. the
/// worker wrote only well-formed frames and no noise.
fn decode_all_responses(buf: &[u8]) -> Vec<DecodedFrame<Response>> {
    let mut out = Vec::new();
    let mut rest = buf;
    while !rest.is_empty() {
        let frame = protocol::decode_response(rest)
            .expect("worker stdout must be a sequence of well-formed response frames")
            .expect("a non-empty buffer must contain a complete frame");
        let consumed = frame.consumed;
        out.push(frame);
        rest = &rest[consumed..];
    }
    out
}

fn parse_request(language: SourceLanguage, source: &[u8]) -> Request {
    Request::Parse {
        language,
        source: source.to_vec(),
    }
}

/// Run a worker session: write `input` to stdin, close it, then read all
/// stdout and wait for the exit status.
fn run_session(input: &[u8]) -> (Vec<u8>, std::process::ExitStatus) {
    let mut child = spawn_worker();
    {
        let mut stdin = child.stdin.take().expect("stdin must be piped");
        stdin
            .write_all(input)
            .expect("writing requests to the worker must succeed");
        // Dropping stdin closes the pipe, signaling end-of-session.
    }
    let mut stdout = child.stdout.take().expect("stdout must be piped");
    let mut out = Vec::new();
    stdout
        .read_to_end(&mut out)
        .expect("reading worker stdout must succeed");
    let status = child.wait().expect("the worker must terminate");
    (out, status)
}

#[test]
fn worker_subprocess_round_trips_a_clean_parse_request() {
    let input = encode_requests(&[(1, parse_request(SourceLanguage::Python, b"print(1)"))]);
    let (out, status) = run_session(&input);

    assert!(status.success(), "a clean session must exit 0");
    let responses = decode_all_responses(&out);
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].request_id, 1);
    assert_eq!(
        responses[0].message,
        Response::Parsed { error_count: 0 },
        "clean Python source must round-trip through the real subprocess"
    );
}

#[test]
fn worker_subprocess_writes_only_frame_bytes_to_stdout() {
    // Two clean requests. The entire stdout buffer must decode to exactly two
    // frames with zero leftover bytes — any extra byte (a log line, a prompt,
    // tracing output) would corrupt the framed stream.
    let input = encode_requests(&[
        (1, parse_request(SourceLanguage::Python, b"x = 1")),
        (2, parse_request(SourceLanguage::Bash, b"echo hi")),
    ]);
    let (out, status) = run_session(&input);

    assert!(status.success());
    let responses = decode_all_responses(&out);
    assert_eq!(
        responses.iter().map(|f| f.request_id).collect::<Vec<_>>(),
        vec![1, 2]
    );
    // `decode_all_responses` asserts no trailing bytes by consuming the whole
    // buffer; reaching here with two frames proves stdout was noise-free.
}

#[test]
fn worker_subprocess_exits_cleanly_when_stdin_closes_with_no_requests() {
    // No requests at all — the parent spawned the worker and immediately closed
    // stdin. The worker must read end-of-input and exit 0 without writing
    // anything.
    let (out, status) = run_session(&[]);
    assert!(status.success(), "an empty session must exit 0");
    assert!(
        out.is_empty(),
        "the worker must write nothing when no requests were sent"
    );
}

#[test]
fn worker_subprocess_stops_on_a_malformed_frame_with_a_nonzero_exit() {
    // A valid first frame, then a frame with bad magic. The worker must serve
    // the first, then stop on the malformed frame and exit non-zero.
    let mut input = encode_requests(&[(1, parse_request(SourceLanguage::Python, b"x = 1"))]);
    input.extend_from_slice(b"XXXX\x01\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00");
    let (out, status) = run_session(&input);

    assert!(
        !status.success(),
        "a malformed frame must cause a non-zero worker exit"
    );
    let responses = decode_all_responses(&out);
    assert_eq!(
        responses.len(),
        1,
        "only the well-formed first request is served before the malformed frame"
    );
}

#[test]
fn worker_subprocess_serves_a_bounded_sequence_over_real_pipes() {
    let input = encode_requests(&[
        (10, parse_request(SourceLanguage::Python, b"a = 1")),
        (
            11,
            parse_request(SourceLanguage::JavaScript, b"const x = 1;"),
        ),
        (12, parse_request(SourceLanguage::Bash, b"echo hi")),
    ]);
    let (out, status) = run_session(&input);

    assert!(status.success());
    let responses = decode_all_responses(&out);
    assert_eq!(
        responses.iter().map(|f| f.request_id).collect::<Vec<_>>(),
        vec![10, 11, 12]
    );
}
