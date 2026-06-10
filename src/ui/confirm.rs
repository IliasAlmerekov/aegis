//! Crossterm TUI confirmation dialogs.
//!
//! All items are re-exported from the `aegis-tui` crate.

pub use aegis_tui::{
    PromptDecision, show_block_via_tty, show_confirmation, show_confirmation_decision,
    show_confirmation_via_tty_with_decision, show_confirmation_with_decision,
    show_confirmation_with_input, show_policy_block, show_policy_block_via_tty,
    tty_unavailable_decision,
};
