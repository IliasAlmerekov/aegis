//! Heredoc / here-string / literal-producer stdin routing (ADR-022 §6, L1
//! Iteration 4 slice 3).
//!
//! Interpreter stdin is only analyzed when the source is statically
//! recoverable: a quoted heredoc, a literal here-string, or a narrowly proven
//! literal-only producer such as `printf '%s'`. Everything else (expanding
//! heredocs/here-strings with detected substitutions, or any other dynamic
//! pipeline) degrades honestly rather than being evaluated or guessed at.

use aegis_parser::extract_heredoc_bodies;
use aegis_types::DegradationReason;

/// A statically-recoverable (or provably-not-recoverable) interpreter stdin
/// source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StdinRoute {
    /// The exact source text stdin will receive.
    Literal(String),
    /// Stdin is known to exist but its content could not be statically
    /// recovered.
    Dynamic(DegradationReason),
}

/// Detect a heredoc (`<<WORD` / `<<'WORD'`) attached to the first line of
/// `command` and classify its recoverability.
///
/// A quoted heredoc (nowdoc) is exact source. An unquoted (expanding) heredoc
/// is only treated as literal when its body contains no `$`/backtick
/// expansion syntax; otherwise it degrades rather than being evaluated
/// (ADR-022 §6). Only the first heredoc in `command` is considered — multiple
/// heredocs per command are out of this slice's scope.
#[must_use]
pub fn heredoc_stdin(command: &str) -> Option<StdinRoute> {
    let first_line = command.lines().next()?;
    if !first_line.contains("<<") {
        return None;
    }
    let heredoc = extract_heredoc_bodies(command).into_iter().next()?;
    Some(classify(heredoc.body, heredoc.is_nowdoc))
}

/// Detect a here-string (`<<< 'literal'` / `<<<"literal"`) among the tokens
/// following an interpreter invocation.
#[must_use]
pub fn here_string_stdin(rest_tokens: &[&str]) -> Option<StdinRoute> {
    let pos = rest_tokens.iter().position(|tok| *tok == "<<<")?;
    let body = rest_tokens.get(pos + 1)?;
    // The tokenizer already strips the surrounding quotes, so single- vs.
    // double-quoted here-strings are indistinguishable here; classify by
    // expansion syntax alone, same as an unquoted heredoc.
    Some(classify((*body).to_owned(), false))
}

/// Classify an already-extracted heredoc/here-string body (shared with
/// [`super::router`]'s heredoc-to-file reuse, which extracts the body itself
/// to also recover the closing delimiter).
pub(crate) fn classify(body: String, is_nowdoc: bool) -> StdinRoute {
    if is_nowdoc || !contains_expansion(&body) {
        StdinRoute::Literal(body)
    } else {
        StdinRoute::Dynamic(DegradationReason::DynamicSource)
    }
}

fn contains_expansion(body: &str) -> bool {
    body.contains('$') || body.contains('`')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_heredoc_is_literal() {
        let command = "bash <<'EOF'\nrm -rf /tmp/x\nEOF";
        assert_eq!(
            heredoc_stdin(command),
            Some(StdinRoute::Literal("rm -rf /tmp/x".to_owned()))
        );
    }

    #[test]
    fn expanding_heredoc_without_expansion_syntax_is_literal() {
        let command = "bash <<EOF\necho hello\nEOF";
        assert_eq!(
            heredoc_stdin(command),
            Some(StdinRoute::Literal("echo hello".to_owned()))
        );
    }

    #[test]
    fn expanding_heredoc_with_variable_expansion_is_dynamic() {
        let command = "bash <<EOF\necho $HOME\nEOF";
        assert_eq!(
            heredoc_stdin(command),
            Some(StdinRoute::Dynamic(DegradationReason::DynamicSource))
        );
    }

    #[test]
    fn no_heredoc_marker_yields_none() {
        assert_eq!(heredoc_stdin("bash script.sh"), None);
    }

    #[test]
    fn literal_here_string_is_recognized() {
        assert_eq!(
            here_string_stdin(&["<<<", "print(1)"]),
            Some(StdinRoute::Literal("print(1)".to_owned()))
        );
    }

    #[test]
    fn here_string_with_expansion_is_dynamic() {
        assert_eq!(
            here_string_stdin(&["<<<", "print($HOME)"]),
            Some(StdinRoute::Dynamic(DegradationReason::DynamicSource))
        );
    }

    #[test]
    fn no_here_string_token_yields_none() {
        assert_eq!(here_string_stdin(&["script.py"]), None);
    }
}
