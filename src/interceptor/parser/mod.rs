//! Parser: tokenizer, heredoc, inline scripts.
//!
//! The implementation lives in the `aegis-parser` crate. This module re-exports
//! its public API so existing `crate::interceptor::parser::*` call sites remain
//! stable while the workspace split (Phase 4) is in progress.

pub use aegis_parser::{
    HeredocBody, InlineScript, ParsedCommand, Parser, PipelineChain, PipelineSegment,
    extract_eval_payloads, extract_heredoc_bodies, extract_inline_scripts, extract_nested_commands,
    extract_prefix, extract_process_substitution_bodies, logical_segments, matches_prefix,
    split_tokens, top_level_pipelines,
};
