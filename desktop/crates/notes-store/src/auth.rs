use chrono::Utc;
use notes_core::User;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::StoreResult;
use crate::schema::{NotesStore, ensure_local_device_id, ms_to_utc};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredAuthSession {
    pub user: User,
    pub access_token: String,
    pub refresh_token: String,
    pub device_id: String,
    pub sync_cursor: i64,
}

impl NotesStore {
    pub fn get_auth_session(&self) -> StoreResult<Option<StoredAuthSession>> {
        let conn = self.lock_conn();
        conn.query_row(
            "SELECT user_id, email, name, access_token, refresh_token, device_id, sync_cursor, created_at, updated_at
             FROM auth_session WHERE id = 1",
            [],
            |row| {
                let user_id: String = row.get(0)?;
                let email: String = row.get(1)?;
                let name: Option<String> = row.get(2)?;
                let access_token: String = row.get(3)?;
                let refresh_token: String = row.get(4)?;
                let device_id: String = row.get(5)?;
                let sync_cursor: i64 = row.get(6)?;
                let created_at: i64 = row.get(7)?;
                let updated_at: i64 = row.get(8)?;
                Ok(StoredAuthSession {
                    user: User {
                        id: user_id,
                        email,
                        name,
                        created_at: ms_to_utc(created_at),
                        updated_at: ms_to_utc(updated_at),
                    },
                    access_token,
                    refresh_token,
                    device_id,
                    sync_cursor,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn save_auth_session(
        &self,
        user: &User,
        access_token: &str,
        refresh_token: &str,
        device_id: Option<&str>,
    ) -> StoreResult<StoredAuthSession> {
        let conn = self.lock_conn();
        let now_ms = Utc::now().timestamp_millis();
        let existing_cursor: i64 = conn
            .query_row(
                "SELECT sync_cursor FROM auth_session WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);

        let resolved_device_id = match device_id.filter(|d| !d.trim().is_empty()) {
            Some(d) => d.to_owned(),
            None => ensure_local_device_id(&conn, now_ms)?,
        };

        conn.execute(
            "INSERT INTO auth_session (id, user_id, email, name, access_token, refresh_token, device_id, sync_cursor, created_at, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                 user_id = excluded.user_id,
                 email = excluded.email,
                 name = excluded.name,
                 access_token = excluded.access_token,
                 refresh_token = excluded.refresh_token,
                 device_id = excluded.device_id,
                 updated_at = excluded.updated_at",
            params![
                user.id,
                user.email,
                user.name,
                access_token,
                refresh_token,
                resolved_device_id,
                existing_cursor,
                user.created_at.timestamp_millis(),
                now_ms
            ],
        )?;

        drop(conn);
        self.get_auth_session()?
            .ok_or_else(|| crate::error::StoreError::NotFound("auth_session".to_owned()))
    }

    pub fn update_auth_tokens(&self, access_token: &str, refresh_token: &str) -> StoreResult<()> {
        let conn = self.lock_conn();
        let now_ms = Utc::now().timestamp_millis();
        conn.execute(
            "UPDATE auth_session SET access_token = ?1, refresh_token = ?2, updated_at = ?3 WHERE id = 1",
            params![access_token, refresh_token, now_ms],
        )?;
        Ok(())
    }

    pub fn clear_auth_session(&self) -> StoreResult<()> {
        let conn = self.lock_conn();
        conn.execute("DELETE FROM auth_session WHERE id = 1", [])?;
        Ok(())
    }

    pub fn get_sync_cursor(&self) -> StoreResult<i64> {
        let conn = self.lock_conn();
        if let Some(cursor) = conn
            .query_row("SELECT sync_cursor FROM auth_session WHERE id = 1", [], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
        {
            return Ok(cursor);
        }
        if let Some(raw_val) = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'local:syncCursor' AND deleted = 0",
                [],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            return Ok(serde_json::from_str::<i64>(&raw_val).unwrap_or(0));
        }
        Ok(0)
    }

    pub fn set_sync_cursor(&self, cursor: i64) -> StoreResult<()> {
        let conn = self.lock_conn();
        let now_ms = Utc::now().timestamp_millis();
        let updated = conn.execute(
            "UPDATE auth_session SET sync_cursor = ?1, updated_at = ?2 WHERE id = 1",
            params![cursor, now_ms],
        )?;
        if updated == 0 {
            let json_val = serde_json::to_string(&cursor)?;
            conn.execute(
                "INSERT INTO settings (id, key, value, local_only, deleted, synced, date_modified, revision)
                 VALUES (?1, 'local:syncCursor', ?2, 1, 0, 1, ?3, 0)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, date_modified = excluded.date_modified",
                params![uuid::Uuid::new_v4().to_string(), json_val, now_ms],
            )?;
        }
        Ok(())
    }

    pub fn get_device_id(&self) -> StoreResult<String> {
        let conn = self.lock_conn();
        if let Some(dev_id) = conn
            .query_row("SELECT device_id FROM auth_session WHERE id = 1", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            && !dev_id.is_empty()
        {
            return Ok(dev_id);
        }
        ensure_local_device_id(&conn, Utc::now().timestamp_millis())
    }
}
