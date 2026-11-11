use std::collections::HashMap;

use chrono::Utc;
use notes_core::{
    BackupBundle, BackupData, ContentFormat, DeletedBy, Note, NoteAttachment, NoteContent,
    NoteNotebook, NoteTag, Notebook, Shortcut, ShortcutItemType, SyncChangeset,
};
use notes_crypto::{seal_backup, unseal_backup};
use rusqlite::params;

use crate::error::StoreResult;
use crate::history::map_history_row;
use crate::organization::{map_monograph_row, map_reminder_row};
use crate::schema::{NotesStore, ms_to_utc, opt_ms_to_utc};
use crate::vault::load_active_vault_conn;

impl NotesStore {
    pub fn export_local_backup(&self, password: Option<&str>) -> StoreResult<BackupBundle> {
        let now = Utc::now();
        let conn = self.lock_conn();
        let mut data = BackupData::default();
        let mut cs = SyncChangeset::default();

        // Active Notes
        {
            let mut stmt = conn.prepare(
                "SELECT id, title, headline, is_generated_title, pinned, favorite, archived,
                        readonly, local_only, conflicted, locked, color_id, expiry_date, expiry_modified_at,
                        trashed_at, deleted_by, date_created, date_edited, date_modified, revision
                 FROM notes WHERE deleted = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                let del_by: Option<String> = r.get(15)?;
                Ok(Note {
                    id: r.get(0)?,
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
                    deleted_by: del_by.as_deref().and_then(DeletedBy::from_db_str),
                    deleted: false,
                    date_created: ms_to_utc(r.get(16)?),
                    date_edited: ms_to_utc(r.get(17)?),
                    date_modified: ms_to_utc(r.get(18)?),
                    revision: r.get(19)?,
                })
            })?;
            for r in rows {
                cs.notes.push(r?);
            }
        }

        // Active NoteContents
        {
            let mut stmt = conn.prepare(
                "SELECT nc.id, nc.note_id, nc.format, nc.data, nc.locked, nc.date_edited,
                        nc.date_modified, nc.date_resolved, nc.revision
                 FROM note_contents nc
                 JOIN notes n ON n.id = nc.note_id
                 WHERE nc.deleted = 0 AND n.deleted = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                let fmt: String = r.get(2)?;
                Ok(NoteContent {
                    id: r.get(0)?,
                    note_id: r.get(1)?,
                    format: ContentFormat::from_db_str(&fmt),
                    data: r.get(3)?,
                    locked: r.get::<_, i32>(4)? != 0,
                    conflicted_data: None,
                    date_edited: ms_to_utc(r.get(5)?),
                    date_modified: ms_to_utc(r.get(6)?),
                    date_resolved: opt_ms_to_utc(r.get(7)?),
                    deleted: false,
                    revision: r.get(8)?,
                })
            })?;
            for r in rows {
                cs.contents.push(r?);
            }
        }

        // Notebooks & NoteNotebooks
        {
            let mut stmt = conn.prepare(
                "SELECT id, parent_id, title, description, pinned, trashed_at, deleted_by,
                        date_created, date_edited, date_modified, revision
                 FROM notebooks WHERE deleted = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                let desc: String = r.get(3)?;
                let del_by: Option<String> = r.get(6)?;
                Ok(Notebook {
                    id: r.get(0)?,
                    user_id: None,
                    parent_id: r.get(1)?,
                    title: r.get(2)?,
                    description: (!desc.is_empty()).then_some(desc),
                    pinned: r.get::<_, i32>(4)? != 0,
                    trashed_at: opt_ms_to_utc(r.get(5)?),
                    deleted_by: del_by.as_deref().and_then(DeletedBy::from_db_str),
                    deleted: false,
                    date_created: ms_to_utc(r.get(7)?),
                    date_edited: ms_to_utc(r.get(8)?),
                    date_modified: ms_to_utc(r.get(9)?),
                    revision: r.get(10)?,
                })
            })?;
            for r in rows {
                cs.notebooks.push(r?);
            }
        }
        {
            let mut stmt = conn.prepare(
                "SELECT id, notebook_id, note_id, date_modified, revision
                 FROM note_notebooks WHERE deleted = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(NoteNotebook {
                    id: r.get(0)?,
                    notebook_id: r.get(1)?,
                    note_id: r.get(2)?,
                    date_modified: ms_to_utc(r.get(3)?),
                    deleted: false,
                    revision: r.get(4)?,
                })
            })?;
            for r in rows {
                cs.note_notebooks.push(r?);
            }
        }

        // NoteTags
        {
            let mut stmt = conn.prepare(
                "SELECT id, tag_id, note_id, date_modified, revision
                 FROM note_tags WHERE deleted = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(NoteTag {
                    id: r.get(0)?,
                    tag_id: r.get(1)?,
                    note_id: r.get(2)?,
                    date_modified: ms_to_utc(r.get(3)?),
                    deleted: false,
                    revision: r.get(4)?,
                })
            })?;
            for r in rows {
                cs.note_tags.push(r?);
            }
        }

        // Shortcuts
        {
            let mut stmt = conn.prepare(
                "SELECT id, item_id, item_type, sort_index, date_created, date_modified, revision
                 FROM shortcuts WHERE deleted = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                let ts: String = r.get(2)?;
                Ok(Shortcut {
                    id: r.get(0)?,
                    user_id: None,
                    item_id: r.get(1)?,
                    item_type: ShortcutItemType::from_db_str(&ts)
                        .unwrap_or(ShortcutItemType::Notebook),
                    sort_index: r.get(3)?,
                    date_created: ms_to_utc(r.get(4)?),
                    date_modified: ms_to_utc(r.get(5)?),
                    deleted: false,
                    revision: r.get(6)?,
                })
            })?;
            for r in rows {
                cs.shortcuts.push(r?);
            }
        }

        // Reminders
        {
            let mut stmt = conn.prepare(
                "SELECT id, note_id, title, description, date, mode, recurring_mode, selected_days,
                        priority, disabled, snooze_until, local_only, date_created, date_modified, revision
                 FROM reminders WHERE deleted = 0",
            )?;
            let rows = stmt.query_map([], |r| map_reminder_row(r, now))?;
            for r in rows {
                cs.reminders.push(r?);
            }
        }

        // NoteAttachments
        {
            let mut stmt = conn.prepare(
                "SELECT na.id, na.note_id, na.attachment_id, a.hash, na.date_modified, na.revision
                 FROM note_attachments na
                 JOIN attachments a ON a.id = na.attachment_id
                 WHERE na.deleted = 0 AND a.deleted = 0",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(NoteAttachment {
                    id: r.get(0)?,
                    note_id: r.get(1)?,
                    attachment_id: r.get(2)?,
                    attachment_hash: Some(r.get(3)?),
                    date_modified: ms_to_utc(r.get(4)?),
                    deleted: false,
                    revision: r.get(5)?,
                })
            })?;
            for r in rows {
                cs.note_attachments.push(r?);
            }
        }

        cs.vault = load_active_vault_conn(&conn)?;

        // HistorySessions & Monographs
        {
            let mut stmt = conn.prepare(
                "SELECT id, note_id, title, format, data, locked, compressed, device_id, date_created, date_modified
                 FROM history_sessions",
            )?;
            let rows = stmt.query_map([], map_history_row)?;
            for r in rows {
                data.history_sessions.push(r?);
            }
        }
        {
            let mut stmt = conn.prepare(
                "SELECT id, note_id, title, content, encrypted_content, self_destruct,
                        view_count, active, date_published, date_updated
                 FROM monographs WHERE deleted = 0",
            )?;
            let rows = stmt.query_map([], map_monograph_row)?;
            for r in rows {
                data.monographs.push(r?);
            }
        }

        drop(conn);

        cs.tags = self.list_tags()?;
        cs.colors = self.list_colors()?;
        cs.attachments = self.list_attachments()?;
        cs.settings = self.list_settings()?;
        data.changes = cs;

        Ok(seal_backup(&data, password, now)?)
    }

    pub fn import_local_backup(
        &self,
        bundle: &BackupBundle,
        password: Option<&str>,
    ) -> StoreResult<HashMap<String, usize>> {
        let data = unseal_backup(bundle, password)?;
        let cur_cursor = self.get_sync_cursor()?;
        self.merge_remote_changes(&data.changes, cur_cursor)?;

        // Restore history sessions & monographs idempotently
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;
        tx.execute_batch("PRAGMA defer_foreign_keys = ON;")?;

        for hs in &data.history_sessions {
            tx.execute(
                "INSERT INTO history_sessions (
                    id, note_id, session_key, title, format, data, locked, compressed, device_id, date_created, date_modified
                 ) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                    title = excluded.title,
                    format = excluded.format,
                    data = excluded.data,
                    locked = excluded.locked,
                    date_modified = excluded.date_modified",
                params![
                    hs.id,
                    hs.note_id,
                    hs.title,
                    hs.format.as_str(),
                    hs.data,
                    i32::from(hs.locked),
                    i32::from(hs.compressed),
                    hs.device_id,
                    hs.date_created.timestamp_millis(),
                    hs.date_modified.timestamp_millis()
                ],
            )?;
        }

        for m in &data.monographs {
            let enc_json = m
                .encrypted_content
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;
            tx.execute(
                "INSERT INTO monographs (
                    id, note_id, title, content, encrypted_content, self_destruct,
                    view_count, active, deleted, synced, date_published, date_updated, date_modified
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 0, ?9, ?10, ?10)
                 ON CONFLICT(note_id) DO UPDATE SET
                    title = excluded.title,
                    content = excluded.content,
                    encrypted_content = excluded.encrypted_content,
                    self_destruct = excluded.self_destruct,
                    active = excluded.active,
                    date_updated = excluded.date_updated,
                    date_modified = excluded.date_modified",
                params![
                    m.id,
                    m.note_id,
                    m.title,
                    m.content,
                    enc_json,
                    i32::from(m.self_destruct),
                    m.view_count as i64,
                    i32::from(m.active),
                    m.date_published.timestamp_millis(),
                    m.date_updated.timestamp_millis()
                ],
            )?;
        }

        tx.commit()?;

        let mut counts = HashMap::new();
        counts.insert("notes".to_owned(), data.changes.notes.len());
        counts.insert("notebooks".to_owned(), data.changes.notebooks.len());
        counts.insert("tags".to_owned(), data.changes.tags.len());
        counts.insert("colors".to_owned(), data.changes.colors.len());
        counts.insert("reminders".to_owned(), data.changes.reminders.len());
        counts.insert("attachments".to_owned(), data.changes.attachments.len());
        counts.insert("historySessions".to_owned(), data.history_sessions.len());
        Ok(counts)
    }
}
