use chrono::{DateTime, Utc};
use notes_core::{
    CipherEnvelope, Color, GroupOptions, Monograph, RecurringMode, Reminder, ReminderMode,
    ReminderPriority, SettingItem, Shortcut, ShortcutItemType, Tag, next_occurrence,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use uuid::Uuid;

use crate::error::{StoreError, StoreResult};
use crate::schema::{NotesStore, monotonic_now_ms, ms_to_utc, opt_ms_to_utc};

#[derive(Debug, Clone)]
pub struct CreateReminderInput {
    pub id: Option<String>,
    pub note_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub date: DateTime<Utc>,
    pub mode: ReminderMode,
    pub recurring_mode: Option<RecurringMode>,
    pub selected_days: Vec<u8>,
    pub priority: ReminderPriority,
    pub disabled: bool,
    pub snooze_until: Option<DateTime<Utc>>,
    pub local_only: bool,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateReminderInput {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub date: Option<DateTime<Utc>>,
    pub mode: Option<ReminderMode>,
    pub recurring_mode: Option<Option<RecurringMode>>,
    pub selected_days: Option<Vec<u8>>,
    pub priority: Option<ReminderPriority>,
    pub disabled: Option<bool>,
    pub snooze_until: Option<Option<DateTime<Utc>>>,
}

#[derive(Debug, Clone)]
pub struct PublishMonographInput {
    pub note_id: String,
    pub title: Option<String>,
    pub password: Option<String>,
    pub self_destruct: bool,
}

impl NotesStore {
    // --- Tags ---
    pub fn create_tag(&self, id: Option<String>, raw_title: &str) -> StoreResult<Tag> {
        let trimmed = raw_title.trim();
        if trimmed.is_empty() {
            return Err(StoreError::Validation("Tag title must not be empty.".to_owned()));
        }

        let conn = self.lock_conn();
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tags WHERE title = ?1 COLLATE BINARY AND deleted = 0)",
            params![trimmed],
            |r| r.get(0),
        )?;
        if exists {
            return Err(StoreError::DuplicateTag(trimmed.to_owned()));
        }

        let tag_id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let now_ms = Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO tags (id, title, deleted, synced, date_created, date_modified, revision)
             VALUES (?1, ?2, 0, 0, ?3, ?3, 0)",
            params![tag_id, trimmed, now_ms],
        )?;

        Ok(Tag {
            id: tag_id,
            user_id: None,
            title: trimmed.to_owned(),
            note_count: Some(0),
            date_created: ms_to_utc(now_ms),
            date_modified: ms_to_utc(now_ms),
            deleted: false,
            revision: 0,
        })
    }

    pub fn list_tags(&self) -> StoreResult<Vec<Tag>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT t.id, t.title, t.date_created, t.date_modified, t.revision,
                    COUNT(DISTINCT n.id) AS active_notes
             FROM tags t
             LEFT JOIN note_tags nt ON nt.tag_id = t.id AND nt.deleted = 0
             LEFT JOIN notes n ON n.id = nt.note_id AND n.deleted = 0 AND n.trashed_at IS NULL AND n.archived = 0
             WHERE t.deleted = 0
             GROUP BY t.id
             ORDER BY LOWER(t.title) ASC, t.title ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Tag {
                id: r.get(0)?,
                user_id: None,
                title: r.get(1)?,
                date_created: ms_to_utc(r.get(2)?),
                date_modified: ms_to_utc(r.get(3)?),
                deleted: false,
                revision: r.get(4)?,
                note_count: Some(r.get::<_, i64>(5)?.max(0) as usize),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn rename_tag(&self, tag_id: &str, raw_new_title: &str) -> StoreResult<Tag> {
        let trimmed = raw_new_title.trim();
        if trimmed.is_empty() {
            return Err(StoreError::Validation("Tag title must not be empty.".to_owned()));
        }

        let conn = self.lock_conn();
        let prev_mod: i64 = conn
            .query_row(
                "SELECT date_modified FROM tags WHERE id = ?1 AND deleted = 0",
                params![tag_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("tag {tag_id}")))?;

        let dup: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tags WHERE title = ?1 COLLATE BINARY AND deleted = 0 AND id != ?2)",
            params![trimmed, tag_id],
            |r| r.get(0),
        )?;
        if dup {
            return Err(StoreError::DuplicateTag(trimmed.to_owned()));
        }

        let now_ms = monotonic_now_ms(prev_mod);
        conn.execute(
            "UPDATE tags SET title = ?1, synced = 0, date_modified = ?2 WHERE id = ?3",
            params![trimmed, now_ms, tag_id],
        )?;

        drop(conn);
        self.list_tags()?
            .into_iter()
            .find(|t| t.id == tag_id)
            .ok_or_else(|| StoreError::NotFound(format!("tag {tag_id}")))
    }

    pub fn delete_tag(&self, tag_id: &str) -> StoreResult<()> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;
        let prev_mod: i64 = tx
            .query_row(
                "SELECT date_modified FROM tags WHERE id = ?1 AND deleted = 0",
                params![tag_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("tag {tag_id}")))?;
        let now_ms = monotonic_now_ms(prev_mod);

        tx.execute(
            "UPDATE note_tags SET deleted = 1, synced = 0, date_modified = ?1 WHERE tag_id = ?2 AND deleted = 0",
            params![now_ms, tag_id],
        )?;
        tx.execute(
            "UPDATE shortcuts SET deleted = 1, synced = 0, date_modified = ?1 WHERE item_type = 'tag' AND item_id = ?2 AND deleted = 0",
            params![now_ms, tag_id],
        )?;
        tx.execute(
            "UPDATE tags SET deleted = 1, synced = 0, date_modified = ?1 WHERE id = ?2",
            params![now_ms, tag_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    // --- Colors ---
    pub fn list_colors(&self) -> StoreResult<Vec<Color>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.title, c.color_code, c.sort_index, c.date_created, c.date_modified, c.revision,
                    COUNT(DISTINCT n.id) AS active_notes
             FROM colors c
             LEFT JOIN notes n ON n.color_id = c.id AND n.deleted = 0 AND n.trashed_at IS NULL AND n.archived = 0
             WHERE c.deleted = 0
             GROUP BY c.id
             ORDER BY c.sort_index ASC, c.date_created ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Color {
                id: r.get(0)?,
                user_id: None,
                title: r.get(1)?,
                color_code: r.get(2)?,
                sort_index: r.get(3)?,
                date_created: ms_to_utc(r.get(4)?),
                date_modified: ms_to_utc(r.get(5)?),
                deleted: false,
                revision: r.get(6)?,
                note_count: Some(r.get::<_, i64>(7)?.max(0) as usize),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn create_color(
        &self,
        id: Option<String>,
        title: &str,
        color_code: &str,
    ) -> StoreResult<Color> {
        let trimmed = title.trim();
        let code = color_code.trim();
        if trimmed.is_empty() || !is_valid_hex_color(code) {
            return Err(StoreError::Validation(
                "Color requires a non-empty title and valid #RRGGBB hex code.".to_owned(),
            ));
        }

        let conn = self.lock_conn();
        let max_sort: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(sort_index), -1) FROM colors WHERE deleted = 0",
                [],
                |r| r.get(0),
            )?;
        let cid = id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let now_ms = Utc::now().timestamp_millis();

        conn.execute(
            "INSERT INTO colors (id, title, color_code, sort_index, deleted, synced, date_created, date_modified, revision)
             VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, ?5, 0)",
            params![cid, trimmed, code, max_sort + 1, now_ms],
        )?;

        Ok(Color {
            id: cid,
            user_id: None,
            title: trimmed.to_owned(),
            color_code: code.to_owned(),
            sort_index: max_sort + 1,
            note_count: Some(0),
            date_created: ms_to_utc(now_ms),
            date_modified: ms_to_utc(now_ms),
            deleted: false,
            revision: 0,
        })
    }

    pub fn update_color(
        &self,
        color_id: &str,
        title: Option<&str>,
        color_code: Option<&str>,
    ) -> StoreResult<Color> {
        let conn = self.lock_conn();
        let (cur_title, cur_code, sort_idx, created_ms, prev_mod, rev): (
            String,
            String,
            i32,
            i64,
            i64,
            i64,
        ) = conn
            .query_row(
                "SELECT title, color_code, sort_index, date_created, date_modified, revision
                 FROM colors WHERE id = ?1 AND deleted = 0",
                params![color_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("color {color_id}")))?;

        let next_title = title.map(str::trim).unwrap_or(&cur_title);
        let next_code = color_code.map(str::trim).unwrap_or(&cur_code);
        if next_title.is_empty() || !is_valid_hex_color(next_code) {
            return Err(StoreError::Validation(
                "Color requires a non-empty title and valid #RRGGBB hex code.".to_owned(),
            ));
        }

        let now_ms = monotonic_now_ms(prev_mod);
        conn.execute(
            "UPDATE colors SET title = ?1, color_code = ?2, synced = 0, date_modified = ?3 WHERE id = ?4",
            params![next_title, next_code, now_ms, color_id],
        )?;

        Ok(Color {
            id: color_id.to_owned(),
            user_id: None,
            title: next_title.to_owned(),
            color_code: next_code.to_owned(),
            sort_index: sort_idx,
            note_count: None,
            date_created: ms_to_utc(created_ms),
            date_modified: ms_to_utc(now_ms),
            deleted: false,
            revision: rev,
        })
    }

    pub fn delete_color(&self, color_id: &str) -> StoreResult<()> {
        let mut conn = self.lock_conn();
        let tx = conn.transaction()?;
        let prev_mod: i64 = tx
            .query_row(
                "SELECT date_modified FROM colors WHERE id = ?1 AND deleted = 0",
                params![color_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("color {color_id}")))?;
        let now_ms = monotonic_now_ms(prev_mod);

        tx.execute(
            "UPDATE notes SET color_id = NULL, synced = 0, date_modified = ?1 WHERE color_id = ?2 AND deleted = 0",
            params![now_ms, color_id],
        )?;
        tx.execute(
            "UPDATE colors SET deleted = 1, synced = 0, date_modified = ?1 WHERE id = ?2",
            params![now_ms, color_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    // --- Shortcuts ---
    pub fn create_shortcut(
        &self,
        id: Option<String>,
        item_id: &str,
        item_type: ShortcutItemType,
        sort_index: Option<i32>,
    ) -> StoreResult<Shortcut> {
        let conn = self.lock_conn();
        let now_ms = Utc::now().timestamp_millis();

        if let Some(existing_id) = conn
            .query_row(
                "SELECT id FROM shortcuts WHERE item_type = ?1 AND item_id = ?2 AND deleted = 0",
                params![item_type.as_str(), item_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            drop(conn);
            return self
                .list_shortcuts()?
                .into_iter()
                .find(|s| s.id == existing_id)
                .ok_or_else(|| StoreError::NotFound(existing_id));
        }

        let sidx = match sort_index {
            Some(idx) => idx,
            None => {
                conn.query_row(
                    "SELECT COALESCE(MAX(sort_index), -1) + 1 FROM shortcuts WHERE deleted = 0",
                    [],
                    |r| r.get::<_, i32>(0),
                )?
            }
        };
        let sid = id.unwrap_or_else(|| Uuid::new_v4().to_string());
        conn.execute(
            "INSERT INTO shortcuts (id, item_id, item_type, sort_index, deleted, synced, date_created, date_modified, revision)
             VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, ?5, 0)",
            params![sid, item_id, item_type.as_str(), sidx, now_ms],
        )?;

        Ok(Shortcut {
            id: sid,
            user_id: None,
            item_id: item_id.to_owned(),
            item_type,
            sort_index: sidx,
            date_created: ms_to_utc(now_ms),
            date_modified: ms_to_utc(now_ms),
            deleted: false,
            revision: 0,
        })
    }

    pub fn list_shortcuts(&self) -> StoreResult<Vec<Shortcut>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT id, item_id, item_type, sort_index, date_created, date_modified, revision
             FROM shortcuts WHERE deleted = 0 ORDER BY sort_index ASC, date_created ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            let t_str: String = r.get(2)?;
            Ok(Shortcut {
                id: r.get(0)?,
                user_id: None,
                item_id: r.get(1)?,
                item_type: ShortcutItemType::from_db_str(&t_str).unwrap_or(ShortcutItemType::Notebook),
                sort_index: r.get(3)?,
                date_created: ms_to_utc(r.get(4)?),
                date_modified: ms_to_utc(r.get(5)?),
                deleted: false,
                revision: r.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn update_shortcut(&self, shortcut_id: &str, sort_index: i32) -> StoreResult<Shortcut> {
        let conn = self.lock_conn();
        let prev_mod: i64 = conn
            .query_row(
                "SELECT date_modified FROM shortcuts WHERE id = ?1 AND deleted = 0",
                params![shortcut_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| StoreError::NotFound(format!("shortcut {shortcut_id}")))?;
        let now_ms = monotonic_now_ms(prev_mod);
        conn.execute(
            "UPDATE shortcuts SET sort_index = ?1, synced = 0, date_modified = ?2 WHERE id = ?3",
            params![sort_index, now_ms, shortcut_id],
        )?;
        drop(conn);
        self.list_shortcuts()?
            .into_iter()
            .find(|s| s.id == shortcut_id)
            .ok_or_else(|| StoreError::NotFound(format!("shortcut {shortcut_id}")))
    }

    pub fn delete_shortcut(&self, shortcut_id: &str) -> StoreResult<()> {
        let conn = self.lock_conn();
        let now_ms = Utc::now().timestamp_millis();
        let changed = conn.execute(
            "UPDATE shortcuts SET deleted = 1, synced = 0, date_modified = ?1 WHERE id = ?2 AND deleted = 0",
            params![now_ms, shortcut_id],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound(format!("shortcut {shortcut_id}")));
        }
        Ok(())
    }

    // --- Reminders ---
    pub fn create_reminder(&self, input: CreateReminderInput) -> StoreResult<Reminder> {
        if input.title.trim().is_empty() {
            return Err(StoreError::Validation(
                "Reminder title must not be empty.".to_owned(),
            ));
        }
        let conn = self.lock_conn();
        let rid = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let now = Utc::now();
        let now_ms = now.timestamp_millis();
        let days_json = serde_json::to_string(&input.selected_days)?;

        conn.execute(
            "INSERT INTO reminders (
                id, note_id, title, description, date, mode, recurring_mode, selected_days,
                priority, disabled, snooze_until, local_only, deleted, synced, date_created, date_modified, revision
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, 0, ?13, ?13, 0)",
            params![
                rid,
                input.note_id,
                input.title.trim(),
                input.description.as_deref().unwrap_or(""),
                input.date.timestamp_millis(),
                input.mode.as_str(),
                input.recurring_mode.map(|m| m.as_str()),
                days_json,
                input.priority.as_str(),
                i32::from(input.disabled),
                input.snooze_until.map(|d| d.timestamp_millis()),
                i32::from(input.local_only),
                now_ms
            ],
        )?;

        load_reminder_conn(&conn, &rid, now)
    }

    pub fn update_reminder(
        &self,
        reminder_id: &str,
        input: UpdateReminderInput,
    ) -> StoreResult<Reminder> {
        let conn = self.lock_conn();
        let now = Utc::now();
        let cur = load_reminder_conn(&conn, reminder_id, now)?;
        let now_ms = monotonic_now_ms(cur.date_modified.timestamp_millis());

        let title = input
            .title
            .map(|t| t.trim().to_owned())
            .unwrap_or(cur.title);
        if title.is_empty() {
            return Err(StoreError::Validation(
                "Reminder title must not be empty.".to_owned(),
            ));
        }
        let desc = match input.description {
            Some(d) => d.unwrap_or_default(),
            None => cur.description.unwrap_or_default(),
        };
        let date = input.date.unwrap_or(cur.date);
        let mode = input.mode.unwrap_or(cur.mode);
        let recurring = match input.recurring_mode {
            Some(r) => r,
            None => cur.recurring_mode,
        };
        let days = input.selected_days.unwrap_or(cur.selected_days);
        let priority = input.priority.unwrap_or(cur.priority);
        let disabled = input.disabled.unwrap_or(cur.disabled);
        let snooze = match input.snooze_until {
            Some(s) => s,
            None => cur.snooze_until,
        };
        let days_json = serde_json::to_string(&days)?;

        conn.execute(
            "UPDATE reminders SET
                title = ?1, description = ?2, date = ?3, mode = ?4, recurring_mode = ?5,
                selected_days = ?6, priority = ?7, disabled = ?8, snooze_until = ?9,
                synced = 0, date_modified = ?10
             WHERE id = ?11 AND deleted = 0",
            params![
                title,
                desc,
                date.timestamp_millis(),
                mode.as_str(),
                recurring.map(|m| m.as_str()),
                days_json,
                priority.as_str(),
                i32::from(disabled),
                snooze.map(|d| d.timestamp_millis()),
                now_ms,
                reminder_id
            ],
        )?;

        load_reminder_conn(&conn, reminder_id, now)
    }

    pub fn list_reminders(&self, reference_now: DateTime<Utc>) -> StoreResult<Vec<Reminder>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT id, note_id, title, description, date, mode, recurring_mode, selected_days,
                    priority, disabled, snooze_until, local_only, date_created, date_modified, revision
             FROM reminders WHERE deleted = 0 ORDER BY disabled ASC, date ASC",
        )?;
        let rows = stmt.query_map([], |r| map_reminder_row(r, reference_now))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn delete_reminder(&self, reminder_id: &str) -> StoreResult<()> {
        let conn = self.lock_conn();
        let now_ms = Utc::now().timestamp_millis();
        let changed = conn.execute(
            "UPDATE reminders SET deleted = 1, synced = 0, date_modified = ?1 WHERE id = ?2 AND deleted = 0",
            params![now_ms, reminder_id],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound(format!("reminder {reminder_id}")));
        }
        Ok(())
    }

    // --- Settings & Per-View GroupOptions ---
    pub fn put_setting(
        &self,
        key: &str,
        value: Value,
        local_only: bool,
    ) -> StoreResult<SettingItem> {
        let conn = self.lock_conn();
        let prev_mod: i64 = conn
            .query_row(
                "SELECT date_modified FROM settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let now_ms = monotonic_now_ms(prev_mod);
        let new_id = Uuid::new_v4().to_string();
        let val_str = serde_json::to_string(&value)?;

        conn.execute(
            "INSERT INTO settings (id, key, value, local_only, deleted, synced, date_modified, revision)
             VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, 0)
             ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                local_only = excluded.local_only,
                deleted = 0,
                synced = 0,
                date_modified = excluded.date_modified",
            params![new_id, key, val_str, i32::from(local_only), now_ms],
        )?;

        let id: String = conn.query_row(
            "SELECT id FROM settings WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )?;

        Ok(SettingItem {
            id,
            user_id: None,
            key: key.to_owned(),
            value,
            date_modified: ms_to_utc(now_ms),
            deleted: false,
            revision: 0,
        })
    }

    pub fn get_setting(&self, key: &str) -> StoreResult<Option<Value>> {
        let conn = self.lock_conn();
        let raw: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1 AND deleted = 0",
                params![key],
                |r| r.get(0),
            )
            .optional()?;
        match raw {
            Some(s) => Ok(Some(serde_json::from_str(&s).unwrap_or(Value::String(s)))),
            None => Ok(None),
        }
    }

    pub fn list_settings(&self) -> StoreResult<Vec<SettingItem>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT id, key, value, date_modified, revision
             FROM settings WHERE deleted = 0 AND key NOT LIKE 'local:%' ORDER BY key ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            let raw: String = r.get(2)?;
            let val = serde_json::from_str(&raw).unwrap_or(Value::String(raw));
            Ok(SettingItem {
                id: r.get(0)?,
                user_id: None,
                key: r.get(1)?,
                value: val,
                date_modified: ms_to_utc(r.get(3)?),
                deleted: false,
                revision: r.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn get_view_group_options(&self, view_key: &str) -> StoreResult<GroupOptions> {
        let key = format!("groupOptions:{view_key}");
        if let Some(val) = self.get_setting(&key)?
            && let Ok(parsed) = serde_json::from_value::<GroupOptions>(val)
        {
            return Ok(parsed);
        }
        Ok(GroupOptions::default())
    }

    pub fn set_view_group_options(
        &self,
        view_key: &str,
        options: GroupOptions,
    ) -> StoreResult<SettingItem> {
        let key = format!("groupOptions:{view_key}");
        self.put_setting(&key, serde_json::to_value(options)?, false)
    }

    // --- Monographs ---
    pub fn publish_monograph(&self, input: PublishMonographInput) -> StoreResult<Monograph> {
        let note = self.get_note(&input.note_id)?;
        if note.locked {
            return Err(StoreError::Validation(
                "Locked notes cannot be published as Monographs.".to_owned(),
            ));
        }
        let body = note
            .content
            .as_ref()
            .map(|c| c.data.as_str())
            .unwrap_or("");
        if body.trim().is_empty() {
            return Err(StoreError::Validation(
                "Cannot publish an empty note as a Monograph.".to_owned(),
            ));
        }

        let title = input
            .title
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
            .unwrap_or(note.title);

        let (plain_content, encrypted_env) = if let Some(ref pw) = input.password
            && !pw.is_empty()
        {
            let env = notes_crypto::encrypt_monograph_content(pw, body)?;
            (None, Some(env))
        } else {
            (Some(body.to_owned()), None)
        };

        let enc_json = encrypted_env
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;

        let conn = self.lock_conn();
        let now_ms = Utc::now().timestamp_millis();
        let new_id = Uuid::new_v4().to_string();

        conn.execute(
            "INSERT INTO monographs (
                id, note_id, title, content, encrypted_content, self_destruct,
                view_count, active, deleted, synced, date_published, date_updated, date_modified
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 1, 0, 0, ?7, ?7, ?7)
             ON CONFLICT(note_id) DO UPDATE SET
                title = excluded.title,
                content = excluded.content,
                encrypted_content = excluded.encrypted_content,
                self_destruct = excluded.self_destruct,
                active = 1,
                deleted = 0,
                synced = 0,
                date_updated = excluded.date_updated,
                date_modified = excluded.date_modified",
            params![
                new_id,
                input.note_id,
                title,
                plain_content,
                enc_json,
                i32::from(input.self_destruct),
                now_ms
            ],
        )?;

        load_monograph_by_note_conn(&conn, &input.note_id)?
            .ok_or_else(|| StoreError::NotFound(format!("monograph for note {}", input.note_id)))
    }

    pub fn list_monographs(&self) -> StoreResult<Vec<Monograph>> {
        let conn = self.lock_conn();
        let mut stmt = conn.prepare(
            "SELECT id, note_id, title, content, encrypted_content, self_destruct,
                    view_count, active, date_published, date_updated
             FROM monographs WHERE deleted = 0 AND active = 1 ORDER BY date_updated DESC",
        )?;
        let rows = stmt.query_map([], map_monograph_row)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    pub fn unpublish_monograph(&self, note_id: &str) -> StoreResult<()> {
        let conn = self.lock_conn();
        let now_ms = Utc::now().timestamp_millis();
        let changed = conn.execute(
            "UPDATE monographs SET active = 0, deleted = 1, synced = 0, date_modified = ?1
             WHERE note_id = ?2 AND deleted = 0",
            params![now_ms, note_id],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound(format!("monograph for note {note_id}")));
        }
        Ok(())
    }
}

fn is_valid_hex_color(code: &str) -> bool {
    let Some(hex) = code.strip_prefix('#') else {
        return false;
    };
    (hex.len() == 6 || hex.len() == 3) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

pub(crate) fn upsert_tag_by_title_tx(
    conn: &Connection,
    trimmed_title: &str,
    now_ms: i64,
) -> StoreResult<Tag> {
    if let Some((id, created_ms, mod_ms, rev)) = conn
        .query_row(
            "SELECT id, date_created, date_modified, revision FROM tags WHERE title = ?1 COLLATE BINARY AND deleted = 0",
            params![trimmed_title],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)),
        )
        .optional()?
    {
        return Ok(Tag {
            id,
            user_id: None,
            title: trimmed_title.to_owned(),
            note_count: None,
            date_created: ms_to_utc(created_ms),
            date_modified: ms_to_utc(mod_ms),
            deleted: false,
            revision: rev,
        });
    }

    let tag_id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO tags (id, title, deleted, synced, date_created, date_modified, revision)
         VALUES (?1, ?2, 0, 0, ?3, ?3, 0)",
        params![tag_id, trimmed_title, now_ms],
    )?;

    Ok(Tag {
        id: tag_id,
        user_id: None,
        title: trimmed_title.to_owned(),
        note_count: None,
        date_created: ms_to_utc(now_ms),
        date_modified: ms_to_utc(now_ms),
        deleted: false,
        revision: 0,
    })
}

pub(crate) fn load_note_tags_tx(conn: &Connection, note_id: &str) -> StoreResult<Vec<Tag>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.title, t.date_created, t.date_modified, t.revision
         FROM note_tags nt
         JOIN tags t ON t.id = nt.tag_id
         WHERE nt.note_id = ?1 AND nt.deleted = 0 AND t.deleted = 0
         ORDER BY LOWER(t.title) ASC, t.title ASC",
    )?;
    let rows = stmt.query_map(params![note_id], |r| {
        Ok(Tag {
            id: r.get(0)?,
            user_id: None,
            title: r.get(1)?,
            note_count: None,
            date_created: ms_to_utc(r.get(2)?),
            date_modified: ms_to_utc(r.get(3)?),
            deleted: false,
            revision: r.get(4)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub(crate) fn load_note_reminders_tx(
    conn: &Connection,
    note_id: &str,
    now: DateTime<Utc>,
) -> StoreResult<Vec<Reminder>> {
    let mut stmt = conn.prepare(
        "SELECT id, note_id, title, description, date, mode, recurring_mode, selected_days,
                priority, disabled, snooze_until, local_only, date_created, date_modified, revision
         FROM reminders WHERE note_id = ?1 AND deleted = 0 ORDER BY date ASC",
    )?;
    let rows = stmt.query_map(params![note_id], |r| map_reminder_row(r, now))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn load_reminder_conn(
    conn: &Connection,
    reminder_id: &str,
    now: DateTime<Utc>,
) -> StoreResult<Reminder> {
    conn.query_row(
        "SELECT id, note_id, title, description, date, mode, recurring_mode, selected_days,
                priority, disabled, snooze_until, local_only, date_created, date_modified, revision
         FROM reminders WHERE id = ?1 AND deleted = 0",
        params![reminder_id],
        |r| map_reminder_row(r, now),
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound(format!("reminder {reminder_id}")))
}

pub(crate) fn map_reminder_row(
    r: &rusqlite::Row<'_>,
    now: DateTime<Utc>,
) -> rusqlite::Result<Reminder> {
    let desc: String = r.get(3)?;
    let mode_str: String = r.get(5)?;
    let rec_str: Option<String> = r.get(6)?;
    let days_raw: String = r.get(7)?;
    let prio_str: String = r.get(8)?;
    let selected_days: Vec<u8> = serde_json::from_str(&days_raw).unwrap_or_default();

    let mut rem = Reminder {
        id: r.get(0)?,
        user_id: None,
        note_id: r.get(1)?,
        title: r.get(2)?,
        description: (!desc.is_empty()).then_some(desc),
        date: ms_to_utc(r.get(4)?),
        mode: ReminderMode::from_db_str(&mode_str),
        recurring_mode: rec_str.as_deref().and_then(RecurringMode::from_db_str),
        selected_days,
        priority: ReminderPriority::from_db_str(&prio_str),
        disabled: r.get::<_, i32>(9)? != 0,
        snooze_until: opt_ms_to_utc(r.get(10)?),
        next_occurrence: None,
        local_only: r.get::<_, i32>(11)? != 0,
        date_created: ms_to_utc(r.get(12)?),
        date_modified: ms_to_utc(r.get(13)?),
        deleted: false,
        revision: r.get(14)?,
    };
    rem.next_occurrence = next_occurrence(&rem, now);
    Ok(rem)
}

fn load_monograph_by_note_conn(
    conn: &Connection,
    note_id: &str,
) -> StoreResult<Option<Monograph>> {
    conn.query_row(
        "SELECT id, note_id, title, content, encrypted_content, self_destruct,
                view_count, active, date_published, date_updated
         FROM monographs WHERE note_id = ?1 AND deleted = 0",
        params![note_id],
        map_monograph_row,
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn map_monograph_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Monograph> {
    let enc_str: Option<String> = r.get(4)?;
    let encrypted_content = enc_str.and_then(|s| serde_json::from_str::<CipherEnvelope>(&s).ok());
    Ok(Monograph {
        id: r.get(0)?,
        note_id: r.get(1)?,
        user_id: None,
        title: r.get(2)?,
        content: r.get(3)?,
        encrypted_content,
        self_destruct: r.get::<_, i32>(5)? != 0,
        view_count: r.get::<_, i64>(6)?.max(0) as u64,
        active: r.get::<_, i32>(7)? != 0,
        date_published: ms_to_utc(r.get(8)?),
        date_updated: ms_to_utc(r.get(9)?),
        publish_url: None,
    })
}
