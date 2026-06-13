//! Runtime context: config, scanner, allowlist, snapshot registry wiring.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use time::OffsetDateTime;
use tokio::runtime::Handle;

use crate::audit::{AuditEntry, AuditLogger, Decision};
use crate::config::{
    AegisConfig, Allowlist, AllowlistContext, AllowlistMatch, AllowlistOverrideLevel, Blocklist,
    SnapshotPolicy,
};
use crate::error::AegisError;
use crate::explanation::CommandExplanation;
use crate::explanation::formatter::{CommandExplanationExt, build_outcome_explanation};
use crate::interceptor;
use crate::interceptor::scanner::{Assessment, Scanner};
use crate::snapshot::{SnapshotRecord, SnapshotRegistry, SnapshotRegistryConfig};
use aegis_starlark::load_starlark_policy;

use super::user::detect_effective_user;

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
    /// Effective snapshot policy.
    pub snapshot_policy: SnapshotPolicy,
}

impl From<&AegisConfig> for RuntimeConfig {
    fn from(config: &AegisConfig) -> Self {
        Self {
            mode: config.mode,
            ci_policy: config.ci_policy,
            strict_allowlist_override: config.allowlist_override_level,
            snapshot_policy: config.snapshot_policy,
        }
    }
}

/// Shared runtime dependencies built once per CLI invocation.
pub struct RuntimeContext {
    runtime_config: RuntimeConfig,
    allowlist: Allowlist,
    blocklist: Blocklist,
    current_user: Option<String>,
    scanner: Arc<Scanner>,
    snapshot_registry_config: SnapshotRegistryConfig,
    snapshot_registry: OnceLock<SnapshotRegistry>,
    async_handle: Handle,
    audit_logger: AuditLogger,
    /// Typed `[[rules]]` entries from the effective config.
    policy_rules: Vec<crate::config::PolicyRule>,
}

/// Options controlling how an audit entry is written.
#[derive(Clone, Copy)]
pub struct AuditWriteOptions<'a> {
    /// Matched allowlist rule, if any.
    pub allowlist_match: Option<&'a AllowlistMatch>,
    /// Whether the allowlist was effective for this command.
    pub allowlist_effective: bool,
    /// Whether CI was detected for this invocation.
    pub ci_detected: bool,
}

/// Watch-mode correlation fields attached to each audit entry in watch transport.
pub struct WatchAuditContext<'a> {
    /// Matched allowlist rule, if any.
    pub allowlist_match: Option<&'a AllowlistMatch>,
    /// Whether the allowlist was effective for this command.
    pub allowlist_effective: bool,
    /// Whether CI was detected for this invocation.
    pub ci_detected: bool,
    /// Origin label for the watch-mode source.
    pub source: Option<String>,
    /// Current working directory at the time of invocation.
    pub cwd: Option<String>,
    /// Correlation ID for tracing across watch-mode frames.
    pub id: Option<String>,
}

impl RuntimeContext {
    /// Load config, build runtime dependencies once, and keep them consistent.
    pub fn load(_verbose: bool, handle: Handle) -> Result<Self, AegisError> {
        let config = AegisConfig::load()?;
        Self::new(config, handle)
    }

    /// Build a runtime context from an already resolved config.
    pub fn new(config: AegisConfig, handle: Handle) -> Result<Self, AegisError> {
        config.validate_runtime_requirements()?;
        let scanner = interceptor::scanner_for(&config.custom_patterns)?;
        let current_user = detect_effective_user();

        // Merge TOML [[rules]] with rules from ~/.aegis/policy.star when present.
        let mut policy_rules = config.rules.clone();
        if let Some(star_path) = starlark_policy_path().filter(|p| p.exists()) {
            let star_rules = load_starlark_policy(&star_path)
                .map_err(|e| AegisError::Config(format!("policy.star: {e}")))?;
            policy_rules.extend(star_rules);
        }

        Ok(Self {
            allowlist: Allowlist::from_layered_rules(&config.layered_allowlist_rules())?,
            blocklist: Blocklist::from_layered_rules(&config.layered_blocklist_rules())?,
            snapshot_registry_config: SnapshotRegistryConfig::try_new(&config)?,
            snapshot_registry: OnceLock::new(),
            async_handle: handle,
            audit_logger: build_audit_logger(&config),
            current_user,
            runtime_config: RuntimeConfig::from(&config),
            policy_rules,
            scanner,
        })
    }

    /// Return the effective config used by all runtime subsystems.
    pub fn config(&self) -> &RuntimeConfig {
        &self.runtime_config
    }

    /// Return the typed `[[rules]]` entries from the effective config.
    pub fn policy_rules(&self) -> &[crate::config::PolicyRule] {
        &self.policy_rules
    }

    /// Assess a command with the context-bound scanner.
    pub fn assess(&self, cmd: &str) -> Assessment {
        self.scanner.assess(cmd)
    }

    /// Return the effective user identity captured for this runtime context.
    pub fn current_user(&self) -> Option<&str> {
        self.current_user.as_deref()
    }

    fn snapshot_registry(&self) -> &SnapshotRegistry {
        self.snapshot_registry
            .get_or_init(|| SnapshotRegistry::from_runtime_config(&self.snapshot_registry_config))
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

    /// Returns `true` when any effective blocklist entry matches the context.
    pub fn is_blocked(&self, context: &AllowlistContext<'_>) -> bool {
        self.blocklist.is_blocked(context)
    }

    /// Returns `true` when any effective blocklist entry matches the command.
    pub fn is_blocked_for_command(&self, command: &str, cwd: Option<&Path>) -> bool {
        let now = OffsetDateTime::now_utc();
        let context = AllowlistContext::with_optional_scope(command, cwd, self.current_user(), now);

        self.is_blocked(&context)
    }

    /// Create best-effort snapshots using the context-bound registry and the
    /// persistent async handle.
    pub fn create_snapshots(&self, cwd: &Path, cmd: &str, _verbose: bool) -> Vec<SnapshotRecord> {
        self.async_handle
            .block_on(self.snapshot_registry().snapshot_all(cwd, cmd))
    }

    /// Return the names of snapshot plugins that would be eligible for `cwd`
    /// without creating any snapshots.
    pub fn applicable_snapshot_plugins(&self, cwd: &Path) -> Vec<&'static str> {
        self.async_handle
            .block_on(self.snapshot_registry().applicable_plugins(cwd))
    }

    /// Return a reference to the persistent async handle.
    pub fn async_handle(&self) -> &Handle {
        &self.async_handle
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
        self.snapshot_registry().snapshot_all(cwd, cmd).await
    }

    /// Append one audit entry with the context-bound logger configuration.
    pub fn append_audit_entry(
        &self,
        assessment: &Assessment,
        decision: Decision,
        snapshots: &[SnapshotRecord],
        explanation: &CommandExplanation,
        options: AuditWriteOptions<'_>,
    ) -> Result<(), AegisError> {
        let entry = self.build_audit_entry(assessment, decision, snapshots, explanation, options);
        Ok(self.audit_logger.append(entry)?)
    }

    /// Append a watch-mode audit entry with frame correlation fields.
    ///
    /// Identical to `append_audit_entry` but attaches `source`, `cwd`, `id`,
    /// and sets `transport = "watch"` via `AuditEntry::with_watch_context`.
    pub fn append_watch_audit_entry(
        &self,
        assessment: &Assessment,
        decision: Decision,
        snapshots: &[SnapshotRecord],
        explanation: &CommandExplanation,
        watch: WatchAuditContext<'_>,
    ) -> Result<(), AegisError> {
        let entry = self
            .build_audit_entry(
                assessment,
                decision,
                snapshots,
                explanation,
                AuditWriteOptions {
                    allowlist_match: watch.allowlist_match,
                    allowlist_effective: watch.allowlist_effective,
                    ci_detected: watch.ci_detected,
                },
            )
            .with_watch_context(watch.source, watch.cwd, watch.id);

        Ok(self.audit_logger.append(entry)?)
    }

    fn build_audit_entry(
        &self,
        assessment: &Assessment,
        decision: Decision,
        snapshots: &[SnapshotRecord],
        explanation: &CommandExplanation,
        options: AuditWriteOptions<'_>,
    ) -> AuditEntry {
        let allowlist_pattern = (options.allowlist_effective)
            .then(|| options.allowlist_match.map(|m| m.pattern.clone()))
            .flatten();
        let allowlist_reason = (options.allowlist_effective)
            .then(|| options.allowlist_match.map(|m| m.reason.clone()))
            .flatten();

        AuditEntry::new(
            assessment.command.raw.clone(),
            assessment.risk,
            assessment.matched.iter().map(Into::into).collect(),
            decision,
            snapshots.iter().map(Into::into).collect(),
            allowlist_pattern,
            allowlist_reason,
        )
        .with_explanation(
            explanation
                .clone()
                .with_runtime_outcome(build_outcome_explanation(decision, snapshots)),
        )
        .with_policy_context(
            self.runtime_config.mode,
            options.ci_detected,
            options.allowlist_match.is_some(),
            options.allowlist_effective,
        )
    }
}

fn build_audit_logger(config: &AegisConfig) -> AuditLogger {
    AuditLogger::from_audit_config(&config.audit)
}

/// Resolve `~/.aegis/policy.star`, returning `None` when `HOME` is unset.
fn starlark_policy_path() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|h| {
        std::path::PathBuf::from(h)
            .join(".aegis")
            .join("policy.star")
    })
}

#[cfg(test)]
mod tests;
