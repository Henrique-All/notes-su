use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use notes_core::{DeletedBy, NoteDetail, NoteSummary, NotebookDto};
use rusqlite::{OptionalExtension, params};

use crate::error::{StoreError, StoreResult};
use crate::notebooks::{load_notebook_dto_conn, map_notebook_dto_row, query_recursive_note_counts_conn};
use crate::notes::query_all_summaries_conn;
use crate::schema::{NotesStore, monotonic_now_ms};

#[derive(Debug, Clone)]
pub struct TrashView {
    pub notes: Vec<NoteSummary>,
    pub notebooks: Vec<NotebookDto>,
}

impl NotesStore {
    pub fn trash_note(&self, note_id: &str) -> StoreResult<NoteDetail> {
        let conn = self.lock_conn();
        let prev_mod: i64 = conn
            .query_row(
                "SELECT date_modified FROM notes WHERE id = ?1 AND deleted = 0",
                params![note_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("note {note_id}")))?;
        let now_ms = monotonic_now_ms(prev_mod);

        conn.execute(
            "UPDATE notes SET trashed_at = ?1, deleted_by = 'user', synced = 0, date_modified = ?1
             WHERE id = ?2 AND deleted = 0",
            params![now_ms, note_id],
        )?;
        drop(conn);
        self.get_note(note_id)
    }

    pub fn restore_note(&self, note_id: &str) -> StoreResult<NoteDetail> {
        let conn = self.lock_conn();
        let prev_mod: i64 = conn
            .query_row(
                "SELECT date_modified FROM notes WHERE id = ?1 AND deleted = 0",
                params![note_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("note {note_id}")))?;
        let now_ms = monotonic_now_ms(prev_mod);

        conn.execute(
            "UPDATE notes SET trashed_at = NULL, deleted_by = NULL, synced = 0, date_modified = ?1
             WHERE id = ?2 AND deleted = 0",
            params![now_ms, note_id],
        )?;
        drop(conn);
        self.get_note(note_id)
    }

    pub fn delete_note(
        &self,
        note_id: &str,
        permanent: bool,
        vault_verified: bool,
    ) -> StoreResult<Option<NoteDetail>> {
        if !permanent {
            let trashed = self.trash_note(note_id)?;
            return Ok(Some(trashed));
        }

        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let (locked, prev_mod): (i32, i64) = tx
            .query_row(
                "SELECT locked, date_modified FROM notes WHERE id = ?1 AND deleted = 0",
                params![note_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("note {note_id}")))?;

        if locked != 0 && !vault_verified {
            return Err(StoreError::VaultVerificationRequired);
        }

        let now_ms = monotonic_now_ms(prev_mod);
        tx.execute(
            "DELETE FROM history_sessions WHERE note_id = ?1",
            params![note_id],
        )?;
        tx.execute(
            "UPDATE note_notebooks SET deleted = 1, synced = 0, date_modified = ?1 WHERE note_id = ?2 AND deleted = 0",
            params![now_ms, note_id],
        )?;
        tx.execute(
            "UPDATE note_tags SET deleted = 1, synced = 0, date_modified = ?1 WHERE note_id = ?2 AND deleted = 0",
            params![now_ms, note_id],
        )?;
        tx.execute(
            "UPDATE note_attachments SET deleted = 1, synced = 0, date_modified = ?1 WHERE note_id = ?2 AND deleted = 0",
            params![now_ms, note_id],
        )?;
        tx.execute(
            "UPDATE monographs SET active = 0, deleted = 1, synced = 0, date_modified = ?1 WHERE note_id = ?2 AND deleted = 0",
            params![now_ms, note_id],
        )?;
        tx.execute(
            "UPDATE note_contents SET data = '', conflicted_data = NULL, deleted = 1, synced = 0, date_modified = ?1 WHERE note_id = ?2",
            params![now_ms, note_id],
        )?;
        tx.execute(
            "UPDATE notes SET title = '', headline = '', trashed_at = NULL, deleted_by = NULL, deleted = 1, synced = 0, date_modified = ?1 WHERE id = ?2",
            params![now_ms, note_id],
        )?;

        tx.commit()?;
        Ok(None)
    }

    pub fn trash_notebook(&self, notebook_id: &str) -> StoreResult<NotebookDto> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let prev_mod: i64 = tx
            .query_row(
                "SELECT date_modified FROM notebooks WHERE id = ?1 AND deleted = 0",
                params![notebook_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("notebook {notebook_id}")))?;
        let now_ms = monotonic_now_ms(prev_mod);

        // Cascade `deleted_by = 'app'` to active descendant sub-notebooks FIRST
        tx.execute(
            "WITH RECURSIVE descendants(id, depth) AS (
                SELECT id, 1 FROM notebooks WHERE parent_id = ?1 AND deleted = 0
                UNION ALL
                SELECT nb.id, d.depth + 1
                FROM notebooks nb JOIN descendants d ON nb.parent_id = d.id
                WHERE nb.deleted = 0 AND d.depth < 256
             )
             UPDATE notebooks
             SET trashed_at = ?2, deleted_by = 'app', synced = 0, date_modified = ?2
             WHERE id IN (SELECT id FROM descendants)
               AND trashed_at IS NULL
               AND deleted = 0",
            params![notebook_id, now_ms],
        )?;

        tx.execute(
            "UPDATE notebooks
             SET trashed_at = ?1, deleted_by = 'user', synced = 0, date_modified = ?1
             WHERE id = ?2 AND deleted = 0",
            params![now_ms, notebook_id],
        )?;

        let counts = query_recursive_note_counts_conn(&tx)?;
        let dto = load_notebook_dto_conn(&tx, notebook_id, &counts)?;
        tx.commit()?;
        Ok(dto)
    }

    pub fn restore_notebook(&self, notebook_id: &str) -> StoreResult<NotebookDto> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let (parent_id, prev_mod): (Option<String>, i64) = tx
            .query_row(
                "SELECT parent_id, date_modified FROM notebooks WHERE id = ?1 AND deleted = 0",
                params![notebook_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("notebook {notebook_id}")))?;
        let now_ms = monotonic_now_ms(prev_mod);

        let detach_parent = if let Some(ref pid) = parent_id {
            let parent_active: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM notebooks WHERE id = ?1 AND deleted = 0 AND trashed_at IS NULL)",
                params![pid],
                |r| r.get(0),
            )?;
            !parent_active
        } else {
            false
        };

        if detach_parent {
            tx.execute(
                "UPDATE notebooks SET parent_id = NULL, trashed_at = NULL, deleted_by = NULL, synced = 0, date_modified = ?1
                 WHERE id = ?2 AND deleted = 0",
                params![now_ms, notebook_id],
            )?;
        } else {
            tx.execute(
                "UPDATE notebooks SET trashed_at = NULL, deleted_by = NULL, synced = 0, date_modified = ?1
                 WHERE id = ?2 AND deleted = 0",
                params![now_ms, notebook_id],
            )?;
        }

        // Restore descendants that were cascade-trashed (`deleted_by = 'app'`)
        tx.execute(
            "WITH RECURSIVE descendants(id, depth) AS (
                SELECT id, 1 FROM notebooks WHERE parent_id = ?1 AND deleted = 0
                UNION ALL
                SELECT nb.id, d.depth + 1
                FROM notebooks nb JOIN descendants d ON nb.parent_id = d.id
                WHERE nb.deleted = 0 AND d.depth < 256
             )
             UPDATE notebooks
             SET trashed_at = NULL, deleted_by = NULL, synced = 0, date_modified = ?2
             WHERE id IN (SELECT id FROM descendants)
               AND deleted_by = 'app'
               AND deleted = 0",
            params![notebook_id, now_ms],
        )?;

        tx.commit()?;
        drop(conn);
        self.get_notebook(notebook_id)
    }

    pub fn delete_notebook(
        &self,
        notebook_id: &str,
        permanent: bool,
    ) -> StoreResult<Option<NotebookDto>> {
        if !permanent {
            let dto = self.trash_notebook(notebook_id)?;
            return Ok(Some(dto));
        }

        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let prev_mod: i64 = tx
            .query_row(
                "SELECT date_modified FROM notebooks WHERE id = ?1 AND deleted = 0",
                params![notebook_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("notebook {notebook_id}")))?;
        let now_ms = monotonic_now_ms(prev_mod);

        let mut target_ids = vec![notebook_id.to_owned()];
        {
            let mut stmt = tx.prepare(
                "WITH RECURSIVE descendants(id, depth) AS (
                    SELECT id, 1 FROM notebooks WHERE parent_id = ?1 AND deleted = 0
                    UNION ALL
                    SELECT nb.id, d.depth + 1
                    FROM notebooks nb JOIN descendants d ON nb.parent_id = d.id
                    WHERE nb.deleted = 0 AND d.depth < 256
                 )
                 SELECT id FROM descendants WHERE deleted_by = 'app' OR trashed_at IS NULL",
            )?;
            let rows = stmt.query_map(params![notebook_id], |r| r.get::<_, String>(0))?;
            for r in rows {
                target_ids.push(r?);
            }
        }

        for id in &target_ids {
            tx.execute(
                "UPDATE note_notebooks SET deleted = 1, synced = 0, date_modified = ?1 WHERE notebook_id = ?2 AND deleted = 0",
                params![now_ms, id],
            )?;
            tx.execute(
                "UPDATE shortcuts SET deleted = 1, synced = 0, date_modified = ?1 WHERE item_type = 'notebook' AND item_id = ?2 AND deleted = 0",
                params![now_ms, id],
            )?;
            tx.execute(
                "UPDATE notebooks SET trashed_at = NULL, deleted_by = NULL, deleted = 1, synced = 0, date_modified = ?1 WHERE id = ?2",
                params![now_ms, id],
            )?;
        }

        tx.commit()?;
        Ok(None)
    }

    pub fn get_trash(&self) -> StoreResult<TrashView> {
        let conn = self.lock_conn();
        let notes = query_all_summaries_conn(&conn, true)?;
        let counts = HashMap::new();

        let mut stmt = conn.prepare(
            "SELECT id, parent_id, title, description, pinned, trashed_at, deleted_by,
                    date_created, date_edited, date_modified, revision
             FROM notebooks
             WHERE deleted = 0 AND trashed_at IS NOT NULL
             ORDER BY trashed_at DESC, LOWER(title) ASC",
        )?;
        let rows = stmt.query_map([], |r| map_notebook_dto_row(r, &counts))?;
        let mut all_trashed_nbs = Vec::new();
        for r in rows {
            all_trashed_nbs.push(r?);
        }

        // Top-level Trash shows `deleted_by == User` (or legacy None), with `deleted_by == App` nested under their trashed parents!
        let trashed_ids: HashSet<String> = all_trashed_nbs.iter().map(|n| n.id.clone()).collect();
        let mut app_children_by_parent: HashMap<String, Vec<NotebookDto>> = HashMap::new();
        let mut top_level: Vec<NotebookDto> = Vec::new();

        for nb in all_trashed_nbs {
            if nb.deleted_by == Some(DeletedBy::App)
                && let Some(ref pid) = nb.parent_id
                && trashed_ids.contains(pid)
            {
                app_children_by_parent
                    .entry(pid.clone())
                    .or_default()
                    .push(nb);
            } else {
                top_level.push(nb);
            }
        }

        fn attach_app_children(
            node: &mut NotebookDto,
            map: &mut HashMap<String, Vec<NotebookDto>>,
        ) {
            if let Some(mut kids) = map.remove(&node.id) {
                for kid in &mut kids {
                    attach_app_children(kid, map);
                }
                node.children = kids;
            }
        }

        for nb in &mut top_level {
            attach_app_children(nb, &mut app_children_by_parent);
        }

        Ok(TrashView {
            notes,
            notebooks: top_level,
        })
    }

    pub fn restore_trash_items(&self, ids: &[String]) -> StoreResult<usize> {
        let mut restored = 0usize;
        for id in ids {
            let is_note: bool = {
                let conn = self.lock_conn();
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1 AND deleted = 0 AND trashed_at IS NOT NULL)",
                    params![id],
                    |r| r.get(0),
                )?
            };
            if is_note {
                self.restore_note(id)?;
                restored += 1;
                continue;
            }
            let is_nb: bool = {
                let conn = self.lock_conn();
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM notebooks WHERE id = ?1 AND deleted = 0 AND trashed_at IS NOT NULL)",
                    params![id],
                    |r| r.get(0),
                )?
            };
            if is_nb {
                self.restore_notebook(id)?;
                restored += 1;
            }
        }
        Ok(restored)
    }

    pub fn sweep_expired_notes(&self, now: DateTime<Utc>) -> StoreResult<usize> {
        let conn = self.lock_conn();
        let now_ms = now.timestamp_millis();
        let swept = conn.execute(
            "UPDATE notes
             SET trashed_at = ?1, deleted_by = 'expired', synced = 0, date_modified = ?1
             WHERE deleted = 0
               AND trashed_at IS NULL
               AND expiry_date IS NOT NULL
               AND expiry_date <= ?1",
            params![now_ms],
        )?;
        Ok(swept)
    }

    pub fn cleanup_trash(&self, now: DateTime<Utc>) -> StoreResult<usize> {
        let interval_days: i64 = self
            .get_setting("trashCleanupInterval")?
            .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse::<i64>().ok()))
            .unwrap_or(7);

        if interval_days < 0 {
            return Ok(0);
        }

        let cutoff_ms = now.timestamp_millis() - interval_days.saturating_mul(86_400_000);
        let now_ms = now.timestamp_millis();

        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let mut expired_note_ids = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT id FROM notes WHERE deleted = 0 AND trashed_at IS NOT NULL AND trashed_at <= ?1",
            )?;
            let rows = stmt.query_map(params![cutoff_ms], |r| r.get::<_, String>(0))?;
            for r in rows {
                expired_note_ids.push(r?);
            }
        }

        for nid in &expired_note_ids {
            tx.execute("DELETE FROM history_sessions WHERE note_id = ?1", params![nid])?;
            tx.execute(
                "UPDATE note_contents SET data = '', conflicted_data = NULL, deleted = 1, synced = 0, date_modified = ?1 WHERE note_id = ?2",
                params![now_ms, nid],
            )?;
            tx.execute(
                "UPDATE notes SET title = '', headline = '', trashed_at = NULL, deleted_by = NULL, deleted = 1, synced = 0, date_modified = ?1 WHERE id = ?2",
                params![now_ms, nid],
            )?;
        }

        let purged_nbs = tx.execute(
            "UPDATE notebooks SET trashed_at = NULL, deleted_by = NULL, deleted = 1, synced = 0, date_modified = ?1
             WHERE deleted = 0 AND trashed_at IS NOT NULL AND trashed_at <= ?2",
            params![now_ms, cutoff_ms],
        )?;

        tx.commit()?;
        Ok(expired_note_ids.len() + purged_nbs)
    }

    pub fn empty_trash(&self, vault_verified: bool) -> StoreResult<usize> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let has_locked_in_trash: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM notes WHERE deleted = 0 AND trashed_at IS NOT NULL AND locked = 1)",
            [],
            |r| r.get(0),
        )?;
        if has_locked_in_trash && !vault_verified {
            return Err(StoreError::VaultVerificationRequired);
        }

        let now_ms = Utc::now().timestamp_millis();
        let mut note_ids = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT id FROM notes WHERE deleted = 0 AND trashed_at IS NOT NULL",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for r in rows {
                note_ids.push(r?);
            }
        }

        for nid in &note_ids {
            tx.execute("DELETE FROM history_sessions WHERE note_id = ?1", params![nid])?;
            tx.execute(
                "UPDATE note_notebooks SET deleted = 1, synced = 0, date_modified = ?1 WHERE note_id = ?2 AND deleted = 0",
                params![now_ms, nid],
            )?;
            tx.execute(
                "UPDATE note_tags SET deleted = 1, synced = 0, date_modified = ?1 WHERE note_id = ?2 AND deleted = 0",
                params![now_ms, nid],
            )?;
            tx.execute(
                "UPDATE note_attachments SET deleted = 1, synced = 0, date_modified = ?1 WHERE note_id = ?2 AND deleted = 0",
                params![now_ms, nid],
            )?;
            tx.execute(
                "UPDATE note_contents SET data = '', conflicted_data = NULL, deleted = 1, synced = 0, date_modified = ?1 WHERE note_id = ?2",
                params![now_ms, nid],
            )?;
            tx.execute(
                "UPDATE notes SET title = '', headline = '', trashed_at = NULL, deleted_by = NULL, deleted = 1, synced = 0, date_modified = ?1 WHERE id = ?2",
                params![now_ms, nid],
            )?;
        }

        let purged_nbs = tx.execute(
            "UPDATE notebooks SET trashed_at = NULL, deleted_by = NULL, deleted = 1, synced = 0, date_modified = ?1
             WHERE deleted = 0 AND trashed_at IS NOT NULL",
            params![now_ms],
        )?;

        tx.commit()?;
        Ok(note_ids.len() + purged_nbs)
    }
}
