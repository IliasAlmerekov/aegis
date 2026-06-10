//! Red tests for the `aegis-explanation` public API.
//!
//! These tests fail until `CommandExplanation`, `PolicyExplanation`,
//! `ScanExplanation`, and `ExecutionContextExplanation` are extracted from
//! `src/explanation/templates.rs` and re-exported from this crate.

/// `CommandExplanation` must be the top-level explanation type exported from
/// this crate.  It contains scan, policy, context, and optional outcome.
#[test]
fn test_command_explanation_is_accessible() {
    use aegis_explanation::CommandExplanation;
    let _: Option<CommandExplanation> = None;
}

/// `PolicyExplanation` must be publicly exported from this crate.
#[test]
fn test_policy_explanation_is_accessible() {
    use aegis_explanation::PolicyExplanation;
    let _: Option<PolicyExplanation> = None;
}

/// `ScanExplanation` must be publicly exported from this crate.
#[test]
fn test_scan_explanation_is_accessible() {
    use aegis_explanation::ScanExplanation;
    let _: Option<ScanExplanation> = None;
}

/// `ExecutionContextExplanation` must be publicly exported from this crate.
#[test]
fn test_execution_context_explanation_is_accessible() {
    use aegis_explanation::ExecutionContextExplanation;
    let _: Option<ExecutionContextExplanation> = None;
}
