#![no_main]

use aegis::interceptor::parser::Parser;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Phase 1 fuzzes the public string-facing parser API, not a raw-byte contract.
    let input = String::from_utf8_lossy(data);
    let _ = Parser::parse(&input);
});
