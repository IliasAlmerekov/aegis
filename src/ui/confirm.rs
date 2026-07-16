//! Crossterm TUI confirmation dialogs.
//!
//! All items are re-exported from the `aegis-tui` crate.

pub use aegis_tui::{
    PromptDecision, RecoveryPromptDecision, show_block_via_tty, show_confirmation,
    show_confirmation_decision, show_confirmation_via_tty_with_decision,
    show_confirmation_with_decision, show_confirmation_with_input, show_policy_block,
    show_policy_block_via_tty, show_recovery_override_decision, show_recovery_override_via_tty,
    show_recovery_override_with_input, tty_unavailable_decision,
};
