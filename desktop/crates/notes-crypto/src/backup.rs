use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use notes_core::{BackupBundle, BackupData};
use serde_json::Value;

use crate::envelope::{decrypt_envelope, encrypt_to_envelope};
use crate::error::CryptoError;
use crate::kdf::sha256_hex;

pub const BACKUP_FORMAT_VERSION: u32 = 1;

pub fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted: BTreeMap<String, Value> = BTreeMap::new();
            for (k, v) in map {
                sorted.insert(k.clone(), canonicalize_json(v));
            }
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(arr) => Value::Array(arr.iter().map(canonicalize_json).collect()),
        other => other.clone(),
    }
}

pub fn compute_canonical_sha256(value: &Value) -> Result<String, CryptoError> {
    let canon = canonicalize_json(value);
    let compact = serde_json::to_string(&canon).map_err(|_| CryptoError::CorruptedBackup)?;
    Ok(sha256_hex(compact.as_bytes()))
}

pub fn seal_backup(
    data: &BackupData,
    password: Option<&str>,
    exported_at: DateTime<Utc>,
) -> Result<BackupBundle, CryptoError> {
    if let Some(pw) = password
        && pw.is_empty()
    {
        return Err(CryptoError::EmptyPassword);
    }

    let raw_val = serde_json::to_value(data).map_err(|e| CryptoError::Json(e.to_string()))?;
    let canon_val = canonicalize_json(&raw_val);
    let canon_str =
        serde_json::to_string(&canon_val).map_err(|e| CryptoError::Json(e.to_string()))?;
    let sha256 = sha256_hex(canon_str.as_bytes());

    if let Some(pw) = password {
        let cipher = encrypt_to_envelope(pw, &canon_str)?;
        Ok(BackupBundle {
            version: BACKUP_FORMAT_VERSION,
            exported_at,
            sha256,
            encrypted: true,
            data: None,
            cipher: Some(cipher),
        })
    } else {
        Ok(BackupBundle {
            version: BACKUP_FORMAT_VERSION,
            exported_at,
            sha256,
            encrypted: false,
            data: Some(canon_val),
            cipher: None,
        })
    }
}

pub fn unseal_backup(
    bundle: &BackupBundle,
    password: Option<&str>,
) -> Result<BackupData, CryptoError> {
    if bundle.version == 0 || bundle.version > BACKUP_FORMAT_VERSION {
        return Err(CryptoError::UnsupportedBackupVersion(bundle.version));
    }

    let (raw_decrypted_str, json_val) = if bundle.encrypted {
        let pw = password.ok_or(CryptoError::PasswordRequired)?;
        if pw.is_empty() {
            return Err(CryptoError::WrongPassword);
        }
        let env = bundle.cipher.as_ref().ok_or(CryptoError::CorruptedBackup)?;
        let decrypted = match decrypt_envelope(pw, env) {
            Ok(s) => s,
            Err(CryptoError::DecryptionFailed) => return Err(CryptoError::WrongPassword),
            Err(other) => return Err(other),
        };
        let parsed: Value =
            serde_json::from_str(&decrypted).map_err(|_| CryptoError::CorruptedBackup)?;
        (Some(decrypted), parsed)
    } else {
        let data_val = bundle.data.clone().ok_or(CryptoError::CorruptedBackup)?;
        (None, data_val)
    };

    let canon_sha = compute_canonical_sha256(&json_val)?;
    let raw_matches = raw_decrypted_str
        .as_ref()
        .is_some_and(|raw| sha256_hex(raw.as_bytes()).eq_ignore_ascii_case(&bundle.sha256));
    let uncanonical_matches = serde_json::to_string(&json_val)
        .ok()
        .is_some_and(|s| sha256_hex(s.as_bytes()).eq_ignore_ascii_case(&bundle.sha256));

    if !canon_sha.eq_ignore_ascii_case(&bundle.sha256) && !raw_matches && !uncanonical_matches {
        return Err(CryptoError::ChecksumMismatch);
    }

    serde_json::from_value::<BackupData>(json_val).map_err(|_| CryptoError::CorruptedBackup)
}
