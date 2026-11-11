use serde::{Deserialize, Serialize};

pub const CIPHER_ALG_V1: &str = "xchacha20poly1305-argon2id-v1";
pub const VAULT_CANARY_PLAINTEXT: &str = "notes-su-vault-canary-v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct KdfParams {
    pub ops: u32,
    #[serde(rename = "memKiB", alias = "memKib", alias = "mem_kib")]
    pub mem_kib: u32,
    pub parallelism: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            ops: 3,
            mem_kib: 19_456,
            parallelism: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CipherEnvelope {
    pub alg: String,
    pub kdf: KdfParams,
    pub salt: String,
    pub iv: String,
    pub cipher: String,
    pub length: usize,
}
