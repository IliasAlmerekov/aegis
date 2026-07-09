//! The result of assessing a shell command through the scanner pipeline.
//!
//! These are the *data* types produced by the scanner. The scanning logic that
//! builds them lives in the scanner layer; only the shapes live here.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::command::ParsedCommand;
use crate::pattern::{Pattern, PatternSource};
use crate::risk::RiskLevel;

/// A concrete byte range inside the original command for confirmation UI highlighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightRange {
    /// Inclusive start byte offset.
    pub start: usize,
    /// Exclusive end byte offset.
    pub end: usize,
}

/// A single pattern match with the actual text fragment that triggered it.
#[derive(Debug, Clone)]
pub struct MatchResult {
    /// The pattern that matched.
    pub pattern: Arc<Pattern>,
    /// The substring of the scanned text that the pattern's regex matched.
    pub matched_text: String,
    /// The concrete span in the original command suitable for confirmation UI highlighting.
    pub highlight_range: Option<HighlightRange>,
}

/// What ultimately caused the final interception decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecisionSource {
    /// Matched one or more built-in patterns compiled into the binary.
    BuiltinPattern,
    /// Matched one or more user-defined patterns from aegis.toml.
    CustomPattern,
    /// No patterns matched; the command was assessed Safe by default.
    Fallback,
}

/// The result of assessing a shell command through the full scanner pipeline.
pub struct Assessment {
    /// The highest `RiskLevel` among all matched patterns (`Safe` when none matched).
    pub risk: RiskLevel,
    /// Whether the command shape is `Effect-opaque execution` — it hands control
    /// to another execution layer (a script file, interpreter stdin, or a
    /// pipe-to-shell sink) whose eventual filesystem/database/network effect
    /// is not visible in argv. Orthogonal to `risk`: an effect-opaque command
    /// does not raise `RiskLevel` by itself; it only requests a recovery
    /// backstop downstream (ADR-016).
    pub effect_opaque: bool,
    /// Every pattern that matched the command (raw + inline scripts).
    pub matched: Vec<MatchResult>,
    /// Sorted, merged highlight spans for the original raw command.
    pub highlight_ranges: Vec<HighlightRange>,
    /// The parsed representation of the original command string.
    pub command: ParsedCommand,
}

impl Assessment {
    /// Determine what caused this assessment, ignoring allowlist (handled by the caller).
    pub fn decision_source(&self) -> DecisionSource {
        if self.matched.is_empty() {
            return DecisionSource::Fallback;
        }
        if self
            .matched
            .iter()
            .any(|m| m.pattern.source == PatternSource::Custom)
        {
            DecisionSource::CustomPattern
        } else {
            DecisionSource::BuiltinPattern
        }
    }
}
