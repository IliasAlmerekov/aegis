pub(crate) mod nested;
pub mod parser;
pub mod patterns;
pub mod scanner;

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, LazyLock, Mutex};

use serde::{Deserialize, Serialize};

use crate::config::UserPattern;
use crate::error::AegisError;

static BUILTIN_SCANNER: LazyLock<Result<Arc<scanner::Scanner>, String>> = LazyLock::new(|| {
    patterns::PatternSet::load()
        .map(scanner::Scanner::new)
        .map(Arc::new)
        .map_err(|e| e.to_string())
});
static CUSTOM_SCANNER_CACHE: LazyLock<Mutex<HashMap<String, Arc<scanner::Scanner>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Classifies the risk level of a shell command.
///
/// Ordered by severity: `Safe < Warn < Danger < Block`.
///
/// `#[non_exhaustive]` ensures match arms in external crates require a wildcard,
/// preserving forward-compatibility if new levels are added in v2.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum RiskLevel {
    Safe,
    Warn,
    Danger,
    Block,
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            RiskLevel::Safe => "safe",
            RiskLevel::Warn => "warn",
            RiskLevel::Danger => "danger",
            RiskLevel::Block => "block",
        };

        f.write_str(value)
    }
}

impl FromStr for RiskLevel {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "safe" => Ok(RiskLevel::Safe),
            "warn" => Ok(RiskLevel::Warn),
            "danger" => Ok(RiskLevel::Danger),
            "block" => Ok(RiskLevel::Block),
            other => Err(format!(
                "invalid risk level '{other}', expected one of: safe, warn, danger, block"
            )),
        }
    }
}

/// Assess a command with the loaded interceptor rules.
pub fn assess(cmd: &str) -> Result<scanner::Assessment, AegisError> {
    Ok(builtin_scanner()?.assess(cmd))
}

/// Assess a command with built-in + user-defined custom patterns from config.
pub fn assess_with_custom_patterns(
    cmd: &str,
    custom_patterns: &[UserPattern],
) -> Result<scanner::Assessment, AegisError> {
    Ok(scanner_for(custom_patterns)?.assess(cmd))
}

/// Resolve the effective scanner for the provided custom-pattern set.
///
/// The built-in scanner stays cached globally, while custom pattern sets are
/// cached by their serialized content so runtime wiring can build a single
/// config-consistent scanner and reuse it across the command flow.
pub fn scanner_for(custom_patterns: &[UserPattern]) -> Result<Arc<scanner::Scanner>, AegisError> {
    if custom_patterns.is_empty() {
        return builtin_scanner();
    }

    let key = CacheKey::new(custom_patterns)?;
    if let Some(scanner) = get_cached_custom_scanner(&key.0)? {
        return Ok(scanner);
    }

    let scanner =
        Arc::new(patterns::PatternSet::from_sources(custom_patterns).map(scanner::Scanner::new)?);
    cache_custom_scanner(key.0, Arc::clone(&scanner))?;
    Ok(scanner)
}

fn builtin_scanner() -> Result<Arc<scanner::Scanner>, AegisError> {
    match &*BUILTIN_SCANNER {
        Ok(scanner) => Ok(Arc::clone(scanner)),
        Err(message) => Err(AegisError::Config(message.clone())),
    }
}

fn get_cached_custom_scanner(key: &str) -> Result<Option<Arc<scanner::Scanner>>, AegisError> {
    let cache = CUSTOM_SCANNER_CACHE
        .lock()
        .map_err(|_| AegisError::Config("custom scanner cache lock poisoned".to_string()))?;
    Ok(cache.get(key).cloned())
}

fn cache_custom_scanner(key: String, scanner: Arc<scanner::Scanner>) -> Result<(), AegisError> {
    let mut cache = CUSTOM_SCANNER_CACHE
        .lock()
        .map_err(|_| AegisError::Config("custom scanner cache lock poisoned".to_string()))?;
    cache.insert(key, scanner);
    Ok(())
}

/// Validated cache key for a custom pattern set.
///
/// Fields are joined with U+001F (Unit Separator) and records with U+001E
/// (Record Separator). Construction fails if any field contains these
/// characters, preventing key collisions from ambiguous serialisations.
#[derive(Debug)]
struct CacheKey(String);

impl CacheKey {
    const FIELD_SEP: char = '\u{1f}';
    const RECORD_SEP: char = '\u{1e}';

    fn new(custom_patterns: &[UserPattern]) -> std::result::Result<Self, AegisError> {
        let mut key = String::new();
        for pattern in custom_patterns {
            let category_str = format!("{:?}", pattern.category);
            let risk_str = pattern.risk.to_string();
            let safe_alt = pattern.safe_alt.as_deref().unwrap_or("");
            let justification = pattern.justification.as_deref().unwrap_or("");
            let fields: [&str; 7] = [
                &pattern.id,
                &category_str,
                &risk_str,
                &pattern.pattern,
                &pattern.description,
                safe_alt,
                justification,
            ];
            for field in fields {
                if field.contains(Self::FIELD_SEP) || field.contains(Self::RECORD_SEP) {
                    return Err(AegisError::Config(format!(
                        "custom pattern field contains reserved separator character \
                         (U+001E or U+001F) which would corrupt the scanner cache key: {field:?}"
                    )));
                }
                key.push_str(field);
                key.push(Self::FIELD_SEP);
            }
            key.push(Self::RECORD_SEP);
        }
        Ok(Self(key))
    }
}

#[cfg(test)]
mod tests {
    use super::{CacheKey, RiskLevel, assess, assess_with_custom_patterns};
    use crate::config::UserPattern;
    use crate::interceptor::patterns::{Category, PatternSource};

    #[test]
    fn safe_is_less_than_warn() {
        assert!(RiskLevel::Safe < RiskLevel::Warn);
    }

    #[test]
    fn warn_is_less_than_danger() {
        assert!(RiskLevel::Warn < RiskLevel::Danger);
    }

    #[test]
    fn danger_is_less_than_block() {
        assert!(RiskLevel::Danger < RiskLevel::Block);
    }

    #[test]
    fn assess_reports_safe_for_benign_command() {
        assert_eq!(assess("echo hello world").unwrap().risk, RiskLevel::Safe);
    }

    #[test]
    fn cache_key_rejects_field_containing_unit_separator() {
        let bad = UserPattern {
            id: "USR-001\u{1f}injected".to_string(),
            category: Category::Cloud,
            risk: RiskLevel::Warn,
            pattern: "test".to_string(),
            description: "test".to_string(),
            safe_alt: None,
            justification: None,
        };
        let err = CacheKey::new(&[bad]).unwrap_err();
        assert!(
            err.to_string().contains("reserved separator character"),
            "expected separator error, got: {err}"
        );
    }

    #[test]
    fn cache_key_rejects_field_containing_record_separator() {
        let bad = UserPattern {
            id: "USR-001".to_string(),
            category: Category::Cloud,
            risk: RiskLevel::Warn,
            pattern: "test\u{1e}injected".to_string(),
            description: "test".to_string(),
            safe_alt: None,
            justification: None,
        };
        let err = CacheKey::new(&[bad]).unwrap_err();
        assert!(
            err.to_string().contains("reserved separator character"),
            "expected separator error, got: {err}"
        );
    }

    #[test]
    fn cache_key_succeeds_for_normal_patterns() {
        let ok = UserPattern {
            id: "USR-001".to_string(),
            category: Category::Cloud,
            risk: RiskLevel::Warn,
            pattern: "internal-teardown".to_string(),
            description: "test".to_string(),
            safe_alt: None,
            justification: None,
        };
        assert!(CacheKey::new(&[ok]).is_ok());
    }

    #[test]
    fn assess_with_custom_patterns_uses_the_effective_merged_pattern_set() {
        let custom = UserPattern {
            id: "USR-REG-001".to_string(),
            category: Category::Cloud,
            risk: RiskLevel::Warn,
            pattern: "internal-teardown".to_string(),
            description: "Internal teardown guard".to_string(),
            safe_alt: Some("internal-teardown --dry-run".to_string()),
            justification: None,
        };

        let assessment =
            assess_with_custom_patterns("internal-teardown && rm -rf /tmp/demo", &[custom])
                .unwrap();

        assert_eq!(assessment.risk, RiskLevel::Danger);
        assert!(
            assessment
                .matched
                .iter()
                .any(|matched| matched.pattern.source == PatternSource::Custom)
        );
        assert!(
            assessment
                .matched
                .iter()
                .any(|matched| matched.pattern.source == PatternSource::Builtin)
        );
    }

    #[test]
    fn cache_key_includes_justification_in_serialization() {
        let with_justification = UserPattern {
            id: "USR-001".to_string(),
            category: Category::Cloud,
            risk: RiskLevel::Warn,
            pattern: "test".to_string(),
            description: "test".to_string(),
            safe_alt: None,
            justification: Some("because".to_string()),
        };
        let without_justification = UserPattern {
            id: "USR-001".to_string(),
            category: Category::Cloud,
            risk: RiskLevel::Warn,
            pattern: "test".to_string(),
            description: "test".to_string(),
            safe_alt: None,
            justification: None,
        };
        let key_with = CacheKey::new(&[with_justification]).unwrap();
        let key_without = CacheKey::new(&[without_justification]).unwrap();
        assert_ne!(
            key_with.0, key_without.0,
            "CacheKey must differentiate patterns with and without justification"
        );
        assert!(key_with.0.contains("because"));
    }
}
