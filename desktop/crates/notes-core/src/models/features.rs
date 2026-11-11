use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::CipherEnvelope;

fn default_vault_title() -> String {
    "Default".to_owned()
}
fn default_lock_after() -> i32 {
    300
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Vault {
    pub id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default = "default_vault_title")]
    pub title: String,
    #[serde(
        alias = "key",
        deserialize_with = "crate::serde_helpers::envelope_or_json_str"
    )]
    pub key_canary: CipherEnvelope,
    #[serde(default = "default_lock_after", alias = "lockAfter")]
    pub lock_after_seconds: i32,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_created: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_modified: DateTime<Utc>,
    #[serde(default)]
    pub revision: i64,
}

pub type VaultDto = Vault;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReminderMode {
    #[default]
    Once,
    Repeat,
    Permanent,
}

impl ReminderMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Repeat => "repeat",
            Self::Permanent => "permanent",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "repeat" => Self::Repeat,
            "permanent" => Self::Permanent,
            _ => Self::Once,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RecurringMode {
    Day,
    Week,
    Month,
    Year,
}

impl RecurringMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "day" => Some(Self::Day),
            "week" => Some(Self::Week),
            "month" => Some(Self::Month),
            "year" => Some(Self::Year),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReminderPriority {
    Silent,
    #[default]
    Vibrate,
    Urgent,
}

impl ReminderPriority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Silent => "silent",
            Self::Vibrate => "vibrate",
            Self::Urgent => "urgent",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "silent" => Self::Silent,
            "urgent" => Self::Urgent,
            _ => Self::Vibrate,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub note_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(with = "crate::serde_helpers::flexible_datetime")]
    pub date: DateTime<Utc>,
    #[serde(default)]
    pub mode: ReminderMode,
    #[serde(default)]
    pub recurring_mode: Option<RecurringMode>,
    #[serde(default)]
    pub selected_days: Vec<u8>,
    #[serde(default)]
    pub priority: ReminderPriority,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default, with = "crate::serde_helpers::flexible_opt_datetime")]
    pub snooze_until: Option<DateTime<Utc>>,
    #[serde(default, with = "crate::serde_helpers::flexible_opt_datetime")]
    pub next_occurrence: Option<DateTime<Utc>>,
    #[serde(default)]
    pub local_only: bool,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_created: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_modified: DateTime<Utc>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    pub hash: String,
    pub filename: String,
    pub mime_type: String,
    pub size: u64,
    #[serde(default)]
    pub encrypted: bool,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::opt_envelope_or_json_str"
    )]
    pub encryption_meta: Option<CipherEnvelope>,
    #[serde(default)]
    pub storage_path: Option<String>,
    #[serde(default)]
    pub note_ids: Vec<String>,
    #[serde(default, with = "crate::serde_helpers::flexible_opt_datetime")]
    pub date_uploaded: Option<DateTime<Utc>>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_created: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_modified: DateTime<Utc>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub revision: i64,
}

pub type AttachmentDto = Attachment;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NoteAttachment {
    pub id: String,
    pub note_id: String,
    pub attachment_id: String,
    #[serde(default)]
    pub attachment_hash: Option<String>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_modified: DateTime<Utc>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub revision: i64,
}

impl NoteAttachment {
    pub fn deterministic_id(note_id: &str, attachment_id: &str) -> String {
        Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!("note_attachment:{note_id}:{attachment_id}").as_bytes(),
        )
        .to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Monograph {
    pub id: String,
    pub note_id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::opt_envelope_or_json_str"
    )]
    pub encrypted_content: Option<CipherEnvelope>,
    #[serde(default)]
    pub self_destruct: bool,
    #[serde(default)]
    pub view_count: u64,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_published: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_updated: DateTime<Utc>,
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default)]
    pub publish_url: Option<String>,
}

pub type MonographDto = Monograph;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PublicMonographDto {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::serde_helpers::opt_envelope_or_json_str"
    )]
    pub encrypted_content: Option<CipherEnvelope>,
    #[serde(default)]
    pub self_destruct: bool,
    #[serde(default)]
    pub view_count: Option<u64>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_published: DateTime<Utc>,
    #[serde(default, with = "crate::serde_helpers::flexible_opt_datetime")]
    pub date_updated: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SettingItem {
    pub id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    pub key: String,
    pub value: serde_json::Value,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_modified: DateTime<Utc>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub revision: i64,
}
