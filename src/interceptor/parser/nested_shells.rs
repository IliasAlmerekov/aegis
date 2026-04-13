use super::split_tokens;

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

    if tokens.get(idx).map(String::as_str) == Some("env") {
        idx += 1;
    }

    while let Some(tok) = tokens.get(idx) {
        if tok.contains('=') && !tok.starts_with('-') {
            idx += 1;
        } else {
            break;
        }
    }

    if !tokens
        .get(idx)
        .map(|s| matches!(s.as_str(), "bash" | "sh" | "dash" | "zsh" | "ksh" | "fish"))
        .unwrap_or(false)
    {
        return None;
    }
    idx += 1;

    let rel_c = tokens[idx..].iter().position(|t| {
        t == "-c" || (t.starts_with('-') && !t.starts_with("--") && t.len() > 2 && t.ends_with('c'))
    })?;
    let inner_idx = idx + rel_c + 1;
    let inner_raw = tokens.get(inner_idx)?;

    let inner = if let Some(stripped) = inner_raw.strip_prefix('$') {
        unescape_ansi_c(stripped)
    } else {
        inner_raw.clone()
    };

    let mut all_sub_cmds: Vec<Vec<String>> = Vec::new();
    for line in inner.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let line_tokens = split_tokens(line);
        all_sub_cmds.extend(split_by_separators(line_tokens));
    }

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
