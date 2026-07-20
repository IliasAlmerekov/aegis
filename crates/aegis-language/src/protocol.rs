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
/// defines exactly one: parse supplied bytes as a language.
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
}

impl Request {
    /// The wire-format kind tag for [`Request::Parse`].
    pub const KIND_PARSE: u8 = 0x01;
}

/// A worker response. The worker sends these; the parent decodes them.
///
/// Iteration 3 is the bounded *parser* process — the worker reports whether
/// the supplied bytes parsed. The shared classifier that turns a parse into
/// `DetectedOperation` evidence lands in Iteration 5, so no operation-level
/// response variant exists yet.
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
}

impl Response {
    /// The wire-format kind tag for [`Response::Parsed`].
    pub const KIND_PARSED: u8 = 0x81;
    /// The wire-format kind tag for [`Response::ParseFailed`].
    pub const KIND_PARSE_FAILED: u8 = 0x82;
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
}

/// Encode `request` with `request_id` into a fresh `Vec<u8>`, or
/// [`Err(EncodeError::Oversized)`](EncodeError) if the source exceeds
/// [`MAX_SOURCE_BYTES`].
pub fn encode_request(request_id: u32, request: &Request) -> Result<Vec<u8>, EncodeError> {
    let (language, source) = match request {
        Request::Parse { language, source } => (*language, source.as_slice()),
    };
    let mut payload = Vec::with_capacity(1 + source.len());
    payload.push(language_to_wire(language));
    payload.extend_from_slice(source);
    encode_frame(request_id, Request::KIND_PARSE, &payload)
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

    // The kind tag discriminates the message type. The only request kind is
    // Parse; any other tag (including response tags) is rejected.
    let message = match kind {
        Request::KIND_PARSE => {
            // Parse payload: [lang_u8][source bytes…].
            let (lang_byte, source) = payload.split_first().ok_or(DecodeError::InvalidPayload(
                "parse request payload missing language tag",
            ))?;
            let language = wire_to_language(*lang_byte).ok_or(DecodeError::InvalidPayload(
                "unknown language tag in parse request",
            ))?;
            Request::Parse {
                language,
                source: source.to_vec(),
            }
        }
        other => return Err(DecodeError::InvalidKind { tag: other }),
    };

    Ok(Some(DecodedFrame {
        request_id,
        message,
        consumed,
    }))
}

/// Encode `response` with `request_id` into a fresh `Vec<u8>`. A `Response`
/// payload is at most 4 bytes, so this is always `Ok` in practice; the
/// `Result` keeps the encoder symmetric and panic-free with the request side.
pub fn encode_response(request_id: u32, response: &Response) -> Result<Vec<u8>, EncodeError> {
    let (kind, payload): (u8, Vec<u8>) = match response {
        Response::Parsed { error_count } => {
            (Response::KIND_PARSED, error_count.to_le_bytes().to_vec())
        }
        Response::ParseFailed => (Response::KIND_PARSE_FAILED, Vec::new()),
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

/// Map a [`SourceLanguage`] to its single-byte wire tag.
fn language_to_wire(language: SourceLanguage) -> u8 {
    match language {
        SourceLanguage::Python => 0x00,
        SourceLanguage::JavaScript => 0x01,
        SourceLanguage::TypeScript => 0x02,
        SourceLanguage::Bash => 0x03,
    }
}

/// Map a single-byte wire tag back to a [`SourceLanguage`], or `None` if the
/// tag does not name a foundation language.
fn wire_to_language(tag: u8) -> Option<SourceLanguage> {
    match tag {
        0x00 => Some(SourceLanguage::Python),
        0x01 => Some(SourceLanguage::JavaScript),
        0x02 => Some(SourceLanguage::TypeScript),
        0x03 => Some(SourceLanguage::Bash),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A known-good `Request::Parse` used across the framing tests.
    fn sample_request() -> Request {
        Request::Parse {
            language: SourceLanguage::Python,
            source: b"import os; os.remove('x')".to_vec(),
        }
    }

    #[test]
    fn encode_request_emits_the_specified_wire_format() {
        // Independent source of truth: the expected bytes are hand-derived
        // from the wire-format spec in this module's docs, NOT by running
        // `encode_request`. Magic `AELW`, version 1 (LE), request_id 0x0A,
        // kind 0x01 (Parse), payload = [lang 0x00 = Python] + source bytes.
        let source = b"import os; os.remove('x')";
        let request_id: u32 = 0x0A;
        let mut expected = Vec::new();
        expected.extend_from_slice(b"AELW");
        expected.extend_from_slice(&1u16.to_le_bytes());
        expected.extend_from_slice(&request_id.to_le_bytes());
        expected.push(Request::KIND_PARSE);
        let payload_len = u32::try_from(1 + source.len()).expect("source fits in u32");
        expected.extend_from_slice(&payload_len.to_le_bytes());
        expected.push(0x00); // Python
        expected.extend_from_slice(source);

        let got = encode_request(request_id, &sample_request()).expect("small source encodes");
        assert_eq!(got, expected, "encoded frame must match the wire spec");
    }

    #[test]
    fn decode_request_round_trips_an_encoded_request() {
        let request_id = 0x0A;
        let encoded = encode_request(request_id, &sample_request()).expect("small source encodes");
        let decoded = decode_request(&encoded)
            .expect("a complete well-formed frame must decode")
            .expect("a complete frame must not be reported incomplete");
        assert_eq!(decoded.consumed, encoded.len());
        assert_eq!(decoded.request_id, request_id);
        assert_eq!(decoded.message, sample_request());
    }

    /// Build a raw frame with explicit header fields, for error-path tests
    /// that need to violate one field while keeping the rest well-formed.
    fn raw_frame(
        magic: &[u8; 4],
        version: u16,
        request_id: u32,
        kind: u8,
        payload: &[u8],
    ) -> Vec<u8> {
        let len = u32::try_from(payload.len()).expect("test payload fits in u32");
        raw_frame_with_declared_len(magic, version, request_id, kind, len, payload)
    }

    /// Build a raw frame whose header declares `declared_len` payload bytes,
    /// independent of how many `payload` bytes are actually appended. Used to
    /// craft oversized (declared > MAX, short body) and truncated (declared >
    /// body) frames.
    fn raw_frame_with_declared_len(
        magic: &[u8; 4],
        version: u16,
        request_id: u32,
        kind: u8,
        declared_len: u32,
        payload: &[u8],
    ) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        out.extend_from_slice(magic);
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&request_id.to_le_bytes());
        out.push(kind);
        out.extend_from_slice(&declared_len.to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    /// A well-formed Parse payload (Python + a small source body).
    fn parse_payload() -> Vec<u8> {
        let mut p = vec![0x00]; // Python
        p.extend_from_slice(b"print(1)");
        p
    }

    #[test]
    fn decode_request_rejects_a_frame_with_wrong_magic() {
        let buf = raw_frame(
            b"XXXX",
            PROTOCOL_VERSION,
            1,
            Request::KIND_PARSE,
            &parse_payload(),
        );
        assert_eq!(
            decode_request(&buf),
            Err(DecodeError::BadMagic { got: *b"XXXX" }),
            "a frame whose magic is not AELW must be rejected as noise/corruption"
        );
    }

    #[test]
    fn decode_request_rejects_a_frame_with_unsupported_version() {
        // A future/unknown version the decoder does not speak. The frame is
        // otherwise well-formed (correct magic, kind, payload).
        let unsupported: u16 = 999;
        let buf = raw_frame(
            &MAGIC,
            unsupported,
            1,
            Request::KIND_PARSE,
            &parse_payload(),
        );
        assert_eq!(
            decode_request(&buf),
            Err(DecodeError::UnsupportedVersion {
                version: unsupported
            }),
            "a frame whose version the decoder does not support must be rejected before its payload is read"
        );
    }

    #[test]
    fn decode_request_rejects_a_frame_declaring_an_oversized_payload() {
        // The header claims a payload larger than MAX_FRAME_PAYLOAD, but only a
        // few body bytes are present. The decoder must reject it from the
        // header alone — without allocating or waiting for the body.
        let declared: u32 = u32::try_from(MAX_FRAME_PAYLOAD + 1).unwrap();
        let buf = raw_frame_with_declared_len(
            &MAGIC,
            PROTOCOL_VERSION,
            1,
            Request::KIND_PARSE,
            declared,
            &parse_payload(),
        );
        assert_eq!(
            decode_request(&buf),
            Err(DecodeError::Oversized {
                declared,
                max: u32::try_from(MAX_FRAME_PAYLOAD).unwrap(),
            }),
            "a frame declaring more than MAX_FRAME_PAYLOAD must be rejected before its body is read"
        );
    }

    #[test]
    fn decode_request_rejects_a_response_kind_tag_sent_as_a_request() {
        // A response kind tag (0x81 = Parsed) must not be accepted as a
        // request, even with an otherwise-valid Parse payload. This pins half
        // of the disjoint-kind-tag contract and the "no hidden request kind"
        // property: the only request kind is Parse.
        let buf = raw_frame(
            &MAGIC,
            PROTOCOL_VERSION,
            1,
            Response::KIND_PARSED,
            &parse_payload(),
        );
        assert_eq!(
            decode_request(&buf),
            Err(DecodeError::InvalidKind {
                tag: Response::KIND_PARSED
            }),
            "a response kind tag must be rejected by the request decoder"
        );
    }

    #[test]
    fn encode_response_parsed_emits_the_specified_wire_format() {
        // Hand-derived: magic AELW, version 1, request_id 0x0B, kind 0x81
        // (Parsed), payload = error_count u32 LE = 2.
        let request_id: u32 = 0x0B;
        let mut expected = Vec::new();
        expected.extend_from_slice(&MAGIC);
        expected.extend_from_slice(&1u16.to_le_bytes());
        expected.extend_from_slice(&request_id.to_le_bytes());
        expected.push(Response::KIND_PARSED);
        expected.extend_from_slice(&4u32.to_le_bytes()); // payload_len
        expected.extend_from_slice(&2u32.to_le_bytes()); // error_count

        let got = encode_response(request_id, &Response::Parsed { error_count: 2 })
            .expect("Parsed response encodes");
        assert_eq!(
            got, expected,
            "encoded Parsed response must match the wire spec"
        );
    }

    #[test]
    fn decode_response_round_trips_parsed_and_parse_failed() {
        let request_id = 0x0B;

        let encoded = encode_response(request_id, &Response::Parsed { error_count: 2 })
            .expect("Parsed response encodes");
        let decoded = decode_response(&encoded)
            .expect("a complete well-formed response must decode")
            .expect("a complete response must not be reported incomplete");
        assert_eq!(decoded.consumed, encoded.len());
        assert_eq!(decoded.request_id, request_id);
        assert_eq!(decoded.message, Response::Parsed { error_count: 2 });

        let encoded = encode_response(request_id, &Response::ParseFailed)
            .expect("ParseFailed response encodes");
        let decoded = decode_response(&encoded)
            .expect("a complete well-formed response must decode")
            .expect("a complete response must not be reported incomplete");
        assert_eq!(decoded.consumed, encoded.len());
        assert_eq!(decoded.request_id, request_id);
        assert_eq!(decoded.message, Response::ParseFailed);
    }

    #[test]
    fn decode_response_rejects_a_request_kind_tag_sent_as_a_response() {
        // Symmetric to the request-side check: a request kind tag (0x01 =
        // Parse) must not be accepted as a response.
        let buf = raw_frame(
            &MAGIC,
            PROTOCOL_VERSION,
            1,
            Request::KIND_PARSE,
            &4u32.to_le_bytes(),
        );
        assert_eq!(
            decode_response(&buf),
            Err(DecodeError::InvalidKind {
                tag: Request::KIND_PARSE
            }),
            "a request kind tag must be rejected by the response decoder"
        );
    }

    #[test]
    fn decode_request_reports_a_short_header_as_incomplete_not_malformed() {
        // Fewer bytes than a full header: the caller may still be waiting for
        // the rest of the frame, so this is `Ok(None)`, not an error.
        let short = b"AELW\x01\x00\x00"; // 7 bytes < HEADER_LEN
        assert_eq!(
            decode_request(short),
            Ok(None),
            "a buffer shorter than the header must be reported incomplete, not malformed"
        );
    }

    #[test]
    fn decode_request_reports_a_truncated_payload_as_incomplete_not_malformed() {
        // The header declares 100 payload bytes but only 10 are present. The
        // frame is incomplete, not corrupted — the caller can wait for more.
        let buf = raw_frame_with_declared_len(
            &MAGIC,
            PROTOCOL_VERSION,
            1,
            Request::KIND_PARSE,
            100,
            &parse_payload(), // only ~9 bytes, not 100
        );
        assert_eq!(
            decode_request(&buf),
            Ok(None),
            "a frame whose declared payload has not fully arrived must be reported incomplete"
        );
    }

    #[test]
    fn decode_request_rejects_a_parse_payload_with_an_unknown_language_tag() {
        // Language tag 0x42 names no foundation language.
        let mut payload = vec![0x42];
        payload.extend_from_slice(b"x");
        let buf = raw_frame(&MAGIC, PROTOCOL_VERSION, 1, Request::KIND_PARSE, &payload);
        assert_eq!(
            decode_request(&buf),
            Err(DecodeError::InvalidPayload(
                "unknown language tag in parse request"
            )),
            "a parse request with an unknown language tag must be rejected"
        );
    }

    #[test]
    fn decode_response_rejects_a_parsed_payload_of_the_wrong_length() {
        // Parsed requires exactly 4 bytes (error_count); 2 bytes is malformed.
        let buf = raw_frame(
            &MAGIC,
            PROTOCOL_VERSION,
            1,
            Response::KIND_PARSED,
            &[0x01, 0x02],
        );
        assert_eq!(
            decode_response(&buf),
            Err(DecodeError::InvalidPayload(
                "Parsed response payload must be 4 bytes (error_count)"
            )),
            "a Parsed response with the wrong payload length must be rejected"
        );
    }

    #[test]
    fn decode_request_accepts_only_the_parse_kind_tag() {
        // The "no path read / no subprocess" property, pinned at the wire
        // level: the only accepted request kind is Parse (0x01). Every other
        // kind byte — including any a future "read path" or "run subprocess"
        // request would use — is rejected as InvalidKind. There is no way to
        // ask the worker for either through this protocol.
        let payload = parse_payload();
        for tag in 0u8..=255 {
            let buf = raw_frame(&MAGIC, PROTOCOL_VERSION, 1, tag, &payload);
            let outcome = decode_request(&buf);
            if tag == Request::KIND_PARSE {
                assert!(
                    outcome.is_ok(),
                    "Parse kind tag {tag:#04x} must be accepted"
                );
            } else {
                assert_eq!(
                    outcome,
                    Err(DecodeError::InvalidKind { tag }),
                    "non-Parse kind tag {tag:#04x} must be rejected — the protocol encodes no path-read or subprocess request"
                );
            }
        }
    }

    #[test]
    fn decode_response_accepts_only_the_two_response_kind_tags() {
        let parsed_payload = 4u32.to_le_bytes();
        for tag in 0u8..=255 {
            let buf = raw_frame(&MAGIC, PROTOCOL_VERSION, 1, tag, &parsed_payload);
            let outcome = decode_response(&buf);
            if tag == Response::KIND_PARSED {
                assert!(
                    outcome.is_ok(),
                    "Parsed kind tag {tag:#04x} must be accepted"
                );
            } else if tag == Response::KIND_PARSE_FAILED {
                // ParseFailed declares an empty payload; the 4-byte body here
                // is a wrong length, so it is rejected as InvalidPayload, not
                // accepted — which is the correct outcome for this buffer.
                assert!(
                    matches!(outcome, Err(DecodeError::InvalidPayload(_))),
                    "ParseFailed with a non-empty payload must be rejected"
                );
            } else {
                assert_eq!(
                    outcome,
                    Err(DecodeError::InvalidKind { tag }),
                    "non-response kind tag {tag:#04x} must be rejected by the response decoder"
                );
            }
        }
    }

    // ── L2: fallible encode (no panic on oversized input) ───────────────────

    #[test]
    fn encode_request_rejects_an_oversized_source_as_oversized() {
        // A source larger than MAX_SOURCE_BYTES must not panic the encoder
        // (no .expect() in production); it returns Err(EncodeError::Oversized).
        let oversized = Request::Parse {
            language: SourceLanguage::Python,
            source: vec![b'x'; MAX_SOURCE_BYTES + 1],
        };
        assert_eq!(
            encode_request(1, &oversized),
            Err(EncodeError::Oversized {
                actual: MAX_SOURCE_BYTES + 2, // lang tag + source
                max: MAX_FRAME_PAYLOAD,
            }),
            "an oversized source must encode to an Oversized error, not panic"
        );
    }

    #[test]
    fn encode_request_accepts_a_small_source() {
        let encoded = encode_request(1, &sample_request());
        assert!(encoded.is_ok(), "a small source must encode: {encoded:?}");
        let encoded = encoded.unwrap();
        assert!(
            decode_request(&encoded).unwrap().is_some(),
            "an encoded small source must round-trip"
        );
    }

    // ── L3: the 1 MiB source ceiling is legal (budget the lang tag) ─────────

    #[test]
    fn encode_request_accepts_a_source_at_the_one_mebibyte_ceiling() {
        // ADR-022 §7 hard per-file source ceiling is 1 MiB. A 1 MiB source is
        // legal and must round-trip — the frame payload budgets the 1-byte
        // language tag on top of MAX_SOURCE_BYTES.
        let at_ceiling = Request::Parse {
            language: SourceLanguage::Python,
            source: vec![b'x'; MAX_SOURCE_BYTES],
        };
        let encoded = encode_request(1, &at_ceiling).expect("1 MiB source must encode");
        let decoded = decode_request(&encoded)
            .expect("1 MiB frame must decode")
            .expect("1 MiB frame must be complete");
        assert_eq!(decoded.request_id, 1);
        match decoded.message {
            Request::Parse { language, source } => {
                assert_eq!(language, SourceLanguage::Python);
                assert_eq!(source.len(), MAX_SOURCE_BYTES);
            }
        }
    }

    #[test]
    fn encode_request_rejects_a_source_one_byte_above_the_ceiling() {
        let above = Request::Parse {
            language: SourceLanguage::Python,
            source: vec![b'x'; MAX_SOURCE_BYTES + 1],
        };
        assert!(
            matches!(
                encode_request(1, &above),
                Err(EncodeError::Oversized { .. })
            ),
            "a source one byte above the ceiling must be rejected as Oversized"
        );
    }
}
