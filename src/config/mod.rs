pub mod allowlist;
pub mod model;

pub use allowlist::{Allowlist, AllowlistMatch};
pub use model::{AegisConfig, CiPolicy, Mode, UserPattern};

pub type Config = AegisConfig;
