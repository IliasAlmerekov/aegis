use serde::{Deserialize, Serialize};

/// Aegis operating mode.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, schemars::JsonSchema,
)]
#[serde(rename_all = "PascalCase")]
pub enum Mode {
    /// Prompt on Warn/Danger (default).
    #[default]
    Protect,
    /// Non-blocking audit-only mode.
    Audit,
    /// Block non-safe and indirect execution by default.
    Strict,
}

/// What aegis does when it detects a CI environment.
///
/// `Block` is the safe default: no interactive TTY is available in CI, so
/// prompting would hang the pipeline.  Instead, non-safe commands are
/// hard-blocked and the pipeline fails fast with a clear error message.
///
/// `Allow` is an explicit opt-in override for cases where a project has
/// audited its CI pipeline and is confident that destructive commands are
/// intentional (e.g., a release script that runs `terraform destroy` in a
/// tear-down job).  Set this only in `.aegis.toml`, not globally.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, schemars::JsonSchema,
)]
#[serde(rename_all = "PascalCase")]
pub enum CiPolicy {
    /// Hard-block all non-safe commands. No dialog. Pipeline fails fast.
    #[default]
    Block,
    /// Pass-through: commands run without prompting. Use only when you have
    /// deliberately reviewed the CI pipeline for destructive commands.
    Allow,
}

/// Audit log integrity protection mode.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, schemars::JsonSchema,
)]
#[serde(rename_all = "PascalCase")]
pub enum AuditIntegrityMode {
    /// No integrity chaining.
    Off,
    /// Tamper-evident chained SHA-256 (default).
    #[default]
    ChainSha256,
}

/// Controls when and how snapshot plugins run before dangerous commands.
///
/// - `None`      — never snapshot.
/// - `Selective` — only plugins enabled by `auto_snapshot_git` /
///   `auto_snapshot_docker` / `auto_snapshot_postgres` /
///   `auto_snapshot_mysql` / `auto_snapshot_supabase` /
///   `auto_snapshot_sqlite`.
/// - `Full`      — run every registered plugin regardless of per-plugin flags.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, schemars::JsonSchema,
)]
#[serde(rename_all = "PascalCase")]
pub enum SnapshotPolicy {
    /// Never create snapshots.
    None,
    /// Honour per-plugin flags (default — backwards-compatible).
    #[default]
    Selective,
    /// Run all snapshot plugins unconditionally.
    Full,
}

/// Maximum override level that structured allowlist rules may grant for
/// non-safe commands in Protect/Strict mode.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, schemars::JsonSchema,
)]
#[serde(rename_all = "PascalCase")]
pub enum AllowlistOverrideLevel {
    /// Auto-approve allowlisted Warn commands (default).
    #[default]
    Warn,
    /// Also auto-approve allowlisted Danger commands.
    Danger,
    /// Disable allowlist auto-approval.
    Never,
}
