use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(default = "Utc::now", with = "crate::serde_helpers::flexible_datetime")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub user: User,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub access_token: Option<String>,
    pub refresh_token: String,
    #[serde(default)]
    pub device_id: Option<String>,
}

impl AuthResponse {
    pub fn resolved_access_token(&self) -> Option<&str> {
        self.access_token.as_deref().or(self.token.as_deref())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResponse {
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub access_token: Option<String>,
    pub refresh_token: String,
}

impl RefreshResponse {
    pub fn resolved_access_token(&self) -> Option<&str> {
        self.access_token.as_deref().or(self.token.as_deref())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MeResponse {
    pub user: User,
    #[serde(default)]
    pub sync_revision: i64,
}
