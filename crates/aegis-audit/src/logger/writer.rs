use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use super::*;
use crate::error::AuditError;
use aegis_config::AuditConfig;

impl AuditEntry {
    /// Build a decision audit entry with all fields computed at the decision point.
    pub fn new(
        command: impl Into<String>,
        risk: RiskLevel,
        matched_patterns: Vec<MatchedPattern>,
        decision: Decision,
        snapshots: Vec<AuditSnapshot>,
        allowlist_pattern: Option<String>,
        allowlist_reason: Option<String>,
    ) -> Self {
        let command = command.into();
        let pattern_ids = matched_patterns.iter().map(|p| p.id.clone()).collect();
        Self::Decision(DecisionEntry {
            timestamp: current_timestamp(),
            sequence: next_sequence(),
            command,
            risk,
            pattern_ids,
            matched_patterns,
            decision,
            snapshots,
            explanation: None,
            mode: None,
            ci_detected: None,
            // Fresh entries explicitly record `false` so that downstream code
            // does not have to distinguish "not set" from "set to false".
            // Legacy entries deserialized with `None` are back-filled in
            // `normalize_legacy_fields` based on `allowlist_pattern` presence.
            allowlist_matched: Some(false),
            allowlist_effective: Some(false),
            chain_alg: None,
            prev_hash: None,
            entry_hash: None,
            allowlist_pattern,
            allowlist_reason,
            sandbox_status: SandboxStatus::NotConfigured,
            // Fresh entries explicitly record `false` so that downstream code
            // does not have to distinguish "not set" from "set to false", the
            // same convention used for the allowlist flags above. Builders
            // below override these when an effect-opaque command required a
            // backstop. Legacy entries deserialized with `None` stay `None`.
            effect_opaque: Some(false),
            snapshots_required: Some(false),
            confinement_required: Some(false),
            recovery_degradation: None,
        })
    }

    /// Attach the nested explanation view without altering legacy top-level fields.
    pub fn with_explanation(mut self, explanation: CommandExplanation) -> Self {
        self.as_base_mut().explanation = Some(explanation);
        self
    }

    /// Convert to a watch-mode entry, attaching frame correlation fields.
    ///
    /// If called on a `Decision` entry it is promoted to `Watch`. If already
    /// `Watch`, only the correlation fields are updated.
    pub fn with_watch_context(
        self,
        source: Option<String>,
        cwd: Option<String>,
        id: Option<String>,
    ) -> Self {
        match self {
            AuditEntry::Decision(base) => AuditEntry::Watch(WatchEntry {
                base,
                source,
                cwd,
                id,
            }),
            AuditEntry::Watch(mut w) => {
                w.source = source;
                w.cwd = cwd;
                w.id = id;
                AuditEntry::Watch(w)
            }
        }
    }

    /// Attach the evaluated policy context captured at runtime.
    pub fn with_policy_context(
        mut self,
        mode: Mode,
        ci_detected: bool,
        allowlist_matched: bool,
        allowlist_effective: bool,
    ) -> Self {
        let base = self.as_base_mut();
        base.mode = Some(mode);
        base.ci_detected = Some(ci_detected);
        base.allowlist_matched = Some(allowlist_matched);
        base.allowlist_effective = Some(allowlist_effective);
        self
    }

    /// Record whether a sandbox profile was applied, bypassed, or not
    /// configured for this execution.
    pub fn with_sandbox_status(mut self, status: SandboxStatus) -> Self {
        self.as_base_mut().sandbox_status = status;
        self
    }

    /// Record whether the assessed command was `Effect-opaque execution`
    /// (ADR-016). Orthogonal to `RiskLevel`; defaults to `false` on fresh
    /// entries built via [`AuditEntry::new`].
    pub fn with_effect_opaque(mut self, effect_opaque: bool) -> Self {
        self.as_base_mut().effect_opaque = Some(effect_opaque);
        self
    }

    /// Record the required recovery backstops for this execution (ADR-016):
    /// `snapshots_required` (the primary recovery axis) and
    /// `confinement_required` (the optional stricter sandbox tier). Both
    /// default to `false` on fresh entries built via [`AuditEntry::new`].
    pub fn with_required_backstops(
        mut self,
        snapshots_required: bool,
        confinement_required: bool,
    ) -> Self {
        let base = self.as_base_mut();
        base.snapshots_required = Some(snapshots_required);
        base.confinement_required = Some(confinement_required);
        self
    }

    /// Record why a required recovery backstop was not available (ADR-016).
    /// `None` by default; set only when a required snapshot could not be
    /// created so the degradation is a first-class, queryable audit event.
    pub fn with_recovery_degradation(mut self, degradation: RecoveryDegradation) -> Self {
        self.as_base_mut().recovery_degradation = Some(degradation);
        self
    }

    pub(super) fn normalize_legacy_fields(mut self) -> Self {
        let base = self.as_base_mut();
        if base.pattern_ids.is_empty() {
            base.pattern_ids = base.matched_patterns.iter().map(|p| p.id.clone()).collect();
        }
        // Only legacy entries (deserialized from old logs) arrive here with
        // `allowlist_matched == None`.  Fresh entries created via `new()` already
        // set these fields to `Some(false)` above.
        let allowlist_present = base.allowlist_pattern.is_some();
        if base.allowlist_matched.is_none() {
            base.allowlist_matched = Some(allowlist_present);
        }
        if base.allowlist_effective.is_none() {
            base.allowlist_effective = Some(allowlist_present);
        }
        self
    }

    pub(super) fn with_integrity_chain(
        mut self,
        prev_hash: Option<String>,
        entry_hash: String,
    ) -> Self {
        let base = self.as_base_mut();
        base.chain_alg = Some(CHAIN_ALG_SHA256.to_string());
        base.prev_hash = prev_hash;
        base.entry_hash = Some(entry_hash);
        self
    }
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new(default_audit_path())
    }
}

impl AuditLock {
    fn exclusive(path: &Path) -> Result<Self> {
        let file = open_lock_file(path, true)?;
        file.lock()?;
        Ok(Self { file })
    }

    fn shared(path: &Path) -> Result<Self> {
        let file = open_lock_file(path, false)?;
        file.lock_shared()?;
        Ok(Self { file })
    }
}

impl Drop for AuditLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

impl AuditLogger {
    /// Create a logger that writes to the given path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            rotation: None,
            integrity_mode: AuditIntegrityMode::ChainSha256,
        }
    }

    /// Override the path on an existing logger (builder pattern).
    pub fn with_path(self, path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            ..self
        }
    }

    /// Enable log rotation with the given policy.
    pub fn with_rotation(mut self, policy: AuditRotationPolicy) -> Self {
        self.rotation = Some(policy);
        self
    }

    /// Set the integrity mode for future entries.
    pub fn with_integrity_mode(mut self, mode: AuditIntegrityMode) -> Self {
        self.integrity_mode = mode;
        self
    }

    /// Build an audit logger from validated config without touching the filesystem.
    ///
    /// This eager constructor establishes the append/query contract only.
    /// Future lazy work must remain internal helper activation and must not move
    /// the append-only write path itself behind a hidden first-use lifecycle.
    pub fn from_audit_config(config: &AuditConfig) -> Self {
        let logger = Self::default().with_integrity_mode(config.integrity_mode);
        if let Some(policy) = AuditRotationPolicy::from_config(config) {
            logger.with_rotation(policy)
        } else {
            logger
        }
    }

    /// Return the configured audit log file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one entry to the audit log, acquiring the file lock first.
    ///
    /// Failures must be handled — ignoring them silently defeats integrity checking.
    #[must_use = "audit write failures must be handled — ignoring them silently defeats integrity checking"]
    pub fn append(&self, entry: AuditEntry) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            // The lock file lives inside that directory, so we must ensure the directory
            // exists before opening the lock path. This leaves a narrow race window around
            // create_dir_all before the lock is acquired, but directory creation is idempotent
            // and the append/chain-critical work still happens only after taking the lock.
            fs::create_dir_all(parent)?;
        }
        let _lock = AuditLock::exclusive(&self.lock_path())?;

        let prev_hash = self.latest_chained_hash()?;
        let entry = self.apply_integrity(entry.normalize_legacy_fields(), prev_hash)?;
        let mut serialized =
            serde_json::to_vec(&entry).map_err(|e| AuditError::Io(std::io::Error::other(e)))?;
        serialized.push(b'\n');

        if let Some(policy) = &self.rotation {
            self.rotate_if_needed(policy, serialized.len() as u64)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        file.write_all(&serialized)?;
        file.flush()?;
        Ok(())
    }

    pub(super) fn lock_path(&self) -> PathBuf {
        let mut file_name = self
            .path
            .file_name()
            .map(|name| name.to_os_string())
            .unwrap_or_else(|| "audit.jsonl".into());
        file_name.push(".lock");

        match self.path.parent() {
            Some(parent) => parent.join(file_name),
            None => PathBuf::from(file_name),
        }
    }

    pub(super) fn acquire_shared_lock(&self) -> Result<Option<AuditLock>> {
        let lock_path = self.lock_path();
        if lock_path.parent().is_some_and(|parent| !parent.exists()) {
            return Ok(None);
        }

        AuditLock::shared(&lock_path).map(Some)
    }
}

fn default_audit_path() -> PathBuf {
    if let Some(path) = env::var_os("AEGIS_AUDIT_PATH").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }

    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .unwrap_or_else(|| ".".into());
    PathBuf::from(home).join(".aegis").join("audit.jsonl")
}

fn open_lock_file(path: &Path, create_parent: bool) -> Result<File> {
    if create_parent && let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(Into::into)
}

fn current_timestamp() -> AuditTimestamp {
    AuditTimestamp::now()
}

fn next_sequence() -> u64 {
    AUDIT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
}
