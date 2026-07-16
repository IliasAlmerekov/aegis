//! Crossterm confirmation TUI for Aegis.

mod block_screen;
mod confirm_screen;
mod recovery_screen;
mod shared;
mod stdout_renderer;
mod tty_renderer;

pub use recovery_screen::{
    RecoveryPromptDecision, show_recovery_override_decision, show_recovery_override_via_tty,
    show_recovery_override_with_input,
};
pub use stdout_renderer::{
    PromptDecision, show_confirmation, show_confirmation_decision, show_confirmation_with_decision,
    show_confirmation_with_input, show_policy_block,
};
pub use tty_renderer::{
    show_block_via_tty, show_confirmation_via_tty_with_decision, show_policy_block_via_tty,
    tty_unavailable_decision,
};

#[cfg(test)]
mod tests;
