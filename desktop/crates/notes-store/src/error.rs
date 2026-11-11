use thiserror::Error;

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("Entity not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Note must have a title or content.")]
    BlankNote,

    #[error("Notebook hierarchy cycle detected for '{0}'.")]
    CycleDetected(String),

    #[error("Cannot modify notebook '{0}' while it is in Trash.")]
    NotebookInTrash(String),

    #[error("Note '{0}' is read-only and cannot be modified until readonly is disabled.")]
    ReadonlyNote(String),

    #[error("Tag '{0}' already exists.")]
    DuplicateTag(String),

    #[error("This attachment is inside a locked note.")]
    AttachmentInLockedNote,

    #[error("Personal Vault has not been initialized.")]
    NoVault,

    #[error("Personal Vault is already initialized.")]
    VaultAlreadyExists,

    #[error("Vault is locked.")]
    VaultLocked,

    #[error("Incorrect vault password.")]
    WrongVaultPassword,

    #[error("Vault password verification is required to permanently delete locked notes.")]
    VaultVerificationRequired,

    #[error("Vault password rotation aborted: failed to decrypt locked note '{note_id}'.")]
    VaultRotationDecryptFailed { note_id: String },

    #[error("Note '{0}' is locked.")]
    NoteLocked(String),

    #[error(transparent)]
    Core(#[from] notes_core::CoreError),

    #[error(transparent)]
    Crypto(#[from] notes_crypto::CryptoError),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
