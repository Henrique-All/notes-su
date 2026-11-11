use argon2::{Algorithm, Argon2, Params, Version};
use notes_core::KdfParams;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::error::CryptoError;

pub const SALT_LEN: usize = 16;
pub const XNONCE_LEN: usize = 24;
pub const KEY_LEN: usize = 32;
pub const POLY1305_TAG_LEN: usize = 16;

/// Bounds check to prevent crafted untrusted envelopes from causing OOM on 0-swap hosts.
pub const MAX_SAFE_MEM_KIB: u32 = 262_144; // 256 MiB
pub const MAX_SAFE_OPS: u32 = 32;
pub const MAX_SAFE_PARALLELISM: u32 = 16;

pub fn derive_key(
    password: &str,
    salt: &[u8; SALT_LEN],
    kdf: &KdfParams,
) -> Result<Zeroizing<[u8; KEY_LEN]>, CryptoError> {
    if kdf.ops == 0
        || kdf.ops > MAX_SAFE_OPS
        || kdf.parallelism == 0
        || kdf.parallelism > MAX_SAFE_PARALLELISM
        || kdf.mem_kib < 8 * kdf.parallelism
        || kdf.mem_kib > MAX_SAFE_MEM_KIB
    {
        return Err(CryptoError::InvalidKdfParams(*kdf));
    }

    let params = Params::new(kdf.mem_kib, kdf.ops, kdf.parallelism, Some(KEY_LEN))
        .map_err(|_| CryptoError::InvalidKdfParams(*kdf))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon2
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|_| CryptoError::InvalidKdfParams(*kdf))?;
    Ok(key)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
