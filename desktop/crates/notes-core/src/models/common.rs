use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ContentFormat {
    #[default]
    Markdown,
    #[serde(alias = "tiptap-json")]
    Tiptap,
}

impl ContentFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Tiptap => "tiptap",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "tiptap" | "tiptap-json" => Self::Tiptap,
            _ => Self::Markdown,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeletedBy {
    User,
    App,
    Expired,
}

impl DeletedBy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::App => "app",
            Self::Expired => "expired",
        }
    }

    pub fn from_db_str(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "user" => Some(Self::User),
            "app" => Some(Self::App),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum GroupBy {
    #[default]
    Default,
    Abc,
    Year,
    Month,
    Week,
    None,
}

impl GroupBy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Abc => "abc",
            Self::Year => "year",
            Self::Month => "month",
            Self::Week => "week",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum SortBy {
    #[default]
    DateEdited,
    DateCreated,
    DateModified,
    Title,
}

impl SortBy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DateEdited => "dateEdited",
            Self::DateCreated => "dateCreated",
            Self::DateModified => "dateModified",
            Self::Title => "title",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    Asc,
    #[default]
    Desc,
}

impl SortDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct GroupOptions {
    #[serde(default)]
    pub group_by: GroupBy,
    #[serde(default)]
    pub sort_by: SortBy,
    #[serde(default)]
    pub sort_direction: SortDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GroupSection {
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub note_ids: Vec<String>,
}
