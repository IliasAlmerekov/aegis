use super::{PipelineChain, PipelineSegment, extract_nested_commands, split_tokens};

/// Split a raw shell command string into its logical segments.
///
/// Segments are delimited by top-level shell control operators (`&&`, `||`, `;`, `|`)
/// and newlines. Quoting and escaping are respected — operators inside quotes are not
/// separators.
///
/// The returned list is scan-oriented rather than execution-oriented:
/// - top-level command chains become separate segments
/// - shell wrappers such as env-prefix forms contribute an additional stripped segment
/// - subshell groups and command substitutions contribute normalized inner segments
/// - quoted shell strings (for example `bash -c "cmd1 && cmd2"`) keep the outer segment
///   and also contribute the inner normalized commands
///
/// # Examples
///
/// ```text
/// "echo ok && rm -rf /"   → ["echo ok", "rm -rf /"]
/// "cmd1; cmd2; cmd3"       → ["cmd1", "cmd2", "cmd3"]
/// "bash -c 'a && b'"       → ["bash -c a && b", "a", "b"]
/// "echo $(rm -rf /)"       → ["echo $(rm -rf /)", "rm -rf /"]
/// ```
///
/// Used by the scanner as a normalization layer so that dangerous payloads
/// keep their command boundaries even when wrapped in shell syntax.
pub fn logical_segments(cmd: &str) -> Vec<String> {
    let mut segments = Vec::new();

    for raw_segment in split_top_level_segments(cmd) {
        collect_scan_segments(&raw_segment, &mut segments);
    }

    segments
}

/// Extract top-level pipeline chains from shell input.
///
/// Examples:
///
/// ```text
/// "echo ok | sh"                    → [["echo ok", "sh"]]
/// "a | b && c | d"                  → [["a", "b"], ["c", "d"]]
/// "echo 'x|y' | bash"               → [["echo x|y", "bash"]]
/// "echo ok && rm -rf /"             → []
/// ```
pub fn top_level_pipelines(cmd: &str) -> Vec<PipelineChain> {
    split_top_level_command_groups(cmd)
        .into_iter()
        .filter_map(|raw_group| {
            let segments = split_pipeline_segments(&raw_group);
            (segments.len() > 1).then_some(PipelineChain {
                raw: raw_group,
                segments,
            })
        })
        .collect()
}

fn collect_scan_segments(raw_segment: &str, segments: &mut Vec<String>) {
    if let Some(normalized) = normalize_segment(raw_segment) {
        push_unique(segments, normalized);
    }

    if let Some(stripped_env_command) = strip_env_prefix(raw_segment) {
        collect_scan_segments(&stripped_env_command, segments);
    }

    for nested in extract_nested_commands(raw_segment) {
        collect_scan_segments(&nested, segments);
    }

    if let Some(subshell_body) = unwrap_subshell_group(raw_segment) {
        collect_scan_segments(&subshell_body, segments);
    }

    for body in extract_command_substitution_bodies(raw_segment) {
        for nested_segment in split_top_level_segments(&body) {
            collect_scan_segments(&nested_segment, segments);
        }
    }
}

pub(super) fn split_top_level_segments(cmd: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = cmd.chars().peekable();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_backticks = false;
    let mut paren_depth = 0usize;
    let mut command_subst_depth = 0usize;

    while let Some(ch) = chars.next() {
        match ch {
            '\\' if !in_single_quote => {
                current.push(ch);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '\'' if !in_double_quote && !in_backticks => {
                in_single_quote = !in_single_quote;
                current.push(ch);
            }
            '"' if !in_single_quote && !in_backticks => {
                in_double_quote = !in_double_quote;
                current.push(ch);
            }
            '`' if !in_single_quote => {
                in_backticks = !in_backticks;
                current.push(ch);
            }
            '$' if !in_single_quote && !in_backticks && chars.peek() == Some(&'(') => {
                command_subst_depth += 1;
                current.push(ch);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '(' if !in_single_quote
                && !in_double_quote
                && !in_backticks
                && command_subst_depth == 0 =>
            {
                paren_depth += 1;
                current.push(ch);
            }
            ')' if !in_single_quote
                && !in_backticks
                && (command_subst_depth > 0 || paren_depth > 0) =>
            {
                if command_subst_depth > 0 {
                    command_subst_depth -= 1;
                } else {
                    paren_depth -= 1;
                }
                current.push(ch);
            }
            '\n' if !in_single_quote
                && !in_double_quote
                && !in_backticks
                && paren_depth == 0
                && command_subst_depth == 0 =>
            {
                finalize_segment(&mut current, &mut segments);
            }
            ';' if !in_single_quote
                && !in_double_quote
                && !in_backticks
                && paren_depth == 0
                && command_subst_depth == 0 =>
            {
                finalize_segment(&mut current, &mut segments);
            }
            '&' if !in_single_quote
                && !in_double_quote
                && !in_backticks
                && paren_depth == 0
                && command_subst_depth == 0
                && chars.peek() == Some(&'&') =>
            {
                chars.next();
                finalize_segment(&mut current, &mut segments);
            }
            '|' if !in_single_quote
                && !in_double_quote
                && !in_backticks
                && paren_depth == 0
                && command_subst_depth == 0 =>
            {
                if chars.peek() == Some(&'|') {
                    chars.next();
                }
                finalize_segment(&mut current, &mut segments);
            }
            _ => current.push(ch),
        }
    }

    finalize_segment(&mut current, &mut segments);
    segments
}

fn split_top_level_command_groups(cmd: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = cmd.chars().peekable();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_backticks = false;
    let mut paren_depth = 0usize;
    let mut command_subst_depth = 0usize;

    while let Some(ch) = chars.next() {
        match ch {
            '\\' if !in_single_quote => {
                current.push(ch);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '\'' if !in_double_quote && !in_backticks => {
                in_single_quote = !in_single_quote;
                current.push(ch);
            }
            '"' if !in_single_quote && !in_backticks => {
                in_double_quote = !in_double_quote;
                current.push(ch);
            }
            '`' if !in_single_quote => {
                in_backticks = !in_backticks;
                current.push(ch);
            }
            '$' if !in_single_quote && !in_backticks && chars.peek() == Some(&'(') => {
                command_subst_depth += 1;
                current.push(ch);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '(' if !in_single_quote
                && !in_double_quote
                && !in_backticks
                && command_subst_depth == 0 =>
            {
                paren_depth += 1;
                current.push(ch);
            }
            ')' if !in_single_quote
                && !in_backticks
                && (command_subst_depth > 0 || paren_depth > 0) =>
            {
                if command_subst_depth > 0 {
                    command_subst_depth -= 1;
                } else {
                    paren_depth -= 1;
                }
                current.push(ch);
            }
            '\n' if !in_single_quote
                && !in_double_quote
                && !in_backticks
                && paren_depth == 0
                && command_subst_depth == 0 =>
            {
                finalize_segment(&mut current, &mut segments);
            }
            ';' if !in_single_quote
                && !in_double_quote
                && !in_backticks
                && paren_depth == 0
                && command_subst_depth == 0 =>
            {
                finalize_segment(&mut current, &mut segments);
            }
            '&' if !in_single_quote
                && !in_double_quote
                && !in_backticks
                && paren_depth == 0
                && command_subst_depth == 0
                && chars.peek() == Some(&'&') =>
            {
                chars.next();
                finalize_segment(&mut current, &mut segments);
            }
            '|' if !in_single_quote
                && !in_double_quote
                && !in_backticks
                && paren_depth == 0
                && command_subst_depth == 0
                && chars.peek() == Some(&'|') =>
            {
                chars.next();
                finalize_segment(&mut current, &mut segments);
            }
            _ => current.push(ch),
        }
    }

    finalize_segment(&mut current, &mut segments);
    segments
}

fn split_pipeline_segments(raw_group: &str) -> Vec<PipelineSegment> {
    let mut raw_segments = Vec::new();
    let mut current = String::new();
    let mut chars = raw_group.chars().peekable();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_backticks = false;
    let mut paren_depth = 0usize;
    let mut command_subst_depth = 0usize;

    while let Some(ch) = chars.next() {
        match ch {
            '\\' if !in_single_quote => {
                current.push(ch);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '\'' if !in_double_quote && !in_backticks => {
                in_single_quote = !in_single_quote;
                current.push(ch);
            }
            '"' if !in_single_quote && !in_backticks => {
                in_double_quote = !in_double_quote;
                current.push(ch);
            }
            '`' if !in_single_quote => {
                in_backticks = !in_backticks;
                current.push(ch);
            }
            '$' if !in_single_quote && !in_backticks && chars.peek() == Some(&'(') => {
                command_subst_depth += 1;
                current.push(ch);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '(' if !in_single_quote
                && !in_double_quote
                && !in_backticks
                && command_subst_depth == 0 =>
            {
                paren_depth += 1;
                current.push(ch);
            }
            ')' if !in_single_quote
                && !in_backticks
                && (command_subst_depth > 0 || paren_depth > 0) =>
            {
                if command_subst_depth > 0 {
                    command_subst_depth -= 1;
                } else {
                    paren_depth -= 1;
                }
                current.push(ch);
            }
            '|' if !in_single_quote
                && !in_double_quote
                && !in_backticks
                && paren_depth == 0
                && command_subst_depth == 0
                && chars.peek() != Some(&'|') =>
            {
                finalize_segment(&mut current, &mut raw_segments);
            }
            _ => current.push(ch),
        }
    }

    finalize_segment(&mut current, &mut raw_segments);

    raw_segments
        .into_iter()
        .filter_map(|raw| {
            normalize_segment(&raw).map(|normalized| PipelineSegment { raw, normalized })
        })
        .collect()
}

fn finalize_segment(current: &mut String, segments: &mut Vec<String>) {
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        segments.push(trimmed.to_string());
    }
    current.clear();
}

fn normalize_segment(raw_segment: &str) -> Option<String> {
    let tokens = split_tokens(raw_segment);
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

fn push_unique(segments: &mut Vec<String>, segment: String) {
    if !segment.is_empty() && !segments.iter().any(|existing| existing == &segment) {
        segments.push(segment);
    }
}

fn strip_env_prefix(raw_segment: &str) -> Option<String> {
    let tokens = split_tokens(raw_segment);
    if tokens.is_empty() {
        return None;
    }

    let mut idx = 0;
    let mut stripped_any = false;

    if tokens.get(idx).map(String::as_str) == Some("env") {
        idx += 1;
        stripped_any = true;
    }

    while let Some(token) = tokens.get(idx) {
        if token.contains('=') && !token.starts_with('-') {
            idx += 1;
            stripped_any = true;
        } else {
            break;
        }
    }

    if stripped_any && idx < tokens.len() {
        Some(tokens[idx..].join(" "))
    } else {
        None
    }
}

fn unwrap_subshell_group(raw_segment: &str) -> Option<String> {
    let trimmed = raw_segment.trim();
    if !trimmed.starts_with('(') {
        return None;
    }

    let chars: Vec<char> = trimmed.chars().collect();
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_backticks = false;
    let mut paren_depth = 0usize;
    let mut command_subst_depth = 0usize;
    let mut close_idx = None;

    while i < chars.len() {
        match chars[i] {
            '\\' if !in_single_quote => {
                i = (i + 2).min(chars.len());
            }
            '\'' if !in_double_quote && !in_backticks => {
                in_single_quote = !in_single_quote;
                i += 1;
            }
            '"' if !in_single_quote && !in_backticks => {
                in_double_quote = !in_double_quote;
                i += 1;
            }
            '`' if !in_single_quote => {
                in_backticks = !in_backticks;
                i += 1;
            }
            '$' if !in_single_quote && !in_backticks && chars.get(i + 1) == Some(&'(') => {
                command_subst_depth += 1;
                i += 2;
            }
            '(' if !in_single_quote
                && !in_double_quote
                && !in_backticks
                && command_subst_depth == 0 =>
            {
                paren_depth += 1;
                i += 1;
            }
            ')' if !in_single_quote
                && !in_backticks
                && (command_subst_depth > 0 || paren_depth > 0) =>
            {
                if command_subst_depth > 0 {
                    command_subst_depth -= 1;
                } else {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        close_idx = Some(i);
                        break;
                    }
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    if close_idx == Some(chars.len() - 1) {
        let inner: String = chars[1..chars.len() - 1].iter().collect();
        let inner = inner.trim();
        if !inner.is_empty() {
            return Some(inner.to_string());
        }
    }

    None
}

fn extract_command_substitution_bodies(raw_segment: &str) -> Vec<String> {
    let chars: Vec<char> = raw_segment.chars().collect();
    let mut bodies = Vec::new();
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while i < chars.len() {
        match chars[i] {
            '\\' if !in_single_quote => {
                i = (i + 2).min(chars.len());
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
                i += 1;
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
                i += 1;
            }
            '$' if !in_single_quote && chars.get(i + 1) == Some(&'(') => {
                if let Some((body, end_idx)) = extract_dollar_paren_body(&chars, i) {
                    bodies.push(body);
                    i = end_idx + 1;
                } else {
                    i += 1;
                }
            }
            '`' if !in_single_quote => {
                if let Some((body, end_idx)) = extract_backtick_body(&chars, i) {
                    bodies.push(body);
                    i = end_idx + 1;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }

    bodies
}

fn extract_dollar_paren_body(chars: &[char], start_idx: usize) -> Option<(String, usize)> {
    let mut body = String::new();
    let mut idx = start_idx + 2;
    let mut depth = 1usize;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_backticks = false;

    while idx < chars.len() {
        match chars[idx] {
            '\\' if !in_single_quote => {
                body.push(chars[idx]);
                idx += 1;
                if let Some(next) = chars.get(idx) {
                    body.push(*next);
                    idx += 1;
                }
            }
            '\'' if !in_double_quote && !in_backticks => {
                in_single_quote = !in_single_quote;
                body.push(chars[idx]);
                idx += 1;
            }
            '"' if !in_single_quote && !in_backticks => {
                in_double_quote = !in_double_quote;
                body.push(chars[idx]);
                idx += 1;
            }
            '`' if !in_single_quote => {
                in_backticks = !in_backticks;
                body.push(chars[idx]);
                idx += 1;
            }
            '$' if !in_single_quote && !in_backticks && chars.get(idx + 1) == Some(&'(') => {
                depth += 1;
                body.push(chars[idx]);
                idx += 1;
                if let Some(next) = chars.get(idx) {
                    body.push(*next);
                    idx += 1;
                }
            }
            '(' if !in_single_quote && !in_double_quote && !in_backticks => {
                depth += 1;
                body.push(chars[idx]);
                idx += 1;
            }
            ')' if !in_single_quote && !in_backticks => {
                depth -= 1;
                if depth == 0 {
                    return Some((body.trim().to_string(), idx));
                }
                body.push(chars[idx]);
                idx += 1;
            }
            _ => {
                body.push(chars[idx]);
                idx += 1;
            }
        }
    }

    None
}

fn extract_backtick_body(chars: &[char], start_idx: usize) -> Option<(String, usize)> {
    let mut body = String::new();
    let mut idx = start_idx + 1;

    while idx < chars.len() {
        match chars[idx] {
            '\\' => {
                body.push(chars[idx]);
                idx += 1;
                if let Some(next) = chars.get(idx) {
                    body.push(*next);
                    idx += 1;
                }
            }
            '`' => return Some((body.trim().to_string(), idx)),
            _ => {
                body.push(chars[idx]);
                idx += 1;
            }
        }
    }

    None
}
