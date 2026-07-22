//! Minimal parse-only worker experiment (Iteration 0) plus the bounded
//! ephemeral worker dispatch loop (Iteration 3).
//!
//! The Iteration 0 [`analyze`] helper is an in-process parse-only experiment
//! that proves the no-source contract and that the four foundation grammars
//! parse inline source on the host build.
//!
//! The Iteration 3 [`run`] dispatch loop is what the real ephemeral worker
//! process runs: it reads length-bounded versioned request frames
//! ([`crate::protocol`]), parses the supplied source bytes with the matching
//! Tree-sitter grammar, and writes one response frame per request. It is
//! **parse-only**: no filesystem access, no subprocess, no daemon, no socket
//! (ADR-022 §2). A bounded sequence of requests is served for one intercepted
//! command, then the loop force-exits.
//!
//! Worker failures (a malformed frame, a truncated trailing frame, a read or
//! write error) stop the loop with a typed [`RunOutcome`]; the parent process
//! converts those into `DegradationReason::WorkerFailure` while retaining
//! baseline and prior target results (ADR-022 §2, §4).

use std::io::{Read, Write};

use crate::language::{self, SourceLanguage};
use crate::protocol::{self, Request, Response};
use crate::router::{self, SourceTarget};

/// The maximum number of requests one worker session will serve before
/// force-stopping (ADR-022 §2: a bounded sequence for one intercepted command).
///
/// This is a worker-side safety cap, deliberately above any realistic
/// single-command target count (the parent's own budgets in ADR-022 §7 bound
/// the real target set); it exists so a runaway or hostile parent cannot keep
/// the worker alive indefinitely.
pub const MAX_REQUESTS_PER_SESSION: u32 = 64;

/// Why the worker dispatch loop stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// The reader reached end-of-input with no bytes left — the clean, expected
    /// end of a session (the parent closed its write end).
    EndOfInput,
    /// The session served its bounded quota of requests and force-stopped.
    MaxRequestsReached,
    /// A partial frame was left in the buffer when the reader hit end-of-input:
    /// the parent sent a truncated frame then closed. A worker failure.
    TruncatedFrame,
    /// A frame decoded as malformed (bad magic, version, kind, payload). A
    /// worker failure — the protocol carries no error-response variant, so the
    /// worker stops and the parent degrades.
    MalformedFrame,
    /// A read from the request stream failed. A worker failure.
    ReadFailed,
    /// Writing a response frame failed (the parent closed its read end early).
    /// A worker failure.
    WriteFailed,
}

/// Run the worker dispatch loop over `reader` / `writer`, serving up to
/// [`MAX_REQUESTS_PER_SESSION`] Parse requests then stopping.
pub fn run<R: Read, W: Write>(reader: R, writer: W) -> RunOutcome {
    run_with_limit(reader, writer, MAX_REQUESTS_PER_SESSION)
}

/// Run the worker dispatch loop with an explicit request cap, for tests that
/// exercise the force-exit bound without sending 64+ frames.
pub(crate) fn run_with_limit<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    max_requests: u32,
) -> RunOutcome {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut served: u32 = 0;
    loop {
        match protocol::decode_request(&buf) {
            Ok(Some(frame)) => {
                // A complete request arrived: drop it from the buffer and serve it.
                buf.drain(..frame.consumed);
                let response = handle_request(&frame.message);
                // A `Parsed`/`ParseFailed`/`UnsupportedLanguage` payload is tiny,
                // but an `Analyzed` payload frames a full `AdapterResult` that can
                // exceed `MAX_FRAME_PAYLOAD` for a large analysis result; treat the
                // resulting `Oversized` as a session-ending write failure (no panic,
                // no malformed frame on the wire).
                let encoded = match protocol::encode_response(frame.request_id, &response) {
                    Ok(bytes) => bytes,
                    Err(_) => return RunOutcome::WriteFailed,
                };
                if writer.write_all(&encoded).is_err() || writer.flush().is_err() {
                    return RunOutcome::WriteFailed;
                }
                served += 1;
                if served >= max_requests {
                    return RunOutcome::MaxRequestsReached;
                }
            }
            Ok(None) => match reader.read(&mut chunk) {
                Ok(0) => {
                    // End of input. A clean session ends with an empty buffer;
                    // leftover bytes mean the parent sent a truncated frame.
                    return if buf.is_empty() {
                        RunOutcome::EndOfInput
                    } else {
                        RunOutcome::TruncatedFrame
                    };
                }
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => return RunOutcome::ReadFailed,
            },
            Err(_) => return RunOutcome::MalformedFrame,
        }
    }
}

/// Parse or analyze the source carried by a [`Request`] and produce the
/// matching [`Response`]. The worker parses only the supplied bytes
/// (ADR-022 §2).
fn handle_request(request: &Request) -> Response {
    let (language, source) = match request {
        Request::Parse { language, source } => (*language, source.as_slice()),
        Request::Analyze { language, source } => {
            return analyze_source(language, source);
        }
    };
    // The parent is responsible for the UTF-8 encoding contract (ADR-022 §7);
    // bytes that are not valid UTF-8 cannot be parsed here, so the worker
    // reports a parse failure and the parent degrades.
    let source_str = match std::str::from_utf8(source) {
        Ok(s) => s,
        Err(_) => return Response::ParseFailed,
    };
    match language::parse(language, source_str) {
        Ok(tree) => Response::Parsed {
            // `has_error()` reports whether the tree contains any ERROR node.
            // A finer-grained count is a classifier concern (Iteration 5); the
            // worker only reports whether the parse was clean.
            error_count: u32::from(tree.root_node().has_error()),
        },
        Err(_) => Response::ParseFailed,
    }
}

/// Run the language adapter for an [`Request::Analyze`] and frame its result
/// (ADR-022 §2: adapters run in the self-spawned worker).
///
/// Python (Iteration 6), JavaScript, and TypeScript (Iteration 7) ship adapters;
/// Bash does not yet, so it yields [`Response::UnsupportedLanguage`], which the
/// parent maps to a degradation reason. The parent owns the UTF-8 encoding
/// contract (ADR-022 §7): bytes that are not valid UTF-8 cannot be handed to the
/// adapter (it takes a `&str`), so the worker reports an [`Response::Analyzed`]
/// result with one parse error and no operations — the parent maps
/// `parse_errors` to a degradation reason rather than treating the target as
/// clean.
fn analyze_source(language: &SourceLanguage, source: &[u8]) -> Response {
    let adapter = match language {
        SourceLanguage::Python => crate::languages::python::analyze,
        SourceLanguage::JavaScript => crate::languages::javascript::analyze,
        SourceLanguage::TypeScript => crate::languages::typescript::analyze,
        // No adapter is wired for Bash yet (L1 Shell/Bash is Iteration 8);
        // report the language as unsupported so the parent degrades rather than
        // silently treating the target as a clean parse.
        SourceLanguage::Bash => return Response::UnsupportedLanguage,
    };
    let source_str = match std::str::from_utf8(source) {
        Ok(s) => s,
        Err(_) => {
            return Response::Analyzed {
                result: crate::operation::AdapterResult {
                    operations: Vec::new(),
                    parse_errors: 1,
                },
            };
        }
    };
    Response::Analyzed {
        result: adapter(source_str),
    }
}

/// The outcome of a parse-only worker experiment on one command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The command exposed no analyzable source targets, so the worker did not
    /// start and performed no work — and no filesystem metadata calls.
    ///
    /// This is the only outcome a no-source command may produce (ADR-022
    /// Iteration 0 RED #3).
    NotStarted,
    /// The worker started and parsed the inline source of `targets` targets.
    ///
    /// `targets` counts successfully parsed bodies; a body that fails to parse
    /// is dropped rather than failing the whole experiment, because typed
    /// degradation reasons are not introduced until Iteration 1.
    Parsed { targets: usize },
}

/// Analyze `command` with the parse-only worker experiment.
///
/// No-source commands return [`Outcome::NotStarted`] without any filesystem
/// access. Commands with inline source are parsed in-process with the matching
/// Tree-sitter grammar.
#[must_use]
pub fn analyze(command: &str) -> Outcome {
    let targets = router::source_targets(command);
    if targets.is_empty() {
        return Outcome::NotStarted;
    }
    Outcome::Parsed {
        targets: count_parseable(targets),
    }
}

/// Count the targets whose inline body parses with its declared grammar.
fn count_parseable(targets: Vec<SourceTarget>) -> usize {
    targets
        .iter()
        .filter(|t| parses(t.language, &t.source))
        .count()
}

/// Parse `source` as `language`, returning whether it produced a tree.
fn parses(language: SourceLanguage, source: &str) -> bool {
    language::parse(language, source).is_ok()
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use crate::protocol::{self, DecodedFrame, Request, Response};

    /// Encode a sequence of requests into one byte buffer the worker can read.
    fn encode_requests(requests: &[(u32, Request)]) -> Vec<u8> {
        let mut buf = Vec::new();
        for (id, req) in requests {
            buf.extend_from_slice(
                &protocol::encode_request(*id, req).expect("test source encodes"),
            );
        }
        buf
    }

    /// Decode every response frame the worker wrote, in order.
    fn decode_responses(buf: &[u8]) -> Vec<DecodedFrame<Response>> {
        let mut out = Vec::new();
        let mut rest = buf;
        while let Some(frame) = protocol::decode_response(rest)
            .expect("worker output must be a sequence of well-formed response frames")
        {
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

    /// Build an `Analyze` request carrying `source` for `language`.
    fn analyze_request(language: SourceLanguage, source: &[u8]) -> Request {
        Request::Analyze {
            language,
            source: source.to_vec(),
        }
    }

    #[test]
    fn run_serves_one_clean_parse_request_and_stops_at_end_of_input() {
        let requests = encode_requests(&[(1, parse_request(SourceLanguage::Python, b"print(1)"))]);
        let reader = std::io::Cursor::new(requests);
        let mut output = Vec::new();

        let outcome = run(reader, &mut output);
        assert_eq!(outcome, RunOutcome::EndOfInput);

        let responses = decode_responses(&output);
        assert_eq!(
            responses.len(),
            1,
            "one request must yield exactly one response"
        );
        assert_eq!(responses[0].request_id, 1);
        assert_eq!(
            responses[0].message,
            Response::Parsed { error_count: 0 },
            "clean Python source must parse with zero errors"
        );
    }

    #[test]
    fn run_serves_a_bounded_sequence_then_stops_at_end_of_input() {
        let requests = encode_requests(&[
            (10, parse_request(SourceLanguage::Python, b"x = 1")),
            (11, parse_request(SourceLanguage::Bash, b"echo hi")),
            (
                12,
                parse_request(SourceLanguage::JavaScript, b"const x = 1;"),
            ),
        ]);
        let reader = std::io::Cursor::new(requests);
        let mut output = Vec::new();

        let outcome = run(reader, &mut output);
        assert_eq!(outcome, RunOutcome::EndOfInput);

        let responses = decode_responses(&output);
        assert_eq!(
            responses.iter().map(|f| f.request_id).collect::<Vec<_>>(),
            vec![10, 11, 12],
            "responses must be served in request order"
        );
        for f in &responses {
            assert_eq!(
                f.message,
                Response::Parsed { error_count: 0 },
                "each clean snippet must parse with zero errors"
            );
        }
    }

    #[test]
    fn run_force_exits_after_the_request_cap_without_serving_the_extra() {
        // Cap at 2; send 3. Only the first two may be served.
        let requests = encode_requests(&[
            (1, parse_request(SourceLanguage::Python, b"a = 1")),
            (2, parse_request(SourceLanguage::Python, b"b = 2")),
            (3, parse_request(SourceLanguage::Python, b"c = 3")),
        ]);
        let reader = std::io::Cursor::new(requests);
        let mut output = Vec::new();

        let outcome = run_with_limit(reader, &mut output, 2);
        assert_eq!(outcome, RunOutcome::MaxRequestsReached);

        let responses = decode_responses(&output);
        assert_eq!(
            responses.iter().map(|f| f.request_id).collect::<Vec<_>>(),
            vec![1, 2],
            "the third request past the cap must not be served"
        );
    }

    #[test]
    fn run_stops_on_a_malformed_frame_without_serving_it() {
        // A valid first frame, then a frame with bad magic.
        let mut buf = encode_requests(&[(1, parse_request(SourceLanguage::Python, b"x = 1"))]);
        buf.extend_from_slice(b"XXXX\x01\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00");
        let reader = std::io::Cursor::new(buf);
        let mut output = Vec::new();

        let outcome = run(reader, &mut output);
        assert_eq!(outcome, RunOutcome::MalformedFrame);

        let responses = decode_responses(&output);
        assert_eq!(
            responses.len(),
            1,
            "only the well-formed first request is served before the malformed frame"
        );
    }

    #[test]
    fn run_reports_a_truncated_trailing_frame_as_a_worker_failure() {
        // A valid frame followed by a partial header (5 bytes, not a full frame).
        let mut buf = encode_requests(&[(1, parse_request(SourceLanguage::Python, b"x = 1"))]);
        buf.extend_from_slice(b"AELW\x01");
        let reader = std::io::Cursor::new(buf);
        let mut output = Vec::new();

        let outcome = run(reader, &mut output);
        assert_eq!(outcome, RunOutcome::TruncatedFrame);

        let responses = decode_responses(&output);
        assert_eq!(
            responses.len(),
            1,
            "the complete first request is still served"
        );
    }

    #[test]
    fn run_reports_parse_failed_for_invalid_utf8_source() {
        // 0xFF is not valid UTF-8; the parent owns the encoding contract, so
        // the worker cannot parse these bytes and reports ParseFailed.
        let requests = encode_requests(&[(7, parse_request(SourceLanguage::Python, b"\xFF\xFE"))]);
        let reader = std::io::Cursor::new(requests);
        let mut output = Vec::new();

        let outcome = run(reader, &mut output);
        assert_eq!(outcome, RunOutcome::EndOfInput);

        let responses = decode_responses(&output);
        assert_eq!(responses.len(), 1);
        assert_eq!(
            responses[0].message,
            Response::ParseFailed,
            "invalid UTF-8 source must yield ParseFailed, not a panic"
        );
    }

    #[test]
    fn run_reports_a_nonzero_error_count_for_incomplete_syntax() {
        // `def f(` is incomplete Python: the grammar still produces a tree, but
        // it contains an ERROR node, so error_count must be nonzero.
        let requests = encode_requests(&[(9, parse_request(SourceLanguage::Python, b"def f("))]);
        let reader = std::io::Cursor::new(requests);
        let mut output = Vec::new();

        let outcome = run(reader, &mut output);
        assert_eq!(outcome, RunOutcome::EndOfInput);

        let responses = decode_responses(&output);
        assert_eq!(responses.len(), 1);
        assert!(
            matches!(responses[0].message, Response::Parsed { error_count } if error_count > 0),
            "incomplete syntax must parse to a tree with a nonzero error count"
        );
    }

    #[test]
    fn run_serves_a_clean_parse_for_each_foundation_grammar() {
        let snippets: &[(SourceLanguage, &[u8])] = &[
            (SourceLanguage::Python, b"print(1)"),
            (SourceLanguage::JavaScript, b"console.log(1);"),
            (SourceLanguage::TypeScript, b"let x: number = 1;"),
            (SourceLanguage::Bash, b"echo hi"),
        ];
        let requests: Vec<(u32, Request)> = snippets
            .iter()
            .enumerate()
            .map(|(i, (lang, src))| (i as u32, parse_request(*lang, src)))
            .collect();
        let encoded = encode_requests(&requests);
        let reader = std::io::Cursor::new(encoded);
        let mut output = Vec::new();

        let outcome = run(reader, &mut output);
        assert_eq!(outcome, RunOutcome::EndOfInput);

        let responses = decode_responses(&output);
        assert_eq!(responses.len(), snippets.len());
        for f in &responses {
            assert_eq!(
                f.message,
                Response::Parsed { error_count: 0 },
                "each foundation grammar must parse its clean snippet with zero errors"
            );
        }
    }

    // ── Analyze dispatch (Iteration 6 Slice A) ──────────────────────────────

    #[test]
    fn run_analyzes_python_source_and_returns_an_analyzed_response() {
        // A fully-qualified destructive call with a literal operand is in the
        // Python adapter's Slice 1 scope, so a clean `os.remove('x')` must
        // produce an `Analyzed` response with a clean parse (zero errors) and
        // at least one detected operation. The exact operation shape is the
        // adapter's own contract (pinned in `languages::python`); this test
        // pins only that the worker *dispatched* to the adapter and framed
        // its result.
        let requests = encode_requests(&[(
            21,
            analyze_request(SourceLanguage::Python, b"os.remove('x')"),
        )]);
        let reader = std::io::Cursor::new(requests);
        let mut output = Vec::new();

        let outcome = run(reader, &mut output);
        assert_eq!(outcome, RunOutcome::EndOfInput);

        let responses = decode_responses(&output);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].request_id, 21);
        match &responses[0].message {
            Response::Analyzed { result } => {
                assert_eq!(
                    result.parse_errors, 0,
                    "valid Python must analyze with a clean parse"
                );
                assert!(
                    !result.operations.is_empty(),
                    "a destructive call must surface at least one detected operation"
                );
            }
            other => panic!("Python Analyze must yield Analyzed, got {other:?}"),
        }
    }

    #[test]
    fn run_analyzes_javascript_source_and_returns_an_analyzed_response() {
        // Iteration 7 Slice 2 wires the JavaScript adapter into the worker, so
        // an Analyze request for a destructive JS body must dispatch to
        // `javascript::analyze` and return an `Analyzed` response. A clean
        // `fs.unlinkSync("data.txt")` is a FilesystemDelete with a Known operand
        // (pinned in `languages::javascript`), so the parse must be clean and at
        // least one operation must surface. The exact operation shape is the
        // adapter's own contract; this test pins only that the worker
        // *dispatched* to the adapter and framed its result.
        let requests = encode_requests(&[(
            24,
            analyze_request(SourceLanguage::JavaScript, b"fs.unlinkSync(\"data.txt\")"),
        )]);
        let reader = std::io::Cursor::new(requests);
        let mut output = Vec::new();

        let outcome = run(reader, &mut output);
        assert_eq!(outcome, RunOutcome::EndOfInput);

        let responses = decode_responses(&output);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].request_id, 24);
        match &responses[0].message {
            Response::Analyzed { result } => {
                assert_eq!(
                    result.parse_errors, 0,
                    "valid JavaScript must analyze with a clean parse"
                );
                assert!(
                    !result.operations.is_empty(),
                    "a destructive call must surface at least one detected operation"
                );
            }
            other => panic!("JavaScript Analyze must yield Analyzed, got {other:?}"),
        }
    }

    #[test]
    fn run_analyzes_typescript_source_and_returns_an_analyzed_response() {
        // Iteration 7 Slice 2 wires the TypeScript adapter into the worker, so an
        // Analyze request for a destructive TS body must dispatch to
        // `typescript::analyze` and return an `Analyzed` response. The call
        // `fs.unlinkSync<void>("data.txt")` carries an explicit type argument —
        // TypeScript-only syntax the JS adapter does not exercise — so a clean
        // parse plus at least one operation proves the worker reached the TS
        // adapter (the `calls.scm` query surfaces the op because
        // `type_arguments` is a separate child, not the `function` field; pinned
        // in `languages::typescript`). The exact operation shape is the adapter's
        // own contract; this test pins only that the worker *dispatched* to the
        // adapter and framed its result.
        let requests = encode_requests(&[(
            25,
            analyze_request(
                SourceLanguage::TypeScript,
                b"fs.unlinkSync<void>(\"data.txt\")",
            ),
        )]);
        let reader = std::io::Cursor::new(requests);
        let mut output = Vec::new();

        let outcome = run(reader, &mut output);
        assert_eq!(outcome, RunOutcome::EndOfInput);

        let responses = decode_responses(&output);
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].request_id, 25);
        match &responses[0].message {
            Response::Analyzed { result } => {
                assert_eq!(
                    result.parse_errors, 0,
                    "valid TypeScript must analyze with a clean parse"
                );
                assert!(
                    !result.operations.is_empty(),
                    "a destructive call must surface at least one detected operation"
                );
            }
            other => panic!("TypeScript Analyze must yield Analyzed, got {other:?}"),
        }
    }

    #[test]
    fn run_returns_unsupported_language_for_a_language_without_an_adapter() {
        // Python, JavaScript, and TypeScript ship adapters; Bash does not yet
        // (L1 Shell/Bash is Iteration 8), so an Analyze request for Bash must
        // yield `UnsupportedLanguage` rather than a fallback parse or a panic.
        // The parent maps this to a degradation reason (ADR-022 §9 honest
        // degradation). TypeScript gained an adapter in Iteration 7 Slice 2, so
        // it no longer exercises this path; Bash is the last unsupported
        // foundation grammar.
        let requests = encode_requests(&[(22, analyze_request(SourceLanguage::Bash, b"x = 1"))]);
        let reader = std::io::Cursor::new(requests);
        let mut output = Vec::new();

        let outcome = run(reader, &mut output);
        assert_eq!(outcome, RunOutcome::EndOfInput);

        let responses = decode_responses(&output);
        assert_eq!(responses.len(), 1);
        assert_eq!(
            responses[0].message,
            Response::UnsupportedLanguage,
            "a language with no adapter must yield UnsupportedLanguage"
        );
    }

    #[test]
    fn run_returns_an_analyzed_parse_error_for_invalid_utf8_python_source() {
        // The parent owns the UTF-8 encoding contract (ADR-022 §7). Bytes that
        // are not valid UTF-8 cannot be handed to the adapter (it takes a
        // `&str`), so the worker reports an `Analyzed` result with a single
        // parse error and no operations — the parent maps `parse_errors` to a
        // degradation reason rather than treating the target as clean.
        let requests =
            encode_requests(&[(23, analyze_request(SourceLanguage::Python, b"\xFF\xFE"))]);
        let reader = std::io::Cursor::new(requests);
        let mut output = Vec::new();

        let outcome = run(reader, &mut output);
        assert_eq!(outcome, RunOutcome::EndOfInput);

        let responses = decode_responses(&output);
        assert_eq!(responses.len(), 1);
        match &responses[0].message {
            Response::Analyzed { result } => {
                assert_eq!(
                    result.parse_errors, 1,
                    "invalid UTF-8 must surface as one parse error"
                );
                assert!(
                    result.operations.is_empty(),
                    "no operations may be reported for unparseable bytes"
                );
            }
            other => panic!("invalid-UTF-8 Python Analyze must yield Analyzed, got {other:?}"),
        }
    }
}
