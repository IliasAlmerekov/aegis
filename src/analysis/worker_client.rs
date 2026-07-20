//! Parent-side client for the ephemeral language worker (ADR-022 §2, L1
//! Iteration 3 slice 4).
//!
//! The parent process owns async orchestration. It spawns the worker as a
//! short-lived subprocess (`aegis --internal-language-worker`), writes
//! length-bounded request frames to the worker's stdin, reads response frames
//! from its stdout, correlates each response to its request by `request_id`
//! in send order, and enforces a per-session deadline. Every worker failure —
//! a timeout, an early close, a malformed frame on stdout, a duplicate or
//! out-of-order response, or an I/O error — is reported as a typed
//! [`WorkerError`] that maps to [`aegis_types::DegradationReason::WorkerFailure`].
//!
//! Responses already received when a failure ends the session are retained;
//! the remaining targets carry the failure (ADR-022 §2: "Results already
//! produced by the shell Scanner or an earlier analysis target are retained").
//!
//! This module is the pure client. Wiring its results into an `Assessment`
//! (monotonic merge with the baseline and prior target results) lands with the
//! Iteration 1 merge function and Iteration 4 source routing.

use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

use aegis_language::SourceLanguage;
use aegis_language::protocol::{self, DecodeError, Request, Response};
use aegis_types::DegradationReason;

/// The undocumented internal flag that turns the `aegis` binary into the
/// ephemeral language worker (ADR-022 §2). This is the single source of truth:
/// `src/main.rs` raw-argv-detects it before clap/runtime construction, and
/// [`Worker::spawn`] appends it when re-execing the binary in worker mode.
pub const INTERNAL_LANGUAGE_WORKER_FLAG: &str = "--internal-language-worker";

/// A single parse request addressed to the worker by `request_id`.
#[derive(Debug, Clone)]
pub struct TargetRequest {
    /// The correlation id used to match this request with its response.
    pub request_id: u32,
    /// The language to parse `source` as.
    pub language: SourceLanguage,
    /// The original source bytes to parse.
    pub source: Vec<u8>,
}

/// Why a target's analysis did not produce a clean [`Response`]. Every variant
/// maps to [`DegradationReason::WorkerFailure`] (ADR-022 §2, §4).
#[derive(Debug, Clone, thiserror::Error)]
pub enum WorkerError {
    /// The per-session deadline elapsed before all responses arrived.
    #[error("language-analysis deadline exceeded")]
    Timeout,
    /// The worker closed its stdout before all expected responses arrived
    /// (crash, early exit, or non-zero termination), or a write to its stdin
    /// failed because the pipe was already closed.
    #[error("worker closed its output before all responses arrived")]
    Closed,
    /// A frame on the worker's stdout failed to decode — protocol noise that
    /// would corrupt the framed stream.
    #[error("worker stdout contained a malformed frame: {0}")]
    ProtocolNoise(#[from] DecodeError),
    /// A response arrived twice for the same `request_id`.
    #[error("duplicate response for request id {request_id}")]
    DuplicateResponse {
        /// The `request_id` that was duplicated.
        request_id: u32,
    },
    /// A response arrived out of send order.
    #[error("out-of-order response: expected {expected}, got {got}")]
    OutOfOrder {
        /// The `request_id` that was expected next.
        expected: u32,
        /// The `request_id` that arrived instead.
        got: u32,
    },
    /// A response arrived for a `request_id` that was never sent.
    #[error("unexpected response for request id {request_id} (not in flight)")]
    UnexpectedResponse {
        /// The `request_id` that was not in flight.
        request_id: u32,
    },
    /// The worker exited with a non-zero status after the session. A non-zero
    /// exit taints the whole session — the worker is untrusted, so even
    /// responses already received degrade (ADR-022 §2: a non-zero exit is a
    /// worker failure). `code` is `None` when the worker was terminated by a
    /// signal rather than exiting with a code.
    #[error("worker exited with non-zero status: {code:?}")]
    NonZeroExit {
        /// The reaped exit code, or `None` if terminated by a signal.
        code: Option<i32>,
    },
    /// A read from the worker pipe failed. Carries the error message (the
    /// underlying `io::Error` is not `Clone`, so the string is kept instead).
    #[error("worker i/o error: {0}")]
    Io(String),
}

impl From<WorkerError> for DegradationReason {
    /// Every worker failure degrades as `WorkerFailure` — the parent retains
    /// prior results and never treats a worker failure as evidence of safety
    /// (ADR-022 §2, §5).
    fn from(_: WorkerError) -> Self {
        DegradationReason::WorkerFailure
    }
}

/// The per-target outcome: either a clean [`Response`] or a typed
/// [`WorkerError`].
#[derive(Debug, Clone)]
pub enum TargetResult {
    /// The worker returned a well-formed response for this target.
    Responded(Response),
    /// The target did not get a clean response; the worker failed for it.
    Failed(WorkerError),
}

/// Send `requests` to a worker over `reader` / `writer`, correlating responses
/// by `request_id` in send order, under `deadline`.
///
/// Returns one [`TargetResult`] per request, in send order. All request frames
/// are written first, then responses are read and matched. Responses already
/// received when a failure ends the session are retained; the remaining targets
/// carry the failure that ended the session.
///
/// This is the transport-agnostic core: `reader` is the worker→parent stream
/// and `writer` is the parent→worker stream. [`Worker`] wraps it for the real
/// subprocess; tests also feed it [`tokio::io::duplex`] ends to exercise
/// timeout, duplicate, and out-of-order responses without spawning a process.
pub async fn analyze<R, W>(
    reader: &mut R,
    writer: &mut W,
    requests: Vec<TargetRequest>,
    deadline: Duration,
) -> Vec<TargetResult>
where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send,
{
    let n = requests.len();
    let sent_ids: Vec<u32> = requests.iter().map(|r| r.request_id).collect();
    let mut results: Vec<Option<TargetResult>> = vec![None; n];

    // Send all requests. A send failure (encode, write, or flush) ends the
    // session with that failure for every target — the flush error is NOT
    // dropped, so it cannot masquerade as a read Timeout later.
    if let Err(fail) = send_requests(writer, &requests).await {
        return all_failed(n, fail);
    }

    // Read responses under a deadline, correlating by request_id in send order.
    let outcome = timeout(deadline, read_responses(reader, &sent_ids, &mut results)).await;
    let session_failure: Option<WorkerError> = match outcome {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => Some(WorkerError::Timeout),
    };

    // Any target that did not get a clean response carries the failure that
    // ended the session (ADR-022 §2: prior results retained, remainder failed).
    if let Some(fail) = session_failure {
        for slot in results.iter_mut() {
            if slot.is_none() {
                *slot = Some(TargetResult::Failed(fail.clone()));
            }
        }
    }

    results
        .into_iter()
        .map(|slot| slot.unwrap_or(TargetResult::Failed(WorkerError::Closed)))
        .collect()
}

/// Encode and send all `requests` to `writer` as one stream of frames, then
/// flush. A failure to encode (oversized source), write, or flush is propagated
/// as a typed [`WorkerError`] rather than dropped — a dropped flush error would
/// leave the worker without the requests and surface later as a read Timeout,
/// masking the real cause.
async fn send_requests<W>(writer: &mut W, requests: &[TargetRequest]) -> Result<(), WorkerError>
where
    W: AsyncWrite + Unpin + Send,
{
    let mut out = Vec::new();
    for req in requests {
        let frame = protocol::encode_request(
            req.request_id,
            &Request::Parse {
                language: req.language,
                source: req.source.clone(),
            },
        )
        .map_err(|_| WorkerError::Closed)?;
        out.extend_from_slice(&frame);
    }
    if out.is_empty() {
        return Ok(());
    }
    writer
        .write_all(&out)
        .await
        .map_err(|e| WorkerError::Io(e.to_string()))?;
    writer
        .flush()
        .await
        .map_err(|e| WorkerError::Io(e.to_string()))?;
    Ok(())
}

/// Build `n` copies of `fail` as `TargetResult::Failed`.
fn all_failed(n: usize, fail: WorkerError) -> Vec<TargetResult> {
    (0..n).map(|_| TargetResult::Failed(fail.clone())).collect()
}

/// Read response frames from `reader` and place them in `results` in send
/// order, correlating each by `request_id`. Returns `Err` with the typed
/// failure that ended the session; on `Err`, `results` contains every response
/// received before the failure (the caller fills the rest).
async fn read_responses<R>(
    reader: &mut R,
    sent_ids: &[u32],
    results: &mut [Option<TargetResult>],
) -> Result<(), WorkerError>
where
    R: AsyncRead + Unpin + Send,
{
    let n = sent_ids.len();
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut expected = 0usize;

    while expected < n {
        // Decode one complete frame, reading more bytes until one is available.
        let frame = loop {
            match protocol::decode_response(&buf) {
                Ok(Some(frame)) => {
                    buf.drain(..frame.consumed);
                    break frame;
                }
                Ok(None) => match reader.read(&mut chunk).await {
                    Ok(0) => return Err(WorkerError::Closed),
                    Ok(k) => buf.extend_from_slice(&chunk[..k]),
                    Err(e) => return Err(WorkerError::Io(e.to_string())),
                },
                Err(e) => return Err(WorkerError::ProtocolNoise(e)),
            }
        };

        // Correlate by request_id, strictly in send order.
        let id = frame.request_id;
        if id == sent_ids[expected] {
            results[expected] = Some(TargetResult::Responded(frame.message));
            expected += 1;
        } else if sent_ids[expected..].contains(&id) {
            return Err(WorkerError::OutOfOrder {
                expected: sent_ids[expected],
                got: id,
            });
        } else if sent_ids[..expected].contains(&id) {
            return Err(WorkerError::DuplicateResponse { request_id: id });
        } else {
            return Err(WorkerError::UnexpectedResponse { request_id: id });
        }
    }
    Ok(())
}

/// A spawned ephemeral language worker subprocess.
pub struct Worker {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    /// The reaped exit code once the child has been waited on; `None` before.
    exit_code: Option<i32>,
    /// Whether the child has already been reaped (so shutdown does not re-wait).
    reaped: bool,
}

impl Worker {
    /// Spawn the `aegis --internal-language-worker` subprocess with piped
    /// stdin/stdout/stderr.
    ///
    /// `aegis_path` overrides the binary to spawn; `None` uses `current_exe`,
    /// which is what production wiring does (the parent re-execs itself in
    /// worker mode). Tests pass a path to the built `aegis` binary or to a
    /// substitute command to exercise crash/noise behavior.
    pub async fn spawn(aegis_path: Option<&str>) -> std::io::Result<Worker> {
        let mut cmd = match aegis_path {
            Some(path) => Command::new(path),
            None => {
                let exe = std::env::current_exe()?;
                Command::new(exe)
            }
        };
        cmd.arg(INTERNAL_LANGUAGE_WORKER_FLAG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        Self::spawn_command(cmd)
    }

    /// Spawn an arbitrary command as the worker (no `--internal-language-worker`
    /// flag appended). Tests use this to run a substitute binary that mimics a
    /// misbehaving worker (responds-then-exits-nonzero, etc.).
    pub fn spawn_command(mut cmd: Command) -> std::io::Result<Worker> {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        Ok(Worker {
            child,
            stdin,
            stdout,
            exit_code: None,
            reaped: false,
        })
    }

    /// Run a full session against this worker under `deadline`:
    ///
    /// 1. Send all request frames and flush (a send failure degrades every
    ///    target with the typed error — the flush error is not dropped).
    /// 2. Close stdin so the worker sees end-of-session and exits (ADR-022 §2
    ///    ephemeral contract).
    /// 3. Read all responses under the deadline, correlating by `request_id`.
    /// 4. If every response was received, wait for the worker to exit; a
    ///    non-zero exit taints the whole session as [`WorkerError::NonZeroExit`]
    ///    (the worker is untrusted). A read failure kills and reaps the worker
    ///    and fills the remaining targets with that failure (prior results
    ///    retained).
    pub async fn analyze(
        &mut self,
        requests: Vec<TargetRequest>,
        deadline: Duration,
    ) -> Vec<TargetResult> {
        let n = requests.len();
        let sent_ids: Vec<u32> = requests.iter().map(|r| r.request_id).collect();
        let mut results: Vec<Option<TargetResult>> = vec![None; n];

        // 1. Send + flush. A send failure ends the session for every target.
        let send_err = match self.stdin.as_mut() {
            Some(writer) => send_requests(writer, &requests).await.err(),
            None => Some(WorkerError::Closed),
        };
        if let Some(fail) = send_err {
            self.reap_kill().await;
            return all_failed(n, fail);
        }

        // 2. Close stdin so the worker sees end-of-session (ADR-022 §2).
        self.stdin.take();

        // 3. Read responses under the deadline.
        let read_result: Result<(), WorkerError> = match self.stdout.as_mut() {
            Some(reader) => {
                match timeout(deadline, read_responses(reader, &sent_ids, &mut results)).await {
                    Ok(r) => r,
                    Err(_) => Err(WorkerError::Timeout),
                }
            }
            None => Err(WorkerError::Closed),
        };

        match read_result {
            Ok(()) => {
                // 4. All responses received; the worker is exiting. Waiting is
                // safe here — the worker finished its work and is reaped
                // promptly (it cannot hang once stdin is closed and all output
                // is drained). A non-zero exit taints the whole session.
                let code = self.child.wait().await.ok().and_then(|s| s.code());
                self.reaped = true;
                self.exit_code = code;
                if code != Some(0) {
                    return all_failed(n, WorkerError::NonZeroExit { code });
                }
                results
                    .into_iter()
                    .map(|s| s.unwrap_or(TargetResult::Failed(WorkerError::Closed)))
                    .collect()
            }
            Err(fail) => {
                // A read failure: the worker may still be running (e.g. Timeout).
                // Kill and reap it best-effort (no deadlock), then fill the
                // remaining targets — prior responses are retained.
                self.reap_kill().await;
                for slot in results.iter_mut() {
                    if slot.is_none() {
                        *slot = Some(TargetResult::Failed(fail.clone()));
                    }
                }
                results
                    .into_iter()
                    .map(|s| s.unwrap_or(TargetResult::Failed(WorkerError::Closed)))
                    .collect()
            }
        }
    }

    /// The worker's exit code, once [`analyze`](Self::analyze) or
    /// [`shutdown`](Self::shutdown) has reaped it; `None` until then (or if it
    /// was terminated by a signal).
    #[must_use]
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Close stdin (signaling end-of-session), then wait for the worker to
    /// exit and return its exit code. If [`analyze`](Self::analyze) already
    /// reaped the child, returns the cached code without re-waiting.
    pub async fn shutdown(mut self) -> std::io::Result<Option<i32>> {
        self.stdin.take();
        self.stdout.take();
        if !self.reaped {
            let status = self.child.wait().await?;
            self.exit_code = status.code();
            self.reaped = true;
        }
        Ok(self.exit_code)
    }

    /// Kill the child if it is still running and reap it, recording the exit
    /// code. Best-effort: errors are ignored (the caller already has a failure
    /// to report). `wait` is safe here because `start_kill` sends SIGKILL, so
    /// the child exits promptly.
    async fn reap_kill(&mut self) {
        let _ = self.child.start_kill();
        if let Ok(status) = self.child.wait().await {
            self.exit_code = status.code();
        }
        self.reaped = true;
    }
}
