use std::borrow::Cow;

use aegis_types::RiskLevel;

use super::{Category, PatternSource, PatternToken, PrefixRule, a, any_star, s};
pub(super) fn rules() -> Vec<PrefixRule> {
    vec![
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
        PrefixRule {
            id: Cow::Borrowed("DB-001"),
            category: Category::Database,
            pattern: vec![s("drop"), s("table")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "DROP TABLE — permanently deletes a database table and all its data",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Back up the table first: 'CREATE TABLE backup AS SELECT * FROM <table>'",
            )),
            justification: Some(Cow::Borrowed(
                "Destroys the table and all data. In most engines this is immediate and irreversible without a backup.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["DROP TABLE users;"],
            not_match_examples: &["SELECT * FROM users;"],
        },
        PrefixRule {
            id: Cow::Borrowed("DB-002"),
            category: Category::Database,
            pattern: vec![s("drop"), s("database")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "DROP DATABASE — permanently destroys an entire database and all its contents",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Take a full backup with pg_dump / mysqldump before dropping",
            )),
            justification: Some(Cow::Borrowed(
                "Destroys the entire database. This removes all schemas, tables, indexes, and data permanently.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["DROP DATABASE myapp_production;"],
            not_match_examples: &["CREATE DATABASE myapp_production;"],
        },
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
            id: Cow::Borrowed("DB-007"),
            category: Category::Database,
            pattern: vec![s("drop"), s("schema")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "DROP SCHEMA — deletes an entire schema including all objects contained within it",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Back up the schema first and verify all dependent objects are accounted for",
            )),
            justification: Some(Cow::Borrowed(
                "Removes a schema and every object inside it. Dependencies on those objects will break.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["DROP SCHEMA public CASCADE;"],
            not_match_examples: &["CREATE SCHEMA public;"],
        },
        PrefixRule {
            id: Cow::Borrowed("DB-008"),
            category: Category::Database,
            pattern: vec![
                s("alter"),
                s("table"),
                PatternToken::Any,
                s("drop"),
                s("column"),
            ],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "ALTER TABLE DROP COLUMN — removes a column and all its data permanently",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Add a NOT NULL DEFAULT before removing to safely migrate dependent queries first",
            )),
            justification: Some(Cow::Borrowed(
                "Removes a column and all its data permanently. Dependent views, triggers, and queries will fail.",
            )),
            source: PatternSource::Builtin,
            match_examples: &["ALTER TABLE users DROP COLUMN avatar;"],
            not_match_examples: &["ALTER TABLE users ADD COLUMN avatar;"],
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
            pattern: vec![
                s("aws"),
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
    ]
}
