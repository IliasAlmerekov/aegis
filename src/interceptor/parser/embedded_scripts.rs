use super::{segmentation::split_top_level_segments, split_tokens};

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

/// Extract process-substitution bodies from shell input forms like `<(...)`.
///
/// The returned strings are the shell commands inside the substitution, without
/// the surrounding `<(` and `)`.
pub fn extract_process_substitution_bodies(cmd: &str) -> Vec<String> {
    let chars: Vec<char> = cmd.chars().collect();
    let mut bodies = Vec::new();
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_backticks = false;

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
            '<' if !in_single_quote
                && !in_double_quote
                && !in_backticks
                && chars.get(i + 1) == Some(&'(') =>
            {
                if let Some((body, end_idx)) = extract_angle_paren_body(&chars, i) {
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

fn extract_angle_paren_body(chars: &[char], start_idx: usize) -> Option<(String, usize)> {
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
            '(' if !in_single_quote && !in_double_quote && !in_backticks => {
                depth += 1;
                body.push(chars[idx]);
                idx += 1;
            }
            ')' if !in_single_quote && !in_double_quote && !in_backticks => {
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

/// Extract `eval` payload strings from logical shell segments.
///
/// This unwraps the arguments passed to `eval` so nested shell or interpreter
/// bodies can be analyzed recursively. Variable-only forms such as `eval "$VAR"`
/// remain opaque and are returned as-is when no literal body is available.
pub fn extract_eval_payloads(cmd: &str) -> Vec<String> {
    let mut payloads = Vec::new();

    for segment in split_top_level_segments(cmd) {
        let tokens = split_tokens(&segment);
        if tokens.is_empty() {
            continue;
        }

        let mut idx = 0;

        if tokens.get(idx).map(String::as_str) == Some("env") {
            idx += 1;
        }

        while let Some(token) = tokens.get(idx) {
            if token.contains('=') && !token.starts_with('-') {
                idx += 1;
            } else {
                break;
            }
        }

        if tokens.get(idx).map(String::as_str) == Some("eval") && idx + 1 < tokens.len() {
            payloads.push(tokens[idx + 1..].join(" "));
        }
    }

    payloads
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

    let (strip_tabs, rest) = if let Some(stripped) = rest.strip_prefix('-') {
        (true, stripped)
    } else {
        (false, rest)
    };

    let rest = rest.trim_start();

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
