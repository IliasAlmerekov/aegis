//! The canonical token-level representation of a parsed shell command.
//!
//! The tokenizer/parser logic lives in the parser layer; this module holds only
//! the data types it produces, so they can sit at the bottom of the dependency
//! DAG and be embedded in [`crate::Assessment`].

use std::fmt;

/// An inline script body extracted from an interpreter invocation.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineScript {
    /// The interpreter name (e.g., `python3`, `node`, `ruby`).
    pub interpreter: String,
    /// The script body passed via `-c` or `-e`.
    pub body: String,
}

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
