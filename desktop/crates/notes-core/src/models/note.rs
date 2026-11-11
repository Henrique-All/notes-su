use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::models::{Attachment, Color, ContentFormat, DeletedBy, Reminder, Tag};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConflictedContent {
    pub data: String,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub format: ContentFormat,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_edited: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_modified: DateTime<Utc>,
    #[serde(default)]
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NoteContent {
    pub id: String,
    pub note_id: String,
    #[serde(default)]
    pub format: ContentFormat,
    #[serde(default)]
    pub data: String,
    #[serde(default)]
    pub locked: bool,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::opt_conflicted_or_json_str"
    )]
    pub conflicted_data: Option<ConflictedContent>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_edited: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_modified: DateTime<Utc>,
    #[serde(default, with = "crate::serde_helpers::flexible_opt_datetime")]
    pub date_resolved: Option<DateTime<Utc>>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub headline: String,
    #[serde(default)]
    pub is_generated_title: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub local_only: bool,
    #[serde(default)]
    pub conflicted: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub color_id: Option<String>,
    #[serde(default, with = "crate::serde_helpers::flexible_opt_datetime")]
    pub expiry_date: Option<DateTime<Utc>>,
    #[serde(default, with = "crate::serde_helpers::flexible_opt_datetime")]
    pub expiry_modified_at: Option<DateTime<Utc>>,
    #[serde(
        default,
        alias = "dateDeleted",
        with = "crate::serde_helpers::flexible_opt_datetime"
    )]
    pub trashed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub deleted_by: Option<DeletedBy>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_created: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_edited: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_modified: DateTime<Utc>,
    #[serde(default)]
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub headline: String,
    #[serde(default)]
    pub is_generated_title: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub local_only: bool,
    #[serde(default)]
    pub conflicted: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub color_id: Option<String>,
    #[serde(default)]
    pub color: Option<Color>,
    #[serde(default, deserialize_with = "crate::serde_helpers::tags_flexible")]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub notebook_ids: Vec<String>,
    #[serde(default, with = "crate::serde_helpers::flexible_opt_datetime")]
    pub expiry_date: Option<DateTime<Utc>>,
    #[serde(
        default,
        alias = "dateDeleted",
        with = "crate::serde_helpers::flexible_opt_datetime"
    )]
    pub trashed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub deleted_by: Option<DeletedBy>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_created: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_edited: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_modified: DateTime<Utc>,
    #[serde(default)]
    pub has_attachments: bool,
    #[serde(default)]
    pub has_reminders: bool,
    #[serde(default)]
    pub snippet: Option<String>,
    #[serde(default)]
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NoteDetail {
    pub id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub headline: String,
    #[serde(default)]
    pub is_generated_title: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub readonly: bool,
    #[serde(default)]
    pub local_only: bool,
    #[serde(default)]
    pub conflicted: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub color_id: Option<String>,
    #[serde(default)]
    pub color: Option<Color>,
    #[serde(default)]
    pub content: Option<NoteContent>,
    #[serde(default, deserialize_with = "crate::serde_helpers::tags_flexible")]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub notebook_ids: Vec<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub reminders: Vec<Reminder>,
    #[serde(default, with = "crate::serde_helpers::flexible_opt_datetime")]
    pub expiry_date: Option<DateTime<Utc>>,
    #[serde(default, with = "crate::serde_helpers::flexible_opt_datetime")]
    pub expiry_modified_at: Option<DateTime<Utc>>,
    #[serde(
        default,
        alias = "dateDeleted",
        with = "crate::serde_helpers::flexible_opt_datetime"
    )]
    pub trashed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub deleted_by: Option<DeletedBy>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_created: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_edited: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_modified: DateTime<Utc>,
    #[serde(default)]
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BacklinkItem {
    pub note_id: String,
    pub title: String,
    pub snippet: String,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_edited: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistorySession {
    pub id: String,
    pub note_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub format: ContentFormat,
    pub data: String,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub compressed: bool,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_created: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_modified: DateTime<Utc>,
}
