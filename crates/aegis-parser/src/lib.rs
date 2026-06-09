#![deny(missing_docs)]

//! Shell command parsing for Aegis.
//!
//! This crate owns the tokenizer (quote/escape-aware splitting, heredoc and
//! inline-script extraction, pipeline segmentation, nested-shell unwrapping) and
//! the token-level `PrefixPattern` matcher. It produces the canonical
//! [`ParsedCommand`] consumed by the scanner. It depends only on `aegis-types`.

mod embedded_scripts;
mod nested_shells;
mod prefix_match;
mod segmentation;
mod tokenizer;

pub use aegis_types::{InlineScript, ParsedCommand};
pub use embedded_scripts::{
    HeredocBody, extract_eval_payloads, extract_heredoc_bodies, extract_inline_scripts,
    extract_process_substitution_bodies,
};
pub use nested_shells::extract_nested_commands;
pub use prefix_match::matches_prefix;
pub use segmentation::{logical_segments, top_level_pipelines};
pub use tokenizer::{extract_prefix, split_tokens};

/// One top-level segment within a pipeline chain.
///
/// `raw` preserves the original shell spelling for diagnostics, while
/// `normalized` joins shell tokens with single spaces so downstream matching can
/// reason about neighboring pipeline stages without quote noise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineSegment {
    /// Original shell spelling of this segment.
    pub raw: String,
    /// Shell tokens joined by single spaces (no quoting noise).
    pub normalized: String,
}

/// A top-level shell pipeline chain such as `cmd1 | cmd2 | cmd3`.
///
/// Chains are delimited only by top-level control operators other than the
/// single pipe (`;`, `&&`, `||`, newlines). This preserves adjacency between
/// neighboring pipeline stages for semantic analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineChain {
    /// Original shell spelling of the full chain.
    pub raw: String,
    /// Individual pipeline stages within the chain.
    pub segments: Vec<PipelineSegment>,
}

/// A stateless parser that converts raw shell command strings into [`ParsedCommand`].
pub struct Parser;

impl Parser {
    /// Parse `cmd` into a [`ParsedCommand`].
    ///
    /// Tokenizes `cmd` (respecting quoting and escaping), then extracts the
    /// program name and argument list from the first logical command. The full
    /// token sequence is joined into `normalized` — the canonical match target
    /// used by the scanner. The raw string is preserved only for audit logging.
    pub fn parse(cmd: &str) -> ParsedCommand {
        let tokens = split_tokens(cmd);

        // Tokens of the first sub-command only (before any shell separator).
        let first_cmd: Vec<&String> = tokens
            .iter()
            .take_while(|t| !matches!(t.as_str(), ";" | "&&" | "||" | "|"))
            .collect();

        let program = first_cmd.first().map(|s| s.to_string());
        let argv: Vec<String> = first_cmd.iter().skip(1).map(|s| s.to_string()).collect();

        // De-quoted, space-joined form of the full token sequence.
        let normalized = tokens.join(" ");

        let inline_scripts = extract_inline_scripts(cmd);

        ParsedCommand {
            program,
            argv,
            normalized,
            inline_scripts,
            raw: cmd.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod parsing_tests;
    mod tokenizer_tests;
}
