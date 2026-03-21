#[allow(dead_code)]
#[derive(thiserror::Error, Debug)]
pub enum AegisError {
    #[error("parse error: {0}")]
    Parse(String),

    #[error("snapshot error: {0}")]
    Snapshot(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
