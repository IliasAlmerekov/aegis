use std::path::Path;
#[cfg(not(windows))]
use std::process::Command;
use std::sync::Arc;

use time::OffsetDateTime;
use tokio::runtime::{Builder, Runtime};

use crate::audit::{AuditEntry, AuditLogger, AuditRotationPolicy, Decision};
use crate::config::{Allowlist, AllowlistContext, AllowlistMatch, AllowlistOverrideLevel, Config};
use crate::error::AegisError;
use crate::interceptor;
use crate::interceptor::scanner::{Assessment, Scanner};
use crate::snapshot::{SnapshotRecord, SnapshotRegistry};

/// Internal runtime view of the effective policy configuration.
///
/// This is intentionally separate from the user-facing config model so the
/// CLI entrypoints can read the values they need without exposing config
/// serialization details.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeConfig {
    /// Effective operating mode.
    pub mode: crate::config::Mode,
    /// Effective CI policy.
    pub ci_policy: crate::config::CiPolicy,
    /// Effective Protect/Strict allowlist ceiling for non-safe commands.
    pub strict_allowlist_override: AllowlistOverrideLevel,
}

impl From<&Config> for RuntimeConfig {
    fn from(config: &Config) -> Self {
        Self {
            mode: config.mode,
            ci_policy: config.ci_policy,
            strict_allowlist_override: config.allowlist_override_level,
        }
    }
}

/// Shared runtime dependencies built once per CLI invocation.
pub struct RuntimeContext {
    runtime_config: RuntimeConfig,
    allowlist: Allowlist,
    current_user: Option<String>,
    scanner: Arc<Scanner>,
    snapshot_registry: SnapshotRegistry,
    snapshot_runtime: SnapshotRuntime,
    audit_logger: AuditLogger,
}

enum SnapshotRuntime {
    Ready(Runtime),
    Unavailable(String),
}

impl RuntimeContext {
    /// Load config, build runtime dependencies once, and keep them consistent.
    pub fn load(_verbose: bool) -> Result<Self, AegisError> {
        Config::load().and_then(Self::new)
    }

    /// Build a runtime context from an already resolved config.
    pub fn new(config: Config) -> Result<Self, AegisError> {
        config.validate_runtime_requirements()?;
        let scanner = interceptor::scanner_for(&config.custom_patterns)?;
        let current_user = detect_effective_user();

        let snapshot_runtime = match Builder::new_current_thread().enable_all().build() {
            Ok(runtime) => SnapshotRuntime::Ready(runtime),
            Err(err) => SnapshotRuntime::Unavailable(err.to_string()),
        };

        Ok(Self {
            allowlist: Allowlist::new(&config.layered_allowlist_rules())?,
            snapshot_registry: SnapshotRegistry::from_config(&config),
            snapshot_runtime,
            audit_logger: build_audit_logger(&config),
            current_user,
            runtime_config: RuntimeConfig::from(&config),
            scanner,
        })
    }

    /// Return the effective config used by all runtime subsystems.
    pub fn config(&self) -> &RuntimeConfig {
        &self.runtime_config
    }

    /// Assess a command with the context-bound scanner.
    pub fn assess(&self, cmd: &str) -> Assessment {
        self.scanner.assess(cmd)
    }

    /// Return the effective user identity captured for this runtime context.
    pub fn current_user(&self) -> Option<&str> {
        self.current_user.as_deref()
    }

    /// Resolve the allowlist rule, if any, that matches the runtime context.
    pub fn allowlist_match(&self, context: &AllowlistContext<'_>) -> Option<AllowlistMatch> {
        self.allowlist.match_reason(context)
    }

    /// Resolve the matching allowlist rule for one command using the runtime user.
    pub fn allowlist_match_for_command(
        &self,
        command: &str,
        cwd: Option<&Path>,
    ) -> Option<AllowlistMatch> {
        let now = OffsetDateTime::now_utc();
        let context = AllowlistContext::with_optional_scope(command, cwd, self.current_user(), now);

        self.allowlist_match(&context)
    }

    /// Create best-effort snapshots using the context-bound registry/runtime.
    pub fn create_snapshots(&self, cwd: &Path, cmd: &str, verbose: bool) -> Vec<SnapshotRecord> {
        match &self.snapshot_runtime {
            SnapshotRuntime::Ready(runtime) => {
                runtime.block_on(self.snapshot_registry.snapshot_all(cwd, cmd))
            }
            SnapshotRuntime::Unavailable(err) => {
                if verbose {
                    eprintln!("warning: failed to initialize snapshot runtime: {err}");
                }

                Vec::new()
            }
        }
    }

    /// Async variant of `create_snapshots` — call from within an async runtime.
    ///
    /// Calls `snapshot_registry.snapshot_all()` directly without `block_on`,
    /// which would panic if called from an already-async context.
    pub async fn create_snapshots_async(
        &self,
        cwd: &std::path::Path,
        cmd: &str,
    ) -> Vec<crate::snapshot::SnapshotRecord> {
        self.snapshot_registry.snapshot_all(cwd, cmd).await
    }

    /// Append one audit entry with the context-bound logger configuration.
    pub fn append_audit_entry(
        &self,
        assessment: &Assessment,
        decision: Decision,
        snapshots: &[SnapshotRecord],
        allowlist_match: Option<&AllowlistMatch>,
        allowlist_effective: bool,
        verbose: bool,
    ) {
        let allowlist_pattern = (allowlist_effective)
            .then(|| allowlist_match.map(|m| m.pattern.clone()))
            .flatten();
        let allowlist_reason = (allowlist_effective)
            .then(|| allowlist_match.map(|m| m.reason.clone()))
            .flatten();

        let entry = AuditEntry::new(
            assessment.command.raw.clone(),
            assessment.risk,
            assessment.matched.iter().map(Into::into).collect(),
            decision,
            snapshots.iter().map(Into::into).collect(),
            allowlist_pattern,
            allowlist_reason,
        );

        if let Err(err) = self.audit_logger.append(entry)
            && verbose
        {
            eprintln!("warning: failed to append audit log entry: {err}");
        }
    }

    /// Append a watch-mode audit entry with frame correlation fields.
    ///
    /// Identical to `append_audit_entry` but attaches `source`, `cwd`, `id`,
    /// and sets `transport = "watch"` via `AuditEntry::with_watch_context`.
    #[allow(clippy::too_many_arguments)]
    pub fn append_watch_audit_entry(
        &self,
        assessment: &Assessment,
        decision: Decision,
        snapshots: &[SnapshotRecord],
        allowlist_match: Option<&AllowlistMatch>,
        allowlist_effective: bool,
        watch_source: Option<String>,
        watch_cwd: Option<String>,
        watch_id: Option<String>,
        verbose: bool,
    ) {
        let allowlist_pattern = (allowlist_effective)
            .then(|| allowlist_match.map(|m| m.pattern.clone()))
            .flatten();
        let allowlist_reason = (allowlist_effective)
            .then(|| allowlist_match.map(|m| m.reason.clone()))
            .flatten();

        let entry = AuditEntry::new(
            assessment.command.raw.clone(),
            assessment.risk,
            assessment.matched.iter().map(Into::into).collect(),
            decision,
            snapshots.iter().map(Into::into).collect(),
            allowlist_pattern,
            allowlist_reason,
        )
        .with_watch_context(watch_source, watch_cwd, watch_id);

        if let Err(err) = self.audit_logger.append(entry)
            && verbose
        {
            eprintln!("warning: failed to append watch audit log entry: {err}");
        }
    }
}

fn build_audit_logger(config: &Config) -> AuditLogger {
    if let Some(policy) = AuditRotationPolicy::from_config(&config.audit) {
        AuditLogger::default().with_rotation(policy)
    } else {
        AuditLogger::default()
    }
}

fn detect_effective_user() -> Option<String> {
    #[cfg(not(windows))]
    {
        detect_effective_user_from_id_command(Path::new("/usr/bin/id"))
    }

    #[cfg(windows)]
    {
        None
    }
}

#[cfg(not(windows))]
fn detect_effective_user_from_id_command(id_path: &Path) -> Option<String> {
    if !id_path.is_absolute() {
        return None;
    }

    let output = Command::new(id_path).arg("-un").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let user = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!user.is_empty()).then_some(user)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::config::{CiPolicy, UserPattern};
    use crate::interceptor::RiskLevel;
    use crate::interceptor::patterns::Category;
    use tempfile::TempDir;
    use time::OffsetDateTime;

    #[test]
    fn custom_patterns_are_built_once_into_runtime_scanner() {
        let mut config = Config::default();
        config.custom_patterns = vec![UserPattern {
            id: "USR-CTX-001".to_string(),
            category: Category::Process,
            risk: RiskLevel::Warn,
            pattern: "echo hello".to_string(),
            description: "custom warning".to_string(),
            safe_alt: None,
        }];

        let context = RuntimeContext::new(config).unwrap();
        let assessment = context.assess("echo hello");

        assert_eq!(assessment.risk, RiskLevel::Warn);
        assert_eq!(assessment.matched.len(), 1);
        assert_eq!(assessment.matched[0].pattern.id.as_ref(), "USR-CTX-001");
    }

    #[test]
    fn invalid_custom_scanner_aborts_runtime_context_construction() {
        let mut config = Config::default();
        config.custom_patterns = vec![UserPattern {
            id: "FS-001".to_string(),
            category: Category::Filesystem,
            risk: RiskLevel::Warn,
            pattern: "echo hello".to_string(),
            description: "duplicate id".to_string(),
            safe_alt: None,
        }];

        let err = match RuntimeContext::new(config) {
            Ok(_) => panic!("invalid custom patterns must abort runtime context construction"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("duplicate pattern id"));
    }

    #[test]
    fn config_is_shared_across_runtime_dependencies() {
        use crate::config::AllowlistRule;

        let mut config = Config::default();
        config.allowlist_override_level = AllowlistOverrideLevel::Danger;
        config.allowlist = vec![AllowlistRule {
            pattern: "echo trusted".to_string(),
            cwd: None,
            user: None,
            expires_at: None,
            reason: "runtime test".to_string(),
        }];
        config.auto_snapshot_git = false;
        config.auto_snapshot_docker = false;
        config.ci_policy = CiPolicy::Allow;

        let context = RuntimeContext::new(config.clone()).unwrap();

        assert_eq!(context.config().mode, config.mode);
        assert_eq!(context.config().ci_policy, config.ci_policy);
        assert_eq!(
            context.config().strict_allowlist_override,
            AllowlistOverrideLevel::Danger
        );
        let Some(current_user) = context.current_user() else {
            panic!("test requires a resolvable user");
        };
        let allowlist_ctx =
            AllowlistContext::new("echo trusted", Path::new("."), current_user, now_utc());
        assert_eq!(
            context.allowlist_match(&allowlist_ctx).map(|m| m.pattern),
            Some("echo trusted".to_string())
        );
        assert!(
            context
                .create_snapshots(Path::new("."), "rm -rf /tmp/runtime-context-test", false)
                .is_empty()
        );
        assert_eq!(context.config().ci_policy, CiPolicy::Allow);
        assert_eq!(
            context.config().strict_allowlist_override,
            AllowlistOverrideLevel::Danger
        );
    }

    #[test]
    fn runtime_context_rejects_expired_allowlist_rules() {
        use crate::config::AllowlistRule;
        use time::{OffsetDateTime, format_description::well_known::Rfc3339};

        let mut config = Config::default();
        config.allowlist = vec![AllowlistRule {
            pattern: "terraform destroy -target=module.test.*".to_string(),
            cwd: None,
            user: None,
            expires_at: Some(OffsetDateTime::parse("2020-01-01T00:00:00Z", &Rfc3339).unwrap()),
            reason: "expired teardown".to_string(),
        }];

        let err = match RuntimeContext::new(config) {
            Ok(_) => panic!("expired allowlist rules must be rejected before runtime setup"),
            Err(err) => err,
        };

        assert!(err.to_string().contains("expired"));
    }

    #[test]
    fn runtime_context_accepts_scoped_allowlist_rules() {
        use crate::config::AllowlistRule;

        let mut config = Config::default();
        config.allowlist = vec![AllowlistRule {
            pattern: "terraform destroy -target=module.test.*".to_string(),
            cwd: Some("/srv/infra".to_string()),
            user: None,
            expires_at: None,
            reason: "scoped teardown".to_string(),
        }];

        let context = RuntimeContext::new(config).unwrap();
        let allowlist_ctx = AllowlistContext::with_optional_scope(
            "terraform destroy -target=module.test.api",
            Some(Path::new("/srv/infra")),
            context.current_user(),
            now_utc(),
        );

        assert!(context.allowlist_match(&allowlist_ctx).is_some());
    }

    #[test]
    fn runtime_context_accepts_user_scoped_allowlist_rules() {
        use crate::config::AllowlistRule;

        let mut config = Config::default();
        let Some(current_user) = detect_effective_user() else {
            return;
        };
        config.allowlist = vec![AllowlistRule {
            pattern: "terraform destroy -target=module.test.*".to_string(),
            cwd: None,
            user: Some(current_user.clone()),
            expires_at: None,
            reason: "scoped teardown".to_string(),
        }];

        let context = RuntimeContext::new(config).unwrap();
        let Some(current_user) = context.current_user() else {
            panic!("test requires a resolvable user");
        };
        let allowlist_ctx = AllowlistContext::new(
            "terraform destroy -target=module.test.api",
            Path::new("/srv/infra"),
            current_user,
            now_utc(),
        );

        assert!(context.allowlist_match(&allowlist_ctx).is_some());
    }

    #[test]
    fn unknown_user_does_not_match_user_scoped_allowlist_rule() {
        use crate::config::AllowlistRule;

        let mut config = Config::default();
        config.allowlist = vec![AllowlistRule {
            pattern: "terraform destroy -target=module.test.*".to_string(),
            cwd: None,
            user: Some("ci".to_string()),
            expires_at: None,
            reason: "user scoped teardown".to_string(),
        }];

        let mut context = RuntimeContext::new(config).unwrap();
        context.current_user = None;

        assert!(
            context
                .allowlist_match_for_command(
                    "terraform destroy -target=module.test.api",
                    Some(Path::new("/srv/infra")),
                )
                .is_none()
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn detect_effective_user_rejects_relative_id_command_path() {
        assert!(detect_effective_user_from_id_command(Path::new("id")).is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn detect_effective_user_reads_name_from_absolute_id_command_path() {
        let detected = detect_effective_user_from_id_command(Path::new("/usr/bin/id"));
        assert!(detected.is_some());
    }

    #[test]
    fn unknown_cwd_does_not_match_cwd_scoped_allowlist_rule() {
        use crate::config::AllowlistRule;

        let mut config = Config::default();
        config.allowlist = vec![AllowlistRule {
            pattern: "terraform destroy -target=module.test.*".to_string(),
            cwd: Some("/srv/infra".to_string()),
            user: None,
            expires_at: None,
            reason: "scoped teardown".to_string(),
        }];

        let context = RuntimeContext::new(config).unwrap();

        assert!(
            context
                .allowlist_match_for_command("terraform destroy -target=module.test.api", None,)
                .is_none()
        );
    }

    #[test]
    fn load_for_preserves_project_allowlist_precedence_into_runtime_matching() {
        let workspace = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let global_dir = home.path().join(".config/aegis");
        fs::create_dir_all(&global_dir).unwrap();

        fs::write(
            global_dir.join("config.toml"),
            r#"
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
reason = "global teardown"
expires_at = "2030-01-01T00:00:00Z"
"#,
        )
        .unwrap();
        fs::write(
            workspace.path().join(".aegis.toml"),
            r#"
[[allowlist]]
pattern = "terraform destroy -target=module.test.*"
reason = "project teardown"
expires_at = "2030-01-01T00:00:00Z"
"#,
        )
        .unwrap();

        let config = Config::load_for(workspace.path(), Some(home.path())).unwrap();
        let context = RuntimeContext::new(config).unwrap();
        let matched = context
            .allowlist_match_for_command(
                "terraform destroy -target=module.test.api",
                Some(workspace.path()),
            )
            .unwrap();

        assert_eq!(matched.reason, "project teardown");
        assert_eq!(
            matched.source_layer,
            crate::config::AllowlistSourceLayer::Project
        );
    }

    fn now_utc() -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}
