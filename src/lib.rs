pub mod audit;
pub mod config;
pub mod decision;
pub mod error;
pub mod explanation;
pub mod interceptor;
pub mod planning;
pub mod runtime;
/// Shared CI detection used by CLI entrypoints.
pub mod runtime_gate;
pub mod snapshot;
/// Global on/off toggle state helpers.
pub mod toggle;
pub mod ui;
pub mod watch;
