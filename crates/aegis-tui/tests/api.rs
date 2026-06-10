//! Red tests for the `aegis-tui` public API.
//!
//! These tests fail until the contents of `src/ui/confirm/` are extracted into
//! this crate and the items below are re-exported from `aegis_tui`.

/// `show_confirmation`, `show_policy_block`, and `PromptDecision` must be
/// exported from this crate — they are the primary interactive-prompt surface.
#[test]
fn test_show_confirmation_and_prompt_decision_accessible() {
    use aegis_tui::{PromptDecision, show_confirmation, show_policy_block};
    // Verify the types are reachable without actually invoking I/O.
    let _: Option<fn(_, _, _) -> bool> = Some(show_confirmation);
    let _: Option<fn(_, _)> = Some(show_policy_block);
    let _: PromptDecision = PromptDecision::Approve;
}

/// `show_block_via_tty` must be exported for callers that route output through
/// /dev/tty rather than stderr.
#[test]
fn test_show_block_via_tty_accessible() {
    use aegis_tui::show_block_via_tty;
    let _f: fn(_, _) = show_block_via_tty;
}

/// `show_confirmation_via_tty_with_decision` must be exported so the main
/// binary can choose the /dev/tty path at runtime.
#[test]
fn test_show_confirmation_via_tty_with_decision_accessible() {
    use aegis_tui::show_confirmation_via_tty_with_decision;
    let _f: fn(_, _, _) -> aegis_tui::PromptDecision = show_confirmation_via_tty_with_decision;
}
