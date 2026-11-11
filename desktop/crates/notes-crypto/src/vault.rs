use std::time::{Duration, Instant};

use notes_core::{CipherEnvelope, VAULT_CANARY_PLAINTEXT};
use zeroize::Zeroizing;

use crate::envelope::{decrypt_envelope, encrypt_to_envelope};
use crate::error::CryptoError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedNotePayload {
    pub envelope: CipherEnvelope,
    pub data_json: String,
    pub headline: String,
}

pub fn create_vault_canary(password: &str) -> Result<CipherEnvelope, CryptoError> {
    if password.is_empty() {
        return Err(CryptoError::EmptyPassword);
    }
    encrypt_to_envelope(password, VAULT_CANARY_PLAINTEXT)
}

pub fn verify_vault_canary(password: &str, canary: &CipherEnvelope) -> Result<(), CryptoError> {
    if password.is_empty() {
        return Err(CryptoError::WrongPassword);
    }
    match decrypt_envelope(password, canary) {
        Ok(plain) if plain.as_str() == VAULT_CANARY_PLAINTEXT => Ok(()),
        Ok(_) | Err(CryptoError::DecryptionFailed) => Err(CryptoError::WrongPassword),
        Err(other) => Err(other),
    }
}

pub fn lock_note_content(
    password: &str,
    canary: &CipherEnvelope,
    plaintext_markdown: &str,
) -> Result<LockedNotePayload, CryptoError> {
    verify_vault_canary(password, canary)?;
    let envelope = encrypt_to_envelope(password, plaintext_markdown)?;
    let data_json =
        serde_json::to_string(&envelope).map_err(|e| CryptoError::Json(e.to_string()))?;
    Ok(LockedNotePayload {
        envelope,
        data_json,
        headline: String::new(),
    })
}

pub fn unlock_note_content(
    password: &str,
    locked_data_json: &str,
) -> Result<Zeroizing<String>, CryptoError> {
    if password.is_empty() {
        return Err(CryptoError::WrongPassword);
    }
    let envelope: CipherEnvelope =
        serde_json::from_str(locked_data_json).map_err(|e| CryptoError::Json(e.to_string()))?;
    match decrypt_envelope(password, &envelope) {
        Ok(pt) => Ok(pt),
        Err(CryptoError::DecryptionFailed) => Err(CryptoError::WrongPassword),
        Err(other) => Err(other),
    }
}

/// 2-phase all-or-nothing vault password rotation:
/// Phase 1 decrypts every locked note under `old_password` first; if ANY note fails,
/// returns `Err(CryptoError::NoteDecryptionFailed { note_id })` without generating
/// partial output. Phase 2 seals a fresh canary and re-encrypts every note with a
/// fresh CSPRNG 16-byte salt and 24-byte nonce.
pub fn rotate_vault_password(
    old_password: &str,
    new_password: &str,
    current_canary: &CipherEnvelope,
    locked_notes: &[(String, String)],
) -> Result<(CipherEnvelope, Vec<(String, String)>), CryptoError> {
    if new_password.is_empty() {
        return Err(CryptoError::EmptyPassword);
    }
    verify_vault_canary(old_password, current_canary)?;

    let mut decrypted_all: Vec<(String, Zeroizing<String>)> =
        Vec::with_capacity(locked_notes.len());
    for (note_id, raw_json) in locked_notes {
        let env: CipherEnvelope = serde_json::from_str(raw_json).map_err(|_| {
            CryptoError::NoteDecryptionFailed {
                note_id: note_id.clone(),
            }
        })?;
        let plain =
            decrypt_envelope(old_password, &env).map_err(|_| CryptoError::NoteDecryptionFailed {
                note_id: note_id.clone(),
            })?;
        decrypted_all.push((note_id.clone(), plain));
    }

    let new_canary = create_vault_canary(new_password)?;
    let mut reencrypted = Vec::with_capacity(decrypted_all.len());
    for (note_id, plain) in decrypted_all {
        let new_env = encrypt_to_envelope(new_password, &plain)?;
        let new_json =
            serde_json::to_string(&new_env).map_err(|e| CryptoError::Json(e.to_string()))?;
        reencrypted.push((note_id, new_json));
    }

    Ok((new_canary, reencrypted))
}

#[derive(Debug)]
pub struct VaultSession {
    password: Option<Zeroizing<String>>,
    last_activity: Option<Instant>,
    lock_after_seconds: i32,
}

impl VaultSession {
    pub fn new(lock_after_seconds: i32) -> Self {
        Self {
            password: None,
            last_activity: None,
            lock_after_seconds,
        }
    }

    pub fn unlock(&mut self, password: &str, canary: &CipherEnvelope) -> Result<(), CryptoError> {
        verify_vault_canary(password, canary)?;
        self.password = Some(Zeroizing::new(password.to_owned()));
        self.last_activity = Some(Instant::now());
        Ok(())
    }

    pub fn lock(&mut self) {
        self.password = None;
        self.last_activity = None;
    }

    pub fn is_unlocked(&self) -> bool {
        self.password.is_some()
    }

    pub fn password(&self) -> Option<&str> {
        self.password.as_deref().map(String::as_str)
    }

    pub fn set_lock_after_seconds(&mut self, seconds: i32) {
        self.lock_after_seconds = seconds;
    }

    pub fn touch_on_save(&mut self) {
        if self.password.is_some() {
            self.last_activity = Some(Instant::now());
        }
    }

    pub fn check_auto_lock(&mut self) -> bool {
        self.check_auto_lock_at(Instant::now())
    }

    pub fn check_auto_lock_at(&mut self, now: Instant) -> bool {
        if self.password.is_none() || self.lock_after_seconds <= 0 {
            return false;
        }
        if let Some(last) = self.last_activity {
            let elapsed = now.saturating_duration_since(last);
            if elapsed >= Duration::from_secs(self.lock_after_seconds as u64) {
                self.lock();
                return true;
            }
        }
        false
    }
}
