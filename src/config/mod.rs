pub mod allowlist;
pub mod model;
pub mod validate;

pub use allowlist::{
    Allowlist, AllowlistContext, AllowlistMatch, AllowlistSourceLayer, AllowlistWarning,
    LayeredAllowlistRule, analyze_allowlist_rule,
};
pub use model::{
    AegisConfig, AllowlistOverrideLevel, AllowlistRule, AuditConfig, CiPolicy, Mode,
    SnapshotPolicy, UserPattern,
};
pub use validate::{
    ConfigSourceMap, ValidationIssue, ValidationReport, validate_config, validate_config_layers,
    validation_load_error,
};

pub type Config = AegisConfig;
