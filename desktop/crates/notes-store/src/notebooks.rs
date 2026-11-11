use std::collections::{HashMap, HashSet};

use chrono::Utc;
use notes_core::{BreadcrumbItem, DeletedBy, NotebookDto, sanitize_title};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{StoreError, StoreResult};
use crate::notes::upsert_note_notebook_tx;
use crate::schema::{NotesStore, monotonic_now_ms, ms_to_utc, opt_ms_to_utc};

#[derive(Debug, Clone, Default)]
pub struct CreateNotebookInput {
    pub id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub parent_id: Option<String>,
    pub pinned: bool,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateNotebookInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub parent_id: Option<Option<String>>,
    pub pinned: Option<bool>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NotebookNoteAction {
    Add,
    Remove,
}

impl NotesStore {
    pub fn create_notebook(&self, input: CreateNotebookInput) -> StoreResult<NotebookDto> {
        let title = sanitize_title(&input.title);
        if title.is_empty() {
            return Err(StoreError::Validation(
                "Notebook title must not be empty.".to_owned(),
            ));
        }

        let conn = self.lock_conn();
        let nb_id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let now_ms = Utc::now().timestamp_millis();

        if let Some(ref pid) = input.parent_id {
            if pid == &nb_id {
                return Err(StoreError::CycleDetected(nb_id));
            }
            ensure_active_parent_exists_conn(&conn, pid)?;
            if would_create_cycle_conn(&conn, &nb_id, pid)? {
                return Err(StoreError::CycleDetected(nb_id));
            }
        }

        conn.execute(
            "INSERT INTO notebooks (
                id, parent_id, title, description, pinned, trashed_at, deleted_by,
                deleted, synced, date_created, date_edited, date_modified, revision
             ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, 0, 0, ?6, ?6, ?6, 0)",
            params![
                nb_id,
                input.parent_id,
                title,
                input.description.unwrap_or_default(),
                i32::from(input.pinned),
                now_ms
            ],
        )?;

        drop(conn);
        self.get_notebook(&nb_id)
    }

    pub fn get_notebook(&self, notebook_id: &str) -> StoreResult<NotebookDto> {
        let conn = self.lock_conn();
        let counts = query_recursive_note_counts_conn(&conn)?;
        load_notebook_dto_conn(&conn, notebook_id, &counts)
    }

    pub fn update_notebook(
        &self,
        notebook_id: &str,
        input: UpdateNotebookInput,
    ) -> StoreResult<NotebookDto> {
        let conn = self.lock_conn();
        let counts = query_recursive_note_counts_conn(&conn)?;
        let existing = load_notebook_dto_conn(&conn, notebook_id, &counts)?;

        if existing.trashed_at.is_some() {
            return Err(StoreError::NotebookInTrash(notebook_id.to_owned()));
        }

        let title = match input.title {
            Some(raw) => {
                let clean = sanitize_title(&raw);
                if clean.is_empty() {
                    return Err(StoreError::Validation(
                        "Notebook title must not be empty.".to_owned(),
                    ));
                }
                clean
            }
            None => existing.title,
        };

        let description = input
            .description
            .or(existing.description)
            .unwrap_or_default();
        let pinned = input.pinned.unwrap_or(existing.pinned);

        let parent_id = match input.parent_id {
            Some(Some(new_parent)) => {
                ensure_active_parent_exists_conn(&conn, &new_parent)?;
                if would_create_cycle_conn(&conn, notebook_id, &new_parent)? {
                    return Err(StoreError::CycleDetected(notebook_id.to_owned()));
                }
                Some(new_parent)
            }
            Some(None) => None,
            None => existing.parent_id,
        };

        let now_ms = monotonic_now_ms(existing.date_modified.timestamp_millis());

        conn.execute(
            "UPDATE notebooks SET
                parent_id = ?1,
                title = ?2,
                description = ?3,
                pinned = ?4,
                synced = 0,
                date_edited = ?5,
                date_modified = ?5
             WHERE id = ?6 AND deleted = 0",
            params![
                parent_id,
                title,
                description,
                i32::from(pinned),
                now_ms,
                notebook_id
            ],
        )?;

        drop(conn);
        self.get_notebook(notebook_id)
    }

    pub fn list_notebooks(&self, tree: bool) -> StoreResult<Vec<NotebookDto>> {
        let conn = self.lock_conn();
        let counts = query_recursive_note_counts_conn(&conn)?;

        let mut stmt = conn.prepare(
            "SELECT id, parent_id, title, description, pinned, trashed_at, deleted_by,
                    date_created, date_edited, date_modified, revision
             FROM notebooks
             WHERE deleted = 0 AND trashed_at IS NULL
             ORDER BY pinned DESC, LOWER(title) ASC, date_created ASC",
        )?;

        let rows = stmt.query_map([], |row| map_notebook_dto_row(row, &counts))?;
        let mut flat: Vec<NotebookDto> = Vec::new();
        for r in rows {
            flat.push(r?);
        }

        if !tree {
            return Ok(flat);
        }

        Ok(assemble_notebook_tree(flat))
    }

    pub fn get_notebook_breadcrumbs(&self, notebook_id: &str) -> StoreResult<Vec<BreadcrumbItem>> {
        let conn = self.lock_conn();
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM notebooks WHERE id = ?1 AND deleted = 0)",
            params![notebook_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(StoreError::NotFound(format!("notebook {notebook_id}")));
        }

        let mut stmt = conn.prepare(
            "WITH RECURSIVE ancestors(id, parent_id, title, depth) AS (
                SELECT id, parent_id, title, 0
                FROM notebooks WHERE id = ?1 AND deleted = 0
                UNION ALL
                SELECT nb.id, nb.parent_id, nb.title, a.depth + 1
                FROM notebooks nb
                JOIN ancestors a ON nb.id = a.parent_id
                WHERE nb.deleted = 0 AND a.depth < 256
             )
             SELECT id, title FROM ancestors ORDER BY depth DESC",
        )?;

        let rows = stmt.query_map(params![notebook_id], |row| {
            Ok(BreadcrumbItem {
                id: row.get(0)?,
                title: row.get(1)?,
            })
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn get_notebook_subtree_ids(&self, notebook_id: &str) -> StoreResult<HashSet<String>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "WITH RECURSIVE subtree(id, depth) AS (
                SELECT id, 0 FROM notebooks WHERE id = ?1 AND deleted = 0 AND trashed_at IS NULL
                UNION ALL
                SELECT nb.id, s.depth + 1
                FROM notebooks nb JOIN subtree s ON nb.parent_id = s.id
                WHERE nb.deleted = 0 AND nb.trashed_at IS NULL AND s.depth < 256
             )
             SELECT id FROM subtree",
        )?;
        let rows = stmt.query_map(params![notebook_id], |r| r.get::<_, String>(0))?;
        let mut set = HashSet::new();
        for r in rows {
            set.insert(r?);
        }
        Ok(set)
    }

    pub fn modify_notebook_notes(
        &self,
        notebook_id: &str,
        note_ids: &[String],
        action: NotebookNoteAction,
    ) -> StoreResult<usize> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM notebooks WHERE id = ?1 AND deleted = 0 AND trashed_at IS NULL)",
            params![notebook_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(StoreError::NotFound(format!("notebook {notebook_id}")));
        }

        let now_ms = Utc::now().timestamp_millis();
        let mut updated = 0usize;

        for note_id in note_ids {
            match action {
                NotebookNoteAction::Add => {
                    let note_exists: bool = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1 AND deleted = 0)",
                        params![note_id],
                        |r| r.get(0),
                    )?;
                    if note_exists {
                        upsert_note_notebook_tx(&tx, notebook_id, note_id, now_ms)?;
                        updated += 1;
                    }
                }
                NotebookNoteAction::Remove => {
                    let rows = tx.execute(
                        "UPDATE note_notebooks SET deleted = 1, synced = 0, date_modified = ?1
                         WHERE notebook_id = ?2 AND note_id = ?3 AND deleted = 0",
                        params![now_ms, notebook_id, note_id],
                    )?;
                    updated += rows;
                }
            }
        }

        tx.commit()?;
        Ok(updated)
    }
}

fn ensure_active_parent_exists_conn(conn: &Connection, parent_id: &str) -> StoreResult<()> {
    let row: Option<Option<i64>> = conn
        .query_row(
            "SELECT trashed_at FROM notebooks WHERE id = ?1 AND deleted = 0",
            params![parent_id],
            |r| r.get(0),
        )
        .optional()?;
    match row {
        None => Err(StoreError::NotFound(format!("parent notebook {parent_id}"))),
        Some(Some(_)) => Err(StoreError::NotebookInTrash(parent_id.to_owned())),
        Some(None) => Ok(()),
    }
}

pub(crate) fn would_create_cycle_conn(
    conn: &Connection,
    notebook_id: &str,
    candidate_parent_id: &str,
) -> StoreResult<bool> {
    if notebook_id == candidate_parent_id {
        return Ok(true);
    }
    let creates_cycle: bool = conn.query_row(
        "WITH RECURSIVE ancestors(id, parent_id, depth) AS (
            SELECT id, parent_id, 0 FROM notebooks WHERE id = ?1 AND deleted = 0
            UNION ALL
            SELECT nb.id, nb.parent_id, a.depth + 1
            FROM notebooks nb JOIN ancestors a ON nb.id = a.parent_id
            WHERE nb.deleted = 0 AND a.depth < 256
         )
         SELECT EXISTS(SELECT 1 FROM ancestors WHERE id = ?2)",
        params![candidate_parent_id, notebook_id],
        |r| r.get(0),
    )?;
    Ok(creates_cycle)
}

pub(crate) fn query_recursive_note_counts_conn(
    conn: &Connection,
) -> StoreResult<HashMap<String, usize>> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE subtree(root_id, desc_id, depth) AS (
            SELECT id, id, 0 FROM notebooks WHERE deleted = 0 AND trashed_at IS NULL
            UNION ALL
            SELECT s.root_id, nb.id, s.depth + 1
            FROM notebooks nb JOIN subtree s ON nb.parent_id = s.desc_id
            WHERE nb.deleted = 0 AND nb.trashed_at IS NULL AND s.depth < 256
         )
         SELECT s.root_id, COUNT(DISTINCT n.id) AS total_notes
         FROM subtree s
         LEFT JOIN note_notebooks nn ON nn.notebook_id = s.desc_id AND nn.deleted = 0
         LEFT JOIN notes n ON n.id = nn.note_id AND n.deleted = 0 AND n.trashed_at IS NULL AND n.archived = 0
         GROUP BY s.root_id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut map = HashMap::new();
    for r in rows {
        let (id, cnt) = r?;
        map.insert(id, cnt.max(0) as usize);
    }
    Ok(map)
}

pub(crate) fn load_notebook_dto_conn(
    conn: &Connection,
    notebook_id: &str,
    counts: &HashMap<String, usize>,
) -> StoreResult<NotebookDto> {
    conn.query_row(
        "SELECT id, parent_id, title, description, pinned, trashed_at, deleted_by,
                date_created, date_edited, date_modified, revision
         FROM notebooks WHERE id = ?1 AND deleted = 0",
        params![notebook_id],
        |r| map_notebook_dto_row(r, counts),
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound(format!("notebook {notebook_id}")))
}

pub(crate) fn map_notebook_dto_row(
    row: &rusqlite::Row<'_>,
    counts: &HashMap<String, usize>,
) -> rusqlite::Result<NotebookDto> {
    let id: String = row.get(0)?;
    let desc: String = row.get(3)?;
    let deleted_by_str: Option<String> = row.get(6)?;
    Ok(NotebookDto {
        total_notes: counts.get(&id).copied().unwrap_or(0),
        id,
        parent_id: row.get(1)?,
        title: row.get(2)?,
        description: (!desc.is_empty()).then_some(desc),
        pinned: row.get::<_, i32>(4)? != 0,
        children: Vec::new(),
        trashed_at: opt_ms_to_utc(row.get(5)?),
        deleted_by: deleted_by_str.as_deref().and_then(DeletedBy::from_db_str),
        date_created: ms_to_utc(row.get(7)?),
        date_edited: ms_to_utc(row.get(8)?),
        date_modified: ms_to_utc(row.get(9)?),
        revision: row.get(10)?,
    })
}

pub(crate) fn assemble_notebook_tree(flat: Vec<NotebookDto>) -> Vec<NotebookDto> {
    let valid_ids: HashSet<String> = flat.iter().map(|n| n.id.clone()).collect();
    let mut children_by_parent: HashMap<Option<String>, Vec<NotebookDto>> = HashMap::new();

    for nb in flat {
        let effective_parent = nb
            .parent_id
            .as_ref()
            .filter(|pid| valid_ids.contains(*pid))
            .cloned();
        children_by_parent
            .entry(effective_parent)
            .or_default()
            .push(nb);
    }

    fn attach_children(
        parent_key: Option<String>,
        map: &mut HashMap<Option<String>, Vec<NotebookDto>>,
    ) -> Vec<NotebookDto> {
        let Some(mut nodes) = map.remove(&parent_key) else {
            return Vec::new();
        };
        for node in &mut nodes {
            node.children = attach_children(Some(node.id.clone()), map);
        }
        nodes
    }

    attach_children(None, &mut children_by_parent)
}
