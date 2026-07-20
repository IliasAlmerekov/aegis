#![no_main]

use libfuzzer_sys::fuzz_target;

// Fuzzes the language-worker protocol decoders (ADR-022 §2, L1 Iteration 3).
// The parent decodes untrusted worker output and the worker decodes untrusted
// parent input; both must be panic-free on arbitrary bytes, returning Ok(None)
// for an incomplete frame or Err for a malformed one. A panic here is a
// decoder-robustness bug that could crash the worker or the parent.

fuzz_target!(|data: &[u8]| {
    let _ = aegis_language::protocol::decode_request(data);
    let _ = aegis_language::protocol::decode_response(data);
});