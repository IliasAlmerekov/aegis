use std::path::Path;
use std::sync::Arc;

use tokio::runtime::{Builder, Runtime};

use crate::audit::{AuditEntry, AuditLogger, AuditRotationPolicy, Decision};
use crate::config::{Allowlist, AllowlistMatch, Config};
use crate::error::AegisError;
use crate::interceptor;
use crate::interceptor::RiskLevel;
use crate::interceptor::parser::Parser as CommandParser;
use crate::interceptor::scanner::{Assessment, Scanner};
use crate::snapshot::{SnapshotRecord, SnapshotRegistry};

/// Shared runtime dependencies built once per CLI invocation.
pub struct RuntimeContext {
    config: Config,
    allowlist: Allowlist,
    scanner: RuntimeScanner,
    snapshot_registry: SnapshotRegistry,
    snapshot_runtime: SnapshotRuntime,
    audit_logger: AuditLogger,
}

enum RuntimeScanner {
    Ready(Arc<Scanner>),
    Unhealthy(String),
}

enum SnapshotRuntime {
    Ready(Runtime),
    Unavailable(String),
}

impl RuntimeContext {
    /// Load config, build runtime dependencies once, and keep them consistent.
    pub fn load(_verbose: bool) -> Result<Self, AegisError> {
        Config::load().map(Self::new)
    }

    /// Build a runtime context from an already resolved config.
    pub fn new(config: Config) -> Self {
        let scanner = match interceptor::scanner_for(&config.custom_patterns) {
            Ok(scanner) => RuntimeScanner::Ready(scanner),
            Err(err) => RuntimeScanner::Unhealthy(err.to_string()),
        };

        let snapshot_runtime = match Builder::new_current_thread().enable_all().build() {
            Ok(runtime) => SnapshotRuntime::Ready(runtime),
            Err(err) => SnapshotRuntime::Unavailable(err.to_string()),
        };

        Self {
            allowlist: Allowlist::new(&config.allowlist),
            snapshot_registry: SnapshotRegistry::from_config(&config),
            snapshot_runtime,
            audit_logger: build_audit_logger(&config),
            config,
            scanner,
        }
    }

    /// Return the effective config used by all runtime subsystems.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Assess a command with the context-bound scanner.
    pub fn assess(&self, cmd: &str) -> Assessment {
        match &self.scanner {
            RuntimeScanner::Ready(scanner) => scanner.assess(cmd),
            RuntimeScanner::Unhealthy(err) => {
                eprintln!("error: interceptor scan initialization failed: {err}");
                eprintln!(
                    "error: scanner is unhealthy — requiring explicit approval for every command"
                );

                Assessment {
                    risk: RiskLevel::Warn,
                    matched: Vec::new(),
                    command: CommandParser::parse(cmd),
                }
            }
        }
    }

    /// Resolve the allowlist rule, if any, that matches `cmd`.
    pub fn allowlist_match(&self, cmd: &str) -> Option<AllowlistMatch> {
        self.allowlist.match_reason(cmd)
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
        verbose: bool,
    ) {
        let entry = AuditEntry::new(
            assessment.command.raw.clone(),
            assessment.risk,
            assessment.matched.iter().map(Into::into).collect(),
            decision,
            snapshots.iter().map(Into::into).collect(),
            allowlist_match.map(|m| m.pattern.clone()),
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
        watch_source: Option<String>,
        watch_cwd: Option<String>,
        watch_id: Option<String>,
        verbose: bool,
    ) {
        let entry = AuditEntry::new(
            assessment.command.raw.clone(),
            assessment.risk,
            assessment.matched.iter().map(Into::into).collect(),
            decision,
            snapshots.iter().map(Into::into).collect(),
            allowlist_match.map(|m| m.pattern.clone()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CiPolicy, UserPattern};
    use crate::interceptor::patterns::Category;

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

        let context = RuntimeContext::new(config);
        let assessment = context.assess("echo hello");

        assert_eq!(assessment.risk, RiskLevel::Warn);
        assert_eq!(assessment.matched.len(), 1);
        assert_eq!(assessment.matched[0].pattern.id.as_ref(), "USR-CTX-001");
    }

    #[test]
    fn invalid_custom_scanner_fails_closed_inside_context() {
        let mut config = Config::default();
        config.custom_patterns = vec![UserPattern {
            id: "FS-001".to_string(),
            category: Category::Filesystem,
            risk: RiskLevel::Warn,
            pattern: "echo hello".to_string(),
            description: "duplicate id".to_string(),
            safe_alt: None,
        }];

        let context = RuntimeContext::new(config);
        let assessment = context.assess("echo hello");

        assert_eq!(assessment.risk, RiskLevel::Warn);
        assert!(assessment.matched.is_empty());
    }

    #[test]
    fn config_is_shared_across_runtime_dependencies() {
        let mut config = Config::default();
        config.allowlist = vec!["echo trusted".to_string()];
        config.auto_snapshot_git = false;
        config.auto_snapshot_docker = false;
        config.ci_policy = CiPolicy::Allow;

        let context = RuntimeContext::new(config.clone());

        assert_eq!(context.config(), &config);
        assert_eq!(
            context.allowlist_match("echo trusted").map(|m| m.pattern),
            Some("echo trusted".to_string())
        );
        assert!(
            context
                .create_snapshots(Path::new("."), "rm -rf /tmp/runtime-context-test", false)
                .is_empty()
        );
        assert_eq!(context.config().ci_policy, CiPolicy::Allow);
    }
}
