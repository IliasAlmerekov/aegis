use regex::Regex;

/// Compiled allowlist matcher for trusted command strings.
#[derive(Debug, Clone, Default)]
pub struct Allowlist {
    matchers: Vec<Regex>,
}

impl Allowlist {
    pub fn new(patterns: &[String]) -> Self {
        let matchers = patterns
            .iter()
            .filter_map(|pattern| compile_pattern(pattern))
            .collect();

        Self { matchers }
    }

    pub fn is_allowed(&self, cmd: &str) -> bool {
        let command = cmd.trim();
        self.matchers
            .iter()
            .any(|matcher| matcher.is_match(command))
    }
}

fn compile_pattern(pattern: &str) -> Option<Regex> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return None;
    }

    Regex::new(&glob_to_regex(trimmed)).ok()
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
    use super::Allowlist;

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
}
