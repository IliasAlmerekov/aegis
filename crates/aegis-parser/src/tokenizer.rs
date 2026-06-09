/// Split a shell command string into tokens, respecting quoting and escaping rules.
///
/// Rules:
/// - Whitespace separates tokens (unless quoted or escaped).
/// - Single-quoted strings are one token; no escape processing inside.
/// - Double-quoted strings are one token; backslash escaping applies inside.
/// - Backslash outside quotes escapes the next character (treated literally).
/// - `;`, `&&`, `||`, and `|` outside quotes are returned as separator tokens.
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
