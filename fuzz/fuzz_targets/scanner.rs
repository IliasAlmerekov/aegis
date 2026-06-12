#![no_main]

use std::sync::LazyLock;

use aegis_scanner::{PatternSet, Scanner};
use libfuzzer_sys::fuzz_target;

static SCANNER: LazyLock<Scanner> = LazyLock::new(|| {
    PatternSet::load()
        .and_then(Scanner::try_new)
        .expect("patterns.toml must load in scanner fuzzing target")
});

fuzz_target!(|data: &[u8]| {
    let input = String::from_utf8_lossy(data);
    let _ = SCANNER.assess(input.as_ref());
});
