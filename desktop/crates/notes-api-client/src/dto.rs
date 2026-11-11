use std::collections::HashMap;

use chrono::{DateTime, Utc};
use notes_core::{
    Attachment, BacklinkItem, CipherEnvelope, Color, ConflictResolution, ContentFormat, GroupBy,
    GroupSection, HistorySession, Monograph, NoteDetail, NoteSummary, NotebookDto, RecurringMode,
    Reminder, ReminderMode, ReminderPriority, SettingItem, Shortcut, SortBy, SortDirection, Tag,
    Vault,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthResponse {
    pub status: String,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NotesQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notebook_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favorite: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trashed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_by: Option<GroupBy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_by: Option<SortBy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_direction: Option<SortDirection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NotesListResponse {
    #[serde(default)]
    pub notes: Vec<NoteSummary>,
    #[serde(default)]
    pub groups: Vec<GroupSection>,
    #[serde(default)]
    pub total: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateNoteRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<ContentFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favorite: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readonly: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiry_date: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notebook_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNoteRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<ContentFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub favorite: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readonly: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_id: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiry_date: Option<Option<DateTime<Utc>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notebook_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateHistorySessionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<ContentFormat>,
    #[serde(default)]
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveConflictRequest {
    pub resolution: ConflictResolution,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveConflictResponse {
    pub note: NoteDetail,
    #[serde(default)]
    pub copy_note: Option<NoteDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateNotebookRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNotebookRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateReminderRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_id: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub date: DateTime<Utc>,
    #[serde(default)]
    pub mode: ReminderMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurring_mode: Option<RecurringMode>,
    #[serde(default)]
    pub selected_days: Vec<u8>,
    #[serde(default)]
    pub priority: ReminderPriority,
    #[serde(default)]
    pub disabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snooze_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReminderRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<ReminderMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurring_mode: Option<Option<RecurringMode>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_days: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<ReminderPriority>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snooze_until: Option<Option<DateTime<Utc>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrashResponse {
    #[serde(default)]
    pub notes: Vec<NoteSummary>,
    #[serde(default)]
    pub notebooks: Vec<NotebookDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutVaultRequest {
    pub key_canary: CipherEnvelope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_after_seconds: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reencrypted_notes: Vec<ReencryptedNoteItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReencryptedNoteItem {
    pub note_id: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteVaultRequest {
    pub mode: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unlocked_notes: Vec<ReencryptedNoteItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadAttachmentJsonRequest {
    pub filename: String,
    pub mime_type: String,
    pub hash: String,
    pub data_base64: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_id: Option<String>,
    #[serde(default)]
    pub encrypted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_meta: Option<CipherEnvelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishMonographRequest {
    pub note_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_content: Option<CipherEnvelope>,
    #[serde(default)]
    pub self_destruct: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupImportResponse {
    pub ok: bool,
    #[serde(default)]
    pub counts: HashMap<String, usize>,
}

// Helper unwrappers tolerating both wrapped (`{ "note": ... }`) and direct (`{ "id": ... }`) JSON responses
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedNote {
    Wrapped { note: NoteDetail },
    Direct(NoteDetail),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedNotebook {
    Wrapped { notebook: NotebookDto },
    Direct(NotebookDto),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedNotebooks {
    Wrapped { notebooks: Vec<NotebookDto> },
    Direct(Vec<NotebookDto>),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedTag {
    Wrapped { tag: Tag },
    Direct(Tag),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedTags {
    Wrapped { tags: Vec<Tag> },
    Direct(Vec<Tag>),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedColor {
    Wrapped { color: Color },
    Direct(Color),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedColors {
    Wrapped { colors: Vec<Color> },
    Direct(Vec<Color>),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedShortcut {
    Wrapped { shortcut: Shortcut },
    Direct(Shortcut),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedShortcuts {
    Wrapped { shortcuts: Vec<Shortcut> },
    Direct(Vec<Shortcut>),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedReminder {
    Wrapped { reminder: Reminder },
    Direct(Reminder),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedReminders {
    Wrapped { reminders: Vec<Reminder> },
    Direct(Vec<Reminder>),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedHistorySessions {
    Wrapped { sessions: Vec<HistorySession> },
    Direct(Vec<HistorySession>),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedHistorySession {
    Wrapped { session: HistorySession },
    Direct(HistorySession),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedBacklinks {
    Wrapped { backlinks: Vec<BacklinkItem> },
    Direct(Vec<BacklinkItem>),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedVault {
    Wrapped { vault: Option<Vault> },
    Direct(Vault),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedAttachment {
    Wrapped { attachment: Attachment },
    Direct(Attachment),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedAttachments {
    Wrapped { attachments: Vec<Attachment> },
    Direct(Vec<Attachment>),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedMonograph {
    Wrapped { monograph: Monograph },
    Direct(Monograph),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedMonographs {
    Wrapped { monographs: Vec<Monograph> },
    Direct(Vec<Monograph>),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum WrappedSettings {
    Wrapped { settings: Vec<SettingItem> },
    Map { settings: HashMap<String, Value> },
    Direct(Vec<SettingItem>),
}
