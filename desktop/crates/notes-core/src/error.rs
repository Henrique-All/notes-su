use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    #[error("Note must have a title or content.")]
    BlankNote,

    #[error("Operation is not permitted on a locked note until unlocked.")]
    NoteLocked,

    #[error("Task line {0} is out of range or not a valid task item.")]
    TaskLineOutOfRange(usize),

    #[error("Task index {0} is out of range.")]
    TaskIndexOutOfRange(usize),

    #[error("YAML serialization error: {0}")]
    Yaml(String),

    #[error("JSON serialization error: {0}")]
    Json(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),
}
