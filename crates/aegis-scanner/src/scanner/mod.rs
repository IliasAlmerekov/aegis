//! Scanner: assess(cmd) -> RiskLevel

mod assessment;
mod highlighting;
mod keywords;
mod pipeline_semantics;
mod prefix_rule;
mod recursive;

use std::collections::HashMap;
use std::sync::Arc;

use aho_corasick::AhoCorasick;
use regex::{Regex, RegexBuilder};

use crate::error::ScannerError;
#[cfg(test)]
use crate::nested::MAX_NESTED_SCAN_DEPTH;
use crate::patterns::{Pattern, PatternSet, PatternSource};

pub use crate::patterns::{PatternToken, PrefixRule};
pub use assessment::{Assessment, DecisionSource, MatchResult};
pub use highlighting::HighlightRange;

/// First-pass scanner backed by an Aho-Corasick automaton.
///
/// Every pattern contributes one or more literal keywords that *must* appear
/// in any command the pattern can match. The automaton checks all keywords in
/// a single linear pass over the command string.
///
/// `quick_scan` is the hot path:
/// - Returns `false`  → no pattern can match → caller returns `Safe` immediately.
/// - Returns `true`   → at least one keyword matched → caller runs full regex scan.
///
/// False positives (extra `true` results) are acceptable; they only cost a regex
/// scan. False negatives (a `false` when a pattern would match) are forbidden.
const MAX_SCAN_COMMAND_LEN: usize = 64 * 1024;
const MAX_INLINE_SCRIPT_LEN: usize = 16 * 1024;

type CompiledPattern = (Arc<Pattern>, Regex);

/// Compiled pattern scanner with Aho-Corasick quick pass + regex full scan.
pub struct Scanner {
    ac: AhoCorasick,
    /// `true` when ≥ 1 pattern yielded no extractable keyword.
    /// In that case `quick_scan` always returns `true` so we never miss a match.
    has_uncovered: bool,
    /// `^`-anchored patterns indexed by first-token program name (lowercase).
    ///
    /// Only patterns whose every top-level alternation starts with `^` are stored
    /// here — they can only fire when the program token is at position 0, so it is
    /// safe to skip them for commands with a different leading program name.
    ///
    /// Built at construction time; looked up O(1) in `full_scan`.
    by_program: HashMap<String, Vec<CompiledPattern>>,
    /// Non-`^`-anchored patterns — always run regardless of the leading program.
    ///
    /// Non-anchored patterns (e.g. `\brm\s+`, `git\s+reset`) can match anywhere in
    /// a string, including inside quoted arguments to an unrelated command. They must
    /// be evaluated for every target regardless of the detected program name.
    universal: Vec<CompiledPattern>,
    /// Token-prefix rules indexed by first-token program name (lowercase).
    ///
    /// Stored as a `Vec` so lookup can be case-insensitive without allocating a
    /// lowercase `String`.  The number of entries is tiny (~8) so a linear scan is
    /// faster than a `HashMap` lookup that requires an allocation.
    prefix_by_program: Vec<(String, Vec<Arc<PrefixRule>>)>,
}

impl Scanner {
    /// Compile a single pattern's regex, enabling case-insensitive mode for
    /// built-in patterns only. Custom user patterns retain their authored case
    /// semantics.
    ///
    /// # Case-folding invariant with `quick_scan`
    ///
    /// `quick_scan` (the Aho-Corasick gate) is built with `ascii_case_insensitive`
    /// (see `try_new`), while built-in regexes here use Unicode `case_insensitive`.
    /// The gate contract requires `quick_scan` to fire for *every* command any
    /// regex can match — i.e. the gate must be a superset of regex matches, never
    /// a subset. This currently holds because every built-in keyword is an ASCII
    /// program/token, and for ASCII letters ASCII-CI and Unicode-CI fold
    /// identically, so the gate detects every case variant the regex would match.
    ///
    /// If a non-ASCII keyword is ever introduced, the gate (ASCII-CI) could miss a
    /// Unicode-cased variant the regex (Unicode-CI) matches — a false-negative
    /// `Safe`. Preserving the invariant then requires the gate's case folding to
    /// cover at least what the regex's does (e.g. make the Aho-Corasick build
    /// Unicode-case-insensitive too), or constrain such keywords to ASCII.
    fn compile_regex(pattern: &Pattern) -> Result<Regex, ScannerError> {
        let mut builder = RegexBuilder::new(pattern.pattern.as_ref());
        if pattern.source == PatternSource::Builtin {
            builder.case_insensitive(true);
        }
        builder.build().map_err(|e| ScannerError::InvalidPattern {
            id: pattern.id.to_string(),
            reason: format!("invalid regex: {e}"),
        })
    }

    /// Build a [`Scanner`] from a compiled [`PatternSet`].
    ///
    /// The Aho-Corasick automaton is constructed once here; subsequent calls to
    /// [`Scanner::quick_scan`] are allocation-free.
    ///
    /// Returns [`ScannerError::InvalidPattern`] if any pattern's regex fails to
    /// compile. Regex validity cannot be checked by [`PatternSet`] (which only
    /// inspects field presence), so it is enforced here — a user-supplied custom
    /// pattern with a malformed regex is a typed error, never a panic.
    pub fn try_new(patterns: PatternSet) -> Result<Self, ScannerError> {
        let effective_patterns = patterns.patterns();

        let mut keywords: Vec<String> = Vec::new();
        let mut has_uncovered = false;
        let mut by_program: HashMap<String, Vec<CompiledPattern>> = HashMap::new();
        let mut universal: Vec<CompiledPattern> = Vec::new();

        for p in effective_patterns {
            // Compile each regex once. Invalid regex (e.g. from a user pattern) is
            // a typed error, not a panic.
            let rx = Self::compile_regex(p)?;
            let entry = (Arc::clone(p), rx);

            // Build AC keyword set.
            let kws = keywords::extract_keywords(&p.pattern);
            if kws.is_empty() {
                has_uncovered = true;
            } else {
                keywords.extend(kws);
            }

            // Route to the right index bucket.
            // `^`-anchored patterns go into `by_program` under each alternative's program key.
            // Non-anchored patterns go into `universal` — they must run for every command.
            let prog_keys = keywords::derive_program_keys(&p.pattern);
            if prog_keys.is_empty() {
                universal.push(entry);
            } else {
                for key in &prog_keys {
                    by_program
                        .entry(key.clone())
                        .or_default()
                        .push(entry.clone());
                }
                // ^-anchored patterns are NOT pushed to universal: they only fire when
                // the program token is at position 0, so skipping them for other programs
                // is safe.
            }
        }

        // Add keywords from prefix rules so that quick_scan still fires when a
        // prefix-rule keyword appears later in the command (e.g. compound commands).
        for rule in patterns.prefix_rules() {
            match rule.pattern.first() {
                Some(PatternToken::Single(s)) => keywords.push(s.as_ref().to_string()),
                Some(PatternToken::Alts(alts)) => {
                    for s in alts {
                        keywords.push(s.as_ref().to_string());
                    }
                }
                _ => continue,
            }
        }

        keywords.sort_unstable();
        keywords.dedup();

        let ac = AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(&keywords)
            .expect("all extracted keywords are valid byte strings");

        // Index prefix rules by first token.
        let mut prefix_by_program: Vec<(String, Vec<Arc<PrefixRule>>)> = Vec::new();
        for rule in patterns.prefix_rules() {
            let keys: Vec<&str> = match rule.pattern.first() {
                Some(PatternToken::Single(first)) => vec![&**first],
                Some(PatternToken::Alts(alts)) => alts.iter().map(|s| &**s).collect(),
                Some(PatternToken::Any) | Some(PatternToken::AnyStar) | None => continue,
            };
            for key in keys {
                let lower = key.to_ascii_lowercase();
                if let Some((_, rules)) = prefix_by_program.iter_mut().find(|(k, _)| k == &lower) {
                    rules.push(Arc::clone(rule));
                } else {
                    prefix_by_program.push((lower, vec![Arc::clone(rule)]));
                }
            }
        }

        Ok(Scanner {
            ac,
            has_uncovered,
            by_program,
            universal,
            prefix_by_program,
        })
    }

    /// Fast first pass: returns `false` only when no pattern could possibly match.
    ///
    /// A `false` result guarantees the command is `Safe` — the caller must **not**
    /// run any further checks. A `true` result means one or more keywords were
    /// found; the caller should proceed with the full regex scan (T3.4).
    ///
    /// **Complexity:** O(n) in the command length, single allocation-free pass.
    pub fn quick_scan(&self, cmd: &str) -> bool {
        if self.has_uncovered || self.ac.is_match(cmd) {
            return true;
        }
        // Token-prefix rules may cover programs with no regex keyword hits.
        if let Some(prog) = cmd.split_whitespace().next()
            && self.prefix_lookup(prog).is_some()
        {
            return true;
        }
        false
    }

    /// Slow path: run compiled regexes against `cmd` and return all matching patterns.
    ///
    /// Always runs the `universal` set (non-`^`-anchored patterns). When `program`
    /// is `Some(p)` and `p` has indexed `^`-anchored patterns, those are additionally
    /// appended — O(universal + indexed) rather than O(all).
    ///
    /// Called only after `quick_scan` returns `true`.
    pub fn full_scan(&self, cmd: &str, program: Option<&str>) -> Vec<MatchResult> {
        let program_patterns: &[CompiledPattern] = program
            .and_then(|prog| self.by_program.get(&prog.to_ascii_lowercase()))
            .map(Vec::as_slice)
            .unwrap_or_default();

        self.universal
            .iter()
            .chain(program_patterns.iter())
            .filter_map(|(pattern, rx)| Self::match_one(cmd, pattern, rx))
            .collect()
    }

    fn match_one(cmd: &str, pattern: &Arc<Pattern>, rx: &Regex) -> Option<MatchResult> {
        rx.find(cmd).map(|m| MatchResult {
            pattern: Arc::clone(pattern),
            matched_text: m.as_str().to_string(),
            highlight_range: Some(HighlightRange {
                start: m.start(),
                end: m.end(),
            }),
        })
    }

    /// Token-prefix scan: match parsed tokens against indexed [`PrefixRule`]s.
    ///
    /// Returns one [`MatchResult`] per matching rule.  The first token is used as
    /// the lookup key; only rules whose first token matches the program are evaluated.
    pub fn prefix_scan(&self, tokens: &[&str]) -> Vec<MatchResult> {
        if tokens.is_empty() {
            return vec![];
        }
        let rules = self
            .prefix_lookup(tokens[0])
            .map_or(&[] as &[Arc<PrefixRule>], Vec::as_slice);
        rules
            .iter()
            .filter_map(|rule| {
                if rule.matches_tokens(tokens) {
                    Some(rule.to_match_result(tokens))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Case-insensitive lookup of prefix rules for `prog` without allocating.
    fn prefix_lookup(&self, prog: &str) -> Option<&Vec<Arc<PrefixRule>>> {
        self.prefix_by_program
            .iter()
            .find(|(key, _)| str_eq_ignore_ascii_case(key, prog))
            .map(|(_, rules)| rules)
    }

    /// Returns the number of regex patterns indexed for the given program name.
    #[cfg(test)]
    pub(crate) fn indexed_program_count(&self, program: &str) -> usize {
        self.by_program.get(program).map_or(0, Vec::len)
    }

    /// Returns the number of prefix rules indexed for the given program name.
    #[cfg(test)]
    pub(crate) fn prefix_indexed_program_count(&self, program: &str) -> usize {
        self.prefix_lookup(program).map_or(0, Vec::len)
    }
}

/// Case-insensitive ASCII comparison without allocation.
fn str_eq_ignore_ascii_case(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .all(|(a, b)| a.eq_ignore_ascii_case(&b))
}

// ── Keyword extraction ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
