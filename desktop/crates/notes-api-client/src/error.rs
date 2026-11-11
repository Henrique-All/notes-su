use thiserror::Error;

pub type ApiResult<T> = Result<T, ApiClientError>;

#[derive(Debug, Error)]
pub enum ApiClientError {
    #[error("Bad request (400): {message}")]
    BadRequest {
        message: String,
        code: Option<String>,
    },

    #[error("Unauthorized (401): {message}")]
    Unauthorized { message: String },

    #[error("Forbidden (403): {message}")]
    Forbidden { message: String },

    #[error("Not found (404): {message}")]
    NotFound { message: String },

    #[error("Conflict (409): {message}")]
    Conflict {
        message: String,
        code: Option<String>,
    },

    #[error("Gone (410): {message}")]
    Gone { message: String },

    #[error("Payload too large (413): {message}")]
    PayloadTooLarge { message: String },

    #[error("Unprocessable entity (422): {message}")]
    Unprocessable { message: String },

    #[error("Server error ({status}): {message}")]
    ServerError { status: u16, message: String },

    #[error("HTTP transport error: {0}")]
    Transport(String),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Store(#[from] notes_store::StoreError),

    #[error(transparent)]
    Crypto(#[from] notes_crypto::CryptoError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
