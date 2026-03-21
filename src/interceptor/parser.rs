// Parser: tokenizer, heredoc, inline scripts

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

#[cfg(test)]
mod tests {
    use super::*;

    // 1. Simple command — space-separated words
    #[test]
    fn simple_command() {
        assert_eq!(split_tokens("echo hello"), vec!["echo", "hello"]);
    }

    // 2. Multiple arguments
    #[test]
    fn multiple_args() {
        assert_eq!(
            split_tokens("ls -la /tmp"),
            vec!["ls", "-la", "/tmp"]
        );
    }

    // 3. Single-quoted string becomes one token
    #[test]
    fn single_quoted_token() {
        assert_eq!(
            split_tokens("echo 'hello world'"),
            vec!["echo", "hello world"]
        );
    }

    // 4. Dangerous path in single quotes is one token
    #[test]
    fn single_quoted_dangerous_path() {
        assert_eq!(split_tokens("'rm -rf /'"), vec!["rm -rf /"]);
    }

    // 5. Double-quoted string becomes one token
    #[test]
    fn double_quoted_token() {
        assert_eq!(
            split_tokens(r#"echo "hello world""#),
            vec!["echo", "hello world"]
        );
    }

    // 6. && is a separator token
    #[test]
    fn double_ampersand_separator() {
        assert_eq!(
            split_tokens("cmd1 && cmd2"),
            vec!["cmd1", "&&", "cmd2"]
        );
    }

    // 7. Semicolon is a separator token
    #[test]
    fn semicolon_separator() {
        assert_eq!(
            split_tokens("cmd1; cmd2"),
            vec!["cmd1", ";", "cmd2"]
        );
    }

    // 8. Semicolon without surrounding spaces
    #[test]
    fn semicolon_no_spaces() {
        assert_eq!(
            split_tokens("cmd1;cmd2"),
            vec!["cmd1", ";", "cmd2"]
        );
    }

    // 9. Pipe is a separator token
    #[test]
    fn pipe_separator() {
        assert_eq!(
            split_tokens("ls | grep foo"),
            vec!["ls", "|", "grep", "foo"]
        );
    }

    // 10. || is a separator token
    #[test]
    fn double_pipe_separator() {
        assert_eq!(
            split_tokens("cmd1 || cmd2"),
            vec!["cmd1", "||", "cmd2"]
        );
    }

    // 11. Backslash escapes a space (keeps words together)
    #[test]
    fn backslash_escaped_space() {
        assert_eq!(split_tokens(r"rm\ -rf\ /"), vec!["rm -rf /"]);
    }

    // 12. && inside double quotes is NOT a separator
    #[test]
    fn ampersand_inside_double_quotes() {
        assert_eq!(
            split_tokens(r#"bash -c "cmd1 && cmd2""#),
            vec!["bash", "-c", "cmd1 && cmd2"]
        );
    }

    // 13. Single quote inside double quotes is literal
    #[test]
    fn single_quote_inside_double_quotes() {
        assert_eq!(split_tokens(r#""it's fine""#), vec!["it's fine"]);
    }

    // 14. Double quote inside single quotes is literal
    #[test]
    fn double_quote_inside_single_quotes() {
        assert_eq!(split_tokens(r#"'say "hi"'"#), vec![r#"say "hi""#]);
    }

    // 15. Empty string produces no tokens
    #[test]
    fn empty_input() {
        assert_eq!(split_tokens(""), Vec::<String>::new());
    }
}
