// Pattern struct, Category, loading

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::UserPattern;
use crate::error::AegisError;
use crate::interceptor::RiskLevel;

/// Whether a pattern was compiled into the binary or loaded from user config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternSource {
    Builtin,
    Custom,
}

/// Which class of operation the pattern guards against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum Category {
    Filesystem,
    Git,
    Database,
    Cloud,
    Docker,
    Process,
    Package,
}

// ── Token-prefix rule types (live alongside regex-based Pattern) ──────────

/// A token-level prefix rule that matches the beginning of a tokenized command.
///
/// Replaces free-form regex for commands whose dangerous semantics are fully
/// captured by a fixed prefix of tokens (e.g. `git push --force`).
#[derive(Debug, Clone)]
pub struct PrefixRule {
    pub id: Cow<'static, str>,
    pub category: Category,
    pub pattern: PrefixPattern,
    pub risk: RiskLevel,
    pub description: Cow<'static, str>,
    pub safe_alt: Option<Cow<'static, str>>,
    pub justification: Option<Cow<'static, str>>,
    pub source: PatternSource,
}

/// A sequence of pattern tokens to match against command tokens.
pub type PrefixPattern = Vec<PatternToken>;

/// One position in a prefix pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternToken {
    /// Exact single token match.
    Single(Cow<'static, str>),
    /// One of several alternative tokens.
    Alts(Vec<Cow<'static, str>>),
    /// Matches exactly one arbitrary token (like `.` in regex).
    Any,
    /// Matches zero or more arbitrary tokens (like `.*` in regex).
    AnyStar,
}

/// Unified runtime pattern.
///
/// Both built-in and user-defined patterns are normalized into the same
/// `Cow<'static, str>`-backed runtime representation.
///
/// This type can carry either borrowed static strings or owned runtime
/// strings, allowing scanner consumers to operate on one normalized shape
/// without depending on how a given pattern was materialized.
#[derive(Debug, Clone)]
pub struct Pattern {
    pub id: Cow<'static, str>,
    pub category: Category,
    pub risk: RiskLevel,
    pub pattern: Cow<'static, str>,
    pub description: Cow<'static, str>,
    pub safe_alt: Option<Cow<'static, str>>,
    pub source: PatternSource,
}

/// Internal helper: TOML-deserializable representation before conversion to [`Pattern`].
#[derive(Debug, Deserialize)]
struct RawPattern {
    id: String,
    category: Category,
    risk: RiskLevel,
    pattern: String,
    description: String,
    safe_alt: Option<String>,
}

impl From<RawPattern> for Pattern {
    fn from(raw: RawPattern) -> Self {
        Pattern {
            id: Cow::Owned(raw.id),
            category: raw.category,
            risk: raw.risk,
            pattern: Cow::Owned(raw.pattern),
            description: Cow::Owned(raw.description),
            safe_alt: raw.safe_alt.map(Cow::Owned),
            source: PatternSource::Builtin,
        }
    }
}

impl From<UserPattern> for Pattern {
    fn from(user: UserPattern) -> Self {
        Pattern {
            id: Cow::Owned(user.id),
            category: user.category,
            risk: user.risk,
            pattern: Cow::Owned(user.pattern),
            description: Cow::Owned(user.description),
            safe_alt: user.safe_alt.map(Cow::Owned),
            source: PatternSource::Custom,
        }
    }
}

/// Wrapper for TOML top-level table: `[[patterns]]`.
#[derive(Debug, Deserialize)]
struct PatternsFile {
    patterns: Vec<RawPattern>,
}

/// Effective merged pattern set consumed when constructing a scanner.
///
/// This is the authoritative runtime view after combining the built-in
/// patterns embedded in the binary with any custom patterns supplied by the
/// resolved config layers.
#[derive(Debug)]
pub struct PatternSet {
    patterns: Vec<Arc<Pattern>>,
    prefix_rules: Vec<Arc<PrefixRule>>,
}

/// Built-in patterns embedded at compile time — binary stays self-contained.
const BUILTIN_PATTERNS_TOML: &str = include_str!("../../config/patterns.toml");

impl PatternSet {
    /// Parse and return the canonical built-in-only pattern set.
    ///
    /// This loads the embedded `config/patterns.toml` without any config
    /// overlays, providing the built-in source of truth before custom patterns
    /// are merged for runtime scanner construction.
    pub fn load() -> Result<PatternSet, AegisError> {
        Self::from_sources(&[])
    }

    /// Build the authoritative merged pattern view for scanner construction.
    ///
    /// Merge order is fixed and explicit:
    /// 1) built-in patterns embedded in the binary
    /// 2) user-defined patterns loaded from config
    ///
    /// The returned set is the effective runtime input consumed by
    /// `Scanner::new`, after validation and normalization into one `Pattern`
    /// representation.
    pub fn from_sources(custom_patterns: &[UserPattern]) -> Result<PatternSet, AegisError> {
        let file: PatternsFile = toml::from_str(BUILTIN_PATTERNS_TOML)
            .map_err(|e| AegisError::Config(format!("failed to parse patterns.toml: {e}")))?;

        // 1) built-in
        let builtin_patterns: Vec<Pattern> = file.patterns.into_iter().map(Pattern::from).collect();

        // 2) custom (already merged global+project in config layer)
        let custom_patterns: Vec<Pattern> =
            custom_patterns.iter().cloned().map(Pattern::from).collect();

        // 3) normalize to one structure (`Pattern`) happened via `From` conversions above.
        // 4) validate unified set (required fields + duplicate IDs forbidden for regex patterns).
        let mut pattern_ids: HashSet<String> =
            HashSet::with_capacity(builtin_patterns.len() + custom_patterns.len());
        let mut patterns: Vec<Arc<Pattern>> =
            Vec::with_capacity(builtin_patterns.len() + custom_patterns.len());

        for pattern in builtin_patterns
            .into_iter()
            .chain(custom_patterns.into_iter())
        {
            Self::validate_pattern(&pattern, &mut pattern_ids)?;
            patterns.push(Arc::new(pattern));
        }

        // 5) compile built-in prefix rules.
        let prefix_rules = builtin_prefix_rules();

        // 6) validate prefix rules: required fields + no conflict with regex pattern IDs.
        //    Duplicate IDs within prefix rules are intentional: the same logical rule can
        //    have multiple syntactic forms (e.g. "docker-compose" vs "docker compose").
        for rule in &prefix_rules {
            Self::validate_prefix_rule(rule, &pattern_ids)?;
        }

        let prefix_rules: Vec<Arc<PrefixRule>> = prefix_rules.into_iter().map(Arc::new).collect();

        // 7) compiled into runtime PatternSet.
        Ok(PatternSet {
            patterns,
            prefix_rules,
        })
    }

    /// Return the effective merged regex pattern set consumed by scanner construction.
    pub fn patterns(&self) -> &[Arc<Pattern>] {
        self.patterns.as_slice()
    }

    /// Return the effective merged prefix-rule set consumed by scanner construction.
    pub fn prefix_rules(&self) -> &[Arc<PrefixRule>] {
        self.prefix_rules.as_slice()
    }

    fn validate_pattern(pattern: &Pattern, ids: &mut HashSet<String>) -> Result<(), AegisError> {
        if pattern.id.trim().is_empty() {
            return Err(AegisError::Config(format!(
                "invalid pattern id: empty id (source={:?})",
                pattern.source
            )));
        }

        if pattern.pattern.trim().is_empty() {
            return Err(AegisError::Config(format!(
                "invalid pattern {}: empty regex pattern",
                pattern.id
            )));
        }

        if pattern.description.trim().is_empty() {
            return Err(AegisError::Config(format!(
                "invalid pattern {}: empty description",
                pattern.id
            )));
        }

        let id = pattern.id.as_ref();
        if !ids.insert(id.to_string()) {
            return Err(AegisError::Config(format!(
                "duplicate pattern id '{id}' is not allowed"
            )));
        }

        Ok(())
    }

    fn validate_prefix_rule(
        rule: &PrefixRule,
        pattern_ids: &HashSet<String>,
    ) -> Result<(), AegisError> {
        if rule.id.trim().is_empty() {
            return Err(AegisError::Config(format!(
                "invalid prefix rule id: empty id (source={:?})",
                rule.source
            )));
        }

        if rule.pattern.is_empty() {
            return Err(AegisError::Config(format!(
                "invalid prefix rule {}: empty pattern",
                rule.id
            )));
        }

        if rule.description.trim().is_empty() {
            return Err(AegisError::Config(format!(
                "invalid prefix rule {}: empty description",
                rule.id
            )));
        }

        // Prevent a prefix rule from shadowing a regex pattern with the same ID.
        let id = rule.id.as_ref();
        if pattern_ids.contains(id) {
            return Err(AegisError::Config(format!(
                "prefix rule id '{id}' conflicts with an existing regex pattern id"
            )));
        }

        Ok(())
    }
}

// ── Built-in prefix rules (replaces regex for token-prefixable commands) ───

fn s(s: &'static str) -> PatternToken {
    PatternToken::Single(Cow::Borrowed(s))
}

fn a(alts: &'static [&'static str]) -> PatternToken {
    PatternToken::Alts(alts.iter().map(|&s| Cow::Borrowed(s)).collect())
}

fn any_star() -> PatternToken {
    PatternToken::AnyStar
}

/// Built-in token-prefix rules embedded at compile time.
fn builtin_prefix_rules() -> Vec<PrefixRule> {
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
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
            justification: None,
            source: PatternSource::Builtin,
        },
        // ── Docker ────────────────────────────────────────────────────────────
        PrefixRule {
            id: Cow::Borrowed("DK-001"),
            category: Category::Docker,
            pattern: vec![s("docker"), s("system"), s("prune")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "docker system prune — removes all stopped containers, dangling images, unused networks, and build cache",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Use '--filter until=24h' to limit pruning to older resources only",
            )),
            justification: None,
            source: PatternSource::Builtin,
        },
        PrefixRule {
            id: Cow::Borrowed("DK-002"),
            category: Category::Docker,
            pattern: vec![s("docker"), s("volume"), s("prune")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "docker volume prune — removes all unused Docker volumes including any persistent data they hold",
            ),
            safe_alt: Some(Cow::Borrowed(
                "List volumes first: 'docker volume ls' and back up data before pruning",
            )),
            justification: None,
            source: PatternSource::Builtin,
        },
        PrefixRule {
            id: Cow::Borrowed("DK-003"),
            category: Category::Docker,
            pattern: vec![
                s("docker-compose"),
                s("down"),
                PatternToken::AnyStar,
                s("-v"),
            ],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "docker-compose down -v — stops services and removes named volumes, deleting persistent data",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Omit '-v' to keep volumes: 'docker-compose down' preserves volume data",
            )),
            justification: None,
            source: PatternSource::Builtin,
        },
        PrefixRule {
            id: Cow::Borrowed("DK-003"),
            category: Category::Docker,
            pattern: vec![
                s("docker"),
                s("compose"),
                s("down"),
                PatternToken::AnyStar,
                s("-v"),
            ],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "docker compose down -v — stops services and removes named volumes, deleting persistent data",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Omit '-v' to keep volumes: 'docker compose down' preserves volume data",
            )),
            justification: None,
            source: PatternSource::Builtin,
        },
        PrefixRule {
            id: Cow::Borrowed("DK-004"),
            category: Category::Docker,
            pattern: vec![s("docker"), s("rmi")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "docker rmi — removes Docker images; rebuild time is lost if image is deleted unintentionally",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Tag images you want to keep before running bulk rmi commands",
            )),
            justification: None,
            source: PatternSource::Builtin,
        },
        PrefixRule {
            id: Cow::Borrowed("DK-005"),
            category: Category::Docker,
            pattern: vec![s("docker"), s("container"), s("prune")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "docker container prune — removes all stopped containers, including those with useful logs or data",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Inspect stopped containers first: 'docker ps -a' before pruning",
            )),
            justification: None,
            source: PatternSource::Builtin,
        },
        PrefixRule {
            id: Cow::Borrowed("DK-006"),
            category: Category::Docker,
            pattern: vec![s("docker"), s("network"), s("prune")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "docker network prune — removes all unused Docker networks; can break containers that reconnect",
            ),
            safe_alt: Some(Cow::Borrowed(
                "List networks in use: 'docker network ls' before pruning",
            )),
            justification: None,
            source: PatternSource::Builtin,
        },
        // ── Process ───────────────────────────────────────────────────────────
        PrefixRule {
            id: Cow::Borrowed("PS-001"),
            category: Category::Process,
            pattern: vec![
                s("kill"),
                PatternToken::Alts(vec![
                    Cow::Borrowed("-9"),
                    Cow::Borrowed("-KILL"),
                    Cow::Borrowed("-SIGKILL"),
                ]),
                s("1"),
            ],
            risk: RiskLevel::Block,
            description: Cow::Borrowed(
                "kill -9 1 — sends SIGKILL to PID 1 (init/systemd), immediately crashing the entire system",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Use 'systemctl stop <service>' to stop individual services safely",
            )),
            justification: None,
            source: PatternSource::Builtin,
        },
        PrefixRule {
            id: Cow::Borrowed("PS-002"),
            category: Category::Process,
            pattern: vec![s("pkill"), PatternToken::AnyStar, s("-9")],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "pkill -9 — sends SIGKILL to all matching processes with no chance for cleanup or graceful shutdown",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Use 'pkill -15' (SIGTERM) first to allow graceful shutdown before escalating",
            )),
            justification: None,
            source: PatternSource::Builtin,
        },
        PrefixRule {
            id: Cow::Borrowed("PS-005"),
            category: Category::Process,
            pattern: vec![s("chmod"), PatternToken::AnyStar, s("777"), s("/")],
            risk: RiskLevel::Danger,
            description: Cow::Borrowed(
                "chmod 777 / — makes the root filesystem world-writable, creating a severe security vulnerability",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Apply permissions only to the specific directory that needs them",
            )),
            justification: None,
            source: PatternSource::Builtin,
        },
        // ── Package ───────────────────────────────────────────────────────────
        PrefixRule {
            id: Cow::Borrowed("PKG-005"),
            category: Category::Package,
            pattern: vec![
                s("pip"),
                s("install"),
                PatternToken::AnyStar,
                s("--trusted-host"),
            ],
            risk: RiskLevel::Warn,
            description: Cow::Borrowed(
                "pip install --trusted-host — disables SSL verification for the specified host",
            ),
            safe_alt: Some(Cow::Borrowed(
                "Fix the SSL/TLS issue instead of bypassing verification; use a proper certificate",
            )),
            justification: None,
            source: PatternSource::Builtin,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::UserPattern;

    #[test]
    fn load_builtin_patterns_parses_without_error() {
        let set = PatternSet::load().expect("patterns.toml should parse cleanly");
        let total = set.patterns().len() + set.prefix_rules().len();
        assert!(
            total >= 50,
            "expected at least 50 total rules, got {} patterns + {} prefix rules",
            set.patterns().len(),
            set.prefix_rules().len()
        );
    }

    #[test]
    fn all_categories_represented() {
        let set = PatternSet::load().unwrap();
        let mut categories: std::collections::HashSet<_> =
            set.patterns().iter().map(|p| p.category).collect();
        // Prefix rules (e.g. Git) are part of PatternSet and contribute their own categories.
        for rule in set.prefix_rules() {
            categories.insert(rule.category);
        }
        assert!(categories.contains(&Category::Filesystem));
        assert!(categories.contains(&Category::Git));
        assert!(categories.contains(&Category::Database));
        assert!(categories.contains(&Category::Cloud));
        assert!(categories.contains(&Category::Docker));
        assert!(categories.contains(&Category::Process));
        assert!(categories.contains(&Category::Package));
    }

    #[test]
    fn all_patterns_have_non_empty_fields() {
        let set = PatternSet::load().unwrap();
        for p in set.patterns() {
            assert!(!p.id.is_empty(), "empty id");
            assert!(!p.pattern.is_empty(), "empty pattern for {}", p.id);
            assert!(!p.description.is_empty(), "empty description for {}", p.id);
        }
    }

    #[test]
    fn from_sources_merges_builtin_and_custom_and_marks_custom_source() {
        let custom = UserPattern {
            id: "USR-999".to_string(),
            category: Category::Cloud,
            risk: RiskLevel::Warn,
            pattern: r"internal-teardown".to_string(),
            description: "Internal teardown guard".to_string(),
            safe_alt: Some("internal-teardown --dry-run".to_string()),
        };

        let set = PatternSet::from_sources(&[custom]).expect("custom pattern set should compile");

        let matched = set
            .patterns()
            .iter()
            .find(|p| p.id.as_ref() == "USR-999")
            .expect("custom pattern id should be present");

        assert_eq!(matched.source, PatternSource::Custom);
    }

    #[test]
    fn from_sources_rejects_duplicate_ids_between_builtin_and_custom() {
        let duplicate = UserPattern {
            id: "FS-001".to_string(),
            category: Category::Filesystem,
            risk: RiskLevel::Warn,
            pattern: r"dummy-pattern".to_string(),
            description: "dummy".to_string(),
            safe_alt: None,
        };

        let err = PatternSet::from_sources(&[duplicate]).expect_err("duplicate id must fail");
        assert!(err.to_string().contains("duplicate pattern id 'FS-001'"));
    }

    #[test]
    fn from_sources_rejects_duplicate_ids_inside_custom_patterns() {
        let first = UserPattern {
            id: "USR-DUP".to_string(),
            category: Category::Cloud,
            risk: RiskLevel::Warn,
            pattern: r"first".to_string(),
            description: "first".to_string(),
            safe_alt: None,
        };
        let second = UserPattern {
            id: "USR-DUP".to_string(),
            category: Category::Cloud,
            risk: RiskLevel::Danger,
            pattern: r"second".to_string(),
            description: "second".to_string(),
            safe_alt: None,
        };

        let err = PatternSet::from_sources(&[first, second]).expect_err("duplicate id must fail");
        assert!(err.to_string().contains("duplicate pattern id 'USR-DUP'"));
    }
}
