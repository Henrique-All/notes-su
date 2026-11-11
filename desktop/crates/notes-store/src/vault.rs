use chrono::Utc;
use notes_core::{CipherEnvelope, NoteDetail, Vault, extract_headline};
use notes_crypto::{
    CryptoError, create_vault_canary, decrypt_envelope, encrypt_to_envelope, lock_note_content,
    unlock_note_content, verify_vault_canary,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::error::{StoreError, StoreResult};
use crate::notes::load_note_detail_conn;
use crate::schema::{NotesStore, monotonic_now_ms, ms_to_utc};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeleteVaultMode {
    UnlockNotes,
    DeleteNotes,
}

impl NotesStore {
    pub fn get_vault(&self) -> StoreResult<Option<Vault>> {
        let conn = self.lock_conn();
        load_active_vault_conn(&conn)
    }

    pub fn init_vault(&self, password: &str, lock_after_seconds: Option<i32>) -> StoreResult<Vault> {
        if password.is_empty() {
            return Err(StoreError::Validation(
                "Vault password must not be empty.".to_owned(),
            ));
        }

        let conn = self.lock_conn();
        if load_active_vault_conn(&conn)?.is_some() {
            return Err(StoreError::VaultAlreadyExists);
        }

        let canary = create_vault_canary(password)?;
        let canary_json = serde_json::to_string(&canary)?;
        let vid = Uuid::new_v4().to_string();
        let lock_secs = lock_after_seconds.unwrap_or(300);
        let now_ms = Utc::now().timestamp_millis();

        conn.execute(
            "INSERT INTO vault (id, title, key_canary, lock_after_seconds, deleted, synced, date_created, date_modified, revision)
             VALUES (?1, 'Default', ?2, ?3, 0, 0, ?4, ?4, 0)",
            params![vid, canary_json, lock_secs, now_ms],
        )?;

        Ok(Vault {
            id: vid,
            user_id: None,
            title: "Default".to_owned(),
            key_canary: canary,
            lock_after_seconds: lock_secs,
            date_created: ms_to_utc(now_ms),
            date_modified: ms_to_utc(now_ms),
            revision: 0,
        })
    }

    pub fn verify_vault(&self, password: &str) -> StoreResult<()> {
        let conn = self.lock_conn();
        let canary = load_active_vault_canary_conn(&conn)?;
        verify_vault_canary(password, &canary).map_err(|e| match e {
            CryptoError::WrongPassword => StoreError::WrongVaultPassword,
            other => StoreError::Crypto(other),
        })
    }

    /// Locks an unlocked note inside an atomic SQLite transaction:
    /// 1. Verifies `password` against `vault.key_canary`
    /// 2. Encrypts `note_contents.data` into `CipherEnvelope` JSON
    /// 3. Sets `notes.locked = 1, notes.headline = '', notes.synced = 0`
    /// 4. Sets `note_contents.locked = 1, note_contents.data = cipher_json, note_contents.synced = 0`
    /// 5. Purges all plaintext `history_sessions` for `note_id`!
    pub fn lock_note(&self, note_id: &str, password: &str) -> StoreResult<NoteDetail> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let canary = load_active_vault_canary_conn(&tx)?;
        let existing = load_note_detail_conn(&tx, note_id)?;
        if existing.deleted {
            return Err(StoreError::NotFound(format!("note {note_id}")));
        }
        if existing.locked {
            verify_vault_canary(password, &canary).map_err(|e| match e {
                CryptoError::WrongPassword => StoreError::WrongVaultPassword,
                other => StoreError::Crypto(other),
            })?;
            return Ok(existing);
        }

        let plain = existing
            .content
            .as_ref()
            .map(|c| c.data.as_str())
            .unwrap_or("");
        let locked_payload = lock_note_content(password, &canary, plain).map_err(|e| match e {
            CryptoError::WrongPassword => StoreError::WrongVaultPassword,
            other => StoreError::Crypto(other),
        })?;

        let now_ms = monotonic_now_ms(existing.date_modified.timestamp_millis());

        tx.execute(
            "DELETE FROM history_sessions WHERE note_id = ?1",
            params![note_id],
        )?;
        tx.execute(
            "UPDATE notes SET locked = 1, headline = '', synced = 0, date_modified = ?1 WHERE id = ?2",
            params![now_ms, note_id],
        )?;
        tx.execute(
            "UPDATE note_contents SET locked = 1, data = ?1, synced = 0, date_modified = ?2 WHERE note_id = ?3",
            params![locked_payload.data_json, now_ms, note_id],
        )?;

        tx.commit()?;
        drop(conn);
        self.get_note(note_id)
    }

    pub fn unlock_note_temporary(
        &self,
        note_id: &str,
        password: &str,
    ) -> StoreResult<Zeroizing<String>> {
        let conn = self.lock_conn();
        let existing = load_note_detail_conn(&conn, note_id)?;
        let data = existing
            .content
            .as_ref()
            .map(|c| c.data.as_str())
            .unwrap_or("");
        if !existing.locked {
            return Ok(Zeroizing::new(data.to_owned()));
        }
        unlock_note_content(password, data).map_err(|e| match e {
            CryptoError::WrongPassword => StoreError::WrongVaultPassword,
            other => StoreError::Crypto(other),
        })
    }

    pub fn unlock_note_permanent(&self, note_id: &str, password: &str) -> StoreResult<NoteDetail> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let existing = load_note_detail_conn(&tx, note_id)?;
        if !existing.locked {
            return Ok(existing);
        }
        let locked_json = existing
            .content
            .as_ref()
            .map(|c| c.data.as_str())
            .unwrap_or("");
        let plain = unlock_note_content(password, locked_json).map_err(|e| match e {
            CryptoError::WrongPassword => StoreError::WrongVaultPassword,
            other => StoreError::Crypto(other),
        })?;

        let headline = extract_headline(&plain, false);
        let now_ms = monotonic_now_ms(existing.date_modified.timestamp_millis());

        tx.execute(
            "UPDATE notes SET locked = 0, headline = ?1, synced = 0, date_modified = ?2 WHERE id = ?3",
            params![headline, now_ms, note_id],
        )?;
        tx.execute(
            "UPDATE note_contents SET locked = 0, data = ?1, synced = 0, date_modified = ?2 WHERE note_id = ?3",
            params![plain.as_str(), now_ms, note_id],
        )?;
        tx.execute(
            "DELETE FROM history_sessions WHERE note_id = ?1 AND locked = 1",
            params![note_id],
        )?;

        tx.commit()?;
        drop(conn);
        self.get_note(note_id)
    }

    /// Atomic 2-phase Vault password rotation:
    /// Decrypts every locked `note_contents` and locked `history_sessions` row in memory first.
    /// If ANY item fails decryption, returns `Err(StoreError::VaultRotationDecryptFailed)`
    /// without mutating SQLite! Otherwise commits new canary + all re-encrypted items in one transaction.
    pub fn rotate_vault_atomic(
        &self,
        old_password: &str,
        new_password: &str,
    ) -> StoreResult<Vault> {
        if new_password.is_empty() {
            return Err(StoreError::Validation(
                "New vault password must not be empty.".to_owned(),
            ));
        }

        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let current_vault = load_active_vault_conn(&tx)?.ok_or(StoreError::NoVault)?;
        verify_vault_canary(old_password, &current_vault.key_canary).map_err(|e| match e {
            CryptoError::WrongPassword => StoreError::WrongVaultPassword,
            other => StoreError::Crypto(other),
        })?;

        let mut locked_contents: Vec<(String, String)> = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT nc.note_id, nc.data
                 FROM note_contents nc
                 JOIN notes n ON n.id = nc.note_id
                 WHERE n.deleted = 0 AND nc.deleted = 0 AND (n.locked = 1 OR nc.locked = 1)",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for r in rows {
                locked_contents.push(r?);
            }
        }

        let mut locked_sessions: Vec<(String, String, String)> = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT id, note_id, data FROM history_sessions WHERE locked = 1",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            for r in rows {
                locked_sessions.push(r?);
            }
        }

        // Phase 1: Decrypt EVERYTHING in memory first!
        let mut decrypted_contents: Vec<(String, Zeroizing<String>)> =
            Vec::with_capacity(locked_contents.len());
        for (nid, raw_json) in &locked_contents {
            let env: CipherEnvelope = serde_json::from_str(raw_json).map_err(|_| {
                StoreError::VaultRotationDecryptFailed {
                    note_id: nid.clone(),
                }
            })?;
            let plain = decrypt_envelope(old_password, &env).map_err(|_| {
                StoreError::VaultRotationDecryptFailed {
                    note_id: nid.clone(),
                }
            })?;
            decrypted_contents.push((nid.clone(), plain));
        }

        let mut decrypted_sessions: Vec<(String, Zeroizing<String>)> =
            Vec::with_capacity(locked_sessions.len());
        for (sid, nid, raw_json) in &locked_sessions {
            let env: CipherEnvelope = serde_json::from_str(raw_json).map_err(|_| {
                StoreError::VaultRotationDecryptFailed {
                    note_id: nid.clone(),
                }
            })?;
            let plain = decrypt_envelope(old_password, &env).map_err(|_| {
                StoreError::VaultRotationDecryptFailed {
                    note_id: nid.clone(),
                }
            })?;
            decrypted_sessions.push((sid.clone(), plain));
        }

        // Phase 2: Re-encrypt all with fresh salts & nonces and commit atomically!
        let new_canary = create_vault_canary(new_password)?;
        let new_canary_json = serde_json::to_string(&new_canary)?;
        let now_ms = monotonic_now_ms(current_vault.date_modified.timestamp_millis());

        tx.execute(
            "UPDATE vault SET key_canary = ?1, synced = 0, date_modified = ?2 WHERE id = ?3",
            params![new_canary_json, now_ms, current_vault.id],
        )?;

        for (nid, plain) in decrypted_contents {
            let new_env = encrypt_to_envelope(new_password, &plain)?;
            let new_json = serde_json::to_string(&new_env)?;
            tx.execute(
                "UPDATE note_contents SET data = ?1, synced = 0, date_modified = ?2 WHERE note_id = ?3",
                params![new_json, now_ms, nid],
            )?;
            tx.execute(
                "UPDATE notes SET synced = 0, date_modified = ?1 WHERE id = ?2",
                params![now_ms, nid],
            )?;
        }

        for (sid, plain) in decrypted_sessions {
            let new_env = encrypt_to_envelope(new_password, &plain)?;
            let new_json = serde_json::to_string(&new_env)?;
            tx.execute(
                "UPDATE history_sessions SET data = ?1, date_modified = ?2 WHERE id = ?3",
                params![new_json, now_ms, sid],
            )?;
        }

        tx.commit()?;
        drop(conn);
        self.get_vault()?.ok_or(StoreError::NoVault)
    }

    pub fn delete_vault(&self, mode: DeleteVaultMode, password: Option<&str>) -> StoreResult<()> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let current_vault = load_active_vault_conn(&tx)?.ok_or(StoreError::NoVault)?;
        let now_ms = monotonic_now_ms(current_vault.date_modified.timestamp_millis());

        match mode {
            DeleteVaultMode::UnlockNotes => {
                let pw = password.ok_or(StoreError::VaultVerificationRequired)?;
                verify_vault_canary(pw, &current_vault.key_canary).map_err(|e| match e {
                    CryptoError::WrongPassword => StoreError::WrongVaultPassword,
                    other => StoreError::Crypto(other),
                })?;

                let mut locked_rows: Vec<(String, String)> = Vec::new();
                {
                    let mut stmt = tx.prepare(
                        "SELECT nc.note_id, nc.data
                         FROM note_contents nc
                         JOIN notes n ON n.id = nc.note_id
                         WHERE n.deleted = 0 AND nc.deleted = 0 AND (n.locked = 1 OR nc.locked = 1)",
                    )?;
                    let rows = stmt.query_map([], |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                    })?;
                    for r in rows {
                        locked_rows.push(r?);
                    }
                }

                let mut unlocked_items = Vec::with_capacity(locked_rows.len());
                for (nid, raw_json) in &locked_rows {
                    let plain = unlock_note_content(pw, raw_json).map_err(|_| {
                        StoreError::VaultRotationDecryptFailed {
                            note_id: nid.clone(),
                        }
                    })?;
                    let hl = extract_headline(&plain, false);
                    unlocked_items.push((nid.clone(), plain, hl));
                }

                for (nid, plain, hl) in unlocked_items {
                    tx.execute(
                        "UPDATE notes SET locked = 0, headline = ?1, synced = 0, date_modified = ?2 WHERE id = ?3",
                        params![hl, now_ms, nid],
                    )?;
                    tx.execute(
                        "UPDATE note_contents SET locked = 0, data = ?1, synced = 0, date_modified = ?2 WHERE note_id = ?3",
                        params![plain.as_str(), now_ms, nid],
                    )?;
                    tx.execute(
                        "DELETE FROM history_sessions WHERE note_id = ?1 AND locked = 1",
                        params![nid],
                    )?;
                }
            }
            DeleteVaultMode::DeleteNotes => {
                let mut locked_ids = Vec::new();
                {
                    let mut stmt = tx.prepare(
                        "SELECT id FROM notes WHERE deleted = 0 AND locked = 1",
                    )?;
                    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                    for r in rows {
                        locked_ids.push(r?);
                    }
                }
                for nid in locked_ids {
                    tx.execute("DELETE FROM history_sessions WHERE note_id = ?1", params![nid])?;
                    tx.execute(
                        "UPDATE note_contents SET data = '', conflicted_data = NULL, deleted = 1, synced = 0, date_modified = ?1 WHERE note_id = ?2",
                        params![now_ms, nid],
                    )?;
                    tx.execute(
                        "UPDATE notes SET title = '', headline = '', deleted = 1, synced = 0, date_modified = ?1 WHERE id = ?2",
                        params![now_ms, nid],
                    )?;
                }
            }
        }

        tx.execute(
            "UPDATE vault SET deleted = 1, synced = 0, date_modified = ?1 WHERE id = ?2",
            params![now_ms, current_vault.id],
        )?;

        tx.commit()?;
        Ok(())
    }
}

pub(crate) fn load_active_vault_conn(conn: &Connection) -> StoreResult<Option<Vault>> {
    let opt = conn
        .query_row(
            "SELECT id, title, key_canary, lock_after_seconds, date_created, date_modified, revision
             FROM vault WHERE deleted = 0 ORDER BY date_modified DESC LIMIT 1",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i32>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                ))
            },
        )
        .optional()?;

    let Some((id, title, canary_raw, lock_secs, created_ms, mod_ms, rev)) = opt else {
        return Ok(None);
    };
    let key_canary: CipherEnvelope = serde_json::from_str(&canary_raw)?;
    Ok(Some(Vault {
        id,
        user_id: None,
        title,
        key_canary,
        lock_after_seconds: lock_secs,
        date_created: ms_to_utc(created_ms),
        date_modified: ms_to_utc(mod_ms),
        revision: rev,
    }))
}

pub(crate) fn load_active_vault_canary_conn(conn: &Connection) -> StoreResult<CipherEnvelope> {
    Ok(load_active_vault_conn(conn)?
        .ok_or(StoreError::NoVault)?
        .key_canary)
}
