use chrono::Utc;
use notes_core::{
    ContentFormat, HistorySession, NoteDetail, NoteDiff, compute_diff, extract_headline,
};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::error::{StoreError, StoreResult};
use crate::notes::{CreateNoteInput, load_note_detail_conn};
use crate::schema::{NotesStore, monotonic_now_ms, ms_to_utc};

impl NotesStore {
    pub fn record_history_session(
        &self,
        note_id: &str,
        session_key: Option<&str>,
        device_id: Option<&str>,
    ) -> StoreResult<HistorySession> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let detail = load_note_detail_conn(&tx, note_id)?;
        let content = detail
            .content
            .as_ref()
            .ok_or_else(|| StoreError::NotFound(format!("note_content for {note_id}")))?;
        let now_ms = Utc::now().timestamp_millis();

        if let Some(skey) = session_key.filter(|k| !k.trim().is_empty())
            && let Some(existing_id) = tx
                .query_row(
                    "SELECT id FROM history_sessions WHERE note_id = ?1 AND session_key = ?2",
                    params![note_id, skey],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
        {
            tx.execute(
                "UPDATE history_sessions SET
                    title = ?1, format = ?2, data = ?3, locked = ?4, date_modified = ?5
                 WHERE id = ?6",
                params![
                    detail.title,
                    content.format.as_str(),
                    content.data,
                    i32::from(detail.locked),
                    now_ms,
                    existing_id
                ],
            )?;
            let sess = load_history_session_conn(&tx, &existing_id)?;
            tx.commit()?;
            return Ok(sess);
        }

        let sid = insert_history_snapshot_with_key_tx(
            &tx,
            None,
            note_id,
            session_key,
            &detail.title,
            content.format,
            &content.data,
            detail.locked,
            device_id,
            now_ms,
        )?;
        let sess = load_history_session_conn(&tx, &sid)?;
        tx.commit()?;
        Ok(sess)
    }

    pub fn create_explicit_history_session(
        &self,
        note_id: &str,
        id: Option<String>,
        title: &str,
        data: &str,
        format: ContentFormat,
        locked: bool,
    ) -> StoreResult<HistorySession> {
        let conn = self.lock_conn();
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1 AND deleted = 0)",
            params![note_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(StoreError::NotFound(format!("note {note_id}")));
        }

        let now_ms = Utc::now().timestamp_millis();
        let sid = insert_history_snapshot_with_key_tx(
            &conn, id, note_id, None, title, format, data, locked, None, now_ms,
        )?;
        load_history_session_conn(&conn, &sid)
    }

    pub fn list_history_sessions(&self, note_id: &str) -> StoreResult<Vec<HistorySession>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT id, note_id, title, format, data, locked, compressed, device_id, date_created, date_modified
             FROM history_sessions
             WHERE note_id = ?1
             ORDER BY date_modified DESC, rowid DESC",
        )?;
        let rows = stmt.query_map(params![note_id], map_history_row)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn diff_history_sessions(
        &self,
        older_session_id: &str,
        newer_session_id: &str,
    ) -> StoreResult<NoteDiff> {
        let conn = self.lock_conn();
        let older = load_history_session_conn(&conn, older_session_id)?;
        let newer = load_history_session_conn(&conn, newer_session_id)?;
        if older.locked || newer.locked {
            return Err(StoreError::NoteLocked(older.note_id));
        }
        Ok(compute_diff(
            &older.title,
            &older.data,
            &newer.title,
            &newer.data,
        ))
    }

    /// Snapshots current pre-restore state as a new `HistorySession` FIRST,
    /// then overwrites the note with the target `HistorySession`!
    pub fn restore_history_session(
        &self,
        note_id: &str,
        session_id: &str,
    ) -> StoreResult<NoteDetail> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let current = load_note_detail_conn(&tx, note_id)?;
        if current.deleted {
            return Err(StoreError::NotFound(format!("note {note_id}")));
        }
        if current.readonly {
            return Err(StoreError::ReadonlyNote(note_id.to_owned()));
        }

        let target = load_history_session_conn(&tx, session_id)?;
        if target.note_id != note_id {
            return Err(StoreError::Validation(format!(
                "History session '{session_id}' does not belong to note '{note_id}'."
            )));
        }

        let snap_ms = monotonic_now_ms(current.date_modified.timestamp_millis());
        if let Some(ref cur_content) = current.content {
            insert_history_snapshot_tx(
                &tx,
                note_id,
                &current.title,
                cur_content.format,
                &cur_content.data,
                current.locked,
                None,
                snap_ms,
            )?;
        }

        let restore_ms = snap_ms + 1;
        let headline = if target.locked {
            String::new()
        } else {
            extract_headline(&target.data, false)
        };

        tx.execute(
            "UPDATE notes SET
                title = ?1,
                headline = ?2,
                locked = ?3,
                synced = 0,
                date_edited = ?4,
                date_modified = ?4
             WHERE id = ?5",
            params![
                target.title,
                headline,
                i32::from(target.locked),
                restore_ms,
                note_id
            ],
        )?;

        tx.execute(
            "UPDATE note_contents SET
                format = ?1,
                data = ?2,
                locked = ?3,
                synced = 0,
                date_edited = ?4,
                date_modified = ?4
             WHERE note_id = ?5",
            params![
                target.format.as_str(),
                target.data,
                i32::from(target.locked),
                restore_ms,
                note_id
            ],
        )?;

        tx.commit()?;
        drop(conn);
        self.get_note(note_id)
    }

    pub fn save_history_session_as_copy(&self, session_id: &str) -> StoreResult<NoteDetail> {
        let sess = {
            let conn = self.lock_conn();
            load_history_session_conn(&conn, session_id)?
        };
        let copy_title = format!("{} (Restored Copy)", sess.title);
        self.create_note(CreateNoteInput {
            id: None,
            title: Some(copy_title),
            content: Some(sess.data),
            format: sess.format,
            locked: sess.locked,
            ..Default::default()
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_history_snapshot_tx(
    conn: &Connection,
    note_id: &str,
    title: &str,
    format: ContentFormat,
    data: &str,
    locked: bool,
    device_id: Option<&str>,
    now_ms: i64,
) -> StoreResult<String> {
    insert_history_snapshot_with_key_tx(
        conn, None, note_id, None, title, format, data, locked, device_id, now_ms,
    )
}

#[allow(clippy::too_many_arguments)]
fn insert_history_snapshot_with_key_tx(
    conn: &Connection,
    explicit_id: Option<String>,
    note_id: &str,
    session_key: Option<&str>,
    title: &str,
    format: ContentFormat,
    data: &str,
    locked: bool,
    device_id: Option<&str>,
    now_ms: i64,
) -> StoreResult<String> {
    let sid = explicit_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    conn.execute(
        "INSERT INTO history_sessions (
            id, note_id, session_key, title, format, data, locked, compressed, device_id, date_created, date_modified
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?9)",
        params![
            sid,
            note_id,
            session_key,
            title,
            format.as_str(),
            data,
            i32::from(locked),
            device_id,
            now_ms
        ],
    )?;
    Ok(sid)
}

fn load_history_session_conn(conn: &Connection, session_id: &str) -> StoreResult<HistorySession> {
    conn.query_row(
        "SELECT id, note_id, title, format, data, locked, compressed, device_id, date_created, date_modified
         FROM history_sessions WHERE id = ?1",
        params![session_id],
        map_history_row,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound(format!("history_session {session_id}")))
}

pub(crate) fn map_history_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<HistorySession> {
    let fmt_str: String = r.get(3)?;
    Ok(HistorySession {
        id: r.get(0)?,
        note_id: r.get(1)?,
        title: r.get(2)?,
        format: ContentFormat::from_db_str(&fmt_str),
        data: r.get(4)?,
        locked: r.get::<_, i32>(5)? != 0,
        compressed: r.get::<_, i32>(6)? != 0,
        device_id: r.get(7)?,
        date_created: ms_to_utc(r.get(8)?),
        date_modified: ms_to_utc(r.get(9)?),
    })
}
