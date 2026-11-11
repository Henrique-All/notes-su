use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::models::{
    Attachment, CipherEnvelope, Color, ConflictedContent, HistorySession, Monograph, Note,
    NoteAttachment, NoteContent, NoteNotebook, NoteTag, Notebook, Reminder, SettingItem, Shortcut,
    Tag, Vault,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncChangeset {
    #[serde(default)]
    pub notes: Vec<Note>,
    #[serde(default)]
    pub contents: Vec<NoteContent>,
    #[serde(default)]
    pub notebooks: Vec<Notebook>,
    #[serde(default)]
    pub note_notebooks: Vec<NoteNotebook>,
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub note_tags: Vec<NoteTag>,
    #[serde(default)]
    pub colors: Vec<Color>,
    #[serde(default)]
    pub shortcuts: Vec<Shortcut>,
    #[serde(default)]
    pub reminders: Vec<Reminder>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    #[serde(default)]
    pub note_attachments: Vec<NoteAttachment>,
    #[serde(default)]
    pub vault: Option<Vault>,
    #[serde(default)]
    pub settings: Vec<SettingItem>,
}

impl SyncChangeset {
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
            && self.contents.is_empty()
            && self.notebooks.is_empty()
            && self.note_notebooks.is_empty()
            && self.tags.is_empty()
            && self.note_tags.is_empty()
            && self.colors.is_empty()
            && self.shortcuts.is_empty()
            && self.reminders.is_empty()
            && self.attachments.is_empty()
            && self.note_attachments.is_empty()
            && self.vault.is_none()
            && self.settings.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConflictRecord {
    pub note_id: String,
    #[serde(default, with = "crate::serde_helpers::flexible_opt_datetime")]
    pub local_date_edited: Option<DateTime<Utc>>,
    #[serde(default, with = "crate::serde_helpers::flexible_opt_datetime")]
    pub remote_date_edited: Option<DateTime<Utc>>,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::opt_conflicted_or_json_str"
    )]
    pub conflicted_data: Option<ConflictedContent>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictResolution {
    #[serde(alias = "keepLocal")]
    KeepLocal,
    #[serde(alias = "keepRemote")]
    KeepRemote,
    #[serde(alias = "keepBoth")]
    KeepBoth,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct BackupData {
    #[serde(flatten)]
    pub changes: SyncChangeset,
    #[serde(default)]
    pub history_sessions: Vec<HistorySession>,
    #[serde(default)]
    pub monographs: Vec<Monograph>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupBundle {
    pub version: u32,
    #[serde(with = "crate::serde_helpers::flexible_datetime")]
    pub exported_at: DateTime<Utc>,
    pub sha256: String,
    pub encrypted: bool,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::opt_envelope_or_json_str"
    )]
    pub cipher: Option<CipherEnvelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncPullRequest {
    pub cursor: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncPullResponse {
    pub next_cursor: i64,
    #[serde(default)]
    pub changes: SyncChangeset,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncPushRequest {
    pub device_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_cursor: Option<i64>,
    pub mutations: SyncChangeset,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SyncPushResponse {
    pub new_cursor: i64,
    #[serde(default)]
    pub applied_ids: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<ConflictRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncSseEvent {
    #[serde(default)]
    pub event: Option<String>,
    pub revision: i64,
    #[serde(default)]
    pub source_device_id: Option<String>,
}
