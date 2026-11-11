use notes_core::KdfParams;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("Password must not be empty.")]
    EmptyPassword,

    #[error("Password is required to unseal encrypted payload.")]
    PasswordRequired,

    #[error("Incorrect vault or encryption password.")]
    WrongPassword,

    #[error("Unsupported cipher algorithm: {0}")]
    UnsupportedAlgorithm(String),

    #[error("Invalid or unsafe Argon2id KDF parameters: {0:?}")]
    InvalidKdfParams(KdfParams),

    #[error("Invalid Base64 encoding in cipher envelope.")]
    InvalidBase64,

    #[error("Invalid salt length: expected 16 bytes, got {0}.")]
    InvalidSaltLength(usize),

    #[error("Invalid XChaCha20 nonce length: expected 24 bytes, got {0}.")]
    InvalidNonceLength(usize),

    #[error("AEAD encryption failed.")]
    EncryptionFailed,

    #[error("AEAD decryption or authentication tag verification failed.")]
    DecryptionFailed,

    #[error("Decrypted byte length mismatch: expected {expected}, got {actual}.")]
    LengthMismatch { expected: usize, actual: usize },

    #[error("Decrypted bytes are not valid UTF-8.")]
    InvalidUtf8,

    #[error("Failed to decrypt locked note '{note_id}' during vault rotation.")]
    NoteDecryptionFailed { note_id: String },

    #[error("Monograph content must not be empty.")]
    EmptyMonographContent,

    #[error("Unsupported backup bundle version: {0}")]
    UnsupportedBackupVersion(u32),

    #[error("Backup SHA-256 checksum verification failed.")]
    ChecksumMismatch,

    #[error("Corrupted or malformed backup bundle.")]
    CorruptedBackup,

    #[error("JSON serialization error: {0}")]
    Json(String),
}
