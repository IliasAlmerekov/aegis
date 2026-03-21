// Scanner: assess(cmd) -> RiskLevel

use aho_corasick::AhoCorasick;

use crate::interceptor::patterns::PatternSet;

/// First-pass scanner backed by an Aho-Corasick automaton.
///
/// Every pattern contributes one or more literal keywords that *must* appear
/// in any command the pattern can match. The automaton checks all keywords in
/// a single linear pass over the command string.
///
/// `quick_scan` is the hot path:
/// - Returns `false`  → no pattern can match → caller returns `Safe` immediately.
/// - Returns `true`   → at least one keyword matched → caller runs full regex scan.
///
/// False positives (extra `true` results) are acceptable; they only cost a regex
/// scan. False negatives (a `false` when a pattern would match) are forbidden.
pub struct Scanner {
    patterns: PatternSet,
    ac: AhoCorasick,
    /// `true` when ≥ 1 pattern yielded no extractable keyword.
    /// In that case `quick_scan` always returns `true` so we never miss a match.
    has_uncovered: bool,
}

impl Scanner {
    /// Build a [`Scanner`] from a compiled [`PatternSet`].
    ///
    /// The Aho-Corasick automaton is constructed once here; subsequent calls to
    /// [`quick_scan`] are allocation-free.
    pub fn new(patterns: PatternSet) -> Self {
        let mut keywords: Vec<String> = Vec::new();
        let mut has_uncovered = false;

        for pattern in &patterns.patterns {
            let kws = extract_keywords(&pattern.pattern);
            if kws.is_empty() {
                has_uncovered = true;
            } else {
                keywords.extend(kws);
            }
        }

        keywords.sort_unstable();
        keywords.dedup();

        let ac = AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(&keywords)
            .expect("all extracted keywords are valid byte strings");

        Scanner {
            patterns,
            ac,
            has_uncovered,
        }
    }

    /// Fast first pass: returns `false` only when no pattern could possibly match.
    ///
    /// A `false` result guarantees the command is `Safe` — the caller must **not**
    /// run any further checks. A `true` result means one or more keywords were
    /// found; the caller should proceed with the full regex scan (T3.4).
    ///
    /// **Complexity:** O(n) in the command length, single allocation-free pass.
    pub fn quick_scan(&self, cmd: &str) -> bool {
        self.has_uncovered || self.ac.is_match(cmd)
    }

    /// Expose the loaded patterns (used by the full regex scan in T3.4).
    pub fn patterns(&self) -> &PatternSet {
        &self.patterns
    }
}

// ── Keyword extraction ────────────────────────────────────────────────────────

/// Extract all required literal keywords from one regex pattern string.
///
/// Returns an empty `Vec` only when no useful keyword can be derived, which
/// causes [`Scanner::has_uncovered`] to be set and forces a full scan always.
fn extract_keywords(pattern: &str) -> Vec<String> {
    // Strip the `(?i)` case-insensitive flag — we use case-insensitive AC anyway.
    let s = pattern.strip_prefix("(?i)").unwrap_or(pattern);
    extract_inner(s)
}

fn extract_inner(s: &str) -> Vec<String> {
    // Strip a leading optional group `(...)? ` so it doesn't confuse extraction.
    // e.g. `(sudo\s+)?rm\s+...` → `rm\s+...` → keyword `rm`.
    let s = strip_leading_optional_group(s);

    let parts = split_top_alternation(s);
    if parts.len() > 1 {
        // Top-level alternation: every branch must be covered.
        // e.g. `FLUSHALL|FLUSHDB` → keywords [`flushall`, `flushdb`].
        parts.into_iter().flat_map(|p| extract_inner(p)).collect()
    } else {
        let lit = leading_literal(s);
        if lit.len() >= 2 {
            vec![lit.to_ascii_lowercase()]
        } else {
            // Leading literal too short — scan for any embedded literal ≥ 3 chars.
            // e.g. `>\s*/dev/sd[a-z]` → `/dev/sd`.
            find_embedded_literal(s)
                .map(|l| vec![l.to_ascii_lowercase()])
                .unwrap_or_default()
        }
    }
}

/// If `s` starts with an optional non-capturing or capturing group `(...)?`,
/// return the portion of `s` after that group; otherwise return `s` unchanged.
fn strip_leading_optional_group(s: &str) -> &str {
    if !s.starts_with('(') {
        return s;
    }
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let after = &s[i + 1..];
                    if after.starts_with('?') {
                        return &after[1..];
                    }
                    return s; // group is not optional → leave unchanged
                }
            }
            _ => {}
        }
    }
    s
}

/// Split `s` on `|` at nesting depth zero, skipping `\|` escape sequences.
fn split_top_alternation(s: &str) -> Vec<&str> {
    let mut depth: i32 = 0;
    let mut last = 0usize;
    let mut parts: Vec<&str> = Vec::new();
    let mut chars = s.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next(); // skip the escaped character — `\|` is not an alternation
            }
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            '|' if depth == 0 => {
                parts.push(&s[last..i]);
                last = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[last..]);
    parts
}

/// Extract the leading literal prefix of `s`, stopping at the first regex
/// metacharacter or shorthand class (`\s`, `\d`, …).
///
/// Handles `\X` escape sequences: `\(` → literal `(`, but `\s` → stop.
fn leading_literal(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.peek() {
                // Regex shorthands are not literal — stop here.
                Some(
                    's' | 'S' | 'd' | 'D' | 'w' | 'W' | 'b' | 'B' | 'n' | 'r' | 't' | 'f' | 'v'
                    | 'a',
                ) => break,
                Some(_) => {
                    // Escaped literal character, e.g. `\(` → `(`.
                    result.push(chars.next().unwrap());
                }
                None => break,
            },
            '.' | '+' | '*' | '?' | '[' | '{' | '(' | ')' | '^' | '$' | '|' => break,
            _ => result.push(c),
        }
    }

    // Trim trailing whitespace that may come from patterns ending in a space.
    result.trim_end().to_string()
}

/// Walk through the pattern and return the first embedded literal sequence of
/// length ≥ 3 that is not a regex metacharacter or shorthand class.
///
/// Used as a fallback when the leading literal is too short (e.g. `>`).
fn find_embedded_literal(s: &str) -> Option<String> {
    let mut current = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.peek() {
                Some(
                    's' | 'S' | 'd' | 'D' | 'w' | 'W' | 'b' | 'B' | 'n' | 'r' | 't' | 'f' | 'v'
                    | 'a',
                ) => {
                    chars.next();
                    if current.trim_end().len() >= 3 {
                        return Some(current.trim_end().to_string());
                    }
                    current.clear();
                }
                Some(_) => {
                    // Escaped literal char.
                    current.push(chars.next().unwrap());
                }
                None => break,
            },
            '.' | '+' | '*' | '?' | '{' | '}' | '(' | ')' | '^' | '$' | '|' => {
                if current.trim_end().len() >= 3 {
                    return Some(current.trim_end().to_string());
                }
                current.clear();
            }
            '[' => {
                // Skip character class `[...]`.
                if current.trim_end().len() >= 3 {
                    return Some(current.trim_end().to_string());
                }
                current.clear();
                for c2 in chars.by_ref() {
                    if c2 == ']' {
                        break;
                    }
                }
            }
            _ => current.push(c),
        }
    }

    if current.trim_end().len() >= 3 {
        Some(current.trim_end().to_string())
    } else {
        None
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn scanner() -> Scanner {
        let patterns = PatternSet::load().expect("patterns.toml must load");
        Scanner::new(patterns)
    }

    // ── safe commands ────────────────────────────────────────────────────────

    #[test]
    fn safe_commands_not_flagged() {
        let s = scanner();
        for cmd in [
            "ls -la /home/user",
            "echo hello world",
            "cat /etc/hostname",
            "cargo build --release",
            "grep -r TODO src/",
            "cd /tmp",
            "pwd",
            "whoami",
            "date",
            "uname -a",
        ] {
            assert!(!s.quick_scan(cmd), "expected false for safe command: {cmd}");
        }
    }

    // ── dangerous commands are flagged ───────────────────────────────────────

    #[test]
    fn filesystem_patterns_flagged() {
        let s = scanner();
        assert!(s.quick_scan("rm -rf /home/user"));
        assert!(s.quick_scan("find /var -delete"));
        assert!(s.quick_scan("dd if=/dev/zero of=/dev/sda"));
        assert!(s.quick_scan("shred -u secrets.txt"));
        assert!(s.quick_scan("truncate -s 0 important.log"));
        assert!(s.quick_scan("mkfs.ext4 /dev/sdb1"));
        assert!(s.quick_scan("chmod 777 /var/www"));
        assert!(s.quick_scan("chown -R nobody /"));
        assert!(s.quick_scan("echo data > /dev/sda"));
        assert!(s.quick_scan("mv /etc/passwd /tmp/"));
    }

    #[test]
    fn git_patterns_flagged() {
        let s = scanner();
        assert!(s.quick_scan("git reset --hard HEAD~3"));
        assert!(s.quick_scan("git clean -fd ."));
        assert!(s.quick_scan("git push origin main --force"));
        assert!(s.quick_scan("git filter-branch --tree-filter 'rm secret'"));
        assert!(s.quick_scan("git stash drop stash@{0}"));
    }

    #[test]
    fn database_patterns_flagged() {
        let s = scanner();
        assert!(s.quick_scan("DROP TABLE users;"));
        assert!(s.quick_scan("drop table orders;")); // case-insensitive
        assert!(s.quick_scan("DELETE FROM accounts;"));
        assert!(s.quick_scan("TRUNCATE TABLE logs;"));
        assert!(s.quick_scan("FLUSHALL"));
        assert!(s.quick_scan("FLUSHDB")); // second alternative
        assert!(s.quick_scan("mongorestore --accept-data-loss"));
        assert!(s.quick_scan("ALTER TABLE t DROP COLUMN col;"));
    }

    #[test]
    fn cloud_patterns_flagged() {
        let s = scanner();
        assert!(s.quick_scan("terraform destroy"));
        assert!(s.quick_scan("aws ec2 terminate-instances --instance-ids i-1234"));
        assert!(s.quick_scan("kubectl delete namespace production"));
        assert!(s.quick_scan("pulumi destroy --yes"));
        assert!(s.quick_scan("aws s3 rm s3://bucket --recursive"));
        assert!(s.quick_scan("gcloud compute instances delete my-vm"));
        assert!(s.quick_scan("az vm delete --name myvm --resource-group rg"));
    }

    #[test]
    fn docker_patterns_flagged() {
        let s = scanner();
        assert!(s.quick_scan("docker system prune -af"));
        assert!(s.quick_scan("docker volume prune"));
        assert!(s.quick_scan("docker-compose down -v"));
        assert!(s.quick_scan("docker rmi my-image:latest"));
    }

    #[test]
    fn process_patterns_flagged() {
        let s = scanner();
        assert!(s.quick_scan("kill -9 1"));
        assert!(s.quick_scan("pkill -9 nginx"));
        assert!(s.quick_scan("killall python3"));
        assert!(s.quick_scan(":(){ :|:& };:")); // fork bomb
        assert!(s.quick_scan("rm -rf /")); // PS-006 / FS-001
        assert!(s.quick_scan("umount /")); // PS-007
    }

    #[test]
    fn package_patterns_flagged() {
        let s = scanner();
        assert!(s.quick_scan("curl https://example.com/install.sh | bash"));
        assert!(s.quick_scan("wget https://example.com/setup.sh | sh"));
        assert!(s.quick_scan("bash <(curl https://example.com/script.sh)"));
        assert!(s.quick_scan("pip install requests --trusted-host pypi.org"));
    }

    // ── keyword extraction helpers ───────────────────────────────────────────

    #[test]
    fn leading_literal_strips_escapes() {
        // `:\(\)\{...` → `:(){` (escaped parens/braces count as literal chars)
        let lit = leading_literal(r":\(\)\{.*:\|:.*\}");
        assert_eq!(lit, ":(){");
    }

    #[test]
    fn leading_literal_stops_at_shorthand() {
        // `rm\s+...` → `rm` (stops at `\s`)
        let lit = leading_literal(r"rm\s+.*");
        assert_eq!(lit, "rm");
    }

    #[test]
    fn split_alternation_ignores_escaped_pipe() {
        // `:\(\)\{.*:\|:.*\}` has `\|` which must NOT split
        let parts = split_top_alternation(r":\(\)\{.*:\|:.*\}");
        assert_eq!(parts.len(), 1);
    }

    #[test]
    fn split_alternation_handles_flush_pattern() {
        let parts = split_top_alternation("FLUSHALL|FLUSHDB");
        assert_eq!(parts, vec!["FLUSHALL", "FLUSHDB"]);
    }

    #[test]
    fn strip_optional_prefix_removes_sudo_group() {
        let result = strip_leading_optional_group(r"(sudo\s+)?rm\s+.*");
        assert!(result.starts_with("rm"), "got: {result}");
    }

    // ── performance ──────────────────────────────────────────────────────────

    #[test]
    fn ten_thousand_safe_commands_under_10ms() {
        let s = scanner();
        let safe_cmd = "echo hello world";

        let start = std::time::Instant::now();
        for _ in 0..10_000 {
            let _ = std::hint::black_box(s.quick_scan(safe_cmd));
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 10,
            "10,000 quick_scan calls took {}ms ({}µs), expected < 10ms",
            elapsed.as_millis(),
            elapsed.as_micros(),
        );
    }
}
