//! Watch-mode NDJSON protocol and runner.

pub mod protocol;
pub mod runner;
mod sandbox;

pub use protocol::{
    InputFrame, MAX_FRAME_BYTES, OutputDecision, OutputFrame, ReadLineResult, emit_frame,
    read_bounded_line,
};
pub use runner::{run, run_disabled};
