use std::borrow::Cow;

use aegis_types::RiskLevel;

use super::{Category, PatternSource, PatternToken, PrefixRule, a, any_star, s};
pub(super) fn rules() -> Vec<PrefixRule> {
    vec![
        // ── Filesystem ───────────────────────────────────────────────────
        // First Filesystem-category token-prefix rules: unlike `rm` (regex
        // FS-001), `wipefs`/`unlink` have no match-anywhere delivery variety —
        // the dangerous verb is always the effective program token (ADR-014).
        PrefixRule {
            id: Cow::Borrowed("FS-011"),
            category: Category::Filesystem,
            // AnyStar lets `-a` follow other flags (e.g. `wipefs -n -a`), an
            // accepted fail-safe FP. `FS-011` also has a local matcher-side predicate
            // for wipefs short flag bundles containing `a` (`-af`, `-fa`, `-fav`);
            // this is intentionally not a generic prefix-rule feature.
            pattern: vec![s("wipefs"), any_star(), a(&["-a", "--all"])],
            // Danger, not Block like mkfs (FS-006): wipefs erases filesystem /
            // partition *signatures* only — the underlying data blocks survive
            // and are often recoverable, so it is strictly less final than a
            // format. A prompt + snapshot is the right checkpoint for its
            // legitimate interactive disk-prep use.
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "wipefs -a — erases all filesystem/partition signatures from a device, making its volumes unmountable",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Back up the partition table first: 'sfdisk -d /dev/sdX > table.bak' and verify the device with 'lsblk'",
            )),
            justification: Some(Cow::Borrowed(
                "Wiping signatures detaches every filesystem on the device at once. Recovery depends on having the partition layout saved; double-check the device path.",
            )),
            source: PatternSource::Builtin,
            match_examples: &[
                "wipefs -a /dev/sda",
                "wipefs --all /dev/sdb",
                "wipefs -af /dev/sda",
                "wipefs -fa /dev/sda",
            ],
            not_match_examples: &[
                "wipefs /dev/sda",
                "wipefs -n /dev/sda",
                "wipefs -f /dev/sda",
            ],
        },
        PrefixRule {
            id: Cow::Borrowed("FS-012"),
            category: Category::Filesystem,
            pattern: vec![s("unlink")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "unlink — removes a single file or link by name with no confirmation or trash",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Move to trash instead: 'trash <file>' or 'mv <file> /tmp/backup-$(date +%s)'",
            )),
            justification: Some(Cow::Borrowed(
                "Deletes the named file immediately with no recovery path. Less destructive than 'rm -rf' but still irreversible.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["unlink important.txt"],
            not_match_examples: &["readlink mylink", "ln -s a b"],
        },
        PrefixRule {
            id: Cow::Borrowed("FS-015"),
            category: Category::Filesystem,
            pattern: vec![
                s("rsync"),
                any_star(),
                a(&[
                    "--delete",
                    "--delete-before",
                    "--delete-during",
                    "--delete-delay",
                    "--delete-after",
                    "--delete-excluded",
                    "--delete-missing-args",
                ]),
            ],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "rsync --delete — removes destination files that are absent from the source",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Dry-run first: 'rsync -n --delete <source> <destination>' and verify the destination path before syncing",
            )),
            justification: Some(Cow::Borrowed(
                "With delete flags, rsync removes files from the destination that are missing from the source. A wrong source or destination path can erase deployed or remote files.",
            )),
            source: PatternSource::Builtin,
            match_examples: &[
                "rsync -av --delete ./dist/ deploy:/srv/site/",
                "rsync --delete-after ./dist/ deploy:/srv/site/",
                "rsync --delete-excluded ./dist/ deploy:/srv/site/",
                "rsync --delete-missing-args --files-from=list ./ dest/",
            ],
            not_match_examples: &["rsync -av ./dist/ deploy:/srv/site/"],
        },
        PrefixRule {
            id: Cow::Borrowed("FS-016"),
            category: Category::Filesystem,
            pattern: vec![s("blkdiscard")],
            risk: RiskLevel::Block,
            description: Cow::Borrowed(
                "blkdiscard — discards all blocks on a block device, effectively wiping stored data",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Verify the target with 'lsblk' and use a dedicated disk-wipe workflow outside Aegis if this is intentional",
            )),
            justification: Some(Cow::Borrowed(
                "blkdiscard can make device data unrecoverable immediately. Aegis treats this as an intrinsic Block-level wipe operation.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["blkdiscard /dev/sda", "blkdiscard -f /dev/nvme0n1"],
            not_match_examples: &["lsblk /dev/sda"],
        },
        PrefixRule {
            id: Cow::Borrowed("FS-017"),
            category: Category::Filesystem,
            pattern: vec![s("sgdisk"), any_star(), a(&["--zap-all", "-Z"])],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "sgdisk --zap-all — destroys GPT and MBR partition table data on a disk",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Back up the partition table first: 'sgdisk --backup=table.gpt /dev/sdX' and verify the device with 'lsblk'",
            )),
            justification: Some(Cow::Borrowed(
                "Zapping partition metadata can make all partitions inaccessible. Recovery depends on having the original layout saved.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["sgdisk --zap-all /dev/sda", "sgdisk -Z /dev/sda"],
            not_match_examples: &["sgdisk --print /dev/sda"],
        },
        PrefixRule {
            id: Cow::Borrowed("FS-018"),
            category: Category::Filesystem,
            pattern: vec![s("parted"), any_star(), a(&["mklabel", "rm"])],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "parted mklabel/rm — rewrites a partition table label or removes a partition",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Print and back up the partition layout first: 'parted /dev/sdX print' and 'sfdisk -d /dev/sdX > table.bak'",
            )),
            justification: Some(Cow::Borrowed(
                "Partition table changes can make data inaccessible immediately. Confirm the target disk and partition number before proceeding.",
            )),
            source: PatternSource::Builtin,
            match_examples: &[
                "parted /dev/sda mklabel gpt",
                "parted -s /dev/sda mklabel msdos",
                "parted /dev/sda rm 1",
            ],
            not_match_examples: &["parted /dev/sda print", "parted -l"],
        },
        // ── Git ──────────────────────────────────────────────────────────
        PrefixRule {
            id: Cow::Borrowed("GIT-001"),
            category: Category::Git,
            pattern: vec![s("git"), s("reset"), s("--hard")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "git reset --hard — discards all uncommitted changes in the working tree and index",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Stash changes first: 'git stash push -m \"backup\"' then reset if needed",
            )),
            justification: Some(Cow::Borrowed(
                "Discards all uncommitted changes permanently with no recovery. Stash first if you need any of the work.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["git reset --hard HEAD~1"],
            not_match_examples: &["git reset --soft HEAD~1"],
        },
        PrefixRule {
            id: Cow::Borrowed("GIT-002"),
            category: Category::Git,
            pattern: vec![
                s("git"),
                s("clean"),
                any_star(),
                a(&["-f", "-fd", "-fdx", "-fx", "-fX", "--force"]),
            ],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "git clean -f — permanently removes untracked files from the working tree",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Preview first: 'git clean -n' (dry-run) before using -f",
            )),
            justification: Some(Cow::Borrowed(
                "Untracked files are deleted immediately without going to trash. A typo in the path can destroy important files.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["git clean -fd .", "git clean --force ."],
            not_match_examples: &["git clean -n ."],
        },
        PrefixRule {
            id: Cow::Borrowed("GIT-003"),
            category: Category::Git,
            pattern: vec![
                s("git"),
                s("push"),
                any_star(),
                a(&["--force", "-f", "--force-with-lease"]),
            ],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "git push --force — rewrites remote history, can discard other contributors' commits",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Prefer '--force-with-lease' over '--force' to avoid overwriting unseen commits",
            )),
            justification: Some(Cow::Borrowed(
                "This command rewrites remote history. Collaborators with local copies will have diverged refs and will need to force-pull or re-clone. Consider --force-with-lease to at least detect concurrent pushes.",
            )),
            source: PatternSource::Builtin,
            match_examples: &[
                "git push origin main --force",
                "git push origin main --force-with-lease",
            ],
            not_match_examples: &["git push origin main"],
        },
        PrefixRule {
            id: Cow::Borrowed("GIT-004"),
            category: Category::Git,
            pattern: vec![s("git"), s("filter-branch")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "git filter-branch — rewrites entire repository history; extremely destructive on shared repos",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Use 'git filter-repo' (faster, safer) and coordinate with all contributors first",
            )),
            justification: Some(Cow::Borrowed(
                "Rewrites every commit in the repository. On a shared repo this invalidates all clones and requires coordinated action.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["git filter-branch --tree-filter 'rm -f secret.txt' HEAD"],
            not_match_examples: &["git filter-repo --path secret.txt"],
        },
        PrefixRule {
            id: Cow::Borrowed("GIT-005"),
            category: Category::Git,
            pattern: vec![s("git"), s("rebase")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "git rebase — rewrites commit history; can cause conflicts and lose work if misused",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Ensure your branch is up-to-date and create a backup branch before rebasing",
            )),
            justification: Some(Cow::Borrowed(
                "Rewrites commit history which changes SHAs. If pushed, collaborators must resolve conflicts. Never rebase public branches.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["git rebase -i HEAD~3"],
            not_match_examples: &["git merge feature-branch"],
        },
        PrefixRule {
            id: Cow::Borrowed("GIT-006"),
            category: Category::Git,
            pattern: vec![s("git"), s("branch"), s("-D")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "git branch -D — force-deletes a branch even with unmerged commits",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Verify the branch is fully merged: 'git branch --merged' before deleting",
            )),
            justification: Some(Cow::Borrowed(
                "Force-deletes a branch with unmerged commits. Those commits become unreachable and may be garbage-collected.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["git branch -D feature/old-experiment"],
            not_match_examples: &["git branch -d old"],
        },
        PrefixRule {
            id: Cow::Borrowed("GIT-006B"),
            category: Category::Git,
            pattern: vec![s("git"), s("branch"), s("--delete"), s("--force")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "git branch --delete --force — force-deletes a branch even with unmerged commits",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Verify the branch is fully merged: 'git branch --merged' before deleting",
            )),
            justification: Some(Cow::Borrowed(
                "Force-deletes a branch with unmerged commits. Those commits become unreachable and may be garbage-collected.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["git branch --delete --force old"],
            not_match_examples: &["git branch --delete old"],
        },
        PrefixRule {
            id: Cow::Borrowed("GIT-006C"),
            category: Category::Git,
            pattern: vec![s("git"), s("branch"), s("-d"), s("--force")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "git branch -d --force — force-deletes a branch even with unmerged commits",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Verify the branch is fully merged: 'git branch --merged' before deleting",
            )),
            justification: Some(Cow::Borrowed(
                "Force-deletes a branch with unmerged commits. Those commits become unreachable and may be garbage-collected.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["git branch -d --force old"],
            not_match_examples: &["git branch -d old"],
        },
        PrefixRule {
            id: Cow::Borrowed("GIT-007"),
            category: Category::Git,
            pattern: vec![s("git"), s("checkout"), s("--"), s(".")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "git checkout -- . — discards all unstaged changes in the working directory",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Stage or stash changes you want to keep before running this",
            )),
            justification: Some(Cow::Borrowed(
                "Discards all unstaged changes in the working directory. There is no undo after this command.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["git checkout -- ."],
            not_match_examples: &["git checkout -- file.txt"],
        },
        PrefixRule {
            id: Cow::Borrowed("GIT-008"),
            category: Category::Git,
            pattern: vec![s("git"), s("stash"), a(&["drop", "clear"])],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "git stash drop/clear — permanently removes saved stash entries with no recovery",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Apply the stash before dropping: 'git stash apply && git stash drop'",
            )),
            justification: Some(Cow::Borrowed(
                "Dropped stashes are immediately removed from the reflog and cannot be recovered.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["git stash drop stash@{0}"],
            not_match_examples: &["git stash list"],
        },
        // ── Database ───────────────────────────────────────────────────────
        // DB-001 (DROP TABLE), DB-002 (DROP DATABASE), DB-007 (DROP SCHEMA), and
        // DB-008 (ALTER TABLE … DROP COLUMN) are regex patterns in patterns.toml,
        // not token-prefix rules: their SQL verbs arrive embedded in `psql -c` /
        // `mysql -e` / heredoc / stdin, never as the leading program token
        // (ADR-015). Only DB-006 (Redis FLUSHALL/FLUSHDB), whose verb *is* the
        // command, stays a prefix rule here.
        PrefixRule {
            id: Cow::Borrowed("DB-006"),
            category: Category::Database,
            pattern: vec![a(&["FLUSHALL", "FLUSHDB"])],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "Redis FLUSHALL / FLUSHDB — wipes all keys in the cache or entire Redis instance",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Use key-pattern-based deletion: 'SCAN + DEL' to remove only the intended keys",
            )),
            justification: Some(Cow::Borrowed(
                "Wipes all keys instantly. Redis has no undo; if you lack persistence backups the data is gone forever.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["FLUSHALL"],
            not_match_examples: &["GET mykey"],
        },
        PrefixRule {
            id: Cow::Borrowed("DB-006"),
            category: Category::Database,
            pattern: vec![s("redis-cli"), any_star(), a(&["FLUSHALL", "FLUSHDB"])],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "redis-cli FLUSHALL / FLUSHDB — wipes all keys in the cache or selected Redis database",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Use key-pattern-based deletion: 'SCAN + DEL' to remove only the intended keys",
            )),
            justification: Some(Cow::Borrowed(
                "Wipes Redis keys instantly through redis-cli. Redis has no undo; if persistence backups are missing the data is gone forever.",
            )),
            source: PatternSource::Builtin,
            match_examples: &[
                "redis-cli FLUSHALL",
                "redis-cli -h cache.local -n 0 FLUSHDB",
                "redis-cli --raw FLUSHALL ASYNC",
            ],
            not_match_examples: &[
                "redis-cli GET mykey",
                "redis-cli --raw INFO",
                "redis-cli GET FLUSHALL",
            ],
        },
        // ── Cloud ──────────────────────────────────────────────────────────
        PrefixRule {
            id: Cow::Borrowed("CL-001"),
            category: Category::Cloud,
            pattern: vec![s("terraform"), s("destroy")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "terraform destroy — tears down all infrastructure resources in the Terraform state",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Use 'terraform plan -destroy' first to review what will be removed",
            )),
            justification: Some(Cow::Borrowed(
                "Destroys all managed infrastructure. State backups are critical; accidental destroy in the wrong workspace can delete production.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["terraform destroy -auto-approve"],
            not_match_examples: &["terraform plan"],
        },
        PrefixRule {
            id: Cow::Borrowed("CL-002"),
            category: Category::Cloud,
            pattern: vec![s("aws"), s("ec2"), s("terminate-instances")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "aws ec2 terminate-instances — permanently terminates EC2 instances and deletes their storage",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Stop instances first: 'aws ec2 stop-instances' to preserve data before terminating",
            )),
            justification: Some(Cow::Borrowed(
                "Termination is permanent. EBS volumes may be deleted depending on settings; recovery is only possible from backups.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["aws ec2 terminate-instances --instance-ids i-1234abcd"],
            not_match_examples: &["aws ec2 describe-instances"],
        },
        PrefixRule {
            id: Cow::Borrowed("CL-003"),
            category: Category::Cloud,
            pattern: vec![s("kubectl"), s("delete")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "kubectl delete — removes Kubernetes resources; some resources (PVCs) may delete persistent storage",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Use '--dry-run=client' first to preview affected resources",
            )),
            justification: Some(Cow::Borrowed(
                "Some deletions cascade and remove persistent storage. Verify the resource type and use --dry-run first.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["kubectl delete deployment my-app"],
            not_match_examples: &["kubectl get pods"],
        },
        PrefixRule {
            id: Cow::Borrowed("CL-004"),
            category: Category::Cloud,
            pattern: vec![s("pulumi"), s("destroy")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed("pulumi destroy — destroys all Pulumi stack resources"),
            safe_alt: Some(Cow::Borrowed(
                "Run 'pulumi preview --diff' to review what will be destroyed before executing",
            )),
            justification: Some(Cow::Borrowed(
                "Destroys every resource in the stack. There is no automatic recovery; you must re-import or re-create everything.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["pulumi destroy --yes"],
            not_match_examples: &["pulumi preview"],
        },
        PrefixRule {
            id: Cow::Borrowed("CL-005"),
            category: Category::Cloud,
            // Leading AnyStar admits global flags before the service token
            // (`aws --profile prod s3 rm …`, `aws --region … s3 rm …`).
            pattern: vec![
                s("aws"),
                any_star(),
                s("s3"),
                s("rm"),
                PatternToken::AnyStar,
                s("--recursive"),
            ],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "aws s3 rm --recursive — recursively deletes all objects under an S3 prefix",
            ),
            safe_alt: Some(Cow::Borrowed(
                "List objects first: 'aws s3 ls <path>' and enable versioning before bulk deletes",
            )),
            justification: Some(Cow::Borrowed(
                "Bulk deletion in S3 is fast and permanent unless versioning is enabled. There is no trash or undo.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["aws s3 rm s3://my-bucket/data --recursive"],
            not_match_examples: &["aws s3 rm s3://bucket/file.txt"],
        },
        PrefixRule {
            id: Cow::Borrowed("CL-006"),
            category: Category::Cloud,
            pattern: vec![s("aws"), s("rds"), s("delete-db-instance")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "aws rds delete-db-instance — permanently deletes an RDS database instance",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Enable deletion protection and take a final snapshot before deleting",
            )),
            justification: Some(Cow::Borrowed(
                "Deleting an RDS instance removes the instance and optionally its automated backups. Final snapshots are your only safety net.",
            )),
            source: PatternSource::Builtin,
            match_examples: &[
                "aws rds delete-db-instance --db-instance-identifier mydb --skip-final-snapshot",
            ],
            not_match_examples: &["aws rds describe-db-instances"],
        },
        PrefixRule {
            id: Cow::Borrowed("CL-007"),
            category: Category::Cloud,
            pattern: vec![s("gcloud"), s("compute"), s("instances"), s("delete")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "gcloud compute instances delete — permanently deletes GCP Compute Engine VM instances",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Create a snapshot of the boot disk before deletion for recovery",
            )),
            justification: Some(Cow::Borrowed(
                "VM deletion is permanent. Unless you kept the boot disk, the instance and its local state are gone.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["gcloud compute instances delete my-vm --zone us-east1-b"],
            not_match_examples: &["gcloud compute instances list"],
        },
        PrefixRule {
            id: Cow::Borrowed("CL-008"),
            category: Category::Cloud,
            pattern: vec![s("az"), s("vm"), s("delete")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "az vm delete — permanently deletes an Azure virtual machine",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Deallocate first: 'az vm deallocate' and capture an image before deleting",
            )),
            justification: Some(Cow::Borrowed(
                "Azure VM deletion removes the VM. Attached disks may persist, but the VM configuration is lost.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["az vm delete --name myvm --resource-group rg1 --yes"],
            not_match_examples: &["az vm list"],
        },
        PrefixRule {
            id: Cow::Borrowed("CL-009"),
            category: Category::Cloud,
            pattern: vec![
                s("aws"),
                s("iam"),
                a(&[
                    "delete-role",
                    "delete-policy",
                    "delete-user",
                    "delete-group",
                ]),
            ],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "aws iam delete — removes IAM identity or policy; can break permissions for dependent services",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Detach all policies and verify no services depend on the role before deleting",
            )),
            justification: Some(Cow::Borrowed(
                "Deleting IAM roles or policies can break running services and pipelines that depend on them. Audit dependencies first.",
            )),
            source: PatternSource::Builtin,
            match_examples: &[
                "aws iam delete-role my-service-role",
                "aws iam delete-policy my-policy",
                "aws iam delete-user my-user",
                "aws iam delete-group my-group",
            ],
            not_match_examples: &["aws iam list-roles"],
        },
        PrefixRule {
            id: Cow::Borrowed("CL-010"),
            category: Category::Cloud,
            pattern: vec![s("kubectl"), s("delete"), s("namespace")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "kubectl delete namespace — deletes a Kubernetes namespace and every resource inside it",
            ),
            safe_alt: Some(Cow::Borrowed(
                "List all resources in the namespace first: 'kubectl get all -n <ns>'",
            )),
            justification: Some(Cow::Borrowed(
                "Deletes every resource in the namespace, including secrets, config maps, and potentially persistent volumes.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["kubectl delete namespace staging"],
            not_match_examples: &["kubectl get namespace staging"],
        },
        PrefixRule {
            id: Cow::Borrowed("CL-011"),
            category: Category::Cloud,
            // Mirrors CL-005. The leading AnyStar admits global flags before the
            // service token (`aws --profile … s3 rb`, `aws --region … s3 rb`).
            pattern: vec![
                s("aws"),
                any_star(),
                s("s3"),
                s("rb"),
                any_star(),
                s("--force"),
            ],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "aws s3 rb --force — deletes an S3 bucket and every object in it, bypassing the non-empty guard",
            ),
            safe_alt: Some(Cow::Borrowed(
                "List the bucket first: 'aws s3 ls s3://<bucket>' and enable versioning before force-removing",
            )),
            justification: Some(Cow::Borrowed(
                "The --force flag empties and removes the bucket in one step. Deletion is permanent unless versioning is enabled; there is no trash or undo.",
            )),
            source: PatternSource::Builtin,
            match_examples: &[
                "aws s3 rb s3://my-bucket --force",
                "aws --profile prod s3 rb s3://my-bucket --force",
            ],
            not_match_examples: &["aws s3 rb s3://my-bucket"],
        },
        PrefixRule {
            id: Cow::Borrowed("CL-012"),
            category: Category::Cloud,
            // Leading AnyStar admits global flags before the service token
            // (`aws --profile … s3 sync`, `aws --region … s3 sync`).
            pattern: vec![
                s("aws"),
                any_star(),
                s("s3"),
                s("sync"),
                any_star(),
                s("--delete"),
            ],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "aws s3 sync --delete — removes destination objects that are absent from the source, deleting remote data",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Dry-run first: 'aws s3 sync … --delete --dryrun' and enable bucket versioning before syncing",
            )),
            justification: Some(Cow::Borrowed(
                "With --delete, files missing from the source are deleted from the destination. A wrong source path can wipe the target; versioning is your only undo.",
            )),
            source: PatternSource::Builtin,
            match_examples: &[
                "aws s3 sync ./dist s3://my-bucket --delete",
                "aws --region us-east-1 s3 sync ./dist s3://my-bucket --delete",
            ],
            not_match_examples: &["aws s3 sync ./dist s3://my-bucket"],
        },
        PrefixRule {
            id: Cow::Borrowed("CL-013"),
            category: Category::Cloud,
            // Leading AnyStar catches the idiomatic `gsutil -m rm -r gs://b`.
            pattern: vec![
                s("gsutil"),
                any_star(),
                s("rm"),
                any_star(),
                a(&["-r", "-R", "--recursive"]),
            ],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "gsutil rm -r — recursively deletes all objects under a Cloud Storage prefix (GCS twin of aws s3 rm --recursive)",
            ),
            safe_alt: Some(Cow::Borrowed(
                "List objects first: 'gsutil ls gs://<bucket>/<prefix>' and enable object versioning before recursive deletes",
            )),
            justification: Some(Cow::Borrowed(
                "Recursive deletion in GCS is fast and permanent unless object versioning is enabled. There is no trash or undo.",
            )),
            source: PatternSource::Builtin,
            match_examples: &[
                "gsutil rm -r gs://my-bucket/data",
                "gsutil -m rm -r gs://my-bucket/data",
                "gsutil rm -R gs://my-bucket/data",
            ],
            not_match_examples: &["gsutil rm gs://my-bucket/file.txt"],
        },
        PrefixRule {
            id: Cow::Borrowed("CL-014"),
            category: Category::Cloud,
            pattern: vec![
                s("gcloud"),
                any_star(),
                s("storage"),
                s("rm"),
                any_star(),
                a(&["-r", "-R", "--recursive"]),
            ],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "gcloud storage rm --recursive — recursively deletes Cloud Storage objects or buckets",
            ),
            safe_alt: Some(Cow::Borrowed(
                "List objects first: 'gcloud storage ls gs://<bucket>/<prefix>' and enable object versioning before recursive deletes",
            )),
            justification: Some(Cow::Borrowed(
                "Recursive Cloud Storage deletion can remove all objects under a prefix and may delete the bucket when aimed at a bucket URL. Versioning is the primary recovery path.",
            )),
            source: PatternSource::Builtin,
            match_examples: &[
                "gcloud storage rm -r gs://my-bucket/data",
                "gcloud storage rm --recursive gs://my-bucket",
                "gcloud --project prod storage rm gs://my-bucket --recursive",
            ],
            not_match_examples: &["gcloud storage rm gs://my-bucket/file.txt"],
        },
    ]
}
