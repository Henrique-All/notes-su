use std::collections::HashSet;

use chrono::Utc;
use notes_core::{
    Attachment, Color, ConflictRecord, ConflictResolution, ConflictedContent, ContentFormat,
    DeletedBy, Note, NoteAttachment, NoteContent, NoteDetail, NoteNotebook, NoteTag, Notebook,
    SettingItem, Shortcut, ShortcutItemType, SyncChangeset, Tag, extract_headline,
    is_content_conflicted,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{StoreError, StoreResult};
use crate::notes::CreateNoteInput;
use crate::organization::map_reminder_row;
use crate::schema::{NotesStore, monotonic_now_ms, ms_to_utc, opt_ms_to_utc};
use crate::vault::load_active_vault_conn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SyncCollection {
    Notes,
    NoteContents,
    Notebooks,
    NoteNotebooks,
    Tags,
    NoteTags,
    Colors,
    Shortcuts,
    Reminders,
    Attachments,
    NoteAttachments,
    Vault,
    Settings,
}

impl SyncCollection {
    fn table_name(self) -> &'static str {
        match self {
            Self::Notes => "notes",
            Self::NoteContents => "note_contents",
            Self::Notebooks => "notebooks",
            Self::NoteNotebooks => "note_notebooks",
            Self::Tags => "tags",
            Self::NoteTags => "note_tags",
            Self::Colors => "colors",
            Self::Shortcuts => "shortcuts",
            Self::Reminders => "reminders",
            Self::Attachments => "attachments",
            Self::NoteAttachments => "note_attachments",
            Self::Vault => "vault",
            Self::Settings => "settings",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncAckToken {
    pub collection: SyncCollection,
    pub id: String,
    pub note_id: Option<String>,
    pub date_modified_ms: i64,
}

impl NotesStore {
    pub fn collect_unsynced_changes(&self) -> StoreResult<(SyncChangeset, Vec<SyncAckToken>)> {
        let conn = self.lock_conn();
        let mut changes = SyncChangeset::default();
        let mut tokens = Vec::new();
        let now = Utc::now();

        // 1. Notes (exclude local_only = 1)
        {
            let mut stmt = conn.prepare(
                "SELECT id, title, headline, is_generated_title, pinned, favorite, archived,
                        readonly, local_only, conflicted, locked, color_id, expiry_date, expiry_modified_at,
                        trashed_at, deleted_by, deleted, date_created, date_edited, date_modified, revision
                 FROM notes WHERE synced = 0 AND local_only = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                let id: String = r.get(0)?;
                let mod_ms: i64 = r.get(19)?;
                let del_by_str: Option<String> = r.get(15)?;
                Ok((
                    Note {
                        id: id.clone(),
                        user_id: None,
                        title: r.get(1)?,
                        headline: r.get(2)?,
                        is_generated_title: r.get::<_, i32>(3)? != 0,
                        pinned: r.get::<_, i32>(4)? != 0,
                        favorite: r.get::<_, i32>(5)? != 0,
                        archived: r.get::<_, i32>(6)? != 0,
                        readonly: r.get::<_, i32>(7)? != 0,
                        local_only: r.get::<_, i32>(8)? != 0,
                        conflicted: r.get::<_, i32>(9)? != 0,
                        locked: r.get::<_, i32>(10)? != 0,
                        color_id: r.get(11)?,
                        expiry_date: opt_ms_to_utc(r.get(12)?),
                        expiry_modified_at: opt_ms_to_utc(r.get(13)?),
                        trashed_at: opt_ms_to_utc(r.get(14)?),
                        deleted_by: del_by_str.as_deref().and_then(DeletedBy::from_db_str),
                        deleted: r.get::<_, i32>(16)? != 0,
                        date_created: ms_to_utc(r.get(17)?),
                        date_edited: ms_to_utc(r.get(18)?),
                        date_modified: ms_to_utc(mod_ms),
                        revision: r.get(20)?,
                    },
                    mod_ms,
                ))
            })?;
            for row in rows {
                let (note, mod_ms) = row?;
                tokens.push(SyncAckToken {
                    collection: SyncCollection::Notes,
                    id: note.id.clone(),
                    note_id: Some(note.id.clone()),
                    date_modified_ms: mod_ms,
                });
                changes.notes.push(note);
            }
        }

        // 2. NoteContents (exclude local_only = 1 on content OR parent note, and exclude conflicted = 1)
        {
            let mut stmt = conn.prepare(
                "SELECT nc.id, nc.note_id, nc.format, nc.data, nc.locked, nc.date_edited,
                        nc.date_modified, nc.date_resolved, nc.deleted, nc.revision
                 FROM note_contents nc
                 JOIN notes n ON n.id = nc.note_id
                 WHERE nc.synced = 0
                   AND nc.local_only = 0
                   AND n.local_only = 0
                   AND n.conflicted = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                let id: String = r.get(0)?;
                let note_id: String = r.get(1)?;
                let fmt_str: String = r.get(2)?;
                let mod_ms: i64 = r.get(6)?;
                Ok((
                    NoteContent {
                        id,
                        note_id,
                        format: ContentFormat::from_db_str(&fmt_str),
                        data: r.get(3)?,
                        locked: r.get::<_, i32>(4)? != 0,
                        conflicted_data: None,
                        date_edited: ms_to_utc(r.get(5)?),
                        date_modified: ms_to_utc(mod_ms),
                        date_resolved: opt_ms_to_utc(r.get(7)?),
                        deleted: r.get::<_, i32>(8)? != 0,
                        revision: r.get(9)?,
                    },
                    mod_ms,
                ))
            })?;
            for row in rows {
                let (nc, mod_ms) = row?;
                tokens.push(SyncAckToken {
                    collection: SyncCollection::NoteContents,
                    id: nc.id.clone(),
                    note_id: Some(nc.note_id.clone()),
                    date_modified_ms: mod_ms,
                });
                changes.contents.push(nc);
            }
        }

        // 3. Notebooks
        {
            let mut stmt = conn.prepare(
                "SELECT id, parent_id, title, description, pinned, trashed_at, deleted_by,
                        deleted, date_created, date_edited, date_modified, revision
                 FROM notebooks WHERE synced = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                let desc: String = r.get(3)?;
                let del_by_str: Option<String> = r.get(6)?;
                let mod_ms: i64 = r.get(10)?;
                Ok((
                    Notebook {
                        id: r.get(0)?,
                        user_id: None,
                        parent_id: r.get(1)?,
                        title: r.get(2)?,
                        description: (!desc.is_empty()).then_some(desc),
                        pinned: r.get::<_, i32>(4)? != 0,
                        trashed_at: opt_ms_to_utc(r.get(5)?),
                        deleted_by: del_by_str.as_deref().and_then(DeletedBy::from_db_str),
                        deleted: r.get::<_, i32>(7)? != 0,
                        date_created: ms_to_utc(r.get(8)?),
                        date_edited: ms_to_utc(r.get(9)?),
                        date_modified: ms_to_utc(mod_ms),
                        revision: r.get(11)?,
                    },
                    mod_ms,
                ))
            })?;
            for row in rows {
                let (nb, mod_ms) = row?;
                tokens.push(SyncAckToken {
                    collection: SyncCollection::Notebooks,
                    id: nb.id.clone(),
                    note_id: None,
                    date_modified_ms: mod_ms,
                });
                changes.notebooks.push(nb);
            }
        }

        // 4. NoteNotebooks (NEVER push join rows for local_only = 1 notes!)
        {
            let mut stmt = conn.prepare(
                "SELECT nn.id, nn.notebook_id, nn.note_id, nn.deleted, nn.date_modified, nn.revision
                 FROM note_notebooks nn
                 JOIN notes n ON n.id = nn.note_id
                 WHERE nn.synced = 0 AND n.local_only = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                let mod_ms: i64 = r.get(4)?;
                Ok((
                    NoteNotebook {
                        id: r.get(0)?,
                        notebook_id: r.get(1)?,
                        note_id: r.get(2)?,
                        deleted: r.get::<_, i32>(3)? != 0,
                        date_modified: ms_to_utc(mod_ms),
                        revision: r.get(5)?,
                    },
                    mod_ms,
                ))
            })?;
            for row in rows {
                let (nn, mod_ms) = row?;
                tokens.push(SyncAckToken {
                    collection: SyncCollection::NoteNotebooks,
                    id: nn.id.clone(),
                    note_id: Some(nn.note_id.clone()),
                    date_modified_ms: mod_ms,
                });
                changes.note_notebooks.push(nn);
            }
        }

        // 5. Tags
        {
            let mut stmt = conn.prepare(
                "SELECT id, title, deleted, date_created, date_modified, revision
                 FROM tags WHERE synced = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                let mod_ms: i64 = r.get(4)?;
                Ok((
                    Tag {
                        id: r.get(0)?,
                        user_id: None,
                        title: r.get(1)?,
                        note_count: None,
                        deleted: r.get::<_, i32>(2)? != 0,
                        date_created: ms_to_utc(r.get(3)?),
                        date_modified: ms_to_utc(mod_ms),
                        revision: r.get(5)?,
                    },
                    mod_ms,
                ))
            })?;
            for row in rows {
                let (t, mod_ms) = row?;
                tokens.push(SyncAckToken {
                    collection: SyncCollection::Tags,
                    id: t.id.clone(),
                    note_id: None,
                    date_modified_ms: mod_ms,
                });
                changes.tags.push(t);
            }
        }

        // 6. NoteTags (NEVER push join rows for local_only = 1 notes!)
        {
            let mut stmt = conn.prepare(
                "SELECT nt.id, nt.tag_id, nt.note_id, nt.deleted, nt.date_modified, nt.revision
                 FROM note_tags nt
                 JOIN notes n ON n.id = nt.note_id
                 WHERE nt.synced = 0 AND n.local_only = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                let mod_ms: i64 = r.get(4)?;
                Ok((
                    NoteTag {
                        id: r.get(0)?,
                        tag_id: r.get(1)?,
                        note_id: r.get(2)?,
                        deleted: r.get::<_, i32>(3)? != 0,
                        date_modified: ms_to_utc(mod_ms),
                        revision: r.get(5)?,
                    },
                    mod_ms,
                ))
            })?;
            for row in rows {
                let (nt, mod_ms) = row?;
                tokens.push(SyncAckToken {
                    collection: SyncCollection::NoteTags,
                    id: nt.id.clone(),
                    note_id: Some(nt.note_id.clone()),
                    date_modified_ms: mod_ms,
                });
                changes.note_tags.push(nt);
            }
        }

        // 7. Colors
        {
            let mut stmt = conn.prepare(
                "SELECT id, title, color_code, sort_index, deleted, date_created, date_modified, revision
                 FROM colors WHERE synced = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                let mod_ms: i64 = r.get(6)?;
                Ok((
                    Color {
                        id: r.get(0)?,
                        user_id: None,
                        title: r.get(1)?,
                        color_code: r.get(2)?,
                        sort_index: r.get(3)?,
                        note_count: None,
                        deleted: r.get::<_, i32>(4)? != 0,
                        date_created: ms_to_utc(r.get(5)?),
                        date_modified: ms_to_utc(mod_ms),
                        revision: r.get(7)?,
                    },
                    mod_ms,
                ))
            })?;
            for row in rows {
                let (c, mod_ms) = row?;
                tokens.push(SyncAckToken {
                    collection: SyncCollection::Colors,
                    id: c.id.clone(),
                    note_id: None,
                    date_modified_ms: mod_ms,
                });
                changes.colors.push(c);
            }
        }

        // 8. Shortcuts
        {
            let mut stmt = conn.prepare(
                "SELECT id, item_id, item_type, sort_index, deleted, date_created, date_modified, revision
                 FROM shortcuts WHERE synced = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                let t_str: String = r.get(2)?;
                let mod_ms: i64 = r.get(6)?;
                Ok((
                    Shortcut {
                        id: r.get(0)?,
                        user_id: None,
                        item_id: r.get(1)?,
                        item_type: ShortcutItemType::from_db_str(&t_str)
                            .unwrap_or(ShortcutItemType::Notebook),
                        sort_index: r.get(3)?,
                        deleted: r.get::<_, i32>(4)? != 0,
                        date_created: ms_to_utc(r.get(5)?),
                        date_modified: ms_to_utc(mod_ms),
                        revision: r.get(7)?,
                    },
                    mod_ms,
                ))
            })?;
            for row in rows {
                let (s, mod_ms) = row?;
                tokens.push(SyncAckToken {
                    collection: SyncCollection::Shortcuts,
                    id: s.id.clone(),
                    note_id: None,
                    date_modified_ms: mod_ms,
                });
                changes.shortcuts.push(s);
            }
        }

        // 9. Reminders (exclude local_only = 1 and reminders pointing to local_only = 1 notes)
        {
            let mut stmt = conn.prepare(
                "SELECT r.id, r.note_id, r.title, r.description, r.date, r.mode, r.recurring_mode,
                        r.selected_days, r.priority, r.disabled, r.snooze_until, r.local_only,
                        r.date_created, r.date_modified, r.revision, r.deleted
                 FROM reminders r
                 LEFT JOIN notes n ON n.id = r.note_id
                 WHERE r.synced = 0 AND r.local_only = 0 AND COALESCE(n.local_only, 0) = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                let mut rem = map_reminder_row(r, now)?;
                rem.deleted = r.get::<_, i32>(15)? != 0;
                let mod_ms: i64 = r.get(13)?;
                Ok((rem, mod_ms))
            })?;
            for row in rows {
                let (rem, mod_ms) = row?;
                tokens.push(SyncAckToken {
                    collection: SyncCollection::Reminders,
                    id: rem.id.clone(),
                    note_id: rem.note_id.clone(),
                    date_modified_ms: mod_ms,
                });
                changes.reminders.push(rem);
            }
        }

        // 10. Attachments & NoteAttachments
        {
            let mut stmt = conn.prepare(
                "SELECT id, hash, filename, mime_type, size, encrypted, encryption_meta,
                        storage_path, date_uploaded, date_created, date_modified, revision, deleted
                 FROM attachments WHERE synced = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                let enc_raw: Option<String> = r.get(6)?;
                let mod_ms: i64 = r.get(10)?;
                Ok((
                    Attachment {
                        id: r.get(0)?,
                        user_id: None,
                        hash: r.get(1)?,
                        filename: r.get(2)?,
                        mime_type: r.get(3)?,
                        size: r.get::<_, i64>(4)?.max(0) as u64,
                        encrypted: r.get::<_, i32>(5)? != 0,
                        encryption_meta: enc_raw.and_then(|s| serde_json::from_str(&s).ok()),
                        storage_path: r.get(7)?,
                        note_ids: Vec::new(),
                        date_uploaded: opt_ms_to_utc(r.get(8)?),
                        date_created: ms_to_utc(r.get(9)?),
                        date_modified: ms_to_utc(mod_ms),
                        revision: r.get(11)?,
                        deleted: r.get::<_, i32>(12)? != 0,
                    },
                    mod_ms,
                ))
            })?;
            for row in rows {
                let (att, mod_ms) = row?;
                tokens.push(SyncAckToken {
                    collection: SyncCollection::Attachments,
                    id: att.id.clone(),
                    note_id: None,
                    date_modified_ms: mod_ms,
                });
                changes.attachments.push(att);
            }
        }
        {
            let mut stmt = conn.prepare(
                "SELECT na.id, na.note_id, na.attachment_id, a.hash, na.deleted, na.date_modified, na.revision
                 FROM note_attachments na
                 JOIN notes n ON n.id = na.note_id
                 JOIN attachments a ON a.id = na.attachment_id
                 WHERE na.synced = 0 AND n.local_only = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                let mod_ms: i64 = r.get(5)?;
                Ok((
                    NoteAttachment {
                        id: r.get(0)?,
                        note_id: r.get(1)?,
                        attachment_id: r.get(2)?,
                        attachment_hash: Some(r.get(3)?),
                        deleted: r.get::<_, i32>(4)? != 0,
                        date_modified: ms_to_utc(mod_ms),
                        revision: r.get(6)?,
                    },
                    mod_ms,
                ))
            })?;
            for row in rows {
                let (na, mod_ms) = row?;
                tokens.push(SyncAckToken {
                    collection: SyncCollection::NoteAttachments,
                    id: na.id.clone(),
                    note_id: Some(na.note_id.clone()),
                    date_modified_ms: mod_ms,
                });
                changes.note_attachments.push(na);
            }
        }

        // 11. Vault (if unsynced and active)
        if let Some((vault_id, mod_ms)) = conn
            .query_row(
                "SELECT id, date_modified FROM vault WHERE synced = 0 AND deleted = 0 ORDER BY date_modified DESC LIMIT 1",
                [],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?
            && let Some(v) = load_active_vault_conn(&conn)?
        {
            tokens.push(SyncAckToken {
                collection: SyncCollection::Vault,
                id: vault_id,
                note_id: None,
                date_modified_ms: mod_ms,
            });
            changes.vault = Some(v);
        }

        // 12. Settings (exclude local_only = 1 and local:* keys)
        {
            let mut stmt = conn.prepare(
                "SELECT id, key, value, deleted, date_modified, revision
                 FROM settings WHERE synced = 0 AND local_only = 0 AND key NOT LIKE 'local:%'",
            )?;
            let rows = stmt.query_map([], |r| {
                let raw: String = r.get(2)?;
                let mod_ms: i64 = r.get(4)?;
                Ok((
                    SettingItem {
                        id: r.get(0)?,
                        user_id: None,
                        key: r.get(1)?,
                        value: serde_json::from_str(&raw)
                            .unwrap_or(serde_json::Value::String(raw)),
                        deleted: r.get::<_, i32>(3)? != 0,
                        date_modified: ms_to_utc(mod_ms),
                        revision: r.get(5)?,
                    },
                    mod_ms,
                ))
            })?;
            for row in rows {
                let (item, mod_ms) = row?;
                tokens.push(SyncAckToken {
                    collection: SyncCollection::Settings,
                    id: item.id.clone(),
                    note_id: None,
                    date_modified_ms: mod_ms,
                });
                changes.settings.push(item);
            }
        }

        Ok((changes, tokens))
    }

    /// Marks pushed items as `synced = 1` using Compare-And-Set (`WHERE id = ?1 AND date_modified = ?2`)
    /// so any concurrent local edit made while the push was in flight stays `synced = 0`!
    pub fn mark_pushed_synced(
        &self,
        ack_tokens: &[SyncAckToken],
        conflicts: &[ConflictRecord],
        new_cursor: i64,
    ) -> StoreResult<usize> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let conflicted_note_ids: HashSet<&str> =
            conflicts.iter().map(|c| c.note_id.as_str()).collect();

        for conflict in conflicts {
            tx.execute(
                "UPDATE notes SET conflicted = 1 WHERE id = ?1",
                params![conflict.note_id],
            )?;
            if let Some(ref cd) = conflict.conflicted_data {
                let cd_json = serde_json::to_string(cd)?;
                tx.execute(
                    "UPDATE note_contents SET conflicted_data = ?1 WHERE note_id = ?2",
                    params![cd_json, conflict.note_id],
                )?;
            }
        }

        let mut acked_count = 0usize;
        for tok in ack_tokens {
            if conflicted_note_ids.contains(tok.id.as_str()) {
                continue;
            }
            if let Some(ref nid) = tok.note_id
                && conflicted_note_ids.contains(nid.as_str())
            {
                continue;
            }
            let sql = format!(
                "UPDATE {} SET synced = 1 WHERE id = ?1 AND date_modified = ?2",
                tok.collection.table_name()
            );
            acked_count += tx.execute(&sql, params![tok.id, tok.date_modified_ms])?;
        }

        update_sync_cursor_monotonic_tx(&tx, new_cursor)?;
        tx.commit()?;
        Ok(acked_count)
    }

    pub fn merge_remote_changes(
        &self,
        changes: &SyncChangeset,
        next_cursor: i64,
    ) -> StoreResult<Vec<String>> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;
        tx.execute_batch("PRAGMA defer_foreign_keys = ON;")?;

        let mut conflicted_ids: Vec<String> = Vec::new();
        let now_ms = Utc::now().timestamp_millis();

        // 1. Colors (deduplicate auto-seeded default colors on first sync!)
        for rc in &changes.colors {
            let r_mod = rc.date_modified.timestamp_millis();
            if !rc.deleted
                && let Some(dup_id) = tx
                    .query_row(
                        "SELECT id FROM colors
                         WHERE id != ?1
                           AND LOWER(title) = LOWER(?2)
                           AND LOWER(color_code) = LOWER(?3)
                           AND synced = 1 AND revision = 0 AND deleted = 0",
                        params![rc.id, rc.title, rc.color_code],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()?
            {
                tx.execute(
                    "UPDATE notes SET color_id = NULL WHERE color_id = ?1",
                    params![dup_id],
                )?;
                tx.execute("DELETE FROM colors WHERE id = ?1", params![dup_id])?;
                // Insert remote color first, then point affected notes to rc.id
                upsert_remote_color_tx(&tx, rc, r_mod)?;
                continue;
            }

            if should_accept_remote_tx(&tx, "colors", &rc.id, r_mod)? {
                upsert_remote_color_tx(&tx, rc, r_mod)?;
            }
        }

        // 2. Vault
        if let Some(ref rv) = changes.vault {
            let r_mod = rv.date_modified.timestamp_millis();
            if should_accept_remote_tx(&tx, "vault", &rv.id, r_mod)? {
                let canary_json = serde_json::to_string(&rv.key_canary)?;
                tx.execute(
                    "INSERT INTO vault (id, title, key_canary, lock_after_seconds, deleted, synced, date_created, date_modified, revision)
                     VALUES (?1, ?2, ?3, ?4, 0, 1, ?5, ?6, ?7)
                     ON CONFLICT(id) DO UPDATE SET
                        title = excluded.title,
                        key_canary = excluded.key_canary,
                        lock_after_seconds = excluded.lock_after_seconds,
                        deleted = 0,
                        synced = 1,
                        date_modified = excluded.date_modified,
                        revision = excluded.revision",
                    params![
                        rv.id,
                        rv.title,
                        canary_json,
                        rv.lock_after_seconds,
                        rv.date_created.timestamp_millis(),
                        r_mod,
                        rv.revision
                    ],
                )?;
            }
        }

        // 3. Notebooks (two-pass parent_id so child-before-parent never violates FK)
        let mut accepted_notebooks: Vec<&Notebook> = Vec::new();
        for rnb in &changes.notebooks {
            let r_mod = rnb.date_modified.timestamp_millis();
            if should_accept_remote_tx(&tx, "notebooks", &rnb.id, r_mod)? {
                tx.execute(
                    "INSERT INTO notebooks (
                        id, parent_id, title, description, pinned, trashed_at, deleted_by,
                        deleted, synced, date_created, date_edited, date_modified, revision
                     ) VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9, ?10, ?11)
                     ON CONFLICT(id) DO UPDATE SET
                        title = excluded.title,
                        description = excluded.description,
                        pinned = excluded.pinned,
                        trashed_at = excluded.trashed_at,
                        deleted_by = excluded.deleted_by,
                        deleted = excluded.deleted,
                        synced = 1,
                        date_edited = excluded.date_edited,
                        date_modified = excluded.date_modified,
                        revision = excluded.revision",
                    params![
                        rnb.id,
                        rnb.title,
                        rnb.description.as_deref().unwrap_or(""),
                        i32::from(rnb.pinned),
                        rnb.trashed_at.map(|d| d.timestamp_millis()),
                        rnb.deleted_by.map(|d| d.as_str()),
                        i32::from(rnb.deleted),
                        rnb.date_created.timestamp_millis(),
                        rnb.date_edited.timestamp_millis(),
                        r_mod,
                        rnb.revision
                    ],
                )?;
                accepted_notebooks.push(rnb);
            }
        }
        for rnb in accepted_notebooks {
            tx.execute(
                "UPDATE notebooks SET parent_id = ?1 WHERE id = ?2",
                params![rnb.parent_id, rnb.id],
            )?;
        }

        // 4. Tags (deduplicate case-sensitive title collisions across devices)
        for rt in &changes.tags {
            let r_mod = rt.date_modified.timestamp_millis();
            if !rt.deleted
                && let Some(dup_tag_id) = tx
                    .query_row(
                        "SELECT id FROM tags WHERE title = ?1 COLLATE BINARY AND deleted = 0 AND id != ?2",
                        params![rt.title, rt.id],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()?
            {
                tx.execute(
                    "DELETE FROM tags WHERE id = ?1",
                    params![dup_tag_id],
                )?;
            }
            if should_accept_remote_tx(&tx, "tags", &rt.id, r_mod)? {
                tx.execute(
                    "INSERT INTO tags (id, title, deleted, synced, date_created, date_modified, revision)
                     VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6)
                     ON CONFLICT(id) DO UPDATE SET
                        title = excluded.title,
                        deleted = excluded.deleted,
                        synced = 1,
                        date_modified = excluded.date_modified,
                        revision = excluded.revision",
                    params![
                        rt.id,
                        rt.title,
                        i32::from(rt.deleted),
                        rt.date_created.timestamp_millis(),
                        r_mod,
                        rt.revision
                    ],
                )?;
            }
        }

        struct LocalNoteMeta {
            synced: i32,
            date_modified: i64,
            deleted_by: Option<String>,
            expiry_modified_at: Option<i64>,
            conflicted: i32,
        }

        // 5. Notes (including Edge Case #7 expiry auto-restore!)
        for rn in &changes.notes {
            let r_mod = rn.date_modified.timestamp_millis();
            let local_row: Option<LocalNoteMeta> = tx
                .query_row(
                    "SELECT synced, date_modified, deleted_by, expiry_modified_at, conflicted
                     FROM notes WHERE id = ?1",
                    params![rn.id],
                    |r| {
                        Ok(LocalNoteMeta {
                            synced: r.get(0)?,
                            date_modified: r.get(1)?,
                            deleted_by: r.get(2)?,
                            expiry_modified_at: r.get(3)?,
                            conflicted: r.get(4)?,
                        })
                    },
                )
                .optional()?;

            let r_exp_ms = rn.expiry_date.map(|d| d.timestamp_millis());
            let r_exp_mod_ms = rn.expiry_modified_at.map(|d| d.timestamp_millis());

            let expiry_auto_restore = if let Some(ref l) = local_row {
                l.deleted_by.as_deref() == Some("expired")
                    && rn.trashed_at.is_none()
                    && r_exp_mod_ms.unwrap_or(0) > l.expiry_modified_at.unwrap_or(0)
                    && r_exp_ms.is_none_or(|exp| exp > now_ms)
            } else {
                false
            };

            let accept = match local_row {
                None => true,
                Some(ref l) => expiry_auto_restore || l.synced == 1 || r_mod >= l.date_modified,
            };

            if accept {
                let effective_trashed = if expiry_auto_restore {
                    None
                } else {
                    rn.trashed_at.map(|d| d.timestamp_millis())
                };
                let effective_del_by = if expiry_auto_restore {
                    None
                } else {
                    rn.deleted_by.map(|d| d.as_str())
                };
                let keep_conflicted =
                    local_row.as_ref().is_some_and(|r| r.conflicted != 0) && !rn.deleted;
                let safe_headline = if rn.locked { "" } else { rn.headline.as_str() };

                tx.execute(
                    "INSERT INTO notes (
                        id, title, headline, is_generated_title, pinned, favorite, archived,
                        readonly, local_only, conflicted, locked, color_id, expiry_date, expiry_modified_at,
                        trashed_at, deleted_by, deleted, synced, date_created, date_edited, date_modified, revision
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 1, ?17, ?18, ?19, ?20)
                     ON CONFLICT(id) DO UPDATE SET
                        title = excluded.title,
                        headline = excluded.headline,
                        is_generated_title = excluded.is_generated_title,
                        pinned = excluded.pinned,
                        favorite = excluded.favorite,
                        archived = excluded.archived,
                        readonly = excluded.readonly,
                        conflicted = excluded.conflicted,
                        locked = excluded.locked,
                        color_id = excluded.color_id,
                        expiry_date = excluded.expiry_date,
                        expiry_modified_at = excluded.expiry_modified_at,
                        trashed_at = excluded.trashed_at,
                        deleted_by = excluded.deleted_by,
                        deleted = excluded.deleted,
                        synced = 1,
                        date_edited = excluded.date_edited,
                        date_modified = excluded.date_modified,
                        revision = excluded.revision",
                    params![
                        rn.id,
                        rn.title,
                        safe_headline,
                        i32::from(rn.is_generated_title),
                        i32::from(rn.pinned),
                        i32::from(rn.favorite),
                        i32::from(rn.archived),
                        i32::from(rn.readonly),
                        i32::from(rn.conflicted || keep_conflicted),
                        i32::from(rn.locked),
                        rn.color_id,
                        r_exp_ms,
                        r_exp_mod_ms,
                        effective_trashed,
                        effective_del_by,
                        i32::from(rn.deleted),
                        rn.date_created.timestamp_millis(),
                        rn.date_edited.timestamp_millis(),
                        r_mod,
                        rn.revision
                    ],
                )?;
            }
        }

        struct LocalContentMeta {
            data: String,
            locked: i32,
            date_edited: i64,
            date_modified: i64,
            date_resolved: Option<i64>,
            synced: i32,
        }

        // 6. NoteContents (60-second conflict preservation!)
        for rnc in &changes.contents {
            let r_edited_ms = rnc.date_edited.timestamp_millis();
            let r_mod_ms = rnc.date_modified.timestamp_millis();

            let local_nc: Option<LocalContentMeta> = tx
                .query_row(
                    "SELECT data, locked, date_edited, date_modified, date_resolved, synced
                     FROM note_contents WHERE note_id = ?1",
                    params![rnc.note_id],
                    |r| {
                        Ok(LocalContentMeta {
                            data: r.get(0)?,
                            locked: r.get(1)?,
                            date_edited: r.get(2)?,
                            date_modified: r.get(3)?,
                            date_resolved: r.get(4)?,
                            synced: r.get(5)?,
                        })
                    },
                )
                .optional()?;

            if let Some(l_nc) = local_nc
                && l_nc.synced == 0
            {
                let already_resolved = l_nc.date_resolved.is_some_and(|res| res >= r_mod_ms);
                if already_resolved {
                    continue;
                }

                let l_locked = l_nc.locked != 0;
                if is_content_conflicted(
                    &l_nc.data,
                    l_locked,
                    ms_to_utc(l_nc.date_edited),
                    true,
                    &rnc.data,
                    rnc.locked,
                    rnc.date_edited,
                ) {
                    let conf_payload = ConflictedContent {
                        data: rnc.data.clone(),
                        locked: rnc.locked,
                        format: rnc.format,
                        date_edited: rnc.date_edited,
                        date_modified: rnc.date_modified,
                        device_id: None,
                    };
                    let conf_json = serde_json::to_string(&conf_payload)?;
                    tx.execute(
                        "UPDATE note_contents SET conflicted_data = ?1 WHERE note_id = ?2",
                        params![conf_json, rnc.note_id],
                    )?;
                    tx.execute(
                        "UPDATE notes SET conflicted = 1 WHERE id = ?1",
                        params![rnc.note_id],
                    )?;
                    conflicted_ids.push(rnc.note_id.clone());
                    continue;
                }

                if r_mod_ms < l_nc.date_modified {
                    continue;
                }
            }

            let conf_json = rnc
                .conflicted_data
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;

            tx.execute(
                "INSERT INTO note_contents (
                    id, note_id, format, locked, data, conflicted_data, local_only,
                    deleted, synced, date_edited, date_modified, date_resolved, revision
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, 1, ?8, ?9, ?10, ?11)
                 ON CONFLICT(note_id) DO UPDATE SET
                    format = excluded.format,
                    locked = excluded.locked,
                    data = excluded.data,
                    conflicted_data = COALESCE(excluded.conflicted_data, note_contents.conflicted_data),
                    deleted = excluded.deleted,
                    synced = 1,
                    date_edited = excluded.date_edited,
                    date_modified = excluded.date_modified,
                    revision = excluded.revision",
                params![
                    rnc.id,
                    rnc.note_id,
                    rnc.format.as_str(),
                    i32::from(rnc.locked),
                    rnc.data,
                    conf_json,
                    i32::from(rnc.deleted),
                    r_edited_ms,
                    r_mod_ms,
                    rnc.date_resolved.map(|d| d.timestamp_millis()),
                    rnc.revision
                ],
            )?;

            if !rnc.deleted {
                let hl = extract_headline(&rnc.data, rnc.locked);
                tx.execute(
                    "UPDATE notes SET locked = ?1, headline = ?2 WHERE id = ?3 AND deleted = 0",
                    params![i32::from(rnc.locked), hl, rnc.note_id],
                )?;
            }
        }

        // 7. Join & Auxiliary Tables (LWW)
        for rnn in &changes.note_notebooks {
            let r_mod = rnn.date_modified.timestamp_millis();
            tx.execute(
                "INSERT INTO note_notebooks (id, notebook_id, note_id, deleted, synced, date_modified, revision)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6)
                 ON CONFLICT(notebook_id, note_id) DO UPDATE SET
                    deleted = excluded.deleted,
                    synced = 1,
                    date_modified = excluded.date_modified,
                    revision = excluded.revision
                 WHERE note_notebooks.synced = 1 OR excluded.date_modified >= note_notebooks.date_modified",
                params![
                    rnn.id,
                    rnn.notebook_id,
                    rnn.note_id,
                    i32::from(rnn.deleted),
                    r_mod,
                    rnn.revision
                ],
            )?;
        }

        for rnt in &changes.note_tags {
            let r_mod = rnt.date_modified.timestamp_millis();
            tx.execute(
                "INSERT INTO note_tags (id, tag_id, note_id, deleted, synced, date_modified, revision)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6)
                 ON CONFLICT(tag_id, note_id) DO UPDATE SET
                    deleted = excluded.deleted,
                    synced = 1,
                    date_modified = excluded.date_modified,
                    revision = excluded.revision
                 WHERE note_tags.synced = 1 OR excluded.date_modified >= note_tags.date_modified",
                params![
                    rnt.id,
                    rnt.tag_id,
                    rnt.note_id,
                    i32::from(rnt.deleted),
                    r_mod,
                    rnt.revision
                ],
            )?;
        }

        for ra in &changes.attachments {
            let r_mod = ra.date_modified.timestamp_millis();
            let enc_meta_str = ra
                .encryption_meta
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;
            if should_accept_remote_tx(&tx, "attachments", &ra.id, r_mod)? {
                tx.execute(
                    "INSERT INTO attachments (
                        id, hash, filename, mime_type, size, encrypted, encryption_meta,
                        local_blob, storage_path, date_uploaded, failed, deleted, synced, date_created, date_modified, revision
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9, NULL, ?10, 1, ?11, ?12, ?13)
                     ON CONFLICT(id) DO UPDATE SET
                        hash = excluded.hash,
                        filename = excluded.filename,
                        mime_type = excluded.mime_type,
                        size = excluded.size,
                        encrypted = excluded.encrypted,
                        encryption_meta = excluded.encryption_meta,
                        storage_path = excluded.storage_path,
                        date_uploaded = excluded.date_uploaded,
                        deleted = excluded.deleted,
                        synced = 1,
                        date_modified = excluded.date_modified,
                        revision = excluded.revision",
                    params![
                        ra.id,
                        ra.hash,
                        ra.filename,
                        ra.mime_type,
                        ra.size as i64,
                        i32::from(ra.encrypted),
                        enc_meta_str,
                        ra.storage_path,
                        ra.date_uploaded.map(|d| d.timestamp_millis()),
                        i32::from(ra.deleted),
                        ra.date_created.timestamp_millis(),
                        r_mod,
                        ra.revision
                    ],
                )?;
            }
        }

        for rna in &changes.note_attachments {
            let r_mod = rna.date_modified.timestamp_millis();
            tx.execute(
                "INSERT INTO note_attachments (id, note_id, attachment_id, deleted, synced, date_modified, revision)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6)
                 ON CONFLICT(note_id, attachment_id) DO UPDATE SET
                    deleted = excluded.deleted,
                    synced = 1,
                    date_modified = excluded.date_modified,
                    revision = excluded.revision
                 WHERE note_attachments.synced = 1 OR excluded.date_modified >= note_attachments.date_modified",
                params![
                    rna.id,
                    rna.note_id,
                    rna.attachment_id,
                    i32::from(rna.deleted),
                    r_mod,
                    rna.revision
                ],
            )?;
        }

        for rs in &changes.shortcuts {
            let r_mod = rs.date_modified.timestamp_millis();
            if should_accept_remote_tx(&tx, "shortcuts", &rs.id, r_mod)? {
                tx.execute(
                    "INSERT INTO shortcuts (id, item_id, item_type, sort_index, deleted, synced, date_created, date_modified, revision)
                     VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, ?8)
                     ON CONFLICT(id) DO UPDATE SET
                        item_id = excluded.item_id,
                        item_type = excluded.item_type,
                        sort_index = excluded.sort_index,
                        deleted = excluded.deleted,
                        synced = 1,
                        date_modified = excluded.date_modified,
                        revision = excluded.revision",
                    params![
                        rs.id,
                        rs.item_id,
                        rs.item_type.as_str(),
                        rs.sort_index,
                        i32::from(rs.deleted),
                        rs.date_created.timestamp_millis(),
                        r_mod,
                        rs.revision
                    ],
                )?;
            }
        }

        for rr in &changes.reminders {
            let r_mod = rr.date_modified.timestamp_millis();
            if should_accept_remote_tx(&tx, "reminders", &rr.id, r_mod)? {
                let days_json = serde_json::to_string(&rr.selected_days)?;
                tx.execute(
                    "INSERT INTO reminders (
                        id, note_id, title, description, date, mode, recurring_mode, selected_days,
                        priority, disabled, snooze_until, local_only, deleted, synced, date_created, date_modified, revision
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, 1, ?13, ?14, ?15)
                     ON CONFLICT(id) DO UPDATE SET
                        note_id = excluded.note_id,
                        title = excluded.title,
                        description = excluded.description,
                        date = excluded.date,
                        mode = excluded.mode,
                        recurring_mode = excluded.recurring_mode,
                        selected_days = excluded.selected_days,
                        priority = excluded.priority,
                        disabled = excluded.disabled,
                        snooze_until = excluded.snooze_until,
                        deleted = excluded.deleted,
                        synced = 1,
                        date_modified = excluded.date_modified,
                        revision = excluded.revision",
                    params![
                        rr.id,
                        rr.note_id,
                        rr.title,
                        rr.description.as_deref().unwrap_or(""),
                        rr.date.timestamp_millis(),
                        rr.mode.as_str(),
                        rr.recurring_mode.map(|m| m.as_str()),
                        days_json,
                        rr.priority.as_str(),
                        i32::from(rr.disabled),
                        rr.snooze_until.map(|d| d.timestamp_millis()),
                        i32::from(rr.deleted),
                        rr.date_created.timestamp_millis(),
                        r_mod,
                        rr.revision
                    ],
                )?;
            }
        }

        for rset in &changes.settings {
            let r_mod = rset.date_modified.timestamp_millis();
            let val_str = serde_json::to_string(&rset.value)?;
            tx.execute(
                "INSERT INTO settings (id, key, value, local_only, deleted, synced, date_modified, revision)
                 VALUES (?1, ?2, ?3, 0, ?4, 1, ?5, ?6)
                 ON CONFLICT(key) DO UPDATE SET
                    value = excluded.value,
                    deleted = excluded.deleted,
                    synced = 1,
                    date_modified = excluded.date_modified,
                    revision = excluded.revision
                 WHERE settings.synced = 1 OR excluded.date_modified >= settings.date_modified",
                params![
                    rset.id,
                    rset.key,
                    val_str,
                    i32::from(rset.deleted),
                    r_mod,
                    rset.revision
                ],
            )?;
        }

        update_sync_cursor_monotonic_tx(&tx, next_cursor)?;
        tx.commit()?;
        Ok(conflicted_ids)
    }

    pub fn resolve_conflict(
        &self,
        note_id: &str,
        resolution: ConflictResolution,
        resolved_data: Option<&str>,
    ) -> StoreResult<(NoteDetail, Option<NoteDetail>)> {
        let current = self.get_note(note_id)?;
        let cur_content = current
            .content
            .as_ref()
            .ok_or_else(|| StoreError::NotFound(format!("note_content for {note_id}")))?;
        let remote_conflicted = cur_content.conflicted_data.clone();

        let now_ms = monotonic_now_ms(current.date_modified.timestamp_millis());

        let (winner_data, winner_locked) = match resolution {
            ConflictResolution::KeepLocal | ConflictResolution::KeepBoth => (
                resolved_data
                    .map(str::to_owned)
                    .unwrap_or_else(|| cur_content.data.clone()),
                current.locked,
            ),
            ConflictResolution::KeepRemote => {
                if let Some(explicit) = resolved_data {
                    (
                        explicit.to_owned(),
                        remote_conflicted
                            .as_ref()
                            .map(|c| c.locked)
                            .unwrap_or(current.locked),
                    )
                } else if let Some(ref rc) = remote_conflicted {
                    (rc.data.clone(), rc.locked)
                } else {
                    (cur_content.data.clone(), current.locked)
                }
            }
        };

        {
            let mut conn = self.lock_conn();
            let tx = conn.transaction()?;
            let hl = extract_headline(&winner_data, winner_locked);

            tx.execute(
                "UPDATE notes SET
                    conflicted = 0,
                    locked = ?1,
                    headline = ?2,
                    synced = 0,
                    date_edited = ?3,
                    date_modified = ?3
                 WHERE id = ?4",
                params![i32::from(winner_locked), hl, now_ms, note_id],
            )?;
            tx.execute(
                "UPDATE note_contents SET
                    data = ?1,
                    locked = ?2,
                    conflicted_data = NULL,
                    date_resolved = ?3,
                    synced = 0,
                    date_edited = ?3,
                    date_modified = ?3
                 WHERE note_id = ?4",
                params![winner_data, i32::from(winner_locked), now_ms, note_id],
            )?;
            tx.commit()?;
        }

        let copy_note = if resolution == ConflictResolution::KeepBoth {
            if let Some(rc) = remote_conflicted {
                let copy_title = format!("{} (Conflict Copy)", current.title);
                Some(self.create_note(CreateNoteInput {
                    id: None,
                    title: Some(copy_title),
                    content: Some(rc.data),
                    format: rc.format,
                    pinned: false,
                    favorite: current.favorite,
                    archived: current.archived,
                    readonly: false,
                    local_only: current.local_only,
                    locked: rc.locked,
                    color_id: current.color_id.clone(),
                    expiry_date: None,
                    notebook_ids: current.notebook_ids.clone(),
                    tags: current.tags.iter().map(|t| t.title.clone()).collect(),
                    apply_default_org_settings: false,
                })?)
            } else {
                None
            }
        } else {
            None
        };

        let updated = self.get_note(note_id)?;
        Ok((updated, copy_note))
    }
}

fn should_accept_remote_tx(
    conn: &Connection,
    table: &str,
    id: &str,
    remote_mod_ms: i64,
) -> StoreResult<bool> {
    let sql = format!("SELECT synced, date_modified FROM {table} WHERE id = ?1");
    let row: Option<(i32, i64)> = conn
        .query_row(&sql, params![id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    Ok(match row {
        None => true,
        Some((synced, local_mod)) => synced == 1 || remote_mod_ms >= local_mod,
    })
}

fn upsert_remote_color_tx(conn: &Connection, rc: &Color, r_mod: i64) -> StoreResult<()> {
    conn.execute(
        "INSERT INTO colors (id, title, color_code, sort_index, deleted, synced, date_created, date_modified, revision)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
            title = excluded.title,
            color_code = excluded.color_code,
            sort_index = excluded.sort_index,
            deleted = excluded.deleted,
            synced = 1,
            date_modified = excluded.date_modified,
            revision = excluded.revision",
        params![
            rc.id,
            rc.title,
            rc.color_code,
            rc.sort_index,
            i32::from(rc.deleted),
            rc.date_created.timestamp_millis(),
            r_mod,
            rc.revision
        ],
    )?;
    Ok(())
}

fn update_sync_cursor_monotonic_tx(conn: &Connection, next_cursor: i64) -> StoreResult<()> {
    let now_ms = Utc::now().timestamp_millis();
    let updated = conn.execute(
        "UPDATE auth_session SET sync_cursor = MAX(sync_cursor, ?1), updated_at = ?2 WHERE id = 1",
        params![next_cursor, now_ms],
    )?;
    if updated == 0 {
        let cur: i64 = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'local:syncCursor' AND deleted = 0",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .and_then(|s| serde_json::from_str::<i64>(&s).ok())
            .unwrap_or(0);
        let merged_cursor = cur.max(next_cursor);
        let val_json = serde_json::to_string(&merged_cursor)?;
        conn.execute(
            "INSERT INTO settings (id, key, value, local_only, deleted, synced, date_modified, revision)
             VALUES (?1, 'local:syncCursor', ?2, 1, 0, 1, ?3, 0)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, date_modified = excluded.date_modified",
            params![Uuid::new_v4().to_string(), val_json, now_ms],
        )?;
    }
    Ok(())
}
