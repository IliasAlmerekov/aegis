use super::*;

// The Analyzed response wraps an `AdapterResult`, so the Analyzed tests build
// one from the operation vocabulary. These are the leaf types this crate owns
// (the root crate maps them into `aegis_types`); importing them here does not
// cross the workspace boundary.
use crate::operation::{
    AdapterResult, ByteSpan, DetectedOperation, NestedTarget, OperandCertainty, OperationKind,
    OperationModifiers,
};

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
fn raw_frame(magic: &[u8; 4], version: u16, request_id: u32, kind: u8, payload: &[u8]) -> Vec<u8> {
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

    let encoded =
        encode_response(request_id, &Response::ParseFailed).expect("ParseFailed response encodes");
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
            "unknown language tag in request"
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
fn decode_request_accepts_only_the_parse_and_analyze_kind_tags() {
    // The "no path read / no subprocess" property, pinned at the wire
    // level: the only accepted request kinds are Parse (0x01) and Analyze
    // (0x02). Every other kind byte — including any a future "read path" or
    // "run subprocess" request would use — is rejected as InvalidKind.
    // There is no way to ask the worker for either through this protocol.
    let payload = parse_payload();
    for tag in 0u8..=255 {
        let buf = raw_frame(&MAGIC, PROTOCOL_VERSION, 1, tag, &payload);
        let outcome = decode_request(&buf);
        if tag == Request::KIND_PARSE || tag == Request::KIND_ANALYZE {
            assert!(
                outcome.is_ok(),
                "request kind tag {tag:#04x} must be accepted"
            );
        } else {
            assert_eq!(
                outcome,
                Err(DecodeError::InvalidKind { tag }),
                "non-request kind tag {tag:#04x} must be rejected — the protocol encodes no path-read or subprocess request"
            );
        }
    }
}

#[test]
fn decode_response_accepts_only_the_known_response_kind_tags() {
    // Parsed carries a 4-byte error_count; Analyzed carries a packed
    // AdapterResult; ParseFailed and UnsupportedLanguage declare an empty
    // payload. The probe payload below is 4 bytes, which is valid for Parsed,
    // a wrong-but-decidable length for the empty-payload kinds (rejected as
    // InvalidPayload), and an invalid AdapterResult for Analyzed (rejected as
    // InvalidPayload by the codec).
    let parsed_payload = 4u32.to_le_bytes();
    for tag in 0u8..=255 {
        let buf = raw_frame(&MAGIC, PROTOCOL_VERSION, 1, tag, &parsed_payload);
        let outcome = decode_response(&buf);
        if tag == Response::KIND_PARSED {
            assert!(
                outcome.is_ok(),
                "Parsed kind tag {tag:#04x} must be accepted"
            );
        } else if tag == Response::KIND_PARSE_FAILED || tag == Response::KIND_UNSUPPORTED {
            // These kinds declare an empty payload; the 4-byte body here is a
            // wrong length, so they are rejected as InvalidPayload, not
            // accepted — which is the correct outcome for this buffer.
            assert!(
                matches!(outcome, Err(DecodeError::InvalidPayload(_))),
                "empty-payload kind {tag:#04x} with a non-empty payload must be rejected"
            );
        } else if tag == Response::KIND_ANALYZED {
            // A 4-byte body cannot be a valid AdapterResult (which needs at
            // least parse_errors u32 + op_count u32 = 8 bytes), so the codec
            // rejects it as InvalidPayload.
            assert!(
                matches!(outcome, Err(DecodeError::InvalidPayload(_))),
                "Analyzed with a malformed AdapterResult payload must be rejected"
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
        other => panic!("a Parse request must round-trip to Parse, got {other:?}"),
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

// ── Analyze / Analyzed / UnsupportedLanguage (Iteration 6 Slice A) ─────────

#[test]
fn decode_request_round_trips_an_analyze_request() {
    let request_id = 0x1F;
    let request = Request::Analyze {
        language: SourceLanguage::Python,
        source: b"os.remove('x')".to_vec(),
    };
    let encoded = encode_request(request_id, &request).expect("analyze encodes");
    let decoded = decode_request(&encoded)
        .expect("a complete analyze frame must decode")
        .expect("a complete analyze frame must not be incomplete");
    assert_eq!(decoded.consumed, encoded.len());
    assert_eq!(decoded.request_id, request_id);
    assert_eq!(decoded.message, request);
}

#[test]
fn encode_response_unsupported_language_emits_the_specified_wire_format() {
    // Independent source of truth: magic AELW, version 1, request_id 0x0C,
    // kind 0x84 (UnsupportedLanguage), empty payload (payload_len = 0).
    let request_id: u32 = 0x0C;
    let mut expected = Vec::new();
    expected.extend_from_slice(&MAGIC);
    expected.extend_from_slice(&1u16.to_le_bytes());
    expected.extend_from_slice(&request_id.to_le_bytes());
    expected.push(Response::KIND_UNSUPPORTED);
    expected.extend_from_slice(&0u32.to_le_bytes()); // payload_len
    // No payload bytes follow.

    let got = encode_response(request_id, &Response::UnsupportedLanguage)
        .expect("UnsupportedLanguage encodes");
    assert_eq!(
        got, expected,
        "encoded UnsupportedLanguage response must match the wire spec"
    );
}

#[test]
fn decode_response_round_trips_unsupported_language() {
    let request_id = 0x0C;
    let encoded = encode_response(request_id, &Response::UnsupportedLanguage)
        .expect("UnsupportedLanguage encodes");
    let decoded = decode_response(&encoded)
        .expect("a complete UnsupportedLanguage frame must decode")
        .expect("a complete UnsupportedLanguage frame must not be incomplete");
    assert_eq!(decoded.consumed, encoded.len());
    assert_eq!(decoded.request_id, request_id);
    assert_eq!(decoded.message, Response::UnsupportedLanguage);
}

/// A minimal `AdapterResult` with one operation carrying a payload, used to
/// exercise the Analyzed frame round-trip. Built directly from the operation
/// vocabulary — an independent construction, not produced by the codec.
fn sample_adapter_result() -> AdapterResult {
    AdapterResult {
        parse_errors: 0,
        operations: vec![DetectedOperation {
            kind: OperationKind::FilesystemDelete,
            modifiers: OperationModifiers {
                recursive: true,
                forced: false,
                destructive_mode: false,
            },
            certainty: OperandCertainty::Known,
            span: ByteSpan {
                line: 3,
                column: 5,
                byte_start: 12,
                byte_end: 28,
            },
            payload: Some(NestedTarget {
                language: SourceLanguage::Python,
                source: "rm".to_string(),
                span: ByteSpan {
                    line: 3,
                    column: 10,
                    byte_start: 18,
                    byte_end: 20,
                },
            }),
        }],
    }
}

#[test]
fn decode_response_round_trips_an_analyzed_response() {
    let request_id = 0x0D;
    let response = Response::Analyzed {
        result: sample_adapter_result(),
    };
    let encoded = encode_response(request_id, &response).expect("Analyzed encodes");
    let decoded = decode_response(&encoded)
        .expect("a complete Analyzed frame must decode")
        .expect("a complete Analyzed frame must not be incomplete");
    assert_eq!(decoded.consumed, encoded.len());
    assert_eq!(decoded.request_id, request_id);
    assert_eq!(decoded.message, response);
}

#[test]
fn decode_response_rejects_a_truncated_analyzed_payload() {
    // A body too short to hold even the AdapterResult header (parse_errors +
    // op_count = 8 bytes) must be rejected by the codec, surfaced as
    // InvalidPayload with the codec's own reason.
    let buf = raw_frame(
        &MAGIC,
        PROTOCOL_VERSION,
        1,
        Response::KIND_ANALYZED,
        &4u32.to_le_bytes(),
    );
    assert!(
        matches!(decode_response(&buf), Err(DecodeError::InvalidPayload(_))),
        "Analyzed with a truncated AdapterResult payload must be rejected as InvalidPayload"
    );
}
