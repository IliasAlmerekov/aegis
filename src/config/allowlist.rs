use regex::Regex;

/// The allowlist entry that caused a command to be trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowlistMatch {
    /// The original glob pattern from the config that matched.
    pub pattern: String,
}

/// Compiled allowlist matcher for trusted command strings.
#[derive(Debug, Clone, Default)]
pub struct Allowlist {
    /// Original pattern text paired with its compiled regex.
    entries: Vec<(String, Regex)>,
}

impl Allowlist {
    pub fn new<T: AsRef<str>>(patterns: &[T]) -> Self {
        let entries = patterns
            .iter()
            .filter_map(|pattern| compile_pattern(pattern.as_ref()))
            .collect();

        Self { entries }
    }

    /// Returns the first allowlist entry whose pattern matches `cmd`, or `None`.
    ///
    /// Use this instead of [`is_allowed`] when you need to know *which* rule
    /// matched (e.g. for verbose output or audit logging).
    pub fn match_reason(&self, cmd: &str) -> Option<AllowlistMatch> {
        let command = cmd.trim();
        self.entries
            .iter()
            .find(|(_, rx)| rx.is_match(command))
            .map(|(pattern, _)| AllowlistMatch {
                pattern: pattern.clone(),
            })
    }

    /// Returns `true` when any allowlist entry matches `cmd`.
    pub fn is_allowed(&self, cmd: &str) -> bool {
        self.match_reason(cmd).is_some()
    }
}

fn compile_pattern(pattern: &str) -> Option<(String, Regex)> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return None;
    }

    Regex::new(&glob_to_regex(trimmed))
        .ok()
        .map(|rx| (trimmed.to_string(), rx))
}

fn glob_to_regex(pattern: &str) -> String {
    let mut regex = String::from("^");

    for ch in pattern.chars() {
        match ch {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '[' | ']' | '\\' => {
                regex.push('\\');
                regex.push(ch);
            }
            _ => regex.push(ch),
        }
    }

    regex.push('$');
    regex
}

#[cfg(test)]
mod tests {
    use super::{Allowlist, AllowlistMatch};

    #[test]
    fn exact_pattern_matches_only_the_same_command() {
        let allowlist = Allowlist::new(&["docker system prune --volumes".to_string()]);

        assert!(allowlist.is_allowed("docker system prune --volumes"));
        assert!(!allowlist.is_allowed("docker system prune"));
    }

    #[test]
    fn glob_pattern_matches_specific_target_family() {
        let allowlist = Allowlist::new(&["terraform destroy -target=module.test.*".to_string()]);

        assert!(allowlist.is_allowed("terraform destroy -target=module.test.api"));
        assert!(allowlist.is_allowed("terraform destroy -target=module.test.api.blue"));
        assert!(!allowlist.is_allowed("terraform destroy -target=module.prod.api"));
    }

    #[test]
    fn empty_patterns_are_ignored() {
        let allowlist = Allowlist::new(&["".to_string(), "   ".to_string()]);

        assert!(!allowlist.is_allowed("terraform destroy"));
    }

    #[test]
    fn match_reason_returns_none_when_no_pattern_matches() {
        let allowlist = Allowlist::new(&["docker system prune --volumes".to_string()]);
        assert_eq!(allowlist.match_reason("docker system prune"), None);
    }

    #[test]
    fn match_reason_returns_matched_pattern_text() {
        let allowlist = Allowlist::new(&[
            "terraform destroy -target=module.test.*".to_string(),
            "docker system prune --volumes".to_string(),
        ]);

        // First rule matches — pattern text is preserved exactly as written in config.
        assert_eq!(
            allowlist.match_reason("terraform destroy -target=module.test.api"),
            Some(AllowlistMatch {
                pattern: "terraform destroy -target=module.test.*".to_string(),
            })
        );

        // Second rule matches.
        assert_eq!(
            allowlist.match_reason("docker system prune --volumes"),
            Some(AllowlistMatch {
                pattern: "docker system prune --volumes".to_string(),
            })
        );
    }

    #[test]
    fn match_reason_returns_first_matching_rule_when_multiple_would_match() {
        let allowlist = Allowlist::new(&[
            "terraform destroy *".to_string(),
            "terraform destroy -target=module.test.*".to_string(),
        ]);

        let m = allowlist
            .match_reason("terraform destroy -target=module.test.api")
            .unwrap();
        assert_eq!(m.pattern, "terraform destroy *");
    }
}
