use chrono::{DateTime, Utc};
use notes_core::{
    BacklinkItem, Color, ConflictedContent, ContentFormat, DeletedBy, GroupOptions, GroupSection,
    NoteContent, NoteDetail, NoteSummary, Tag, extract_headline, extract_internal_links,
    group_and_sort_notes, resolve_note_title, sanitize_title, validate_note_not_blank,
};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::attachments::sync_note_attachment_links_tx;
use crate::error::{StoreError, StoreResult};
use crate::organization::{load_note_reminders_tx, load_note_tags_tx, upsert_tag_by_title_tx};
use crate::schema::{NotesStore, monotonic_now_ms, ms_to_utc, opt_ms_to_utc};

#[derive(Debug, Clone, Default)]
pub struct CreateNoteInput {
    pub id: Option<String>,
    pub title: Option<String>,
    pub content: Option<String>,
    pub format: ContentFormat,
    pub pinned: bool,
    pub favorite: bool,
    pub archived: bool,
    pub readonly: bool,
    pub local_only: bool,
    pub locked: bool,
    pub color_id: Option<String>,
    pub expiry_date: Option<DateTime<Utc>>,
    pub notebook_ids: Vec<String>,
    pub tags: Vec<String>,
    pub apply_default_org_settings: bool,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateNoteInput {
    pub title: Option<String>,
    pub content: Option<String>,
    pub format: Option<ContentFormat>,
    pub pinned: Option<bool>,
    pub favorite: Option<bool>,
    pub archived: Option<bool>,
    pub readonly: Option<bool>,
    pub local_only: Option<bool>,
    pub color_id: Option<Option<String>>,
    pub expiry_date: Option<Option<DateTime<Utc>>>,
    pub notebook_ids: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub vault_password: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct NoteListFilter {
    pub notebook_id: Option<String>,
    pub tag_id: Option<String>,
    pub tag_title: Option<String>,
    pub color_id: Option<String>,
    pub pinned: Option<bool>,
    pub favorite: Option<bool>,
    pub archived: Option<bool>,
    pub locked: Option<bool>,
    pub trashed: Option<bool>,
    pub query: Option<String>,
    pub group_options: Option<GroupOptions>,
    pub view_key: Option<String>,
}

impl NotesStore {
    pub fn create_note(&self, input: CreateNoteInput) -> StoreResult<NoteDetail> {
        validate_note_not_blank(input.title.as_deref(), input.content.as_deref())
            .map_err(|_| StoreError::BlankNote)?;

        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let now = Utc::now();
        let now_ms = now.timestamp_millis();
        let note_id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let content_id = Uuid::new_v4().to_string();
        let raw_content = input.content.unwrap_or_default();

        let title_fmt = get_setting_str_tx(&tx, "titleFormat")?;
        let existing_count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM notes WHERE deleted = 0",
            [],
            |r| r.get(0),
        )?;

        let (resolved_title, is_generated_title) = resolve_note_title(
            input.title.as_deref(),
            &raw_content,
            input.locked,
            title_fmt.as_deref(),
            now,
            existing_count as usize,
        );

        let headline = extract_headline(&raw_content, input.locked);
        let expiry_ms = input.expiry_date.map(|dt| dt.timestamp_millis());
        let expiry_mod_ms = expiry_ms.map(|_| now_ms);

        tx.execute(
            "INSERT INTO notes (
                id, title, headline, is_generated_title, pinned, favorite, archived,
                readonly, local_only, conflicted, locked, color_id, expiry_date, expiry_modified_at,
                trashed_at, deleted_by, deleted, synced, date_created, date_edited, date_modified, revision
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11, ?12, ?13, NULL, NULL, 0, 0, ?14, ?14, ?14, 0)",
            params![
                note_id,
                resolved_title,
                headline,
                i32::from(is_generated_title),
                i32::from(input.pinned),
                i32::from(input.favorite),
                i32::from(input.archived),
                i32::from(input.readonly),
                i32::from(input.local_only),
                i32::from(input.locked),
                input.color_id,
                expiry_ms,
                expiry_mod_ms,
                now_ms
            ],
        )?;

        tx.execute(
            "INSERT INTO note_contents (
                id, note_id, format, locked, data, conflicted_data, local_only,
                deleted, synced, date_edited, date_modified, date_resolved, revision
             ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, 0, 0, ?7, ?7, NULL, 0)",
            params![
                content_id,
                note_id,
                input.format.as_str(),
                i32::from(input.locked),
                raw_content,
                i32::from(input.local_only),
                now_ms
            ],
        )?;

        let mut notebook_ids = input.notebook_ids;
        if notebook_ids.is_empty()
            && input.apply_default_org_settings
            && let Some(def_nb) = get_setting_str_tx(&tx, "defaultNotebook")?
            && !def_nb.is_empty()
        {
            notebook_ids.push(def_nb);
        }
        for nb_id in &notebook_ids {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM notebooks WHERE id = ?1 AND deleted = 0)",
                params![nb_id],
                |r| r.get(0),
            )?;
            if exists {
                upsert_note_notebook_tx(&tx, nb_id, &note_id, now_ms)?;
            }
        }

        let mut tag_titles = input.tags;
        if tag_titles.is_empty()
            && input.apply_default_org_settings
            && let Some(def_tag) = get_setting_str_tx(&tx, "defaultTag")?
            && !def_tag.is_empty()
        {
            tag_titles.push(def_tag);
        }
        for tag_title in &tag_titles {
            let trimmed = tag_title.trim();
            if !trimmed.is_empty() {
                let tag = upsert_tag_by_title_tx(&tx, trimmed, now_ms)?;
                upsert_note_tag_tx(&tx, &tag.id, &note_id, now_ms)?;
            }
        }

        if !input.locked {
            sync_note_attachment_links_tx(&tx, &note_id, &raw_content, now_ms)?;
        }

        tx.commit()?;
        drop(conn);
        self.get_note(&note_id)
    }

    pub fn get_note(&self, note_id: &str) -> StoreResult<NoteDetail> {
        let conn = self.lock_conn();
        load_note_detail_conn(&conn, note_id)
    }

    pub fn update_note(&self, note_id: &str, input: UpdateNoteInput) -> StoreResult<NoteDetail> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;

        let existing = load_note_detail_conn(&tx, note_id)?;
        if existing.deleted {
            return Err(StoreError::NotFound(format!("note {note_id}")));
        }

        let mutating_text = input.title.is_some() || input.content.is_some();
        if existing.readonly && input.readonly != Some(false) && mutating_text {
            return Err(StoreError::ReadonlyNote(note_id.to_owned()));
        }

        let now_ms = monotonic_now_ms(existing.date_modified.timestamp_millis());
        let now = ms_to_utc(now_ms);

        let existing_content = existing.content.as_ref();
        let mut final_data = existing_content
            .map(|c| c.data.clone())
            .unwrap_or_default();
        let mut content_edited = false;

        if let Some(new_body) = input.content {
            if existing.locked {
                let pw = input
                    .vault_password
                    .as_deref()
                    .ok_or_else(|| StoreError::NoteLocked(note_id.to_owned()))?;
                let canary = crate::vault::load_active_vault_canary_conn(&tx)?;
                let locked_payload =
                    notes_crypto::lock_note_content(pw, &canary, &new_body).map_err(|e| match e {
                        notes_crypto::CryptoError::WrongPassword => StoreError::WrongVaultPassword,
                        other => StoreError::Crypto(other),
                    })?;
                final_data = locked_payload.data_json;
            } else {
                final_data = new_body;
            }
            content_edited = true;
        }

        let mut final_title = existing.title.clone();
        let mut is_generated = existing.is_generated_title;
        if let Some(raw_title) = input.title {
            let clean = sanitize_title(&raw_title);
            if clean.is_empty() {
                if !existing.locked && final_data.trim().is_empty() {
                    return Err(StoreError::BlankNote);
                }
                let title_fmt = get_setting_str_tx(&tx, "titleFormat")?;
                let count: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM notes WHERE deleted = 0",
                    [],
                    |r| r.get(0),
                )?;
                let (gen_t, gen_flag) = resolve_note_title(
                    None,
                    &final_data,
                    existing.locked,
                    title_fmt.as_deref(),
                    now,
                    count as usize,
                );
                final_title = gen_t;
                is_generated = gen_flag;
            } else {
                final_title = clean;
                is_generated = false;
            }
            content_edited = true;
        } else if content_edited && existing.is_generated_title && !existing.locked {
            let title_fmt = get_setting_str_tx(&tx, "titleFormat")?;
            let count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM notes WHERE deleted = 0",
                [],
                |r| r.get(0),
            )?;
            let (gen_t, gen_flag) = resolve_note_title(
                None,
                &final_data,
                false,
                title_fmt.as_deref(),
                now,
                count as usize,
            );
            final_title = gen_t;
            is_generated = gen_flag;
        }

        let headline = if existing.locked {
            String::new()
        } else {
            extract_headline(&final_data, false)
        };

        let pinned = input.pinned.unwrap_or(existing.pinned);
        let favorite = input.favorite.unwrap_or(existing.favorite);
        let archived = input.archived.unwrap_or(existing.archived);
        let readonly = input.readonly.unwrap_or(existing.readonly);
        let local_only = input.local_only.unwrap_or(existing.local_only);
        let color_id = match input.color_id {
            Some(c) => c,
            None => existing.color_id.clone(),
        };
        let (expiry_ms, expiry_mod_ms) = match input.expiry_date {
            Some(Some(dt)) => (Some(dt.timestamp_millis()), Some(now_ms)),
            Some(None) => (None, Some(now_ms)),
            None => (
                existing.expiry_date.map(|d| d.timestamp_millis()),
                existing.expiry_modified_at.map(|d| d.timestamp_millis()),
            ),
        };
        let date_edited_ms = if content_edited {
            now_ms
        } else {
            existing.date_edited.timestamp_millis()
        };

        // Record pre-edit snapshot in history_sessions whenever unlocked or title-only edit occurs!
        // Preserves `locked` flag on title-only edits of locked notes per PROJECT.md § 3 F9!
        if content_edited
            && let Some(prev_c) = existing_content
            && (prev_c.data != final_data || existing.title != final_title)
        {
            crate::history::insert_history_snapshot_tx(
                &tx,
                note_id,
                &existing.title,
                prev_c.format,
                &prev_c.data,
                existing.locked,
                None,
                now_ms.saturating_sub(1),
            )?;
        }

        tx.execute(
            "UPDATE notes SET
                title = ?1,
                headline = ?2,
                is_generated_title = ?3,
                pinned = ?4,
                favorite = ?5,
                archived = ?6,
                readonly = ?7,
                local_only = ?8,
                color_id = ?9,
                expiry_date = ?10,
                expiry_modified_at = ?11,
                synced = 0,
                date_edited = ?12,
                date_modified = ?13
             WHERE id = ?14",
            params![
                final_title,
                headline,
                i32::from(is_generated),
                i32::from(pinned),
                i32::from(favorite),
                i32::from(archived),
                i32::from(readonly),
                i32::from(local_only),
                color_id,
                expiry_ms,
                expiry_mod_ms,
                date_edited_ms,
                now_ms,
                note_id
            ],
        )?;

        let fmt = input
            .format
            .or_else(|| existing_content.map(|c| c.format))
            .unwrap_or(ContentFormat::Markdown);

        if content_edited || input.local_only.is_some() || input.format.is_some() {
            let content_id = existing_content
                .map(|c| c.id.clone())
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            let c_edited_ms = if content_edited {
                now_ms
            } else {
                existing_content
                    .map(|c| c.date_edited.timestamp_millis())
                    .unwrap_or(now_ms)
            };
            tx.execute(
                "INSERT INTO note_contents (
                    id, note_id, format, locked, data, conflicted_data, local_only,
                    deleted, synced, date_edited, date_modified, date_resolved, revision
                 ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, 0, 0, ?7, ?8, NULL, 0)
                 ON CONFLICT(note_id) DO UPDATE SET
                    format = excluded.format,
                    data = excluded.data,
                    local_only = excluded.local_only,
                    deleted = 0,
                    synced = 0,
                    date_edited = ?7,
                    date_modified = ?8",
                params![
                    content_id,
                    note_id,
                    fmt.as_str(),
                    i32::from(existing.locked),
                    final_data,
                    i32::from(local_only),
                    c_edited_ms,
                    now_ms
                ],
            )?;
        }

        if let Some(new_nb_ids) = input.notebook_ids {
            replace_note_notebooks_tx(&tx, note_id, &new_nb_ids, now_ms)?;
        }

        if let Some(new_tag_titles) = input.tags {
            replace_note_tags_tx(&tx, note_id, &new_tag_titles, now_ms)?;
        }

        if content_edited && !existing.locked {
            sync_note_attachment_links_tx(&tx, note_id, &final_data, now_ms)?;
        }

        tx.commit()?;
        drop(conn);
        self.get_note(note_id)
    }

    pub fn duplicate_note(&self, source_note_id: &str) -> StoreResult<NoteDetail> {
        let orig = self.get_note(source_note_id)?;
        if orig.deleted {
            return Err(StoreError::NotFound(format!("note {source_note_id}")));
        }

        let copy_title = format!("{} (Copy)", orig.title);
        let content_data = orig
            .content
            .as_ref()
            .map(|c| c.data.clone())
            .unwrap_or_default();
        let format = orig
            .content
            .as_ref()
            .map(|c| c.format)
            .unwrap_or(ContentFormat::Markdown);

        if orig.locked {
            // Preserve encrypted envelope verbatim on duplicate
            let mut conn = self.lock_conn();
            let tx = conn.transaction()?;
            let now_ms = Utc::now().timestamp_millis();
            let new_id = Uuid::new_v4().to_string();
            let new_cid = Uuid::new_v4().to_string();

            tx.execute(
                "INSERT INTO notes (
                    id, title, headline, is_generated_title, pinned, favorite, archived,
                    readonly, local_only, conflicted, locked, color_id, expiry_date, expiry_modified_at,
                    trashed_at, deleted_by, deleted, synced, date_created, date_edited, date_modified, revision
                 ) VALUES (?1, ?2, '', 0, 0, ?3, ?4, ?5, ?6, 0, 1, ?7, NULL, NULL, NULL, NULL, 0, 0, ?8, ?8, ?8, 0)",
                params![
                    new_id,
                    copy_title,
                    i32::from(orig.favorite),
                    i32::from(orig.archived),
                    i32::from(orig.readonly),
                    i32::from(orig.local_only),
                    orig.color_id,
                    now_ms
                ],
            )?;
            tx.execute(
                "INSERT INTO note_contents (
                    id, note_id, format, locked, data, conflicted_data, local_only,
                    deleted, synced, date_edited, date_modified, date_resolved, revision
                 ) VALUES (?1, ?2, ?3, 1, ?4, NULL, ?5, 0, 0, ?6, ?6, NULL, 0)",
                params![
                    new_cid,
                    new_id,
                    format.as_str(),
                    content_data,
                    i32::from(orig.local_only),
                    now_ms
                ],
            )?;
            for nb_id in &orig.notebook_ids {
                upsert_note_notebook_tx(&tx, nb_id, &new_id, now_ms)?;
            }
            for tag in &orig.tags {
                upsert_note_tag_tx(&tx, &tag.id, &new_id, now_ms)?;
            }
            tx.commit()?;
            drop(conn);
            return self.get_note(&new_id);
        }

        self.create_note(CreateNoteInput {
            id: None,
            title: Some(copy_title),
            content: Some(content_data),
            format,
            pinned: false,
            favorite: orig.favorite,
            archived: orig.archived,
            readonly: orig.readonly,
            local_only: orig.local_only,
            locked: false,
            color_id: orig.color_id,
            expiry_date: orig.expiry_date,
            notebook_ids: orig.notebook_ids,
            tags: orig.tags.into_iter().map(|t| t.title).collect(),
            apply_default_org_settings: false,
        })
    }

    pub fn list_notes(
        &self,
        filter: NoteListFilter,
    ) -> StoreResult<(Vec<NoteSummary>, Vec<GroupSection>)> {
        let effective_options = match filter.group_options {
            Some(opts) => opts,
            None => {
                let view = filter.view_key.as_deref().unwrap_or("notes");
                self.get_view_group_options(view)?
            }
        };

        let mut base_summaries: Vec<NoteSummary> = if let Some(ref q) = filter.query
            && !q.trim().is_empty()
        {
            let scope = if filter.trashed == Some(true) {
                crate::search::SearchScope::Trash
            } else if filter.archived == Some(true) {
                crate::search::SearchScope::Archive
            } else if filter.favorite == Some(true) {
                crate::search::SearchScope::Favorites
            } else {
                crate::search::SearchScope::Default
            };
            self.search_notes(q, scope)?
                .into_iter()
                .map(|mut hit| {
                    if !hit.highlighted_snippet.is_empty() {
                        hit.note.snippet = Some(hit.highlighted_snippet);
                    }
                    hit.note
                })
                .collect()
        } else {
            let conn = self.lock_conn();
            query_all_summaries_conn(&conn, filter.trashed.unwrap_or(false))?
        };

        // Apply relational filters consistently
        let subtree_nb_ids = if let Some(ref nb_id) = filter.notebook_id {
            Some(self.get_notebook_subtree_ids(nb_id)?)
        } else {
            None
        };

        base_summaries.retain(|n| {
            if let Some(trashed_req) = filter.trashed {
                if n.trashed_at.is_some() != trashed_req {
                    return false;
                }
            } else if n.trashed_at.is_some() {
                return false;
            }

            if let Some(arch_req) = filter.archived {
                if n.archived != arch_req {
                    return false;
                }
            } else if filter.trashed != Some(true)
                && filter.query.as_deref().unwrap_or("").trim().is_empty()
                && n.archived
            {
                return false;
            }

            if let Some(pin_req) = filter.pinned
                && n.pinned != pin_req
            {
                return false;
            }
            if let Some(fav_req) = filter.favorite
                && n.favorite != fav_req
            {
                return false;
            }
            if let Some(lock_req) = filter.locked
                && n.locked != lock_req
            {
                return false;
            }
            if let Some(ref cid) = filter.color_id
                && n.color_id.as_ref() != Some(cid)
            {
                return false;
            }
            if let Some(ref tid) = filter.tag_id
                && !n.tags.iter().any(|t| &t.id == tid)
            {
                return false;
            }
            if let Some(ref ttitle) = filter.tag_title
                && !n.tags.iter().any(|t| &t.title == ttitle)
            {
                return false;
            }
            if let Some(ref allowed_nbs) = subtree_nb_ids
                && !n.notebook_ids.iter().any(|id| allowed_nbs.contains(id))
            {
                return false;
            }
            true
        });

        Ok(group_and_sort_notes(
            base_summaries,
            effective_options,
            Utc::now(),
        ))
    }

    pub fn get_backlinks(&self, target_note_id: &str) -> StoreResult<Vec<BacklinkItem>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT n.id, n.title, n.date_edited, COALESCE(nc.data, '')
             FROM notes n
             JOIN note_contents nc ON nc.note_id = n.id AND nc.deleted = 0
             WHERE n.deleted = 0
               AND n.trashed_at IS NULL
               AND n.locked = 0
               AND nc.locked = 0
               AND n.id != ?1
             ORDER BY n.date_edited DESC",
        )?;

        let rows = stmt.query_map(params![target_note_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        let mut backlinks = Vec::new();
        for row in rows {
            let (source_id, source_title, edited_ms, data) = row?;
            let links = extract_internal_links(&data, false);
            if let Some(matched) = links.into_iter().find(|l| l.note_id == target_note_id) {
                backlinks.push(BacklinkItem {
                    note_id: source_id,
                    title: source_title,
                    snippet: matched.snippet,
                    date_edited: ms_to_utc(edited_ms),
                });
            }
        }

        Ok(backlinks)
    }
}

pub(crate) fn get_setting_str_tx(conn: &Connection, key: &str) -> StoreResult<Option<String>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1 AND deleted = 0",
            params![key],
            |r| r.get(0),
        )
        .optional()?;
    let Some(raw_json) = raw else {
        return Ok(None);
    };
    if let Ok(s) = serde_json::from_str::<String>(&raw_json) {
        Ok(Some(s))
    } else {
        Ok(Some(raw_json))
    }
}

pub(crate) fn upsert_note_notebook_tx(
    conn: &Connection,
    notebook_id: &str,
    note_id: &str,
    now_ms: i64,
) -> StoreResult<()> {
    let new_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO note_notebooks (id, notebook_id, note_id, deleted, synced, date_modified, revision)
         VALUES (?1, ?2, ?3, 0, 0, ?4, 0)
         ON CONFLICT(notebook_id, note_id) DO UPDATE SET
            deleted = 0,
            synced = 0,
            date_modified = excluded.date_modified",
        params![new_id, notebook_id, note_id, now_ms],
    )?;
    Ok(())
}

pub(crate) fn upsert_note_tag_tx(
    conn: &Connection,
    tag_id: &str,
    note_id: &str,
    now_ms: i64,
) -> StoreResult<()> {
    let new_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO note_tags (id, tag_id, note_id, deleted, synced, date_modified, revision)
         VALUES (?1, ?2, ?3, 0, 0, ?4, 0)
         ON CONFLICT(tag_id, note_id) DO UPDATE SET
            deleted = 0,
            synced = 0,
            date_modified = excluded.date_modified",
        params![new_id, tag_id, note_id, now_ms],
    )?;
    Ok(())
}

fn replace_note_notebooks_tx(
    conn: &Connection,
    note_id: &str,
    notebook_ids: &[String],
    now_ms: i64,
) -> StoreResult<()> {
    conn.execute(
        "UPDATE note_notebooks SET deleted = 1, synced = 0, date_modified = ?1 WHERE note_id = ?2 AND deleted = 0",
        params![now_ms, note_id],
    )?;
    for nb_id in notebook_ids {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM notebooks WHERE id = ?1 AND deleted = 0)",
            params![nb_id],
            |r| r.get(0),
        )?;
        if exists {
            upsert_note_notebook_tx(conn, nb_id, note_id, now_ms)?;
        }
    }
    Ok(())
}

fn replace_note_tags_tx(
    conn: &Connection,
    note_id: &str,
    tag_titles: &[String],
    now_ms: i64,
) -> StoreResult<()> {
    conn.execute(
        "UPDATE note_tags SET deleted = 1, synced = 0, date_modified = ?1 WHERE note_id = ?2 AND deleted = 0",
        params![now_ms, note_id],
    )?;
    for raw_title in tag_titles {
        let trimmed = raw_title.trim();
        if !trimmed.is_empty() {
            let tag = upsert_tag_by_title_tx(conn, trimmed, now_ms)?;
            upsert_note_tag_tx(conn, &tag.id, note_id, now_ms)?;
        }
    }
    Ok(())
}

pub(crate) fn load_note_detail_conn(conn: &Connection, note_id: &str) -> StoreResult<NoteDetail> {
    let base = conn
        .query_row(
            "SELECT id, title, headline, is_generated_title, pinned, favorite, archived,
                    readonly, local_only, conflicted, locked, color_id, expiry_date, expiry_modified_at,
                    trashed_at, deleted_by, deleted, date_created, date_edited, date_modified, revision
             FROM notes WHERE id = ?1",
            params![note_id],
            |row| {
                let deleted_by_str: Option<String> = row.get(15)?;
                Ok(NoteDetail {
                    id: row.get(0)?,
                    user_id: None,
                    title: row.get(1)?,
                    headline: row.get(2)?,
                    is_generated_title: row.get::<_, i32>(3)? != 0,
                    pinned: row.get::<_, i32>(4)? != 0,
                    favorite: row.get::<_, i32>(5)? != 0,
                    archived: row.get::<_, i32>(6)? != 0,
                    readonly: row.get::<_, i32>(7)? != 0,
                    local_only: row.get::<_, i32>(8)? != 0,
                    conflicted: row.get::<_, i32>(9)? != 0,
                    locked: row.get::<_, i32>(10)? != 0,
                    color_id: row.get(11)?,
                    color: None,
                    content: None,
                    tags: Vec::new(),
                    notebook_ids: Vec::new(),
                    attachments: Vec::new(),
                    reminders: Vec::new(),
                    expiry_date: opt_ms_to_utc(row.get(12)?),
                    expiry_modified_at: opt_ms_to_utc(row.get(13)?),
                    trashed_at: opt_ms_to_utc(row.get(14)?),
                    deleted_by: deleted_by_str.as_deref().and_then(DeletedBy::from_db_str),
                    deleted: row.get::<_, i32>(16)? != 0,
                    date_created: ms_to_utc(row.get(17)?),
                    date_edited: ms_to_utc(row.get(18)?),
                    date_modified: ms_to_utc(row.get(19)?),
                    revision: row.get(20)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("note {note_id}")))?;

    let mut detail = base;

    detail.content = conn
        .query_row(
            "SELECT id, note_id, format, data, locked, conflicted_data, date_edited, date_modified, date_resolved, deleted, revision
             FROM note_contents WHERE note_id = ?1",
            params![note_id],
            |row| {
                let fmt_str: String = row.get(2)?;
                let conf_raw: Option<String> = row.get(5)?;
                let conflicted_data = conf_raw.and_then(|s| {
                    serde_json::from_str::<ConflictedContent>(&s).ok().or_else(|| {
                        let now = Utc::now();
                        Some(ConflictedContent {
                            data: s,
                            locked: false,
                            format: ContentFormat::Markdown,
                            date_edited: now,
                            date_modified: now,
                            device_id: None,
                        })
                    })
                });
                Ok(NoteContent {
                    id: row.get(0)?,
                    note_id: row.get(1)?,
                    format: ContentFormat::from_db_str(&fmt_str),
                    data: row.get(3)?,
                    locked: row.get::<_, i32>(4)? != 0,
                    conflicted_data,
                    date_edited: ms_to_utc(row.get(6)?),
                    date_modified: ms_to_utc(row.get(7)?),
                    date_resolved: opt_ms_to_utc(row.get(8)?),
                    deleted: row.get::<_, i32>(9)? != 0,
                    revision: row.get(10)?,
                })
            },
        )
        .optional()?;

    if let Some(ref cid) = detail.color_id {
        detail.color = load_color_conn(conn, cid)?;
    }
    detail.tags = load_note_tags_tx(conn, note_id)?;
    detail.notebook_ids = load_note_notebook_ids_conn(conn, note_id)?;
    detail.attachments = crate::attachments::load_note_attachments_conn(conn, note_id)?;
    detail.reminders = load_note_reminders_tx(conn, note_id, Utc::now())?;

    Ok(detail)
}

pub(crate) fn load_color_conn(conn: &Connection, color_id: &str) -> StoreResult<Option<Color>> {
    conn.query_row(
        "SELECT id, title, color_code, sort_index, deleted, date_created, date_modified, revision
         FROM colors WHERE id = ?1 AND deleted = 0",
        params![color_id],
        |r| {
            Ok(Color {
                id: r.get(0)?,
                user_id: None,
                title: r.get(1)?,
                color_code: r.get(2)?,
                sort_index: r.get(3)?,
                note_count: None,
                deleted: r.get::<_, i32>(4)? != 0,
                date_created: ms_to_utc(r.get(5)?),
                date_modified: ms_to_utc(r.get(6)?),
                revision: r.get(7)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn load_note_notebook_ids_conn(
    conn: &Connection,
    note_id: &str,
) -> StoreResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT nn.notebook_id
         FROM note_notebooks nn
         JOIN notebooks nb ON nb.id = nn.notebook_id
         WHERE nn.note_id = ?1 AND nn.deleted = 0 AND nb.deleted = 0 AND nb.trashed_at IS NULL",
    )?;
    let rows = stmt.query_map(params![note_id], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub(crate) fn load_note_summary_conn(conn: &Connection, note_id: &str) -> StoreResult<NoteSummary> {
    let mut summary = conn.query_row(
        "SELECT n.id, n.title, n.headline, n.is_generated_title, n.pinned, n.favorite, n.archived,
                n.readonly, n.local_only, n.conflicted, n.locked, n.color_id, n.expiry_date,
                n.trashed_at, n.deleted_by, n.date_created, n.date_edited, n.date_modified, n.revision,
                EXISTS(SELECT 1 FROM note_attachments na JOIN attachments a ON a.id = na.attachment_id WHERE na.note_id = n.id AND na.deleted = 0 AND a.deleted = 0),
                EXISTS(SELECT 1 FROM reminders r WHERE r.note_id = n.id AND r.deleted = 0 AND r.disabled = 0)
         FROM notes n WHERE n.id = ?1",
        params![note_id],
        map_note_summary_row,
    )?;
    populate_summary_relations_conn(conn, &mut summary)?;
    Ok(summary)
}

pub(crate) fn query_all_summaries_conn(
    conn: &Connection,
    trashed: bool,
) -> StoreResult<Vec<NoteSummary>> {
    let sql = if trashed {
        "SELECT n.id, n.title, n.headline, n.is_generated_title, n.pinned, n.favorite, n.archived,
                n.readonly, n.local_only, n.conflicted, n.locked, n.color_id, n.expiry_date,
                n.trashed_at, n.deleted_by, n.date_created, n.date_edited, n.date_modified, n.revision,
                EXISTS(SELECT 1 FROM note_attachments na JOIN attachments a ON a.id = na.attachment_id WHERE na.note_id = n.id AND na.deleted = 0 AND a.deleted = 0),
                EXISTS(SELECT 1 FROM reminders r WHERE r.note_id = n.id AND r.deleted = 0 AND r.disabled = 0)
         FROM notes n WHERE n.deleted = 0 AND n.trashed_at IS NOT NULL"
    } else {
        "SELECT n.id, n.title, n.headline, n.is_generated_title, n.pinned, n.favorite, n.archived,
                n.readonly, n.local_only, n.conflicted, n.locked, n.color_id, n.expiry_date,
                n.trashed_at, n.deleted_by, n.date_created, n.date_edited, n.date_modified, n.revision,
                EXISTS(SELECT 1 FROM note_attachments na JOIN attachments a ON a.id = na.attachment_id WHERE na.note_id = n.id AND na.deleted = 0 AND a.deleted = 0),
                EXISTS(SELECT 1 FROM reminders r WHERE r.note_id = n.id AND r.deleted = 0 AND r.disabled = 0)
         FROM notes n WHERE n.deleted = 0 AND n.trashed_at IS NULL"
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], map_note_summary_row)?;
    let mut list = Vec::new();
    for r in rows {
        let mut s = r?;
        populate_summary_relations_conn(conn, &mut s)?;
        list.push(s);
    }
    Ok(list)
}

fn map_note_summary_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<NoteSummary> {
    let deleted_by_str: Option<String> = row.get(14)?;
    Ok(NoteSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        headline: row.get(2)?,
        is_generated_title: row.get::<_, i32>(3)? != 0,
        pinned: row.get::<_, i32>(4)? != 0,
        favorite: row.get::<_, i32>(5)? != 0,
        archived: row.get::<_, i32>(6)? != 0,
        readonly: row.get::<_, i32>(7)? != 0,
        local_only: row.get::<_, i32>(8)? != 0,
        conflicted: row.get::<_, i32>(9)? != 0,
        locked: row.get::<_, i32>(10)? != 0,
        color_id: row.get(11)?,
        color: None,
        tags: Vec::new(),
        notebook_ids: Vec::new(),
        expiry_date: opt_ms_to_utc(row.get(12)?),
        trashed_at: opt_ms_to_utc(row.get(13)?),
        deleted_by: deleted_by_str.as_deref().and_then(DeletedBy::from_db_str),
        date_created: ms_to_utc(row.get(15)?),
        date_edited: ms_to_utc(row.get(16)?),
        date_modified: ms_to_utc(row.get(17)?),
        revision: row.get(18)?,
        has_attachments: row.get::<_, i32>(19)? != 0,
        has_reminders: row.get::<_, i32>(20)? != 0,
        snippet: None,
    })
}

pub(crate) fn populate_summary_relations_conn(
    conn: &Connection,
    summary: &mut NoteSummary,
) -> StoreResult<()> {
    if let Some(ref cid) = summary.color_id {
        summary.color = load_color_conn(conn, cid)?;
    }
    summary.tags = load_note_tags_tx(conn, &summary.id)?;
    summary.notebook_ids = load_note_notebook_ids_conn(conn, &summary.id)?;
    Ok(())
}

#[allow(dead_code)]
pub(crate) fn _keep_tag_import(_: Tag) {}
