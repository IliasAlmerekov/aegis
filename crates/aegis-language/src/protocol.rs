//! Versioned, length-bounded request/response framing for the language worker
//! (ADR-022 §2, L1 Iteration 3).
//!
//! The parent process and the ephemeral worker communicate over a pipe using
//! this binary frame format. Source bytes travel *through* the protocol; the
//! worker never reads the filesystem or spawns a subprocess, and the frame
//! format encodes no way to ask for either — the only [`Request`] variant is
//! [`Request::Parse`], which carries the source bytes directly.
//!
//! # Wire format
//!
//! Every frame shares a fixed 15-byte header followed by a variable payload:
//!
//! ```text
//!  offset  field         type        notes
//!  ------  -----------   ----------  --------------------------------
//!   0      magic         [u8; 4]     b"AELW" (Aegis Language Worker)
//!   4      version       u16 LE      wire-format version (currently 1)
//!   6      request_id    u32 LE      correlates a request with its response
//!  10      kind          u8          message-type tag (see Request/Response)
//!  11      payload_len   u32 LE      number of bytes following the header
//!  15      payload       [u8]        `payload_len` bytes, kind-specific
//! ```
//!
//! All multi-byte integers are little-endian. `payload_len` is bounded by
//! [`MAX_FRAME_PAYLOAD`] (1 MiB, the ADR-022 §7 hard per-file source ceiling);
//! a frame declaring more is rejected without allocating or reading its body.
//!
//! Kind tags are disjoint across directions so a response tag can never be
//! decoded as a request and vice versa: request tags live in `0x01..=0x7F`,
//! response tags in `0x80..=0xFF`.
//!
//! This module is pure: it operates on `&[u8]` / `Vec<u8>` and performs no I/O.
//! The pipe reader/writer and the worker dispatch loop live in [`crate::worker`].

use crate::language::SourceLanguage;

/// The magic bytes identifying a language-worker frame: `AELW`.
pub const MAGIC: [u8; 4] = *b"AELW";

/// The current wire-format version.
pub const PROTOCOL_VERSION: u16 = 1;

/// The fixed size of a frame header, in bytes.
pub const HEADER_LEN: usize = 15;

/// The hard per-target source-byte ceiling (ADR-022 §7): 1 MiB, non-configurable.
/// This is the maximum number of *source* bytes a single Parse request may
/// carry.
pub const MAX_SOURCE_BYTES: usize = 1 << 20; // 1 MiB

/// The maximum payload a single frame may carry. A Parse request payload is one
/// language-tag byte plus the source bytes, so this is [`MAX_SOURCE_BYTES`] +
/// 1 — a legal-max (1 MiB) source fits without being rejected as oversized.
/// The decoder rejects a frame declaring a larger payload before its body is
/// read or allocated; the encoder rejects one before it is built.
pub const MAX_FRAME_PAYLOAD: usize = MAX_SOURCE_BYTES + 1;

// Compile-time guarantee that a payload bounded by MAX_FRAME_PAYLOAD fits in a
// u32 length field, so the encoder can use a non-panicking `as u32` cast.
const _: () = assert!(MAX_FRAME_PAYLOAD <= u32::MAX as usize);

/// A worker request. The parent sends these; the worker decodes them.
///
/// `#[non_exhaustive]` so later iterations can add request kinds (e.g. a
/// shutdown signal) without breaking serialization consumers. Iteration 3
/// defined [`Request::Parse`]; Iteration 6 adds [`Request::Analyze`] so the
/// ephemeral worker can run the language adapter and return a full
/// [`AdapterResult`](crate::operation::AdapterResult) (ADR-022 §2: adapters
/// run in the self-spawned worker, not the parent).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Parse `source` as `language`. The worker parses only these bytes; it
    /// may not read the filesystem or spawn a subprocess (ADR-022 §2).
    Parse {
        /// The language to parse `source` as.
        language: SourceLanguage,
        /// The original source bytes to parse. Owned because they are sliced
        /// out of the parent's command string and sent across the pipe.
        source: Vec<u8>,
    },
    /// Analyze `source` as `language`: parse it and run the language adapter
    /// to produce a full [`AdapterResult`](crate::operation::AdapterResult).
    /// The payload shape is identical to [`Request::Parse`] (one language-tag
    /// byte plus the source); only the kind tag differs, so the same 1 MiB
    /// source ceiling and the same "no path read / no subprocess" property
    /// apply.
    Analyze {
        /// The language to analyze `source` as.
        language: SourceLanguage,
        /// The original source bytes to analyze.
        source: Vec<u8>,
    },
}

impl Request {
    /// The wire-format kind tag for [`Request::Parse`].
    pub const KIND_PARSE: u8 = 0x01;
    /// The wire-format kind tag for [`Request::Analyze`]. Disjoint from the
    /// response tags (which live in `0x80..=0xFF`) and from
    /// [`Request::KIND_PARSE`].
    pub const KIND_ANALYZE: u8 = 0x02;
}

/// A worker response. The worker sends these; the parent decodes them.
///
/// Iteration 3 is the bounded *parser* process — the worker reports whether
/// the supplied bytes parsed. Iteration 6 adds the analyze surface: a full
/// [`AdapterResult`](crate::operation::AdapterResult) for a language the worker
/// has an adapter for, or [`Response::UnsupportedLanguage`] when it does not
/// (ADR-022 §2: adapters run in the self-spawned worker).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    /// The source parsed into a tree. `error_count` is the Tree-sitter error
    /// count (0 = clean parse); a non-zero count means the tree has ERROR
    /// nodes but a tree was still produced.
    Parsed { error_count: u32 },
    /// The source could not be parsed into a tree at all. The client maps this
    /// to [`crate::language`]'s parse-failure path, which becomes
    /// `DegradationReason::IncompleteSyntax` once merged into an `Assessment`.
    ParseFailed,
    /// The full adapter output for an [`Request::Analyze`] the worker had a
    /// language adapter for (ADR-022 §2). The parent maps `result` into its
    /// language-neutral `analysis` vocabulary.
    Analyzed {
        /// The detected operations and parse-error count for the analyzed
        /// source target.
        result: crate::operation::AdapterResult,
    },
    /// The worker was asked to [`Request::Analyze`] a language it has no
    /// adapter for. The parent maps this to a degradation reason (e.g.
    /// `GrammarUnavailable`) rather than treating it as a clean parse.
    UnsupportedLanguage,
}

impl Response {
    /// The wire-format kind tag for [`Response::Parsed`].
    pub const KIND_PARSED: u8 = 0x81;
    /// The wire-format kind tag for [`Response::ParseFailed`].
    pub const KIND_PARSE_FAILED: u8 = 0x82;
    /// The wire-format kind tag for [`Response::Analyzed`]. Lives in the
    /// response half of the kind-tag space (`0x80..=0xFF`), disjoint from
    /// every request tag.
    pub const KIND_ANALYZED: u8 = 0x83;
    /// The wire-format kind tag for [`Response::UnsupportedLanguage`].
    pub const KIND_UNSUPPORTED: u8 = 0x84;
}

/// A decoded frame: the correlated request id and the typed message, plus how
/// many bytes the frame consumed from the front of the buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame<T> {
    /// The request id copied from the frame header, for response correlation.
    pub request_id: u32,
    /// The typed request or response.
    pub message: T,
    /// The number of bytes consumed from the front of the input buffer.
    pub consumed: usize,
}

/// A frame decoding failure. Incomplete input (a buffer that does not yet
/// contain a whole frame) is **not** an error — [`decode_request`] and
/// [`decode_response`] return `Ok(None)` for that, so the caller can wait for
/// more bytes. This enum covers only malformed-but-decidable frames.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// The frame's first four bytes were not [`MAGIC`].
    #[error("bad frame magic; expected {:?}, got {got:?}", MAGIC)]
    BadMagic { got: [u8; 4] },
    /// The frame's version is not one the decoder supports.
    #[error("unsupported wire-format version {version} (supported {supported})", supported = PROTOCOL_VERSION)]
    UnsupportedVersion { version: u16 },
    /// The frame declared a payload larger than [`MAX_FRAME_PAYLOAD`].
    #[error("oversized frame payload: declared {declared} bytes, max {max}")]
    Oversized { declared: u32, max: u32 },
    /// The frame's kind tag is not a known message type for this direction.
    #[error("invalid message kind tag {tag:#04x}")]
    InvalidKind { tag: u8 },
    /// The frame's payload was malformed for its kind (bad length, unknown
    /// language tag, …). The string identifies which sub-field was wrong.
    #[error("invalid payload: {0}")]
    InvalidPayload(&'static str),
}

/// A frame encoding failure. The encoder is fallible so it can reject an
/// oversized payload without panicking (ADR-022 §7; the production path has no
/// `.expect()`). This is symmetric with [`DecodeError::Oversized`] on the
/// decode side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EncodeError {
    /// The payload exceeded [`MAX_FRAME_PAYLOAD`].
    #[error("frame payload ({actual} bytes) exceeds the {max}-byte ceiling")]
    Oversized { actual: usize, max: usize },
    /// A `usize` field (a byte offset, a length, or an operation count) did not
    /// fit in a `u32` wire field. Unreachable for a source bounded by the
    /// protocol's 1 MiB ceiling (ADR-022 §7), but the codec is fallible rather
    /// than truncating with `as u32` so the production path never panics.
    #[error("wire field {field} overflows u32")]
    FieldOverflow { field: &'static str },
}

/// Encode `request` with `request_id` into a fresh `Vec<u8>`, or
/// [`Err(EncodeError::Oversized)`](EncodeError) if the source exceeds
/// [`MAX_SOURCE_BYTES`].
pub fn encode_request(request_id: u32, request: &Request) -> Result<Vec<u8>, EncodeError> {
    // Parse and Analyze share the same payload shape (one language-tag byte
    // plus the source), so the payload build is common; only the kind tag
    // differs.
    let (kind, language, source) = match request {
        Request::Parse { language, source } => (Request::KIND_PARSE, *language, source.as_slice()),
        Request::Analyze { language, source } => {
            (Request::KIND_ANALYZE, *language, source.as_slice())
        }
    };
    let mut payload = Vec::with_capacity(1 + source.len());
    payload.push(language_to_wire(language));
    payload.extend_from_slice(source);
    encode_frame(request_id, kind, &payload)
}

/// Decode one request frame from the front of `buf`.
///
/// Returns `Ok(None)` when `buf` does not yet contain a whole frame (the
/// caller should read more bytes); `Err` for a malformed-but-decidable frame;
/// and `Ok(Some)` with the typed request and the number of bytes consumed.
pub fn decode_request(buf: &[u8]) -> Result<Option<DecodedFrame<Request>>, DecodeError> {
    let (request_id, kind, payload, consumed) = match decode_header(buf)? {
        Some(h) => h,
        None => return Ok(None),
    };

    // The kind tag discriminates the message type. Parse and Analyze share a
    // payload shape (one language-tag byte plus the source); any other tag
    // (including response tags) is rejected.
    let message = match kind {
        Request::KIND_PARSE => {
            let (language, source) = decode_lang_source_payload(payload)?;
            Request::Parse { language, source }
        }
        Request::KIND_ANALYZE => {
            let (language, source) = decode_lang_source_payload(payload)?;
            Request::Analyze { language, source }
        }
        other => return Err(DecodeError::InvalidKind { tag: other }),
    };

    Ok(Some(DecodedFrame {
        request_id,
        message,
        consumed,
    }))
}

/// Encode `response` with `request_id` into a fresh `Vec<u8>`. The payload is
/// bounded by [`MAX_FRAME_PAYLOAD`]: `Parsed`/`ParseFailed`/`UnsupportedLanguage`
/// are tiny, but an [`Response::Analyzed`] frames a full `AdapterResult` whose
/// wire form scales with the number of detected operations, so encoding a large
/// analysis result can fail as [`EncodeError::Oversized`]. The `Result` keeps
/// the encoder symmetric and panic-free with the request side; callers must
/// treat `Oversized` as a real (degradation) failure mode, not an impossible
/// one.
pub fn encode_response(request_id: u32, response: &Response) -> Result<Vec<u8>, EncodeError> {
    let (kind, payload): (u8, Vec<u8>) = match response {
        Response::Parsed { error_count } => {
            (Response::KIND_PARSED, error_count.to_le_bytes().to_vec())
        }
        Response::ParseFailed => (Response::KIND_PARSE_FAILED, Vec::new()),
        Response::Analyzed { result } => {
            // The AdapterResult codec is the single source of truth for its
            // wire form; the protocol just frames it.
            (
                Response::KIND_ANALYZED,
                crate::operation::encode_adapter_result(result)?,
            )
        }
        Response::UnsupportedLanguage => (Response::KIND_UNSUPPORTED, Vec::new()),
    };
    encode_frame(request_id, kind, &payload)
}

/// Decode one response frame from the front of `buf`.
///
/// See [`decode_request`] for the `Ok(None)` / `Err` / `Ok(Some)` contract.
pub fn decode_response(buf: &[u8]) -> Result<Option<DecodedFrame<Response>>, DecodeError> {
    let (request_id, kind, payload, consumed) = match decode_header(buf)? {
        Some(h) => h,
        None => return Ok(None),
    };

    let message = match kind {
        Response::KIND_PARSED => {
            // Parsed payload: exactly error_count u32 LE.
            let error_count = u32::from_le_bytes(payload.try_into().map_err(|_| {
                DecodeError::InvalidPayload("Parsed response payload must be 4 bytes (error_count)")
            })?);
            Response::Parsed { error_count }
        }
        Response::KIND_PARSE_FAILED => {
            if !payload.is_empty() {
                return Err(DecodeError::InvalidPayload(
                    "ParseFailed response payload must be empty",
                ));
            }
            Response::ParseFailed
        }
        Response::KIND_ANALYZED => {
            // The AdapterResult codec owns validation and returns the same
            // `DecodeError` type, so a malformed payload propagates its
            // precise reason directly (e.g. "trailing bytes after
            // AdapterResult", "unknown operation kind discriminant").
            let result = crate::operation::decode_adapter_result(payload)?;
            Response::Analyzed { result }
        }
        Response::KIND_UNSUPPORTED => {
            if !payload.is_empty() {
                return Err(DecodeError::InvalidPayload(
                    "UnsupportedLanguage response payload must be empty",
                ));
            }
            Response::UnsupportedLanguage
        }
        other => return Err(DecodeError::InvalidKind { tag: other }),
    };

    Ok(Some(DecodedFrame {
        request_id,
        message,
        consumed,
    }))
}

/// Write a frame with `request_id`, `kind`, and `payload` into a fresh buffer,
/// or [`Err(EncodeError::Oversized)`](EncodeError) if the payload exceeds
/// [`MAX_FRAME_PAYLOAD`].
fn encode_frame(request_id: u32, kind: u8, payload: &[u8]) -> Result<Vec<u8>, EncodeError> {
    if payload.len() > MAX_FRAME_PAYLOAD {
        return Err(EncodeError::Oversized {
            actual: payload.len(),
            max: MAX_FRAME_PAYLOAD,
        });
    }
    // payload.len() ≤ MAX_FRAME_PAYLOAD ≤ u32::MAX (const-asserted above), so
    // the cast is exact — no `.expect()`, no truncation, no panic.
    let payload_len = payload.len() as u32;
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    out.extend_from_slice(&request_id.to_le_bytes());
    out.push(kind);
    out.extend_from_slice(&payload_len.to_le_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// The parsed header fields of a complete frame: the request id, kind tag,
/// payload slice, and total bytes consumed from the front of the buffer.
type DecodedHeader<'a> = (u32, u8, &'a [u8], usize);

/// Decode the shared frame header and, on a complete frame, return the request
/// id, kind tag, payload slice, and total bytes consumed. Returns `Ok(None)`
/// when more bytes are needed and `Err` for a malformed-but-decidable header.
fn decode_header(buf: &[u8]) -> Result<Option<DecodedHeader<'_>>, DecodeError> {
    // Need the full header to know how many payload bytes follow.
    if buf.len() < HEADER_LEN {
        return Ok(None);
    }
    // Reject noise/corruption as soon as the magic is readable.
    let magic: [u8; 4] = buf[0..4].try_into().expect("checked len");
    if magic != MAGIC {
        return Err(DecodeError::BadMagic { got: magic });
    }
    let version = u16::from_le_bytes(buf[4..6].try_into().expect("checked len"));
    if version != PROTOCOL_VERSION {
        return Err(DecodeError::UnsupportedVersion { version });
    }
    let request_id = u32::from_le_bytes(buf[6..10].try_into().expect("checked len"));
    let kind = buf[10];
    let payload_len = u32::from_le_bytes(buf[11..15].try_into().expect("checked len"));
    if payload_len as usize > MAX_FRAME_PAYLOAD {
        // Reject from the header alone, before allocating or reading the body.
        return Err(DecodeError::Oversized {
            declared: payload_len,
            max: u32::try_from(MAX_FRAME_PAYLOAD).expect("MAX_FRAME_PAYLOAD fits in u32"),
        });
    }
    let total = HEADER_LEN
        .checked_add(payload_len as usize)
        .expect("frame total fits in usize");
    if buf.len() < total {
        // The declared payload has not fully arrived yet.
        return Ok(None);
    }
    let payload = &buf[HEADER_LEN..total];
    Ok(Some((request_id, kind, payload, total)))
}

/// Decode the shared `[lang_u8][source bytes…]]` payload used by both
/// [`Request::Parse`] and [`Request::Analyze`]. Both request kinds carry one
/// language-tag byte plus the source; the error messages are kept generic
/// ("request", not "parse request") so they stay accurate for either kind.
/// Returns the typed language and the owned source bytes.
fn decode_lang_source_payload(payload: &[u8]) -> Result<(SourceLanguage, Vec<u8>), DecodeError> {
    let (lang_byte, source) = payload.split_first().ok_or(DecodeError::InvalidPayload(
        "request payload missing language tag",
    ))?;
    let language = wire_to_language(*lang_byte).ok_or(DecodeError::InvalidPayload(
        "unknown language tag in request",
    ))?;
    Ok((language, source.to_vec()))
}

/// Map a [`SourceLanguage`] to its single-byte wire tag.
///
/// `pub(crate)` so the [`crate::operation`] codec shares this single source of
/// truth rather than duplicating the language→tag mapping (a `NestedTarget`
/// carries a `SourceLanguage` that is serialized with the same tags).
pub(crate) fn language_to_wire(language: SourceLanguage) -> u8 {
    match language {
        SourceLanguage::Python => 0x00,
        SourceLanguage::JavaScript => 0x01,
        SourceLanguage::TypeScript => 0x02,
        SourceLanguage::Bash => 0x03,
    }
}

/// Map a single-byte wire tag back to a [`SourceLanguage`], or `None` if the
/// tag does not name a foundation language. `pub(crate)` for the same reason as
/// [`language_to_wire`].
pub(crate) fn wire_to_language(tag: u8) -> Option<SourceLanguage> {
    match tag {
        0x00 => Some(SourceLanguage::Python),
        0x01 => Some(SourceLanguage::JavaScript),
        0x02 => Some(SourceLanguage::TypeScript),
        0x03 => Some(SourceLanguage::Bash),
        _ => None,
    }
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
