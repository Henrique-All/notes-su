use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::DeletedBy;

pub const DEFAULT_COLORS: [(&str, &str); 7] = [
    ("red", "#f44336"),
    ("orange", "#FF9800"),
    ("yellow", "#FFD600"),
    ("green", "#4CAF50"),
    ("blue", "#2196F3"),
    ("purple", "#673AB7"),
    ("gray", "#9E9E9E"),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Notebook {
    pub id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub pinned: bool,
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
pub struct NotebookDto {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub total_notes: usize,
    #[serde(default)]
    pub children: Vec<NotebookDto>,
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
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BreadcrumbItem {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NoteNotebook {
    pub id: String,
    pub notebook_id: String,
    pub note_id: String,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_modified: DateTime<Utc>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub revision: i64,
}

impl NoteNotebook {
    pub fn deterministic_id(notebook_id: &str, note_id: &str) -> String {
        Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!("note_notebook:{notebook_id}:{note_id}").as_bytes(),
        )
        .to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    pub title: String,
    #[serde(default, alias = "totalNotes")]
    pub note_count: Option<usize>,
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
pub struct NoteTag {
    pub id: String,
    pub tag_id: String,
    pub note_id: String,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_modified: DateTime<Utc>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub revision: i64,
}

impl NoteTag {
    pub fn deterministic_id(tag_id: &str, note_id: &str) -> String {
        Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!("note_tag:{tag_id}:{note_id}").as_bytes(),
        )
        .to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Color {
    pub id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    pub title: String,
    #[serde(alias = "hex")]
    pub color_code: String,
    #[serde(default)]
    pub sort_index: i32,
    #[serde(default, alias = "totalNotes")]
    pub note_count: Option<usize>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_created: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_modified: DateTime<Utc>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub revision: i64,
}

impl Color {
    pub fn seed_defaults(user_id: Option<&str>, now: DateTime<Utc>) -> Vec<Self> {
        DEFAULT_COLORS
            .iter()
            .enumerate()
            .map(|(idx, (title, code))| Self {
                id: Uuid::new_v4().to_string(),
                user_id: user_id.map(str::to_owned),
                title: (*title).to_owned(),
                color_code: (*code).to_owned(),
                sort_index: idx as i32,
                note_count: Some(0),
                date_created: now,
                date_modified: now,
                deleted: false,
                revision: 0,
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShortcutItemType {
    Notebook,
    Tag,
}

impl ShortcutItemType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Notebook => "notebook",
            Self::Tag => "tag",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "notebook" => Some(Self::Notebook),
            "tag" => Some(Self::Tag),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Shortcut {
    pub id: String,
    #[serde(default)]
    pub user_id: Option<String>,
    pub item_id: String,
    pub item_type: ShortcutItemType,
    #[serde(default)]
    pub sort_index: i32,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_created: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub date_modified: DateTime<Utc>,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub revision: i64,
}
