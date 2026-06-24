use std::iter::Peekable;
use std::str::Chars;

/// Split a shell command string into tokens, respecting quoting and escaping rules.
///
/// Rules:
/// - Whitespace separates tokens (unless quoted or escaped).
/// - Single-quoted strings are one token; no escape processing inside.
/// - Double-quoted strings are one token; backslash escaping applies inside.
/// - Backslash outside quotes escapes the next character (treated literally).
/// - `;`, `&&`, `||`, and `|` outside quotes are returned as separator tokens.
/// - Unquoted literal `$IFS` / `${IFS}` are treated as shell word-separators
///   (see [`ifs_marker_len`]), closing the C2 obfuscation bypass.
pub fn split_tokens(cmd: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = cmd.chars().peekable();
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            '\\' if !in_single_quote => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ';' if !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
                tokens.push(";".to_string());
            }
            '&' if !in_single_quote && !in_double_quote => {
                if chars.peek() == Some(&'&') {
                    chars.next();
                    if !current.is_empty() {
                        tokens.push(current.clone());
                        current.clear();
                    }
                    tokens.push("&&".to_string());
                } else {
                    current.push(ch);
                }
            }
            '|' if !in_single_quote && !in_double_quote => {
                if chars.peek() == Some(&'|') {
                    chars.next();
                    if !current.is_empty() {
                        tokens.push(current.clone());
                        current.clear();
                    }
                    tokens.push("||".to_string());
                } else {
                    if !current.is_empty() {
                        tokens.push(current.clone());
                        current.clear();
                    }
                    tokens.push("|".to_string());
                }
            }
            c if c.is_whitespace() && !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            '$' if !in_single_quote && !in_double_quote => {
                // `$` already consumed; `chars` is positioned just past it.
                if let Some(marker_len) = ifs_marker_len(&chars) {
                    if !current.is_empty() {
                        tokens.push(current.clone());
                        current.clear();
                    }
                    // Consume the rest of the marker (the leading `$` is gone).
                    for _ in 1..marker_len {
                        chars.next();
                    }
                } else {
                    current.push(ch);
                }
            }
            c => {
                current.push(c);
            }
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

/// Recognize a literal unquoted IFS expansion at the tokenizer's current cursor.
///
/// `chars` must be positioned **just past** the leading `$`. Returns the full
/// marker length in characters — `4` for `$IFS`, `6` for `${IFS}` — when the
/// upcoming characters form a literal IFS expansion, or `None` otherwise. The
/// iterator is never advanced; the caller consumes the remaining marker
/// characters only on a match.
///
/// The bare `$IFS` form matches only at an identifier boundary so that distinct
/// variables such as `$IFSHOME` are left intact. The braced `${IFS}` form is
/// self-delimited by its closing brace. Unknown variables stay opaque: this is a
/// narrow, deterministic normalization, not full shell expansion.
///
/// Out of scope (see TASKS.md "C-next"): parameter-expansion modifiers such as
/// `${IFS:-x}` / `${IFS:+x}` and runtime `IFS=` reassignment are not normalized.
/// This helper only recognizes the two literal default-IFS spellings.
fn ifs_marker_len(chars: &Peekable<Chars<'_>>) -> Option<usize> {
    let mut lookahead = chars.clone();
    match lookahead.next()? {
        '{' => {
            let braces_wrap_ifs = lookahead.next() == Some('I')
                && lookahead.next() == Some('F')
                && lookahead.next() == Some('S')
                && lookahead.next() == Some('}');
            braces_wrap_ifs.then_some(6)
        }
        'I' => {
            let spells_ifs = lookahead.next() == Some('F') && lookahead.next() == Some('S');
            if !spells_ifs {
                return None;
            }
            let extends_identifier = lookahead
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
            (!extends_identifier).then_some(4)
        }
        _ => None,
    }
}

/// Extract a command prefix suitable for an `[[allow]]` rule.
///
/// Rules:
/// - Keep the program name (first token).
/// - Keep flag tokens (starting with `-`) as meaningful modifiers.
/// - Keep subcommand tokens that come immediately after the program (e.g. `git push`).
/// - Stop at the first non-flag, non-subcommand token that looks like a file path or value.
/// - Stop at `--` (end-of-options marker).
/// - Tokens starting with `/`, `.`, `~`, or containing `.` / `/` are treated as paths/values and stripped.
pub fn extract_prefix(tokens: &[String]) -> Vec<String> {
    if tokens.is_empty() {
        return Vec::new();
    }

    let mut prefix = Vec::new();
    prefix.push(tokens[0].clone());

    let has_any_flag = tokens.iter().skip(1).any(|t| t.starts_with('-'));

    for (i, token) in tokens.iter().skip(1).enumerate() {
        if token == "--" {
            break;
        }
        if token.starts_with('-') {
            prefix.push(token.clone());
            continue;
        }
        // Stop at the first token that looks like a file path or value.
        if token.starts_with('/')
            || token.starts_with('.')
            || token.starts_with('~')
            || token.contains('/')
            || token.contains('.')
        {
            break;
        }
        // When there are no flags and more than three tokens total,
        // treat only the first non-program token as a subcommand.
        if !has_any_flag && tokens.len() > 3 && i > 0 {
            break;
        }
        prefix.push(token.clone());
    }

    prefix
}
