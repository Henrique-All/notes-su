use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STD};
use chacha20poly1305::{
    Key, XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use notes_core::{CIPHER_ALG_V1, CipherEnvelope, KdfParams};
use zeroize::Zeroizing;

use crate::error::CryptoError;
use crate::kdf::{POLY1305_TAG_LEN, SALT_LEN, XNONCE_LEN, derive_key};

pub fn random_salt_and_iv() -> ([u8; SALT_LEN], [u8; XNONCE_LEN]) {
    let mut salt = [0u8; SALT_LEN];
    let mut iv = [0u8; XNONCE_LEN];
    rand::fill(&mut salt[..]);
    rand::fill(&mut iv[..]);
    (salt, iv)
}

pub fn encrypt_with_salt_and_iv(
    password: &str,
    plaintext: &str,
    salt: [u8; SALT_LEN],
    iv: [u8; XNONCE_LEN],
    kdf: KdfParams,
) -> Result<CipherEnvelope, CryptoError> {
    let key = derive_key(password, &salt, &kdf)?;
    let cipher = XChaCha20Poly1305::new(&Key::from(*key));
    let nonce = XNonce::from(iv);

    let pt_bytes = plaintext.as_bytes();
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: pt_bytes,
                aad: b"",
            },
        )
        .map_err(|_| CryptoError::EncryptionFailed)?;

    debug_assert_eq!(ciphertext.len(), pt_bytes.len() + POLY1305_TAG_LEN);

    Ok(CipherEnvelope {
        alg: CIPHER_ALG_V1.to_owned(),
        kdf,
        salt: BASE64_STD.encode(salt),
        iv: BASE64_STD.encode(iv),
        cipher: BASE64_STD.encode(&ciphertext),
        length: pt_bytes.len(),
    })
}

pub fn encrypt_to_envelope(password: &str, plaintext: &str) -> Result<CipherEnvelope, CryptoError> {
    let (salt, iv) = random_salt_and_iv();
    encrypt_with_salt_and_iv(password, plaintext, salt, iv, KdfParams::default())
}

pub fn decrypt_envelope(
    password: &str,
    envelope: &CipherEnvelope,
) -> Result<Zeroizing<String>, CryptoError> {
    if envelope.alg != CIPHER_ALG_V1 {
        return Err(CryptoError::UnsupportedAlgorithm(envelope.alg.clone()));
    }

    let salt_vec = BASE64_STD
        .decode(&envelope.salt)
        .map_err(|_| CryptoError::InvalidBase64)?;
    let salt: [u8; SALT_LEN] = salt_vec
        .try_into()
        .map_err(|v: Vec<u8>| CryptoError::InvalidSaltLength(v.len()))?;

    let iv_vec = BASE64_STD
        .decode(&envelope.iv)
        .map_err(|_| CryptoError::InvalidBase64)?;
    let iv: [u8; XNONCE_LEN] = iv_vec
        .try_into()
        .map_err(|v: Vec<u8>| CryptoError::InvalidNonceLength(v.len()))?;

    let cipher_bytes = BASE64_STD
        .decode(&envelope.cipher)
        .map_err(|_| CryptoError::InvalidBase64)?;
    if cipher_bytes.len() < POLY1305_TAG_LEN {
        return Err(CryptoError::DecryptionFailed);
    }

    let key = derive_key(password, &salt, &envelope.kdf)?;
    let aead = XChaCha20Poly1305::new(&Key::from(*key));
    let nonce = XNonce::from(iv);

    let pt_bytes = Zeroizing::new(
        aead.decrypt(
            &nonce,
            Payload {
                msg: &cipher_bytes,
                aad: b"",
            },
        )
        .map_err(|_| CryptoError::DecryptionFailed)?,
    );

    if pt_bytes.len() != envelope.length {
        return Err(CryptoError::LengthMismatch {
            expected: envelope.length,
            actual: pt_bytes.len(),
        });
    }

    let text = String::from_utf8(pt_bytes.to_vec()).map_err(|_| CryptoError::InvalidUtf8)?;
    Ok(Zeroizing::new(text))
}
