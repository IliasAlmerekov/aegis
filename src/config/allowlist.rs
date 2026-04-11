use std::path::Path;

use regex::Regex;
use time::OffsetDateTime;

use crate::config::AllowlistRule;
use crate::error::AegisError;

type Result<T> = std::result::Result<T, AegisError>;

/// Configuration layer that supplied an allowlist rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowlistSourceLayer {
    /// Rule loaded from the global config file.
    Global,
    /// Rule loaded from the project-local config file.
    Project,
}

/// Runtime context used to evaluate scoped allowlist rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllowlistContext<'a> {
    /// Raw command string about to be evaluated.
    pub command: &'a str,
    /// Working directory for the command execution, when it could be resolved.
    pub cwd: Option<&'a Path>,
    /// Effective user running Aegis, when it could be resolved reliably.
    pub user: Option<&'a str>,
    /// Current time used for expiry evaluation.
    pub now: OffsetDateTime,
}

impl<'a> AllowlistContext<'a> {
    /// Create a new allowlist matching context.
    pub fn new(command: &'a str, cwd: &'a Path, user: &'a str, now: OffsetDateTime) -> Self {
        Self::with_optional_scope(command, Some(cwd), Some(user), now)
    }

    /// Create a new allowlist matching context with optional cwd and user scope.
    pub fn with_optional_scope(
        command: &'a str,
        cwd: Option<&'a Path>,
        user: Option<&'a str>,
        now: OffsetDateTime,
    ) -> Self {
        Self {
            command,
            cwd,
            user,
            now,
        }
    }

    /// Create a new allowlist matching context when cwd resolution failed.
    pub fn without_cwd(command: &'a str, user: Option<&'a str>, now: OffsetDateTime) -> Self {
        Self::with_optional_scope(command, None, user, now)
    }

    /// Return a copy of this context with a different user.
    pub fn with_user(self, user: &'a str) -> Self {
        Self {
            user: Some(user),
            ..self
        }
    }

    /// Return a copy of this context without a resolved user.
    pub fn without_user(self) -> Self {
        Self { user: None, ..self }
    }
}

/// Structured allowlist rule paired with the config layer that defined it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayeredAllowlistRule {
    /// Original config rule.
    pub rule: AllowlistRule,
    /// Source config layer for precedence decisions.
    pub source_layer: AllowlistSourceLayer,
}

impl LayeredAllowlistRule {
    /// Create a rule sourced from the global config layer.
    pub fn global(rule: AllowlistRule) -> Self {
        Self {
            rule,
            source_layer: AllowlistSourceLayer::Global,
        }
    }

    /// Create a rule sourced from the project config layer.
    pub fn project(rule: AllowlistRule) -> Self {
        Self {
            rule,
            source_layer: AllowlistSourceLayer::Project,
        }
    }
}

impl From<AllowlistRule> for LayeredAllowlistRule {
    fn from(rule: AllowlistRule) -> Self {
        Self::project(rule)
    }
}

/// The allowlist entry that caused a command to be trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowlistMatch {
    /// The original glob pattern from the config that matched.
    pub pattern: String,
    /// Operator-facing justification attached to the rule.
    pub reason: String,
    /// Config layer that supplied the winning rule.
    pub source_layer: AllowlistSourceLayer,
}

/// Advisory warning produced by allowlist quality analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowlistWarning {
    /// Stable machine-readable warning code.
    pub code: &'static str,
    /// Human-readable explanation.
    pub message: String,
    /// Best-effort source location string for the rule.
    pub location: String,
}

/// Compiled allowlist matcher for trusted command strings.
#[derive(Debug, Clone, Default)]
pub struct Allowlist {
    project_entries: Vec<CompiledAllowlistRule>,
    global_entries: Vec<CompiledAllowlistRule>,
}

#[derive(Debug, Clone)]
struct CompiledAllowlistRule {
    pattern: String,
    cwd: Option<String>,
    user: Option<String>,
    expires_at: Option<OffsetDateTime>,
    reason: String,
    source_layer: AllowlistSourceLayer,
    regex: Regex,
}

impl Allowlist {
    /// Compile layered allowlist rules into a contextual matcher.
    pub fn new<T>(rules: &[T]) -> Result<Self>
    where
        T: Clone + Into<LayeredAllowlistRule>,
    {
        let mut project_entries = Vec::new();
        let mut global_entries = Vec::new();

        for rule in rules.iter().cloned().map(Into::into) {
            let compiled = compile_rule(rule)?;
            match compiled.source_layer {
                AllowlistSourceLayer::Project => project_entries.push(compiled),
                AllowlistSourceLayer::Global => global_entries.push(compiled),
            }
        }

        Ok(Self {
            project_entries,
            global_entries,
        })
    }

    /// Returns the first effective allowlist entry for the current context.
    ///
    /// Effective means the pattern matches, any optional `cwd` / `user` scope
    /// matches, and the rule is not expired for `context.now`.
    pub fn match_reason(&self, context: &AllowlistContext<'_>) -> Option<AllowlistMatch> {
        self.project_entries
            .iter()
            .chain(self.global_entries.iter())
            .find(|entry| entry.is_effective(context))
            .map(|entry| AllowlistMatch {
                pattern: entry.pattern.clone(),
                reason: entry.reason.clone(),
                source_layer: entry.source_layer,
            })
    }

    /// Returns `true` when any effective allowlist entry matches the context.
    pub fn is_allowed(&self, context: &AllowlistContext<'_>) -> bool {
        self.match_reason(context).is_some()
    }
}

/// Produce advisory warnings for one structured allowlist rule.
pub fn analyze_allowlist_rule(rule: &AllowlistRule) -> Vec<AllowlistWarning> {
    let mut warnings = Vec::new();
    let location = warning_location(rule);

    if !has_scope(rule.cwd.as_deref()) && !has_scope(rule.user.as_deref()) {
        warnings.push(AllowlistWarning {
            code: "missing_scope",
            message: "allowlist rule has no cwd or user scope".to_string(),
            location: location.clone(),
        });
    }

    if is_broad_pattern(rule.pattern.trim()) {
        warnings.push(AllowlistWarning {
            code: "broad_pattern",
            message: "allowlist rule uses wildcard matching that may be broader than intended"
                .to_string(),
            location,
        });
    }

    warnings
}

impl CompiledAllowlistRule {
    fn is_effective(&self, context: &AllowlistContext<'_>) -> bool {
        self.matches_pattern(context.command)
            && self.matches_cwd(context.cwd)
            && self.matches_user(context.user)
            && !self.is_expired(context.now)
    }

    fn matches_pattern(&self, command: &str) -> bool {
        self.regex.is_match(command.trim())
    }

    fn matches_cwd(&self, cwd: Option<&Path>) -> bool {
        match self.cwd.as_deref() {
            Some(rule_cwd) => cwd.is_some_and(|cwd| Path::new(rule_cwd) == cwd),
            None => true,
        }
    }

    fn matches_user(&self, user: Option<&str>) -> bool {
        match self.user.as_deref() {
            Some(rule_user) => user.is_some_and(|user| rule_user == user),
            None => true,
        }
    }

    fn is_expired(&self, now: OffsetDateTime) -> bool {
        self.expires_at.is_some_and(|expires_at| expires_at <= now)
    }
}

fn compile_rule(rule: LayeredAllowlistRule) -> Result<CompiledAllowlistRule> {
    let pattern = required_field("pattern", &rule.rule.pattern)?;
    let reason = required_field("reason", &rule.rule.reason)?;
    let cwd = optional_scope_field("cwd", rule.rule.cwd.as_deref())?;
    let user = optional_scope_field("user", rule.rule.user.as_deref())?;

    let regex = Regex::new(&glob_to_regex(pattern)).map_err(|error| {
        AegisError::Config(format!(
            "invalid allowlist rule pattern {:?}: {error}",
            rule.rule.pattern
        ))
    })?;

    Ok(CompiledAllowlistRule {
        pattern: pattern.to_string(),
        cwd,
        user,
        expires_at: rule.rule.expires_at,
        reason: reason.to_string(),
        source_layer: rule.source_layer,
        regex,
    })
}

fn required_field<'a>(field: &str, value: &'a str) -> Result<&'a str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AegisError::Config(format!(
            "allowlist rule {field} must not be empty"
        )));
    }

    Ok(trimmed)
}

fn optional_scope_field(field: &str, value: Option<&str>) -> Result<Option<String>> {
    match value {
        Some(value) => Ok(Some(required_field(field, value)?.to_string())),
        None => Ok(None),
    }
}

fn has_scope(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn is_broad_pattern(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?')
}

fn warning_location(rule: &AllowlistRule) -> String {
    format!("allowlist:{:?}", rule.pattern.trim())
}

fn glob_to_regex(pattern: &str) -> String {
    let mut regex = String::from("^");

    for ch in pattern.chars() {
        match ch {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '[' | ']' | '\\' => {
                regex.push('\\');
                regex.push(ch);
            }
            _ => regex.push(ch),
        }
    }

    regex.push('$');
    regex
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use time::{Duration, OffsetDateTime};

    use super::{
        Allowlist, AllowlistContext, AllowlistMatch, AllowlistSourceLayer, LayeredAllowlistRule,
        analyze_allowlist_rule,
    };
    use crate::config::AllowlistRule;

    #[test]
    fn exact_pattern_matches_only_the_same_command() {
        let allowlist = Allowlist::new(&[rule("docker system prune --volumes", "exact")]).unwrap();

        assert!(allowlist.is_allowed(&ctx("docker system prune --volumes")));
        assert!(!allowlist.is_allowed(&ctx("docker system prune")));
    }

    #[test]
    fn glob_pattern_matches_specific_target_family() {
        let allowlist = Allowlist::new(&[rule(
            "terraform destroy -target=module.test.*",
            "scoped target family",
        )])
        .unwrap();

        assert!(allowlist.is_allowed(&ctx("terraform destroy -target=module.test.api")));
        assert!(allowlist.is_allowed(&ctx("terraform destroy -target=module.test.api.blue")));
        assert!(!allowlist.is_allowed(&ctx("terraform destroy -target=module.prod.api")));
    }

    #[test]
    fn match_reason_returns_none_when_no_pattern_matches() {
        let allowlist = Allowlist::new(&[rule("docker system prune --volumes", "exact")]).unwrap();
        assert_eq!(allowlist.match_reason(&ctx("docker system prune")), None);
    }

    #[test]
    fn match_reason_returns_matched_pattern_text() {
        let allowlist = Allowlist::new(&[
            rule(
                "terraform destroy -target=module.test.*",
                "terraform test teardown",
            ),
            rule("docker system prune --volumes", "docker cleanup"),
        ])
        .unwrap();

        assert_eq!(
            allowlist.match_reason(&ctx("terraform destroy -target=module.test.api")),
            Some(AllowlistMatch {
                pattern: "terraform destroy -target=module.test.*".to_string(),
                reason: "terraform test teardown".to_string(),
                source_layer: AllowlistSourceLayer::Project,
            })
        );

        assert_eq!(
            allowlist.match_reason(&ctx("docker system prune --volumes")),
            Some(AllowlistMatch {
                pattern: "docker system prune --volumes".to_string(),
                reason: "docker cleanup".to_string(),
                source_layer: AllowlistSourceLayer::Project,
            })
        );
    }

    #[test]
    fn project_layer_beats_global_layer_when_both_match() {
        let allowlist = Allowlist::new(&[
            LayeredAllowlistRule::global(rule("terraform destroy *", "global")),
            LayeredAllowlistRule::project(rule("terraform destroy *", "project")),
        ])
        .unwrap();

        let matched = allowlist
            .match_reason(&ctx("terraform destroy -target=module.test.api"))
            .unwrap();
        assert_eq!(matched.reason, "project");
        assert_eq!(matched.source_layer, AllowlistSourceLayer::Project);
    }

    #[test]
    fn first_declared_rule_wins_within_same_layer() {
        let allowlist = Allowlist::new(&[
            LayeredAllowlistRule::project(rule("terraform destroy *", "first")),
            LayeredAllowlistRule::project(rule("terraform destroy *", "second")),
        ])
        .unwrap();

        let matched = allowlist
            .match_reason(&ctx("terraform destroy -target=module.test.api"))
            .unwrap();
        assert_eq!(matched.reason, "first");
    }

    #[test]
    fn match_requires_scope_to_fit_context() {
        let allowlist = Allowlist::new(&[AllowlistRule {
            pattern: "terraform destroy -target=module.test.*".to_string(),
            cwd: Some("/srv/infra".to_string()),
            user: Some("ci".to_string()),
            expires_at: None,
            reason: "test teardown".to_string(),
        }])
        .unwrap();

        let ctx = AllowlistContext::new(
            "terraform destroy -target=module.test.api",
            Path::new("/srv/infra"),
            "ci",
            now_utc(),
        );

        assert!(allowlist.match_reason(&ctx).is_some());
        assert!(allowlist.match_reason(&ctx.with_user("alice")).is_none());
    }

    #[test]
    fn expired_rule_is_not_effective_for_matching() {
        let allowlist = Allowlist::new(&[AllowlistRule {
            pattern: "terraform destroy -target=module.test.*".to_string(),
            cwd: None,
            user: None,
            expires_at: Some(now_utc() - Duration::minutes(1)),
            reason: "expired teardown".to_string(),
        }])
        .unwrap();

        assert!(
            allowlist
                .match_reason(&ctx("terraform destroy -target=module.test.api"))
                .is_none()
        );
    }

    #[test]
    fn warning_flags_broad_rule_without_scope() {
        let warnings = analyze_allowlist_rule(&AllowlistRule {
            pattern: "terraform destroy *".to_string(),
            cwd: None,
            user: None,
            expires_at: None,
            reason: "broad teardown".to_string(),
        });

        assert!(warnings.iter().any(|w| w.code == "missing_scope"));
        assert!(warnings.iter().any(|w| w.code == "broad_pattern"));
    }

    fn rule(pattern: &str, reason: &str) -> AllowlistRule {
        AllowlistRule {
            pattern: pattern.to_string(),
            cwd: None,
            user: None,
            expires_at: None,
            reason: reason.to_string(),
        }
    }

    fn ctx(command: &str) -> AllowlistContext<'_> {
        AllowlistContext::new(command, Path::new("/srv/infra"), "ci", now_utc())
    }

    fn now_utc() -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}
