//! Policy-configuration enums shared between the config layer and the policy engine.
//!
//! These describe *how* decisions are made (operating mode, CI behaviour,
//! snapshot policy, allowlist override ceiling). They are pure data — config
//! deserializes them and the policy engine consumes them — so they live here, at
//! the bottom of the dependency DAG, rather than in either crate.

use serde::{Deserialize, Serialize};

/// Decision produced by a typed `[[rules]]` policy entry in `aegis.toml`.
///
/// Serializes and deserializes as a snake_case string (`"allow"`, `"prompt"`,
/// `"block"`), but also accepts an internally-tagged map `{ decision = "allow" }`
/// when deserialized from a bare TOML document (used in unit tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq, schemars::JsonSchema)]
pub enum PolicyRuleDecision {
    /// Auto-approve the command without prompting.
    Allow,
    /// Show an interactive confirmation prompt.
    Prompt,
    /// Hard-block the command without prompting.
    Block,
}

impl serde::Serialize for PolicyRuleDecision {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let s = match self {
            Self::Allow => "allow",
            Self::Prompt => "prompt",
            Self::Block => "block",
        };
        serializer.serialize_str(s)
    }
}

impl<'de> serde::Deserialize<'de> for PolicyRuleDecision {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        struct PolicyRuleDecisionVisitor;

        impl<'de> Visitor<'de> for PolicyRuleDecisionVisitor {
            type Value = PolicyRuleDecision;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(
                    r#"one of "allow", "prompt", "block" or a map with a "decision" key"#,
                )
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<PolicyRuleDecision, E> {
                match value {
                    "allow" => Ok(PolicyRuleDecision::Allow),
                    "prompt" => Ok(PolicyRuleDecision::Prompt),
                    "block" => Ok(PolicyRuleDecision::Block),
                    other => Err(E::unknown_variant(other, &["allow", "prompt", "block"])),
                }
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<PolicyRuleDecision, A::Error> {
                let mut decision: Option<String> = None;
                while let Some(key) = map.next_key::<String>()? {
                    if key == "decision" {
                        decision = Some(map.next_value()?);
                    } else {
                        map.next_value::<serde::de::IgnoredAny>()?;
                    }
                }
                match decision.as_deref() {
                    Some("allow") => Ok(PolicyRuleDecision::Allow),
                    Some("prompt") => Ok(PolicyRuleDecision::Prompt),
                    Some("block") => Ok(PolicyRuleDecision::Block),
                    Some(other) => Err(<A::Error as de::Error>::unknown_variant(
                        other,
                        &["allow", "prompt", "block"],
                    )),
                    None => Err(<A::Error as de::Error>::missing_field("decision")),
                }
            }
        }

        deserializer.deserialize_any(PolicyRuleDecisionVisitor)
    }
}

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
/// tear-down job).  Because a project-local `.aegis.toml` is untrusted, `Allow`
/// is only honored from the trusted global config
/// (`~/.config/aegis/config.toml`); a project cannot weaken an inherited
/// `Block` to `Allow` (see ADR-013).
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

/// Controls when and how Snapshot plugins run for planned recovery.
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
