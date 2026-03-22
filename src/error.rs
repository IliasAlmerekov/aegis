#[allow(dead_code)]
#[derive(thiserror::Error, Debug)]
pub enum AegisError {
    #[error("parse error: {0}")]
    Parse(String),

    #[error("snapshot error: {0}")]
    Snapshot(String),

    /// `git stash pop` conflicted during rollback.
    ///
    /// The stash entry is preserved — git does not drop it on a conflicted pop.
    /// The error message includes exact recovery commands so the user can finish
    /// the restore manually without losing any work.
    #[error(
        "rollback conflict in '{cwd}': git stash pop failed for {stash_ref}.\n\
         Your changes are still saved in the stash. To recover manually:\n  \
           1. Resolve conflicts:  cd '{cwd}' && git diff\n  \
           2. Stage resolutions:  git add <files>\n  \
           3. Drop the stash:     git stash drop {stash_ref}\n\
         Details: {details}"
    )]
    RollbackConflict {
        stash_ref: String,
        cwd: String,
        details: String,
    },

    #[error("config error: {0}")]
    Config(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
