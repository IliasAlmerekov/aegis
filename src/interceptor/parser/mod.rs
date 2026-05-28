// Parser: tokenizer, heredoc, inline scripts

mod embedded_scripts;
mod nested_shells;
mod segmentation;
mod tokenizer;

use std::fmt;

pub use embedded_scripts::{
    HeredocBody, InlineScript, extract_eval_payloads, extract_heredoc_bodies,
    extract_inline_scripts, extract_process_substitution_bodies,
};
pub use nested_shells::extract_nested_commands;
pub use segmentation::{logical_segments, top_level_pipelines};
pub use tokenizer::{extract_prefix, split_tokens};

// ── T2.4: ParsedCommand struct and public API ─────────────────────────────────

/// The canonical token-level representation of a shell command.
///
/// The tokenizer runs first; all scanner stages consume this struct rather than
/// the raw string. The raw string is retained only for display and audit logging.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCommand {
    /// The first token of the first logical command (e.g. `rm`, `git`, `bash`).
    /// `None` only when the input string is empty or consists solely of separators.
    pub program: Option<String>,
    /// Argument tokens after `program` in the first logical command (separators stripped).
    pub argv: Vec<String>,
    /// De-quoted, space-joined form of the full token sequence. Used by the scanner
    /// as the primary match target — free of shell quoting and escape noise.
    pub normalized: String,
    /// Inline scripts extracted from interpreter invocations (python -c, node -e, etc.).
    pub inline_scripts: Vec<InlineScript>,
    /// The original, unmodified command string. Used only for display and audit logging.
    pub raw: String,
}

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

impl fmt::Display for ParsedCommand {
    /// Formats the command for audit log output.
    ///
    /// Shows `program [argv...]` if parsing succeeded, or falls back to
    /// the raw string if no program was found.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.program {
            Some(prog) if !self.argv.is_empty() => {
                write!(f, "{} {}", prog, self.argv.join(" "))
            }
            Some(prog) => write!(f, "{}", prog),
            None => write!(f, "{}", self.raw),
        }
    }
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
