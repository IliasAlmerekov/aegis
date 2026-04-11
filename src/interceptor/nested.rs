use std::collections::{HashSet, VecDeque};

use crate::interceptor::parser::{
    Parser, extract_eval_payloads, extract_heredoc_bodies, extract_process_substitution_bodies,
    logical_segments,
};

const MAX_NESTED_SCAN_DEPTH: usize = 8;

/// Collect recursive scan targets derived from nested execution wrappers.
///
/// The returned list always contains `cmd` itself plus any normalized or
/// unwrapped payloads discovered through shell nesting, heredocs, inline
/// interpreters, process substitution, and `eval`.
pub fn recursive_scan_targets(cmd: &str) -> Vec<String> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([(cmd.trim().to_string(), 0usize)]);

    while let Some((candidate, depth)) = queue.pop_front() {
        if candidate.is_empty() || !seen.insert(candidate.clone()) {
            continue;
        }

        targets.push(candidate.clone());

        if depth >= MAX_NESTED_SCAN_DEPTH {
            continue;
        }

        for target in expand_nested_targets(&candidate) {
            let trimmed = target.trim();
            if !trimmed.is_empty() && !seen.contains(trimmed) {
                queue.push_back((trimmed.to_string(), depth + 1));
            }
        }
    }

    targets
}

fn expand_nested_targets(cmd: &str) -> Vec<String> {
    let mut targets = logical_segments(cmd);
    let parsed = Parser::parse(cmd);

    for script in parsed.inline_scripts {
        targets.push(script.body);
    }

    for heredoc in extract_heredoc_bodies(cmd) {
        targets.push(heredoc.body);
    }

    for body in extract_process_substitution_bodies(cmd) {
        targets.push(body);
    }

    for payload in extract_eval_payloads(cmd) {
        targets.push(payload);
    }

    targets
}

#[cfg(test)]
mod tests {
    use super::recursive_scan_targets;

    #[test]
    fn recursive_targets_include_inline_script_body_from_nested_shell() {
        let targets = recursive_scan_targets(r#"bash -c 'python3 -c "print(42)"'"#);
        assert!(targets.iter().any(|target| target == "print(42)"));
    }

    #[test]
    fn recursive_targets_include_heredoc_body_and_nested_inline_script() {
        let cmd = "bash <<'EOF'\npython3 -c \"print(42)\"\nEOF";
        let targets = recursive_scan_targets(cmd);

        assert!(
            targets
                .iter()
                .any(|target| target == r#"python3 -c "print(42)""#)
        );
        assert!(targets.iter().any(|target| target == "print(42)"));
    }

    #[test]
    fn recursive_targets_include_process_substitution_body() {
        let targets = recursive_scan_targets(r#"source <(python3 -c "print(42)")"#);

        assert!(
            targets
                .iter()
                .any(|target| target == r#"python3 -c "print(42)""#)
        );
        assert!(targets.iter().any(|target| target == "print(42)"));
    }

    #[test]
    fn recursive_targets_include_eval_payload() {
        let targets = recursive_scan_targets(r#"eval "python3 -c 'print(42)'"#);

        assert!(
            targets
                .iter()
                .any(|target| target == "python3 -c 'print(42)'")
        );
        assert!(targets.iter().any(|target| target == "print(42)"));
    }
}
