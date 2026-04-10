pub mod allowlist;
pub mod model;

pub use allowlist::{Allowlist, AllowlistMatch};
pub use model::{
    AegisConfig, AllowlistOverrideLevel, AllowlistRule, AuditConfig, CiPolicy, Mode, UserPattern,
};

pub type Config = AegisConfig;
