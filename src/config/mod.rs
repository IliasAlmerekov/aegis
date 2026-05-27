pub mod allowlist;
pub mod amend;
pub mod model;
pub mod snapshot;
pub mod validate;

pub use allowlist::{
    Allowlist, AllowlistContext, AllowlistMatch, AllowlistWarning, Blocklist, BlocklistMatch,
    BlocklistWarning, ConfigSourceLayer, LayeredAllowlistRule, LayeredBlocklistRule,
    analyze_allowlist_rule, analyze_blocklist_rule,
};
pub use amend::{active_config_path_for_append, append_allow_rule, append_block_rule};
pub use model::{
    AegisConfig, AllowlistOverrideLevel, AllowlistRule, AuditConfig, AuditIntegrityMode, BlockRule,
    CiPolicy, Mode, SnapshotPolicy, UserPattern,
};
pub use snapshot::{
    DockerScope, DockerScopeMode, MysqlSnapshotConfig, PostgresSnapshotConfig,
    SupabaseSnapshotConfig,
};
pub use validate::{
    ConfigSourceMap, ValidationIssue, ValidationReport, validate_config, validate_config_layers,
    validation_load_error,
};

pub type Config = AegisConfig;
