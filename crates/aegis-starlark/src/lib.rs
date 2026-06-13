#![deny(missing_docs)]
//! Starlark policy loader for Aegis.
//!
//! Exposes [`load_starlark_policy`] which evaluates a `.star` file written by
//! the user and returns the list of [`PolicyRule`] values it defines via
//! `prefix_rule(...)` calls.

use std::cell::RefCell;
use std::path::Path;

use aegis_config::model::{PolicyPatternToken, WhenClause};
use aegis_config::validate::validate_policy_rules;
use aegis_config::{PolicyRule, PolicyRuleDecision};
use starlark::any::ProvidesStaticType;
use starlark::environment::GlobalsBuilder;
use starlark::environment::Module;
use starlark::eval::Evaluator;
use starlark::syntax::{AstModule, Dialect};
use starlark::values::Value;
use starlark::values::dict::DictRef;
use starlark::values::list::ListRef;
use starlark::values::none::NoneType;
use thiserror::Error;

// ─── Execution limits ────────────────────────────────────────────────────────
//
// Policy files are user-supplied code. Without limits an erroneous or
// malicious `.star` file could hang Aegis on startup or exhaust memory.
const MAX_CALLSTACK: usize = 500;
const MAX_HEAP_BYTES: usize = 10 * 1024 * 1024; // 10 MiB
const MAX_TICK_COUNT: u64 = 100_000;

/// Errors produced by [`load_starlark_policy`].
#[derive(Debug, Error)]
pub enum StarlarkPolicyError {
    /// The `.star` file could not be read from disk.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The Starlark source contains a syntax or evaluation error.
    ///
    /// The string includes the file name and line/column from the Starlark
    /// diagnostic so users can pinpoint the offending line.
    #[error("Starlark parse error: {0}")]
    ParseError(String),

    /// A `prefix_rule` call used an unrecognised `decision` string.
    ///
    /// The string includes the file/line from the Starlark diagnostic.
    #[error("Invalid decision value: {0}")]
    InvalidDecision(String),

    /// A required field (e.g. `pattern`) was absent from a `prefix_rule` call.
    ///
    /// The string includes the file/line from the Starlark diagnostic.
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// A field had an unexpected type.
    #[error("Invalid field type: {0}")]
    InvalidFieldType(String),

    /// A rule failed semantic validation (empty pattern, bad examples, etc.).
    ///
    /// Produced by [`aegis_config::validate::validate_policy_rules`] after all
    /// rules have been collected from the `.star` file.
    #[error("Semantic validation failed for rule [{index}]: {message}")]
    Validation {
        /// Zero-based index of the failing rule.
        index: usize,
        /// Human-readable validation message.
        message: String,
    },
}

/// Tagged wrapper that carries our typed error through the `anyhow` chain.
///
/// Starlark native functions return `anyhow::Result`, so we wrap our typed
/// error in this newtype, return it as `anyhow::Error`, and recover it by
/// downcasting after evaluation.
#[derive(Debug)]
enum TypedPolicyError {
    /// Unknown decision string, with optional file-span prefix.
    InvalidDecision(String),
    /// Required field absent.
    MissingField(String),
    /// Wrong value type for a field.
    InvalidFieldType(String),
}

impl std::fmt::Display for TypedPolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDecision(s) => write!(f, "Invalid decision value: {s}"),
            Self::MissingField(s) => write!(f, "Missing required field: {s}"),
            Self::InvalidFieldType(s) => write!(f, "Invalid field type: {s}"),
        }
    }
}

impl std::error::Error for TypedPolicyError {}

/// Collector passed via `eval.extra` during evaluation.
#[derive(Debug, Default, ProvidesStaticType)]
struct RuleCollector {
    rules: RefCell<Vec<PolicyRule>>,
}

impl RuleCollector {
    fn push(&self, rule: PolicyRule) {
        self.rules.borrow_mut().push(rule);
    }
}

/// Parse a `decision` string into a [`PolicyRuleDecision`].
fn parse_decision(s: &str) -> anyhow::Result<PolicyRuleDecision> {
    match s {
        "allow" => Ok(PolicyRuleDecision::Allow),
        "prompt" => Ok(PolicyRuleDecision::Prompt),
        "block" | "deny" => Ok(PolicyRuleDecision::Block),
        other => Err(anyhow::Error::new(TypedPolicyError::InvalidDecision(
            other.to_string(),
        ))),
    }
}

/// Parse the `pattern` argument value into `Vec<PolicyPatternToken>`.
fn parse_pattern(value: Value<'_>) -> anyhow::Result<Vec<PolicyPatternToken>> {
    let outer = ListRef::from_value(value).ok_or_else(|| {
        anyhow::Error::new(TypedPolicyError::InvalidFieldType(
            "pattern must be a list".to_string(),
        ))
    })?;

    let mut tokens = Vec::new();
    for item in outer.iter() {
        if let Some(s) = item.unpack_str() {
            tokens.push(PolicyPatternToken::Single(s.to_string()));
        } else if let Some(inner) = ListRef::from_value(item) {
            let mut alts = Vec::new();
            for alt in inner.iter() {
                let s = alt.unpack_str().ok_or_else(|| {
                    anyhow::Error::new(TypedPolicyError::InvalidFieldType(
                        "alternatives inside pattern must be strings".to_string(),
                    ))
                })?;
                alts.push(s.to_string());
            }
            tokens.push(PolicyPatternToken::Alts(alts));
        } else {
            return Err(anyhow::Error::new(TypedPolicyError::InvalidFieldType(
                "pattern element must be a string or list of strings".to_string(),
            )));
        }
    }
    Ok(tokens)
}

/// Parse a list-of-strings `Value` into `Vec<String>`.
fn parse_string_list(value: Value<'_>, field: &str) -> anyhow::Result<Vec<String>> {
    let list = ListRef::from_value(value).ok_or_else(|| {
        anyhow::Error::new(TypedPolicyError::InvalidFieldType(format!(
            "{field} must be a list of strings"
        )))
    })?;
    let mut out = Vec::new();
    for item in list.iter() {
        let s = item.unpack_str().ok_or_else(|| {
            anyhow::Error::new(TypedPolicyError::InvalidFieldType(format!(
                "{field} elements must be strings"
            )))
        })?;
        out.push(s.to_string());
    }
    Ok(out)
}

/// Parse the `justification` argument — must be a string if provided.
fn parse_justification(value: Value<'_>) -> anyhow::Result<String> {
    value.unpack_str().map(str::to_string).ok_or_else(|| {
        anyhow::Error::new(TypedPolicyError::InvalidFieldType(
            "justification must be a string".to_string(),
        ))
    })
}

/// Parse a `when` dict `Value` into a [`WhenClause`].
fn parse_when(value: Value<'_>) -> anyhow::Result<WhenClause> {
    let dict = DictRef::from_value(value).ok_or_else(|| {
        anyhow::Error::new(TypedPolicyError::InvalidFieldType(
            "when must be a dict".to_string(),
        ))
    })?;

    let env = dict
        .get_str("env")
        .ok_or_else(|| anyhow::Error::new(TypedPolicyError::MissingField("when.env".to_string())))?
        .unpack_str()
        .ok_or_else(|| {
            anyhow::Error::new(TypedPolicyError::InvalidFieldType(
                "when.env must be a string".to_string(),
            ))
        })?
        .to_string();

    let value_str = dict
        .get_str("value")
        .ok_or_else(|| {
            anyhow::Error::new(TypedPolicyError::MissingField("when.value".to_string()))
        })?
        .unpack_str()
        .ok_or_else(|| {
            anyhow::Error::new(TypedPolicyError::InvalidFieldType(
                "when.value must be a string".to_string(),
            ))
        })?
        .to_string();

    let then_str = dict
        .get_str("then")
        .ok_or_else(|| anyhow::Error::new(TypedPolicyError::MissingField("when.then".to_string())))?
        .unpack_str()
        .ok_or_else(|| {
            anyhow::Error::new(TypedPolicyError::InvalidFieldType(
                "when.then must be a string".to_string(),
            ))
        })?
        .to_string();

    let then = parse_decision(&then_str)?;

    Ok(WhenClause {
        env,
        value: value_str,
        then,
    })
}

/// Starlark built-in module registering `prefix_rule`.
#[starlark::starlark_module]
fn aegis_builtins(builder: &mut GlobalsBuilder) {
    /// Declare a single typed prefix-match policy rule.
    ///
    /// May be called one or more times in a `.star` policy file to register
    /// rules that the Aegis policy engine will evaluate at runtime.
    fn prefix_rule<'v>(
        #[starlark(require = named)] pattern: Option<Value<'v>>,
        #[starlark(require = named)] decision: Option<Value<'v>>,
        #[starlark(require = named)] justification: Option<Value<'v>>,
        #[starlark(require = named)] match_examples: Option<Value<'v>>,
        #[starlark(require = named)] not_match_examples: Option<Value<'v>>,
        #[starlark(require = named)] when: Option<Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<NoneType> {
        // --- pattern (required) ---
        let pattern_val = pattern.ok_or_else(|| {
            anyhow::Error::new(TypedPolicyError::MissingField("pattern".to_string()))
        })?;
        let parsed_pattern = parse_pattern(pattern_val)?;

        // --- decision (required) ---
        let decision_val = decision.ok_or_else(|| {
            anyhow::Error::new(TypedPolicyError::MissingField("decision".to_string()))
        })?;
        let decision_str = decision_val.unpack_str().ok_or_else(|| {
            anyhow::Error::new(TypedPolicyError::InvalidFieldType(
                "decision must be a string".to_string(),
            ))
        })?;
        let parsed_decision = parse_decision(decision_str)?;

        // --- justification (optional, must be string if present) ---
        let parsed_justification = justification.map(parse_justification).transpose()?;

        // --- match_examples (optional) ---
        let parsed_match_examples = match match_examples {
            Some(v) => parse_string_list(v, "match_examples")?,
            None => Vec::new(),
        };

        // --- not_match_examples (optional) ---
        let parsed_not_match_examples = match not_match_examples {
            Some(v) => parse_string_list(v, "not_match_examples")?,
            None => Vec::new(),
        };

        // --- when (optional) ---
        let parsed_when = when.map(parse_when).transpose()?;

        let rule = PolicyRule {
            pattern: parsed_pattern,
            decision: parsed_decision,
            justification: parsed_justification,
            match_examples: parsed_match_examples,
            not_match_examples: parsed_not_match_examples,
            when: parsed_when,
        };

        // SAFETY: `eval.extra` is set unconditionally in `load_starlark_policy`
        // before evaluation begins. A missing collector is a programmer error in
        // this crate's own call stack, so we return an error rather than panic.
        let collector = eval
            .extra
            .and_then(|e| e.downcast_ref::<RuleCollector>())
            .ok_or_else(|| {
                anyhow::Error::new(TypedPolicyError::MissingField(
                    "internal: RuleCollector not available".to_string(),
                ))
            })?;

        collector.push(rule);
        Ok(NoneType)
    }
}

/// Convert a `starlark::Error` into a [`StarlarkPolicyError`].
///
/// When the error originated from our native `prefix_rule` function, the
/// `ErrorKind::Native` variant holds an `anyhow::Error` wrapping a
/// [`TypedPolicyError`]; we downcast to recover the specific variant.
///
/// The Starlark span (file name + line/column) is prepended to every message
/// so users can identify the exact location of the error in their `.star` file.
fn convert_starlark_error(err: starlark::Error) -> StarlarkPolicyError {
    use starlark::ErrorKind;

    // Capture the human-readable span (e.g. "policy.star:12:5") before
    // consuming the error — `into_kind()` drops the location info.
    let span_prefix = err.span().map(|s| format!("{s}: ")).unwrap_or_default();

    let anyhow_err = match err.into_kind() {
        ErrorKind::Native(e) => e,
        other => {
            return StarlarkPolicyError::ParseError(format!("{span_prefix}{other}"));
        }
    };

    match anyhow_err.downcast::<TypedPolicyError>() {
        Ok(typed) => match typed {
            TypedPolicyError::InvalidDecision(s) => {
                StarlarkPolicyError::InvalidDecision(format!("{span_prefix}{s}"))
            }
            TypedPolicyError::MissingField(s) => {
                StarlarkPolicyError::MissingField(format!("{span_prefix}{s}"))
            }
            TypedPolicyError::InvalidFieldType(s) => {
                StarlarkPolicyError::InvalidFieldType(format!("{span_prefix}{s}"))
            }
        },
        Err(other) => StarlarkPolicyError::ParseError(format!("{span_prefix}{other}")),
    }
}

/// Evaluate a Starlark policy file and return the rules it defines.
///
/// The file may call `prefix_rule(...)` zero or more times. Each call
/// contributes one [`PolicyRule`] to the returned `Vec`. Order is preserved.
///
/// Execution is bounded by [`MAX_CALLSTACK`], [`MAX_HEAP_BYTES`], and
/// [`MAX_TICK_COUNT`] to guard against runaway or malicious policy files.
/// After evaluation each rule is validated semantically via
/// [`validate_policy_rules`].
///
/// # Errors
///
/// - [`StarlarkPolicyError::Io`] — file cannot be opened.
/// - [`StarlarkPolicyError::ParseError`] — Starlark syntax or runtime error.
/// - [`StarlarkPolicyError::InvalidDecision`] — unknown decision string.
/// - [`StarlarkPolicyError::MissingField`] — required argument absent.
/// - [`StarlarkPolicyError::Validation`] — semantic rule validation failed.
pub fn load_starlark_policy(path: &Path) -> Result<Vec<PolicyRule>, StarlarkPolicyError> {
    let source = std::fs::read_to_string(path)?;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("policy.star")
        .to_string();

    let ast = AstModule::parse(&filename, source, &Dialect::Standard)
        .map_err(|e| StarlarkPolicyError::ParseError(e.to_string()))?;

    let globals = GlobalsBuilder::new().with(aegis_builtins).build();
    let collector = RuleCollector::default();

    let result = Module::with_temp_heap(|module| {
        let mut eval = Evaluator::new(&module);
        eval.extra = Some(&collector);

        eval.set_max_callstack_size(MAX_CALLSTACK)
            .map_err(starlark::Error::new_other)?;
        eval.set_max_heap_size(MAX_HEAP_BYTES)
            .map_err(starlark::Error::new_other)?;
        eval.set_max_tick_count(MAX_TICK_COUNT)
            .map_err(starlark::Error::new_other)?;

        eval.eval_module(ast, &globals)?;
        starlark::Result::Ok(())
    });

    if let Err(e) = result {
        return Err(convert_starlark_error(e));
    }

    let rules = collector.rules.into_inner();

    validate_policy_rules(&rules).map_err(|(index, config_err)| {
        StarlarkPolicyError::Validation {
            index,
            message: config_err.to_string(),
        }
    })?;

    Ok(rules)
}
