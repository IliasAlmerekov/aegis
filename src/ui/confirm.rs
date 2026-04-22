// crossterm TUI confirmation dialog

mod block_screen;
mod confirm_screen;
mod shared;
mod stdout_renderer;
mod tty_renderer;

pub use stdout_renderer::{show_confirmation, show_confirmation_with_input, show_policy_block};
pub use tty_renderer::{
    show_block_via_tty, show_confirmation_via_tty, show_policy_block_via_tty,
    tty_unavailable_decision,
};

#[cfg(test)]
mod tests;
