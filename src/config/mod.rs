pub mod allowlist;
pub mod model;

pub use allowlist::{
    Allowlist, AllowlistContext, AllowlistMatch, AllowlistSourceLayer, AllowlistWarning,
    LayeredAllowlistRule, analyze_allowlist_rule,
};
pub use model::{
    AegisConfig, AllowlistOverrideLevel, AllowlistRule, AuditConfig, CiPolicy, Mode, UserPattern,
};

pub type Config = AegisConfig;
