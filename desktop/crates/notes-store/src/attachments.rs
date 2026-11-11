use chrono::Utc;
use notes_core::{Attachment, CipherEnvelope};
use notes_crypto::sha256_hex;
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::error::{StoreError, StoreResult};
use crate::schema::{NotesStore, monotonic_now_ms, ms_to_utc, opt_ms_to_utc};

#[derive(Debug, Clone)]
pub struct UpsertAttachmentInput<'a> {
    pub id: Option<String>,
    pub filename: String,
    pub mime_type: String,
    pub bytes: &'a [u8],
    pub explicit_hash: Option<String>,
    pub note_id: Option<String>,
    pub encrypted: bool,
    pub encryption_meta: Option<CipherEnvelope>,
}

impl NotesStore {
    pub fn upsert_attachment(&self, input: UpsertAttachmentInput<'_>) -> StoreResult<Attachment> {
        let hash = input
            .explicit_hash
            .filter(|h| !h.trim().is_empty())
            .unwrap_or_else(|| sha256_hex(input.bytes));
        let size = input.bytes.len() as i64;
        let enc_meta_str = input
            .encryption_meta
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;

        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;
        let now_ms = Utc::now().timestamp_millis();

        let att_id = if let Some((existing_id, prev_mod)) = tx
            .query_row(
                "SELECT id, date_modified FROM attachments WHERE hash = ?1 AND deleted = 0",
                params![hash],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?
        {
            let bump = monotonic_now_ms(prev_mod);
            tx.execute(
                "UPDATE attachments SET
                    local_blob = COALESCE(?1, local_blob),
                    date_modified = ?2
                 WHERE id = ?3",
                params![input.bytes, bump, existing_id],
            )?;
            existing_id
        } else {
            let aid = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
            tx.execute(
                "INSERT INTO attachments (
                    id, hash, filename, mime_type, size, encrypted, encryption_meta,
                    local_blob, storage_path, date_uploaded, failed, deleted, synced, date_created, date_modified, revision
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL, NULL, 0, 0, ?9, ?9, 0)",
                params![
                    aid,
                    hash,
                    input.filename,
                    input.mime_type,
                    size,
                    i32::from(input.encrypted),
                    enc_meta_str,
                    input.bytes,
                    now_ms
                ],
            )?;
            aid
        };

        if let Some(ref nid) = input.note_id {
            let note_exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1 AND deleted = 0)",
                params![nid],
                |r| r.get(0),
            )?;
            if note_exists {
                upsert_note_attachment_tx(&tx, nid, &att_id, now_ms)?;
            }
        }

        tx.commit()?;
        drop(conn);
        self.get_attachment_by_hash(&hash)?
            .ok_or_else(|| StoreError::NotFound(format!("attachment {hash}")))
    }

    pub fn list_attachments(&self) -> StoreResult<Vec<Attachment>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT id, hash, filename, mime_type, size, encrypted, encryption_meta,
                    storage_path, date_uploaded, date_created, date_modified, revision
             FROM attachments WHERE deleted = 0 ORDER BY date_created DESC",
        )?;
        let rows = stmt.query_map([], map_attachment_row)?;
        let mut out = Vec::new();
        for r in rows {
            let mut att = r?;
            att.note_ids = load_attachment_note_ids_conn(&conn, &att.id)?;
            out.push(att);
        }
        Ok(out)
    }

    pub fn get_attachment_by_hash(&self, hash: &str) -> StoreResult<Option<Attachment>> {
        let conn = self.lock_conn();
        let opt = conn
            .query_row(
                "SELECT id, hash, filename, mime_type, size, encrypted, encryption_meta,
                        storage_path, date_uploaded, date_created, date_modified, revision
                 FROM attachments WHERE hash = ?1 AND deleted = 0",
                params![hash],
                map_attachment_row,
            )
            .optional()?;
        if let Some(mut att) = opt {
            att.note_ids = load_attachment_note_ids_conn(&conn, &att.id)?;
            Ok(Some(att))
        } else {
            Ok(None)
        }
    }

    pub fn get_attachment_blob(&self, hash: &str) -> StoreResult<Option<Vec<u8>>> {
        let conn = self.lock_conn();
        conn.query_row(
            "SELECT local_blob FROM attachments WHERE hash = ?1 AND deleted = 0",
            params![hash],
            |r| r.get::<_, Option<Vec<u8>>>(0),
        )
        .optional()
        .map(Option::flatten)
        .map_err(Into::into)
    }

    pub fn find_orphan_attachments(&self) -> StoreResult<Vec<Attachment>> {
        let all = self.list_attachments()?;
        Ok(all.into_iter().filter(|a| a.note_ids.is_empty()).collect())
    }

    pub fn delete_attachment_by_hash(&self, hash: &str) -> StoreResult<()> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let (att_id, prev_mod): (String, i64) = tx
            .query_row(
                "SELECT id, date_modified FROM attachments WHERE hash = ?1 AND deleted = 0",
                params![hash],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("attachment {hash}")))?;

        let in_locked_note: bool = tx.query_row(
            "SELECT EXISTS(
                SELECT 1
                FROM note_attachments na
                JOIN notes n ON n.id = na.note_id
                WHERE na.attachment_id = ?1
                  AND na.deleted = 0
                  AND n.deleted = 0
                  AND n.locked = 1
             )",
            params![att_id],
            |r| r.get(0),
        )?;

        if in_locked_note {
            return Err(StoreError::AttachmentInLockedNote);
        }

        let now_ms = monotonic_now_ms(prev_mod);
        tx.execute(
            "UPDATE note_attachments SET deleted = 1, synced = 0, date_modified = ?1
             WHERE attachment_id = ?2 AND deleted = 0",
            params![now_ms, att_id],
        )?;
        tx.execute(
            "UPDATE attachments SET deleted = 1, local_blob = NULL, synced = 0, date_modified = ?1
             WHERE id = ?2",
            params![now_ms, att_id],
        )?;

        tx.commit()?;
        Ok(())
    }
}

pub(crate) fn upsert_note_attachment_tx(
    conn: &Connection,
    note_id: &str,
    attachment_id: &str,
    now_ms: i64,
) -> StoreResult<()> {
    let new_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO note_attachments (id, note_id, attachment_id, deleted, synced, date_modified, revision)
         VALUES (?1, ?2, ?3, 0, 0, ?4, 0)
         ON CONFLICT(note_id, attachment_id) DO UPDATE SET
            deleted = 0,
            synced = 0,
            date_modified = excluded.date_modified",
        params![new_id, note_id, attachment_id, now_ms],
    )?;
    Ok(())
}

pub(crate) fn sync_note_attachment_links_tx(
    conn: &Connection,
    note_id: &str,
    markdown: &str,
    now_ms: i64,
) -> StoreResult<()> {
    let mut referenced_hashes = Vec::new();
    let mut search_slice = markdown;
    while let Some(pos) = search_slice.find("attachment://") {
        let after = &search_slice[pos + "attachment://".len()..];
        let hash: String = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if !hash.is_empty() && !referenced_hashes.contains(&hash) {
            referenced_hashes.push(hash);
        }
        search_slice = after;
    }

    for hash in referenced_hashes {
        if let Some(att_id) = conn
            .query_row(
                "SELECT id FROM attachments WHERE hash = ?1 AND deleted = 0",
                params![hash],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            upsert_note_attachment_tx(conn, note_id, &att_id, now_ms)?;
        }
    }
    Ok(())
}

pub(crate) fn load_note_attachments_conn(
    conn: &Connection,
    note_id: &str,
) -> StoreResult<Vec<Attachment>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.hash, a.filename, a.mime_type, a.size, a.encrypted, a.encryption_meta,
                a.storage_path, a.date_uploaded, a.date_created, a.date_modified, a.revision
         FROM note_attachments na
         JOIN attachments a ON a.id = na.attachment_id
         WHERE na.note_id = ?1 AND na.deleted = 0 AND a.deleted = 0
         ORDER BY a.date_created ASC",
    )?;
    let rows = stmt.query_map(params![note_id], map_attachment_row)?;
    let mut out = Vec::new();
    for r in rows {
        let mut att = r?;
        att.note_ids = load_attachment_note_ids_conn(conn, &att.id)?;
        out.push(att);
    }
    Ok(out)
}

fn load_attachment_note_ids_conn(
    conn: &Connection,
    attachment_id: &str,
) -> StoreResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT na.note_id
         FROM note_attachments na
         JOIN notes n ON n.id = na.note_id
         WHERE na.attachment_id = ?1 AND na.deleted = 0 AND n.deleted = 0",
    )?;
    let rows = stmt.query_map(params![attachment_id], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn map_attachment_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Attachment> {
    let enc_meta_raw: Option<String> = r.get(6)?;
    let encryption_meta =
        enc_meta_raw.and_then(|s| serde_json::from_str::<CipherEnvelope>(&s).ok());
    Ok(Attachment {
        id: r.get(0)?,
        user_id: None,
        hash: r.get(1)?,
        filename: r.get(2)?,
        mime_type: r.get(3)?,
        size: r.get::<_, i64>(4)?.max(0) as u64,
        encrypted: r.get::<_, i32>(5)? != 0,
        encryption_meta,
        storage_path: r.get(7)?,
        note_ids: Vec::new(),
        date_uploaded: opt_ms_to_utc(r.get(8)?),
        date_created: ms_to_utc(r.get(9)?),
        date_modified: ms_to_utc(r.get(10)?),
        deleted: false,
        revision: r.get(11)?,
    })
}
