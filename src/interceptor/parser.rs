// Parser: tokenizer, heredoc, inline scripts
#![allow(dead_code)]

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

/// Detect and unwrap nested shell commands from `bash -c '...'` / `sh -c '...'` invocations.
///
/// Returns the flat list of sub-commands found inside the `-c` argument,
/// split at shell separators (`&&`, `||`, `;`, `|`) and newlines.
///
/// Handles:
/// - `bash -c 'cmd'` and `sh -c 'cmd'`
/// - `bash -c "cmd1 && cmd2"` → `["cmd1", "cmd2"]`
/// - `bash -c $'escaped\nnewline'` (ANSI-C `$'...'` quoting)
/// - `env VAR=val bash -c '...'` (env prefix with variable assignments)
/// - Recursive nesting: `bash -c 'bash -c "inner cmd"'` → `["inner cmd"]`
///
/// Returns an empty `Vec` if `cmd` does not start with a recognized shell invocation.
pub fn extract_nested_commands(cmd: &str) -> Vec<String> {
    let tokens = split_tokens(cmd);
    try_unwrap_shell_tokens(&tokens).unwrap_or_default()
}

/// Try to extract inner commands from a shell `-c` invocation represented as a token slice.
///
/// Returns `None` when the token slice does not begin with a shell invocation.
fn try_unwrap_shell_tokens(tokens: &[String]) -> Option<Vec<String>> {
    let mut idx = 0;

    // Skip optional `env` prefix.
    if tokens.get(idx).map(String::as_str) == Some("env") {
        idx += 1;
    }

    // Skip VAR=val environment variable assignments (contain '=' and do not start with '-').
    while let Some(tok) = tokens.get(idx) {
        if tok.contains('=') && !tok.starts_with('-') {
            idx += 1;
        } else {
            break;
        }
    }

    // Must be a known shell executable.
    if !tokens
        .get(idx)
        .map(|s| matches!(s.as_str(), "bash" | "sh" | "dash" | "zsh" | "ksh" | "fish"))
        .unwrap_or(false)
    {
        return None;
    }
    idx += 1;

    // Find the -c flag in the remaining tokens.
    let rel_c = tokens[idx..].iter().position(|t| t == "-c")?;
    let inner_idx = idx + rel_c + 1;
    let inner_raw = tokens.get(inner_idx)?;

    // Handle $'...' ANSI-C quoting: split_tokens keeps the '$' prefix and stores the
    // single-quoted body with literal backslash sequences (e.g. "\\n").  Strip the '$' and
    // unescape so that `\n` becomes an actual newline, enabling newline-based separation below.
    let inner = if let Some(stripped) = inner_raw.strip_prefix('$') {
        unescape_ansi_c(stripped)
    } else {
        inner_raw.clone()
    };

    // Split the inner string on newlines; each line is an independent command sequence.
    let mut all_sub_cmds: Vec<Vec<String>> = Vec::new();
    for line in inner.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let line_tokens = split_tokens(line);
        all_sub_cmds.extend(split_by_separators(line_tokens));
    }

    // Recursively unwrap any nested shell invocations; otherwise stringify and keep.
    let mut result = Vec::new();
    for cmd_tokens in all_sub_cmds {
        if let Some(nested) = try_unwrap_shell_tokens(&cmd_tokens) {
            result.extend(nested);
        } else {
            result.push(cmd_tokens.join(" "));
        }
    }
    Some(result)
}

/// Split a token list into sub-commands at separator tokens (`;`, `&&`, `||`, `|`).
fn split_by_separators(tokens: Vec<String>) -> Vec<Vec<String>> {
    let mut commands: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for tok in tokens {
        if matches!(tok.as_str(), ";" | "&&" | "||" | "|") {
            if !current.is_empty() {
                commands.push(current);
                current = Vec::new();
            }
        } else {
            current.push(tok);
        }
    }
    if !current.is_empty() {
        commands.push(current);
    }
    commands
}

/// Unescape ANSI-C escape sequences as used in bash's `$'...'` quoting.
fn unescape_ansi_c(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('\\') => result.push('\\'),
                Some('\'') => result.push('\''),
                Some('"') => result.push('"'),
                Some('a') => result.push('\x07'),
                Some('b') => result.push('\x08'),
                Some('f') => result.push('\x0C'),
                Some('v') => result.push('\x0B'),
                Some(c) => {
                    result.push('\\');
                    result.push(c);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(ch);
        }
    }
    result
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
        assert_eq!(split_tokens("ls -la /tmp"), vec!["ls", "-la", "/tmp"]);
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
        assert_eq!(split_tokens("cmd1 && cmd2"), vec!["cmd1", "&&", "cmd2"]);
    }

    // 7. Semicolon is a separator token
    #[test]
    fn semicolon_separator() {
        assert_eq!(split_tokens("cmd1; cmd2"), vec!["cmd1", ";", "cmd2"]);
    }

    // 8. Semicolon without surrounding spaces
    #[test]
    fn semicolon_no_spaces() {
        assert_eq!(split_tokens("cmd1;cmd2"), vec!["cmd1", ";", "cmd2"]);
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
        assert_eq!(split_tokens("cmd1 || cmd2"), vec!["cmd1", "||", "cmd2"]);
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

    // --- T2.2: extract_nested_commands ---

    // 16. bash -c 'cmd' — single-quoted inner command
    #[test]
    fn nested_bash_single_quote() {
        assert_eq!(
            extract_nested_commands("bash -c 'echo hello'"),
            vec!["echo hello"]
        );
    }

    // 17. sh -c 'cmd' — sh variant is recognized
    #[test]
    fn nested_sh_single_quote() {
        assert_eq!(
            extract_nested_commands("sh -c 'echo hello'"),
            vec!["echo hello"]
        );
    }

    // 18. bash -c "cmd1 && cmd2" — && splits into two commands
    #[test]
    fn nested_double_quote_and_separator() {
        assert_eq!(
            extract_nested_commands(r#"bash -c "cmd1 && cmd2""#),
            vec!["cmd1", "cmd2"]
        );
    }

    // 19. bash -c "cmd1; cmd2; cmd3" — semicolons produce three commands
    #[test]
    fn nested_semicolons_three_cmds() {
        assert_eq!(
            extract_nested_commands(r#"bash -c "cmd1; cmd2; cmd3""#),
            vec!["cmd1", "cmd2", "cmd3"]
        );
    }

    // 20. bash -c $'cmd1\ncmd2' — ANSI-C $'...' quoting: \n becomes a newline separator
    #[test]
    fn nested_ansi_c_newline() {
        // In the Rust string literal "\\n" is a literal backslash-n, mirroring what the
        // shell passes when $'...' quoting embeds a \n escape.
        assert_eq!(
            extract_nested_commands("bash -c $'cmd1\\ncmd2'"),
            vec!["cmd1", "cmd2"]
        );
    }

    // 21. env VAR=val bash -c '...' — env prefix with one assignment
    #[test]
    fn nested_env_prefix_single_var() {
        assert_eq!(
            extract_nested_commands("env VAR=val bash -c 'echo hello'"),
            vec!["echo hello"]
        );
    }

    // 22. env A=1 B=2 bash -c '...' — env prefix with multiple assignments
    #[test]
    fn nested_env_prefix_multi_var() {
        assert_eq!(
            extract_nested_commands("env A=1 B=2 bash -c 'cmd'"),
            vec!["cmd"]
        );
    }

    // 23. bash -c 'bash -c "inner cmd"' — recursive unwrap preserves argument grouping
    #[test]
    fn nested_recursive_unwrap() {
        assert_eq!(
            extract_nested_commands(r#"bash -c 'bash -c "inner cmd"'"#),
            vec!["inner cmd"]
        );
    }

    // 24. python -c '...' — non-shell interpreter returns empty vec
    #[test]
    fn nested_non_shell_returns_empty() {
        assert_eq!(
            extract_nested_commands(r#"python -c 'print("hi")'"#),
            Vec::<String>::new()
        );
    }

    // 25. bash -c "cmd1 || cmd2" — || splits into two commands
    #[test]
    fn nested_double_pipe_separator() {
        assert_eq!(
            extract_nested_commands(r#"bash -c "cmd1 || cmd2""#),
            vec!["cmd1", "cmd2"]
        );
    }
}
