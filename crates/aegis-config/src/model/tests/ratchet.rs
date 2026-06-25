use super::ratchet_helpers::{
    assert_has_warning_for, assert_no_warning_for, load_global_base, project_ratchet_warnings,
};
use super::*;

// The ratchet test bodies live in `ratchet/` fragments below and are textually
// inlined here via `include!` so they share this module's scope (notably the
// `use super::*` imports and the `ratchet_helpers` re-exports). The split keeps
// every file under the 800-line M5.1 file-size budget; no test body was modified.
include!("ratchet/c3_a.rs");
include!("ratchet/c3_b.rs");
include!("ratchet/bugs.rs");
