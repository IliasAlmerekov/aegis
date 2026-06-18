use serde::Deserialize;

use super::AllowlistRule;
use super::CURRENT_CONFIG_VERSION;

const LEGACY_ALLOWLIST_REASON: &str = "migrated from legacy allowlist entry";

pub(super) fn deserialize_allowlist_rules<'de, D>(
    deserializer: D,
) -> std::result::Result<Vec<AllowlistRule>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum AllowlistField {
        Structured(Vec<AllowlistRule>),
        Legacy(Vec<String>),
    }

    let field = Option::<AllowlistField>::deserialize(deserializer)?;
    Ok(match field {
        None => Vec::new(),
        Some(AllowlistField::Structured(rules)) => rules,
        Some(AllowlistField::Legacy(patterns)) => patterns
            .into_iter()
            .map(|pattern| AllowlistRule {
                pattern,
                cwd: None,
                user: None,
                expires_at: None,
                reason: LEGACY_ALLOWLIST_REASON.to_string(),
            })
            .collect(),
    })
}

pub(super) fn default_config_version() -> u32 {
    CURRENT_CONFIG_VERSION
}

pub(super) fn deserialize_config_version<'de, D>(
    deserializer: D,
) -> std::result::Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let version = u32::deserialize(deserializer)?;
    validate_config_version(version).map_err(serde::de::Error::custom)
}

pub(super) fn deserialize_optional_config_version<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<u32>::deserialize(deserializer)?
        .map(validate_config_version)
        .transpose()
        .map_err(serde::de::Error::custom)
}

fn validate_config_version(version: u32) -> std::result::Result<u32, String> {
    match version.cmp(&CURRENT_CONFIG_VERSION) {
        std::cmp::Ordering::Equal => Ok(version),
        std::cmp::Ordering::Greater => Err(format!(
            "config_version {version} requires a newer version of Aegis \
             (this binary supports schema version {CURRENT_CONFIG_VERSION}).\n\
             To upgrade: install a newer Aegis release that supports schema {version}, \
             then run `aegis config validate` to confirm compatibility.\n\
             To downgrade the config to schema {CURRENT_CONFIG_VERSION}: \
             run `aegis config init` to regenerate a fresh config file."
        )),
        std::cmp::Ordering::Less => Err(format!(
            "config_version {version} is below the minimum supported version \
             ({CURRENT_CONFIG_VERSION}); run `aegis config init` to regenerate your config."
        )),
    }
}
