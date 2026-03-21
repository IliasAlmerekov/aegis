// Parser: tokenizer, heredoc, inline scripts
#![allow(dead_code)]

use std::fmt;

// ── T2.4: ParsedCommand struct and public API ─────────────────────────────────

/// The result of parsing a raw shell command string.
///
/// Captures the first executable, its arguments, any inline scripts detected
/// inside the command, and the original raw string for audit purposes.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCommand {
    /// The first token of the command (e.g. `rm`, `git`, `bash`).
    /// `None` only when the input string is empty or consists solely of separators.
    pub executable: Option<String>,
    /// All argument tokens after the executable (separators stripped).
    pub args: Vec<String>,
    /// Inline scripts extracted from interpreter invocations (python -c, node -e, etc.).
    pub inline_scripts: Vec<InlineScript>,
    /// The original, unmodified command string.
    pub raw: String,
}

impl fmt::Display for ParsedCommand {
    /// Formats the command for audit log output.
    ///
    /// Shows `executable [args...]` if parsing succeeded, or falls back to
    /// the raw string if no executable was found.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.executable {
            Some(exe) if !self.args.is_empty() => {
                write!(f, "{} {}", exe, self.args.join(" "))
            }
            Some(exe) => write!(f, "{}", exe),
            None => write!(f, "{}", self.raw),
        }
    }
}

/// A stateless parser that converts raw shell command strings into [`ParsedCommand`].
pub struct Parser;

impl Parser {
    /// Parse `cmd` into a [`ParsedCommand`].
    ///
    /// Extracts the executable and argument list from the first logical command
    /// in `cmd` (stopping at shell separators `&&`, `||`, `;`, `|`), and also
    /// collects any inline scripts found anywhere in `cmd`.
    pub fn parse(cmd: &str) -> ParsedCommand {
        let tokens = split_tokens(cmd);

        // Take only the tokens belonging to the first sub-command.
        let first_cmd: Vec<&String> = tokens
            .iter()
            .take_while(|t| !matches!(t.as_str(), ";" | "&&" | "||" | "|"))
            .collect();

        let executable = first_cmd.first().map(|s| s.to_string());
        let args: Vec<String> = first_cmd.iter().skip(1).map(|s| s.to_string()).collect();
        let inline_scripts = extract_inline_scripts(cmd);

        ParsedCommand {
            executable,
            args,
            inline_scripts,
            raw: cmd.to_string(),
        }
    }
}

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

// ── T2.3: Heredoc and inline script scanning ─────────────────────────────────

/// A heredoc (or nowdoc) body extracted from a multi-line command string.
#[derive(Debug, PartialEq)]
pub struct HeredocBody {
    /// The delimiter word (e.g., `EOF`, `SCRIPT`).
    pub delimiter: String,
    /// The lines of text between the opening marker and the closing delimiter.
    pub body: String,
    /// `true` when the delimiter was quoted (`<<'EOF'`), indicating nowdoc semantics
    /// (no variable substitution in the original shell).
    pub is_nowdoc: bool,
}

/// An inline script body extracted from an interpreter invocation.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineScript {
    /// The interpreter name (e.g., `python3`, `node`, `ruby`).
    pub interpreter: String,
    /// The script body passed via `-c` or `-e`.
    pub body: String,
}

// Private helper — result of scanning a single line for a heredoc operator.
struct HeredocMarker {
    delimiter: String,
    is_nowdoc: bool,
    strip_tabs: bool,
}

/// Scan one line for a heredoc operator (`<<`) and return the parsed marker.
///
/// Recognises:
/// - `<<WORD`          — regular heredoc
/// - `<<'WORD'`        — nowdoc (single-quoted delimiter)
/// - `<<-WORD`         — heredoc with leading-tab stripping
/// - `<<-'WORD'`       — nowdoc with leading-tab stripping
fn find_heredoc_marker(line: &str) -> Option<HeredocMarker> {
    let start = line.find("<<")?;
    let rest = &line[start + 2..];

    // Optional `-` enables tab-stripping.
    let (strip_tabs, rest) = if let Some(stripped) = rest.strip_prefix('-') {
        (true, stripped)
    } else {
        (false, rest)
    };

    let rest = rest.trim_start();

    // Nowdoc: <<'WORD'
    if let Some(inner) = rest.strip_prefix('\'') {
        let close = inner.find('\'')?;
        let delim = &inner[..close];
        if delim.is_empty() {
            return None;
        }
        return Some(HeredocMarker {
            delimiter: delim.to_string(),
            is_nowdoc: true,
            strip_tabs,
        });
    }

    // Regular heredoc: <<WORD  (alphanumeric + underscore)
    let word: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();

    if word.is_empty() {
        None
    } else {
        Some(HeredocMarker {
            delimiter: word,
            is_nowdoc: false,
            strip_tabs,
        })
    }
}

/// Extract all heredoc (and nowdoc) bodies from a multi-line command string.
///
/// Each call to this function scans `cmd` line by line, looking for `<<WORD`
/// or `<<'WORD'` markers. When found, it collects every subsequent line until
/// the closing delimiter appears on its own line (with leading tabs stripped
/// when `<<-` was used).
///
/// # Examples
///
/// ```text
/// cmd <<EOF
/// rm -rf /
/// EOF
/// ```
/// → `[HeredocBody { delimiter: "EOF", body: "rm -rf /", is_nowdoc: false }]`
pub fn extract_heredoc_bodies(cmd: &str) -> Vec<HeredocBody> {
    let mut bodies = Vec::new();
    let lines: Vec<&str> = cmd.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        if let Some(marker) = find_heredoc_marker(lines[i]) {
            i += 1;
            let mut body_lines: Vec<&str> = Vec::new();

            while i < lines.len() {
                let candidate = if marker.strip_tabs {
                    lines[i].trim_start_matches('\t')
                } else {
                    lines[i]
                };
                if candidate == marker.delimiter {
                    break;
                }
                body_lines.push(lines[i]);
                i += 1;
            }

            bodies.push(HeredocBody {
                delimiter: marker.delimiter,
                body: body_lines.join("\n"),
                is_nowdoc: marker.is_nowdoc,
            });
        }
        i += 1;
    }

    bodies
}

/// Extract all inline scripts from interpreter invocations in `cmd`.
///
/// Recognises the following patterns (the script flag immediately precedes
/// the script body as the next shell token):
///
/// | Interpreter       | Flag |
/// |-------------------|------|
/// | `python` / `python3` | `-c` |
/// | `node` / `nodejs` | `-e` |
/// | `ruby`            | `-e` |
/// | `perl`            | `-e` |
///
/// # Examples
///
/// ```text
/// python3 -c "import os; os.system('rm -rf /')"
/// ```
/// → `[InlineScript { interpreter: "python3", body: "import os; os.system('rm -rf /')" }]`
pub fn extract_inline_scripts(cmd: &str) -> Vec<InlineScript> {
    // (interpreter name, script flag)
    const INTERPRETERS: &[(&str, &str)] = &[
        ("python", "-c"),
        ("python3", "-c"),
        ("node", "-e"),
        ("nodejs", "-e"),
        ("ruby", "-e"),
        ("perl", "-e"),
    ];

    let mut scripts = Vec::new();
    let tokens = split_tokens(cmd);
    let mut i = 0;

    while i < tokens.len() {
        for &(interp, flag) in INTERPRETERS {
            if tokens[i] == interp {
                // Search for the flag in the remaining tokens.
                if let Some(rel) = tokens[i..].iter().position(|t| t == flag) {
                    let body_idx = i + rel + 1;
                    if let Some(body) = tokens.get(body_idx) {
                        scripts.push(InlineScript {
                            interpreter: interp.to_string(),
                            body: body.clone(),
                        });
                    }
                }
                break;
            }
        }
        i += 1;
    }

    scripts
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

    // ── T2.3: heredoc and inline script tests ────────────────────────────────

    // 26. Basic heredoc — body between <<EOF and EOF is extracted
    #[test]
    fn heredoc_basic_body() {
        let cmd = "cat <<EOF\nrm -rf /\nEOF";
        let bodies = extract_heredoc_bodies(cmd);
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].delimiter, "EOF");
        assert_eq!(bodies[0].body, "rm -rf /");
        assert!(!bodies[0].is_nowdoc);
    }

    // 27. Nowdoc — <<'EOF' is flagged as nowdoc, body is still extracted
    #[test]
    fn heredoc_nowdoc_flag() {
        let cmd = "cat <<'EOF'\nsome secret\nEOF";
        let bodies = extract_heredoc_bodies(cmd);
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].body, "some secret");
        assert!(bodies[0].is_nowdoc);
    }

    // 28. Multi-line heredoc body — all lines joined with newline
    #[test]
    fn heredoc_multiline_body() {
        let cmd = "bash <<SCRIPT\necho hello\nrm -rf /tmp/foo\nSCRIPT";
        let bodies = extract_heredoc_bodies(cmd);
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].body, "echo hello\nrm -rf /tmp/foo");
    }

    // 29. Heredoc with tab-stripping (<<-) — leading tabs removed from delimiter line
    #[test]
    fn heredoc_strip_tabs_delimiter() {
        // The closing delimiter has a leading tab that should be stripped.
        let cmd = "cat <<-EOF\n\trm -rf /\n\tEOF";
        let bodies = extract_heredoc_bodies(cmd);
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].delimiter, "EOF");
        // Body line is kept as-is (only the delimiter line is stripped for matching).
        assert_eq!(bodies[0].body, "\trm -rf /");
    }

    // 30. No heredoc — empty vec returned
    #[test]
    fn heredoc_no_match_returns_empty() {
        let bodies = extract_heredoc_bodies("echo hello world");
        assert!(bodies.is_empty());
    }

    // 31. python -c "..." — inline Python script extracted
    #[test]
    fn inline_script_python() {
        let scripts =
            extract_inline_scripts(r#"python3 -c "import os; os.system('cmd')""#);
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].interpreter, "python3");
        assert_eq!(scripts[0].body, "import os; os.system('cmd')");
    }

    // 32. node -e "..." — inline Node.js script extracted
    #[test]
    fn inline_script_node() {
        let scripts = extract_inline_scripts(r#"node -e "process.exit(1)""#);
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].interpreter, "node");
        assert_eq!(scripts[0].body, "process.exit(1)");
    }

    // 33. ruby -e "..." — inline Ruby script extracted
    #[test]
    fn inline_script_ruby() {
        let scripts = extract_inline_scripts(r#"ruby -e "system('rm -rf /')""#);
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].interpreter, "ruby");
        assert_eq!(scripts[0].body, "system('rm -rf /')");
    }

    // ── T2.4: ParsedCommand and Parser::parse ────────────────────────────────

    // 34. Simple command — executable and args split correctly
    #[test]
    fn parse_simple_command() {
        let p = Parser::parse("ls -la /tmp");
        assert_eq!(p.executable.as_deref(), Some("ls"));
        assert_eq!(p.args, vec!["-la", "/tmp"]);
        assert!(p.inline_scripts.is_empty());
        assert_eq!(p.raw, "ls -la /tmp");
    }

    // 35. Only executable, no args
    #[test]
    fn parse_no_args() {
        let p = Parser::parse("pwd");
        assert_eq!(p.executable.as_deref(), Some("pwd"));
        assert!(p.args.is_empty());
    }

    // 36. Empty input — executable is None
    #[test]
    fn parse_empty_input() {
        let p = Parser::parse("");
        assert_eq!(p.executable, None);
        assert!(p.args.is_empty());
    }

    // 37. Command with separators — only first sub-command is parsed
    #[test]
    fn parse_first_subcommand_only() {
        let p = Parser::parse("echo hello && rm -rf /");
        assert_eq!(p.executable.as_deref(), Some("echo"));
        assert_eq!(p.args, vec!["hello"]);
    }

    // 38. Quoted argument is treated as a single arg
    #[test]
    fn parse_quoted_arg() {
        let p = Parser::parse(r#"git commit -m "fix: my message""#);
        assert_eq!(p.executable.as_deref(), Some("git"));
        assert_eq!(p.args, vec!["commit", "-m", "fix: my message"]);
    }

    // 39. Inline python script is captured in inline_scripts
    #[test]
    fn parse_captures_inline_script() {
        let p = Parser::parse(r#"python3 -c "import os; os.remove('x')""#);
        assert_eq!(p.executable.as_deref(), Some("python3"));
        assert_eq!(p.inline_scripts.len(), 1);
        assert_eq!(p.inline_scripts[0].interpreter, "python3");
    }

    // 40. Display shows executable + args
    #[test]
    fn display_shows_executable_and_args() {
        let p = Parser::parse("rm -rf /tmp/foo");
        assert_eq!(p.to_string(), "rm -rf /tmp/foo");
    }

    // 41. Display of a no-arg command shows just the executable
    #[test]
    fn display_no_args() {
        let p = Parser::parse("pwd");
        assert_eq!(p.to_string(), "pwd");
    }

    // 42. Display of empty input falls back to raw (empty string)
    #[test]
    fn display_empty_falls_back_to_raw() {
        let p = Parser::parse("");
        assert_eq!(p.to_string(), "");
    }

    // 43. Performance: parse 50 varied commands in under 1ms total
    #[test]
    fn parse_50_commands_under_1ms() {
        let cases = [
            "ls -la /tmp",
            "rm -rf /var/log/*",
            r#"git commit -m "feat: add feature""#,
            "bash -c 'echo hello && rm /tmp/x'",
            r#"python3 -c "import os; os.remove('x')""#,
            "docker ps -a",
            "kubectl delete pod my-pod",
            "terraform destroy -auto-approve",
            "find / -name '*.log' -delete",
            "dd if=/dev/urandom of=/dev/sda",
            "cat /etc/passwd",
            "chmod 777 /etc/shadow",
            "curl -s https://example.com | bash",
            "npm install --global evil-pkg",
            "pip install requests",
            r#"node -e "process.exit(1)""#,
            "aws ec2 terminate-instances --instance-ids i-1234",
            "gcloud compute instances delete my-vm",
            "kubectl delete namespace production",
            "docker system prune -a --volumes",
            "git reset --hard HEAD~10",
            "git push --force origin main",
            "git filter-branch --all",
            "DROP TABLE users;",
            "DELETE FROM orders;",
            "echo foo; echo bar; echo baz",
            "env A=1 B=2 bash -c 'cmd'",
            "sh -c 'ls | grep foo'",
            r#"ruby -e "system('cmd')""#,
            "perl -e 'print 42'",
            "pkill -9 nginx",
            "kill -9 1",
            "truncate -s 0 /var/log/syslog",
            "shred -u /etc/hosts",
            "docker rm -f $(docker ps -aq)",
            "docker volume prune -f",
            "docker-compose down -v",
            "pulumi destroy --yes",
            "helm uninstall my-release",
            "terraform workspace delete production",
            "git stash drop stash@{0}",
            "git clean -fdx",
            "cargo build --release",
            "cargo test -- --nocapture",
            "cargo clippy -- -D warnings",
            "rustfmt src/main.rs",
            "rustup update stable",
            "cargo audit",
            "cargo deny check",
            "echo all done",
        ];
        assert_eq!(cases.len(), 50, "must have exactly 50 test cases");

        let start = std::time::Instant::now();
        for cmd in &cases {
            let _ = Parser::parse(cmd);
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() < 1,
            "50 parses took {}µs, expected < 1ms",
            elapsed.as_micros()
        );
    }
}
