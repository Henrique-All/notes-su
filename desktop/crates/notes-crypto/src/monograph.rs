use notes_core::CipherEnvelope;

use crate::envelope::{decrypt_envelope, encrypt_to_envelope};
use crate::error::CryptoError;

pub fn encrypt_monograph_content(
    reader_password: &str,
    markdown: &str,
) -> Result<CipherEnvelope, CryptoError> {
    if reader_password.is_empty() {
        return Err(CryptoError::EmptyPassword);
    }
    if markdown.trim().is_empty() {
        return Err(CryptoError::EmptyMonographContent);
    }
    encrypt_to_envelope(reader_password, markdown)
}

pub fn decrypt_monograph_content(
    reader_password: &str,
    envelope: &CipherEnvelope,
) -> Result<String, CryptoError> {
    if reader_password.is_empty() {
        return Err(CryptoError::WrongPassword);
    }
    match decrypt_envelope(reader_password, envelope) {
        Ok(plain) => Ok(plain.as_str().to_owned()),
        Err(CryptoError::DecryptionFailed) => Err(CryptoError::WrongPassword),
        Err(other) => Err(other),
    }
}
