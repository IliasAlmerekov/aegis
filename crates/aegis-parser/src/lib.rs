#![deny(missing_docs)]

//! Shell command parsing for Aegis.
//!
//! This crate owns the tokenizer (quote/escape-aware splitting, heredoc and
//! inline-script extraction, pipeline segmentation, nested-shell unwrapping) and
//! the token-level `PrefixPattern` matcher. It produces the canonical
//! [`ParsedCommand`] consumed by the scanner. It depends only on `aegis-types`.

mod embedded_scripts;
mod nested_shells;
mod prefix_match;
mod segmentation;
mod tokenizer;

pub use aegis_types::{InlineScript, ParsedCommand};
pub use embedded_scripts::{
    HeredocBody, extract_eval_payloads, extract_heredoc_bodies, extract_inline_scripts,
    extract_process_substitution_bodies,
};
pub use nested_shells::extract_nested_commands;
pub use prefix_match::matches_prefix;
pub use segmentation::{logical_segments, top_level_pipelines};
pub use tokenizer::{extract_prefix, split_tokens};

/// A token slice resolved to the program that prefix-style detection should use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveTokenSlice<'a> {
    /// The token sequence with the effective program in position 0.
    pub tokens: Vec<&'a str>,
    /// The basename-normalized program token used as an index key.
    pub program: &'a str,
}

/// Resolve candidate token slices after stripping known launcher prefixes.
///
/// This is detection-only normalization: it never changes the parsed command or
/// the command that will be executed. Absolute program paths are reduced to
/// their basename, and launchers such as `sudo`, `env`, `rtk`, `timeout`, and
/// `command` are skipped recursively so token-prefix rules see the program they
/// are meant to protect.
pub fn effective_token_slices<'a>(tokens: &'a [&'a str]) -> Vec<EffectiveTokenSlice<'a>> {
    if tokens.is_empty() {
        return Vec::new();
    }

    let starts = effective_program_indices(tokens);
    let mut slices = Vec::with_capacity(starts.len());
    for start in starts {
        let Some(program) = tokens.get(start).map(|token| program_basename(token)) else {
            continue;
        };

        let mut effective_tokens = Vec::with_capacity(tokens.len().saturating_sub(start));
        effective_tokens.push(program);
        effective_tokens.extend(tokens[start + 1..].iter().copied());

        slices.push(EffectiveTokenSlice {
            tokens: effective_tokens,
            program,
        });
    }
    slices
}

/// Resolve the basename-normalized program token used for detection matching.
///
/// This is the allocation-free companion to [`effective_token_slices`] for call
/// sites that only need the lookup key, not a rewritten token slice.
pub fn effective_program<'a>(tokens: &'a [&'a str]) -> Option<&'a str> {
    effective_program_indices(tokens)
        .into_iter()
        .next()
        .and_then(|start| tokens.get(start).map(|token| program_basename(token)))
}

fn effective_program_indices(tokens: &[&str]) -> Vec<usize> {
    let mut starts = Vec::new();
    collect_effective_program_indices(tokens, 0, &mut starts);
    starts.sort_unstable();
    starts.dedup();
    starts
}

fn collect_effective_program_indices(tokens: &[&str], index: usize, starts: &mut Vec<usize>) {
    if index >= tokens.len() {
        return;
    }

    match launcher_prefix_lengths(&tokens[index..]) {
        Some(lengths) => {
            for len in lengths {
                if len == 0 {
                    starts.push(index);
                } else {
                    collect_effective_program_indices(tokens, index + len, starts);
                }
            }
        }
        None => starts.push(index),
    }
}

fn launcher_prefix_lengths(tokens: &[&str]) -> Option<Vec<usize>> {
    let launcher = program_basename(tokens.first().copied()?);
    if launcher.eq_ignore_ascii_case("rtk")
        || launcher.eq_ignore_ascii_case("nohup")
        || launcher.eq_ignore_ascii_case("time")
        || launcher.eq_ignore_ascii_case("command")
        || launcher.eq_ignore_ascii_case("doas")
    {
        return Some(vec![1]);
    }

    if launcher.eq_ignore_ascii_case("timeout") {
        return Some(timeout_prefix_lengths(tokens));
    }

    if launcher.eq_ignore_ascii_case("nice") {
        return Some(vec![nice_prefix_len(tokens)]);
    }

    if launcher.eq_ignore_ascii_case("sudo") {
        return Some(sudo_prefix_lengths(tokens));
    }

    if launcher.eq_ignore_ascii_case("env") {
        return Some(env_prefix_lengths(tokens));
    }

    None
}

fn sudo_prefix_lengths(tokens: &[&str]) -> Vec<usize> {
    sudo_prefix_lengths_from(tokens, 1)
}

fn sudo_prefix_lengths_from(tokens: &[&str], mut index: usize) -> Vec<usize> {
    while index < tokens.len() {
        let token = tokens[index];
        if token.contains('=') {
            index += 1;
            continue;
        }
        if !token.starts_with('-') || token == "-" {
            break;
        }
        index += 1;
        if matches!(
            token,
            "-u" | "--user"
                | "-g"
                | "--group"
                | "-h"
                | "--host"
                | "-p"
                | "--prompt"
                | "-C"
                | "--close-from"
                | "-T"
                | "--command-timeout"
        ) && index < tokens.len()
        {
            index += 1;
        } else if index < tokens.len() {
            let mut candidates = sudo_prefix_lengths_from(tokens, index);
            if index < tokens.len() {
                candidates.push(index + 1);
            }
            candidates.sort_unstable();
            candidates.dedup();
            return candidates;
        }
    }
    vec![index]
}

fn env_prefix_lengths(tokens: &[&str]) -> Vec<usize> {
    let mut index = 1;
    while index < tokens.len() {
        let token = tokens[index];
        if token.contains('=') || token == "-" {
            index += 1;
            continue;
        }
        if matches!(token, "-i" | "-0" | "--ignore-environment" | "--null") {
            index += 1;
            continue;
        }
        if matches!(
            token,
            "-u" | "--unset" | "-C" | "--chdir" | "-S" | "--split-string"
        ) {
            index += 2.min(tokens.len() - index);
            continue;
        }
        if token.starts_with('-') {
            return if index + 1 < tokens.len() {
                vec![index + 1, index + 2]
            } else {
                vec![index + 1]
            };
        }
        break;
    }
    vec![index]
}

fn timeout_prefix_lengths(tokens: &[&str]) -> Vec<usize> {
    let mut index = 1;
    while index < tokens.len() {
        let token = tokens[index];
        if token == "--" {
            index += 1;
            break;
        }
        if matches!(
            token,
            "-v" | "--verbose" | "--foreground" | "--preserve-status"
        ) || token.starts_with("--signal=")
            || token.starts_with("--kill-after=")
            || short_timeout_option_with_value(token)
        {
            index += 1;
            continue;
        }
        if matches!(token, "-s" | "--signal" | "-k" | "--kill-after") {
            index += 2.min(tokens.len() - index);
            continue;
        }
        if token.starts_with('-') {
            return if index + 1 < tokens.len() {
                vec![index + 1, index + 2]
            } else {
                vec![index + 1]
            };
        }
        break;
    }
    vec![(index + 1).min(tokens.len())]
}

fn short_timeout_option_with_value(token: &str) -> bool {
    token.len() > 2 && (token.starts_with("-s") || token.starts_with("-k"))
}

fn nice_prefix_len(tokens: &[&str]) -> usize {
    if tokens.len() > 2 && matches!(tokens[1], "-n" | "--adjustment") {
        3
    } else if tokens.len() > 1 && tokens[1].starts_with('-') && tokens[1].len() > 1 {
        2
    } else {
        1
    }
}

fn program_basename(token: &str) -> &str {
    if let Some((_, basename)) = token.rsplit_once('/') {
        basename
    } else {
        token
    }
}

/// One top-level segment within a pipeline chain.
///
/// `raw` preserves the original shell spelling for diagnostics, while
/// `normalized` joins shell tokens with single spaces so downstream matching can
/// reason about neighboring pipeline stages without quote noise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineSegment {
    /// Original shell spelling of this segment.
    pub raw: String,
    /// Shell tokens joined by single spaces (no quoting noise).
    pub normalized: String,
}

/// A top-level shell pipeline chain such as `cmd1 | cmd2 | cmd3`.
///
/// Chains are delimited only by top-level control operators other than the
/// single pipe (`;`, `&&`, `||`, newlines). This preserves adjacency between
/// neighboring pipeline stages for semantic analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineChain {
    /// Original shell spelling of the full chain.
    pub raw: String,
    /// Individual pipeline stages within the chain.
    pub segments: Vec<PipelineSegment>,
}

/// A stateless parser that converts raw shell command strings into [`ParsedCommand`].
pub struct Parser;

impl Parser {
    /// Parse `cmd` into a [`ParsedCommand`].
    ///
    /// Tokenizes `cmd` (respecting quoting and escaping), then extracts the
    /// program name and argument list from the first logical command. The full
    /// token sequence is joined into `normalized` — the canonical match target
    /// used by the scanner. The raw string is preserved only for audit logging.
    pub fn parse(cmd: &str) -> ParsedCommand {
        let tokens = split_tokens(cmd);

        // Tokens of the first sub-command only (before any shell separator).
        let first_cmd: Vec<&String> = tokens
            .iter()
            .take_while(|t| !matches!(t.as_str(), ";" | "&&" | "||" | "|"))
            .collect();

        let program = first_cmd.first().map(|s| s.to_string());
        let argv: Vec<String> = first_cmd.iter().skip(1).map(|s| s.to_string()).collect();

        // De-quoted, space-joined form of the full token sequence.
        let normalized = tokens.join(" ");

        let inline_scripts = extract_inline_scripts(cmd);

        ParsedCommand {
            program,
            argv,
            normalized,
            inline_scripts,
            raw: cmd.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod parsing_tests;
    mod tokenizer_tests;

    #[test]
    fn effective_token_slices_strip_launchers_and_absolute_paths() {
        let tokens = [
            "sudo",
            "env",
            "FOO=bar",
            "rtk",
            "/usr/bin/git",
            "reset",
            "--hard",
        ];
        let slices = effective_token_slices(&tokens);

        assert_eq!(slices.len(), 1);
        assert_eq!(slices[0].program, "git");
        assert_eq!(slices[0].tokens, vec!["git", "reset", "--hard"]);
    }

    #[test]
    fn effective_token_slices_skip_launcher_options() {
        let tokens = [
            "sudo",
            "-u",
            "root",
            "timeout",
            "5s",
            "/bin/kill",
            "-9",
            "1",
        ];
        let slices = effective_token_slices(&tokens);

        assert_eq!(slices[0].program, "kill");
        assert_eq!(slices[0].tokens, vec!["kill", "-9", "1"]);
    }

    #[test]
    fn effective_token_slices_parse_timeout_options() {
        let tokens = [
            "timeout",
            "-s",
            "KILL",
            "-k",
            "10s",
            "5s",
            "/usr/bin/git",
            "reset",
            "--hard",
        ];
        let slices = effective_token_slices(&tokens);

        assert_eq!(slices[0].program, "git");
        assert_eq!(slices[0].tokens, vec!["git", "reset", "--hard"]);
    }

    #[test]
    fn effective_token_slices_keep_conservative_unknown_sudo_flag_candidates() {
        let tokens = ["sudo", "--new-opt", "value", "git", "reset", "--hard"];
        let slices = effective_token_slices(&tokens);
        let programs: Vec<&str> = slices.iter().map(|slice| slice.program).collect();

        assert!(programs.contains(&"value"));
        assert!(programs.contains(&"git"));
    }

    #[test]
    fn effective_token_slices_keep_scanning_after_unknown_sudo_flags() {
        let tokens = ["sudo", "-n", "-u", "postgres", "psql", "-c", "DROP TABLE t"];
        let slices = effective_token_slices(&tokens);
        let programs: Vec<&str> = slices.iter().map(|slice| slice.program).collect();

        assert!(programs.contains(&"psql"));
    }

    #[test]
    fn effective_token_slices_keep_env_no_arg_candidate_for_unknown_flag() {
        let tokens = ["env", "-X", "git", "reset", "--hard"];
        let slices = effective_token_slices(&tokens);
        let programs: Vec<&str> = slices.iter().map(|slice| slice.program).collect();

        assert!(programs.contains(&"git"));
    }

    #[test]
    fn effective_token_slices_skip_sudo_environment_assignment() {
        let tokens = ["sudo", "FOO=bar", "git", "reset", "--hard"];
        let slices = effective_token_slices(&tokens);

        assert_eq!(slices[0].program, "git");
        assert_eq!(slices[0].tokens, vec!["git", "reset", "--hard"]);
    }
}
