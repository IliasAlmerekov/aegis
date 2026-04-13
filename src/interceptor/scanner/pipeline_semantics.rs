use std::sync::Arc;

use crate::interceptor::RiskLevel;
use crate::interceptor::parser::PipelineChain;
use crate::interceptor::patterns::{Category, Pattern, PatternSource};

use super::MatchResult;

pub(super) fn semantic_pipeline_matches(pipelines: &[PipelineChain]) -> Vec<MatchResult> {
    let mut matches = Vec::new();

    for chain in pipelines {
        for window in chain.segments.windows(2) {
            let left = &window[0].normalized;
            let right = &window[1].normalized;
            let evidence_text = format!("{} | {}", left, right);

            if is_shell_sink(right) {
                push_semantic_match(
                    &mut matches,
                    "PIPE-001",
                    RiskLevel::Danger,
                    &evidence_text,
                    "pipeline feeds data directly into sh/bash",
                    Some("Write the payload to a reviewed script file before executing it"),
                );
            }

            if is_xargs_rm_sink(right) {
                push_semantic_match(
                    &mut matches,
                    "PIPE-002",
                    RiskLevel::Danger,
                    &evidence_text,
                    "pipeline feeds data into xargs rm, turning upstream output into file deletions",
                    Some("Write the candidate paths to a file and review them before deletion"),
                );
            }

            if is_obvious_secret_source(left) && is_network_sink(right) {
                push_semantic_match(
                    &mut matches,
                    "PIPE-003",
                    RiskLevel::Danger,
                    &evidence_text,
                    "pipeline sends obvious secret material into a network sink",
                    Some(
                        "Write the data to a local file, inspect it, and avoid piping secrets into network clients",
                    ),
                );
            }
        }
    }

    matches
}

fn push_semantic_match(
    matches: &mut Vec<MatchResult>,
    id: &'static str,
    risk: RiskLevel,
    matched_text: &str,
    description: &'static str,
    safe_alt: Option<&'static str>,
) {
    if matches.iter().any(|existing| existing.pattern.id == id) {
        return;
    }

    matches.push(MatchResult {
        pattern: Arc::new(Pattern {
            id: id.into(),
            category: Category::Process,
            risk,
            pattern: id.into(),
            description: description.into(),
            safe_alt: safe_alt.map(Into::into),
            source: PatternSource::Builtin,
        }),
        matched_text: matched_text.to_string(),
        highlight_range: None,
    });
}

fn is_shell_sink(segment: &str) -> bool {
    matches!(first_token(segment).as_deref(), Some("sh") | Some("bash"))
}

fn is_xargs_rm_sink(segment: &str) -> bool {
    let tokens = crate::interceptor::parser::split_tokens(segment);
    if tokens.first().map(String::as_str) != Some("xargs") {
        return false;
    }

    extract_xargs_command(&tokens).is_some_and(|command| command == "rm")
}

fn extract_xargs_command(tokens: &[String]) -> Option<&str> {
    let mut idx = 1;

    while let Some(token) = tokens.get(idx) {
        let token = token.as_str();
        if !token.starts_with('-') || token == "-" {
            return Some(token);
        }

        if xargs_option_takes_value(token) {
            idx += 2;
        } else {
            idx += 1;
        }
    }

    None
}

fn xargs_option_takes_value(option: &str) -> bool {
    matches!(
        option,
        "-E" | "-I"
            | "-L"
            | "-P"
            | "-d"
            | "-n"
            | "-s"
            | "--delimiter"
            | "--eof"
            | "--max-args"
            | "--max-chars"
            | "--max-lines"
            | "--max-procs"
            | "--replace"
    )
}

fn is_network_sink(segment: &str) -> bool {
    matches!(first_token(segment).as_deref(), Some("curl") | Some("wget"))
}

fn is_obvious_secret_source(segment: &str) -> bool {
    let tokens = crate::interceptor::parser::split_tokens(segment);
    let Some(first) = tokens.first().map(String::as_str) else {
        return false;
    };

    if first == "cat"
        && tokens
            .iter()
            .skip(1)
            .any(|token| is_known_secret_path(token))
    {
        return true;
    }

    if first == "printenv" {
        return match tokens.get(1).map(String::as_str) {
            None => true,
            Some(name) => is_secret_like_env_name(name),
        };
    }

    first == "env" && tokens.len() == 1
}

fn is_known_secret_path(path: &str) -> bool {
    matches!(path, "~/.ssh/id_rsa" | "~/.aws/credentials")
}

fn is_secret_like_env_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.contains("_SECRET")
        || upper.contains("_TOKEN")
        || upper.contains("_KEY")
        || upper.contains("_PASSWORD")
}

fn first_token(segment: &str) -> Option<String> {
    crate::interceptor::parser::split_tokens(segment)
        .into_iter()
        .next()
}
