use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use chrono::Utc;
use notes_core::{
    Attachment, AuthResponse, BacklinkItem, BackupBundle, BreadcrumbItem, Color,
    HistorySession, MeResponse, Monograph, NoteDetail, NotebookDto, PublicMonographDto,
    RefreshResponse, Reminder, SettingItem, Shortcut, ShortcutItemType, SyncPullRequest,
    SyncPullResponse, SyncPushRequest, SyncPushResponse, Tag, Vault,
};
use notes_store::{NotebookNoteAction, NotesStore};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use ureq::unversioned::multipart::{Form, Part};
use ureq::{Agent, Body, config::Config, http::Response};
use url::Url;

use crate::dto::*;
use crate::error::{ApiClientError, ApiResult};

pub const MAX_API_BODY_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB

#[derive(Debug, Clone)]
pub struct TokenSession {
    pub access_token: String,
    pub refresh_token: String,
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthMode {
    None,
    Bearer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Clone)]
pub struct NotesApiClient {
    pub(crate) base_url: String,
    pub(crate) rest_agent: Agent,
    pub(crate) sse_agent: Agent,
    pub(crate) session: Arc<RwLock<Option<TokenSession>>>,
    pub(crate) refresh_lock: Arc<Mutex<()>>,
    pub(crate) store: Option<NotesStore>,
}

impl std::fmt::Debug for NotesApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotesApiClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl NotesApiClient {
    pub fn new(base_url: impl Into<String>, store: Option<NotesStore>) -> ApiResult<Self> {
        let raw = base_url.into();
        let trimmed = raw.trim().trim_end_matches('/').to_owned();
        let normalized = if trimmed.ends_with("/api") {
            trimmed
        } else {
            format!("{trimmed}/api")
        };

        let rest_config = Config::builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_recv_body(Some(Duration::from_secs(60)))
            .build();

        let sse_config = Config::builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(None)
            .timeout_recv_response(None)
            .timeout_recv_body(None)
            .build();

        let initial_session = if let Some(ref st) = store
            && let Some(saved) = st.get_auth_session()?
        {
            Some(TokenSession {
                access_token: saved.access_token,
                refresh_token: saved.refresh_token,
                device_id: Some(saved.device_id),
            })
        } else {
            None
        };

        Ok(Self {
            base_url: normalized,
            rest_agent: Agent::new_with_config(rest_config),
            sse_agent: Agent::new_with_config(sse_config),
            session: Arc::new(RwLock::new(initial_session)),
            refresh_lock: Arc::new(Mutex::new(())),
            store,
        })
    }

    pub fn set_tokens(
        &self,
        access_token: impl Into<String>,
        refresh_token: impl Into<String>,
        device_id: Option<String>,
    ) {
        let mut guard = self.session.write().unwrap_or_else(|e| e.into_inner());
        *guard = Some(TokenSession {
            access_token: access_token.into(),
            refresh_token: refresh_token.into(),
            device_id,
        });
    }

    pub fn current_tokens(&self) -> Option<TokenSession> {
        self.session
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn clear_tokens(&self) {
        let mut guard = self.session.write().unwrap_or_else(|e| e.into_inner());
        *guard = None;
    }

    pub(crate) fn endpoint_url(&self, path: &str) -> String {
        let clean = path.trim_start_matches('/');
        format!("{}/{}", self.base_url, clean)
    }

    fn execute_raw(
        &self,
        method: HttpMethod,
        url: &str,
        auth: AuthMode,
        body: Option<&Value>,
    ) -> ApiResult<Response<Body>> {
        let used_token = if auth == AuthMode::Bearer {
            let sess = self.current_tokens().ok_or_else(|| ApiClientError::Unauthorized {
                message: "No active authentication token.".to_owned(),
            })?;
            Some(sess.access_token)
        } else {
            None
        };

        let resp = self.send_single(method, url, used_token.as_deref(), body)?;
        if resp.status().as_u16() != 401 || auth == AuthMode::None {
            return Ok(resp);
        }

        // Single-flight 401 token refresh + one automatic retry!
        let stale_access = used_token.as_deref().unwrap_or("");
        let fresh_access = self.refresh_single_flight(stale_access)?;
        self.send_single(method, url, Some(&fresh_access), body)
    }

    fn send_single(
        &self,
        method: HttpMethod,
        url: &str,
        bearer: Option<&str>,
        body: Option<&Value>,
    ) -> ApiResult<Response<Body>> {
        match method {
            HttpMethod::Get => {
                let mut req = self.rest_agent.get(url);
                if let Some(tok) = bearer {
                    req = req.header("Authorization", &format!("Bearer {tok}"));
                }
                req.call().map_err(|e| ApiClientError::Transport(e.to_string()))
            }
            HttpMethod::Post => {
                let mut req = self.rest_agent.post(url);
                if let Some(tok) = bearer {
                    req = req.header("Authorization", &format!("Bearer {tok}"));
                }
                match body {
                    Some(val) => req
                        .send_json(val)
                        .map_err(|e| ApiClientError::Transport(e.to_string())),
                    None => req
                        .send_empty()
                        .map_err(|e| ApiClientError::Transport(e.to_string())),
                }
            }
            HttpMethod::Put => {
                let mut req = self.rest_agent.put(url);
                if let Some(tok) = bearer {
                    req = req.header("Authorization", &format!("Bearer {tok}"));
                }
                match body {
                    Some(val) => req
                        .send_json(val)
                        .map_err(|e| ApiClientError::Transport(e.to_string())),
                    None => req
                        .send_empty()
                        .map_err(|e| ApiClientError::Transport(e.to_string())),
                }
            }
            HttpMethod::Patch => {
                let mut req = self.rest_agent.patch(url);
                if let Some(tok) = bearer {
                    req = req.header("Authorization", &format!("Bearer {tok}"));
                }
                match body {
                    Some(val) => req
                        .send_json(val)
                        .map_err(|e| ApiClientError::Transport(e.to_string())),
                    None => req
                        .send_empty()
                        .map_err(|e| ApiClientError::Transport(e.to_string())),
                }
            }
            HttpMethod::Delete => {
                let mut req = self.rest_agent.delete(url);
                if let Some(tok) = bearer {
                    req = req.header("Authorization", &format!("Bearer {tok}"));
                }
                match body {
                    Some(val) => req
                        .force_send_body()
                        .send_json(val)
                        .map_err(|e| ApiClientError::Transport(e.to_string())),
                    None => req
                        .call()
                        .map_err(|e| ApiClientError::Transport(e.to_string())),
                }
            }
        }
    }

    pub(crate) fn refresh_single_flight(&self, stale_access_token: &str) -> ApiResult<String> {
        let _guard = self.refresh_lock.lock().unwrap_or_else(|e| e.into_inner());

        let current = self.current_tokens().ok_or_else(|| ApiClientError::Unauthorized {
            message: "Session logged out.".to_owned(),
        })?;

        // Another concurrent caller already refreshed while we waited for refresh_lock!
        if current.access_token != stale_access_token {
            return Ok(current.access_token);
        }

        let refresh_url = self.endpoint_url("auth/refresh");
        let resp = self
            .rest_agent
            .post(&refresh_url)
            .send_json(json!({ "refreshToken": current.refresh_token }))
            .map_err(|e| ApiClientError::Transport(e.to_string()))?;

        let status = resp.status().as_u16();
        let bytes = read_body_bytes(resp)?;
        if !(200..300).contains(&status) {
            self.clear_tokens();
            if let Some(ref st) = self.store {
                let _ = st.clear_auth_session();
            }
            return Err(map_error_status(status, &bytes));
        }

        let parsed: RefreshResponse = serde_json::from_slice(&bytes)?;
        let new_access = parsed
            .resolved_access_token()
            .ok_or_else(|| ApiClientError::Unauthorized {
                message: "Refresh response did not include an access token.".to_owned(),
            })?
            .to_owned();
        let new_refresh = parsed.refresh_token;

        self.set_tokens(
            new_access.clone(),
            new_refresh.clone(),
            current.device_id.clone(),
        );
        if let Some(ref st) = self.store {
            st.update_auth_tokens(&new_access, &new_refresh)?;
        }

        Ok(new_access)
    }

    fn request_json<T: DeserializeOwned>(
        &self,
        method: HttpMethod,
        path: &str,
        auth: AuthMode,
        body: Option<Value>,
    ) -> ApiResult<T> {
        let url = self.endpoint_url(path);
        let resp = self.execute_raw(method, &url, auth, body.as_ref())?;
        let status = resp.status().as_u16();
        let bytes = read_body_bytes(resp)?;
        if !(200..300).contains(&status) {
            return Err(map_error_status(status, &bytes));
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn request_empty(
        &self,
        method: HttpMethod,
        path: &str,
        auth: AuthMode,
        body: Option<Value>,
    ) -> ApiResult<()> {
        let url = self.endpoint_url(path);
        let resp = self.execute_raw(method, &url, auth, body.as_ref())?;
        let status = resp.status().as_u16();
        let bytes = read_body_bytes(resp)?;
        if !(200..300).contains(&status) {
            return Err(map_error_status(status, &bytes));
        }
        Ok(())
    }

    // --- 1. Health ---
    pub fn health(&self) -> ApiResult<HealthResponse> {
        self.request_json(HttpMethod::Get, "health", AuthMode::None, None)
    }

    // --- 2. Auth ---
    pub fn register(
        &self,
        email: &str,
        password: &str,
        name: Option<&str>,
    ) -> ApiResult<AuthResponse> {
        let device_id = self
            .store
            .as_ref()
            .and_then(|s| s.get_device_id().ok());
        let resp: AuthResponse = self.request_json(
            HttpMethod::Post,
            "auth/register",
            AuthMode::None,
            Some(json!({
                "email": email,
                "password": password,
                "name": name,
                "deviceId": device_id,
            })),
        )?;
        self.persist_auth_response(&resp)?;
        Ok(resp)
    }

    pub fn login(&self, email: &str, password: &str) -> ApiResult<AuthResponse> {
        let device_id = self
            .store
            .as_ref()
            .and_then(|s| s.get_device_id().ok());
        let resp: AuthResponse = self.request_json(
            HttpMethod::Post,
            "auth/login",
            AuthMode::None,
            Some(json!({
                "email": email,
                "password": password,
                "deviceId": device_id,
            })),
        )?;
        self.persist_auth_response(&resp)?;
        Ok(resp)
    }

    fn persist_auth_response(&self, resp: &AuthResponse) -> ApiResult<()> {
        let access = resp
            .resolved_access_token()
            .ok_or_else(|| ApiClientError::Unauthorized {
                message: "Auth response missing access token.".to_owned(),
            })?;
        self.set_tokens(
            access.to_owned(),
            resp.refresh_token.clone(),
            resp.device_id.clone(),
        );
        if let Some(ref st) = self.store {
            st.save_auth_session(
                &resp.user,
                access,
                &resp.refresh_token,
                resp.device_id.as_deref(),
            )?;
        }
        Ok(())
    }

    pub fn refresh_tokens(&self) -> ApiResult<String> {
        self.refresh_single_flight("")
    }

    pub fn logout(&self) -> ApiResult<()> {
        let refresh_tok = self.current_tokens().map(|t| t.refresh_token);
        let _ = self.request_empty(
            HttpMethod::Post,
            "auth/logout",
            AuthMode::Bearer,
            Some(json!({ "refreshToken": refresh_tok })),
        );
        self.clear_tokens();
        if let Some(ref st) = self.store {
            st.clear_auth_session()?;
        }
        Ok(())
    }

    pub fn me(&self) -> ApiResult<MeResponse> {
        self.request_json(HttpMethod::Get, "auth/me", AuthMode::Bearer, None)
    }

    pub fn change_password(&self, current_password: &str, new_password: &str) -> ApiResult<()> {
        let val: Value = self.request_json(
            HttpMethod::Post,
            "auth/change-password",
            AuthMode::Bearer,
            Some(json!({
                "currentPassword": current_password,
                "newPassword": new_password,
            })),
        )?;
        if let Ok(refreshed) = serde_json::from_value::<RefreshResponse>(val)
            && let Some(new_access) = refreshed.resolved_access_token()
        {
            let dev_id = self.current_tokens().and_then(|t| t.device_id);
            self.set_tokens(new_access.to_owned(), refreshed.refresh_token.clone(), dev_id);
            if let Some(ref st) = self.store {
                st.update_auth_tokens(new_access, &refreshed.refresh_token)?;
            }
        }
        Ok(())
    }

    // --- 3. Notes, History & Conflicts ---
    pub fn list_notes(&self, query: &NotesQuery) -> ApiResult<NotesListResponse> {
        let mut pairs: Vec<(&str, String)> = Vec::new();
        if let Some(ref v) = query.notebook_id {
            pairs.push(("notebookId", v.clone()));
        }
        if let Some(ref v) = query.tag_id {
            pairs.push(("tagId", v.clone()));
        }
        if let Some(ref v) = query.tag {
            pairs.push(("tag", v.clone()));
        }
        if let Some(ref v) = query.color_id {
            pairs.push(("colorId", v.clone()));
        }
        if let Some(v) = query.pinned {
            pairs.push(("pinned", v.to_string()));
        }
        if let Some(v) = query.favorite {
            pairs.push(("favorite", v.to_string()));
        }
        if let Some(v) = query.archived {
            pairs.push(("archived", v.to_string()));
        }
        if let Some(v) = query.locked {
            pairs.push(("locked", v.to_string()));
        }
        if let Some(v) = query.trashed {
            pairs.push(("trashed", v.to_string()));
        }
        if let Some(ref v) = query.q {
            pairs.push(("q", v.clone()));
        }
        if let Some(v) = query.group_by {
            pairs.push(("groupBy", v.as_str().to_owned()));
        }
        if let Some(v) = query.sort_by {
            pairs.push(("sortBy", v.as_str().to_owned()));
        }
        if let Some(v) = query.sort_direction {
            pairs.push(("sortDirection", v.as_str().to_owned()));
        }

        let path = build_path_with_query("notes", &pairs);
        let val: Value = self.request_json(HttpMethod::Get, &path, AuthMode::Bearer, None)?;
        if val.is_array() {
            let notes: Vec< notes_core::NoteSummary > = serde_json::from_value(val)?;
            Ok(NotesListResponse {
                total: Some(notes.len()),
                notes,
                groups: Vec::new(),
            })
        } else {
            Ok(serde_json::from_value(val)?)
        }
    }

    pub fn create_note(&self, req: &CreateNoteRequest) -> ApiResult<NoteDetail> {
        let wrapped: WrappedNote = self.request_json(
            HttpMethod::Post,
            "notes",
            AuthMode::Bearer,
            Some(serde_json::to_value(req)?),
        )?;
        Ok(match wrapped {
            WrappedNote::Wrapped { note } | WrappedNote::Direct(note) => note,
        })
    }

    pub fn get_note(&self, note_id: &str) -> ApiResult<NoteDetail> {
        let wrapped: WrappedNote = self.request_json(
            HttpMethod::Get,
            &format!("notes/{note_id}"),
            AuthMode::Bearer,
            None,
        )?;
        Ok(match wrapped {
            WrappedNote::Wrapped { note } | WrappedNote::Direct(note) => note,
        })
    }

    pub fn update_note(&self, note_id: &str, req: &UpdateNoteRequest) -> ApiResult<NoteDetail> {
        let wrapped: WrappedNote = self.request_json(
            HttpMethod::Patch,
            &format!("notes/{note_id}"),
            AuthMode::Bearer,
            Some(serde_json::to_value(req)?),
        )?;
        Ok(match wrapped {
            WrappedNote::Wrapped { note } | WrappedNote::Direct(note) => note,
        })
    }

    pub fn duplicate_note(&self, note_id: &str) -> ApiResult<NoteDetail> {
        let wrapped: WrappedNote = self.request_json(
            HttpMethod::Post,
            &format!("notes/{note_id}/duplicate"),
            AuthMode::Bearer,
            Some(json!({})),
        )?;
        Ok(match wrapped {
            WrappedNote::Wrapped { note } | WrappedNote::Direct(note) => note,
        })
    }

    pub fn restore_note(&self, note_id: &str) -> ApiResult<NoteDetail> {
        let wrapped: WrappedNote = self.request_json(
            HttpMethod::Post,
            &format!("notes/{note_id}/restore"),
            AuthMode::Bearer,
            Some(json!({})),
        )?;
        Ok(match wrapped {
            WrappedNote::Wrapped { note } | WrappedNote::Direct(note) => note,
        })
    }

    pub fn delete_note(
        &self,
        note_id: &str,
        permanent: bool,
        vault_verified: bool,
    ) -> ApiResult<()> {
        let pairs = [
            ("permanent", permanent.to_string()),
            ("vaultVerified", vault_verified.to_string()),
        ];
        let path = build_path_with_query(&format!("notes/{note_id}"), &pairs);
        self.request_empty(HttpMethod::Delete, &path, AuthMode::Bearer, None)
    }

    pub fn get_backlinks(&self, note_id: &str) -> ApiResult<Vec<BacklinkItem>> {
        let wrapped: WrappedBacklinks = self.request_json(
            HttpMethod::Get,
            &format!("notes/{note_id}/backlinks"),
            AuthMode::Bearer,
            None,
        )?;
        Ok(match wrapped {
            WrappedBacklinks::Wrapped { backlinks } | WrappedBacklinks::Direct(backlinks) => {
                backlinks
            }
        })
    }

    pub fn list_note_history(&self, note_id: &str) -> ApiResult<Vec<HistorySession>> {
        let wrapped: WrappedHistorySessions = self.request_json(
            HttpMethod::Get,
            &format!("notes/{note_id}/history"),
            AuthMode::Bearer,
            None,
        )?;
        Ok(match wrapped {
            WrappedHistorySessions::Wrapped { sessions }
            | WrappedHistorySessions::Direct(sessions) => sessions,
        })
    }

    pub fn create_note_history(
        &self,
        note_id: &str,
        req: &CreateHistorySessionRequest,
    ) -> ApiResult<HistorySession> {
        let wrapped: WrappedHistorySession = self.request_json(
            HttpMethod::Post,
            &format!("notes/{note_id}/history"),
            AuthMode::Bearer,
            Some(serde_json::to_value(req)?),
        )?;
        Ok(match wrapped {
            WrappedHistorySession::Wrapped { session }
            | WrappedHistorySession::Direct(session) => session,
        })
    }

    pub fn restore_note_history(
        &self,
        note_id: &str,
        session_id: &str,
    ) -> ApiResult<NoteDetail> {
        let wrapped: WrappedNote = self.request_json(
            HttpMethod::Post,
            &format!("notes/{note_id}/history/{session_id}/restore"),
            AuthMode::Bearer,
            Some(json!({})),
        )?;
        Ok(match wrapped {
            WrappedNote::Wrapped { note } | WrappedNote::Direct(note) => note,
        })
    }

    pub fn resolve_note_conflict(
        &self,
        note_id: &str,
        req: &ResolveConflictRequest,
    ) -> ApiResult<ResolveConflictResponse> {
        self.request_json(
            HttpMethod::Post,
            &format!("notes/{note_id}/resolve-conflict"),
            AuthMode::Bearer,
            Some(serde_json::to_value(req)?),
        )
    }

    // --- 4. Notebooks ---
    pub fn list_notebooks(&self, tree: bool) -> ApiResult<Vec<NotebookDto>> {
        let path = build_path_with_query("notebooks", &[("tree", tree.to_string())]);
        let wrapped: WrappedNotebooks =
            self.request_json(HttpMethod::Get, &path, AuthMode::Bearer, None)?;
        Ok(match wrapped {
            WrappedNotebooks::Wrapped { notebooks } | WrappedNotebooks::Direct(notebooks) => {
                notebooks
            }
        })
    }

    pub fn create_notebook(&self, req: &CreateNotebookRequest) -> ApiResult<NotebookDto> {
        let wrapped: WrappedNotebook = self.request_json(
            HttpMethod::Post,
            "notebooks",
            AuthMode::Bearer,
            Some(serde_json::to_value(req)?),
        )?;
        Ok(match wrapped {
            WrappedNotebook::Wrapped { notebook } | WrappedNotebook::Direct(notebook) => notebook,
        })
    }

    pub fn update_notebook(
        &self,
        notebook_id: &str,
        req: &UpdateNotebookRequest,
    ) -> ApiResult<NotebookDto> {
        let wrapped: WrappedNotebook = self.request_json(
            HttpMethod::Patch,
            &format!("notebooks/{notebook_id}"),
            AuthMode::Bearer,
            Some(serde_json::to_value(req)?),
        )?;
        Ok(match wrapped {
            WrappedNotebook::Wrapped { notebook } | WrappedNotebook::Direct(notebook) => notebook,
        })
    }

    pub fn delete_notebook(&self, notebook_id: &str, permanent: bool) -> ApiResult<()> {
        let path = build_path_with_query(
            &format!("notebooks/{notebook_id}"),
            &[("permanent", permanent.to_string())],
        );
        self.request_empty(HttpMethod::Delete, &path, AuthMode::Bearer, None)
    }

    pub fn restore_notebook(&self, notebook_id: &str) -> ApiResult<NotebookDto> {
        let wrapped: WrappedNotebook = self.request_json(
            HttpMethod::Post,
            &format!("notebooks/{notebook_id}/restore"),
            AuthMode::Bearer,
            Some(json!({})),
        )?;
        Ok(match wrapped {
            WrappedNotebook::Wrapped { notebook } | WrappedNotebook::Direct(notebook) => notebook,
        })
    }

    pub fn get_notebook_breadcrumbs(&self, notebook_id: &str) -> ApiResult<Vec<BreadcrumbItem>> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum WrappedBreadcrumbs {
            Wrapped { breadcrumbs: Vec<BreadcrumbItem> },
            Direct(Vec<BreadcrumbItem>),
        }
        let wrapped: WrappedBreadcrumbs = self.request_json(
            HttpMethod::Get,
            &format!("notebooks/{notebook_id}/breadcrumbs"),
            AuthMode::Bearer,
            None,
        )?;
        Ok(match wrapped {
            WrappedBreadcrumbs::Wrapped { breadcrumbs }
            | WrappedBreadcrumbs::Direct(breadcrumbs) => breadcrumbs,
        })
    }

    pub fn modify_notebook_notes(
        &self,
        notebook_id: &str,
        note_ids: &[String],
        action: NotebookNoteAction,
    ) -> ApiResult<()> {
        self.request_empty(
            HttpMethod::Post,
            &format!("notebooks/{notebook_id}/notes"),
            AuthMode::Bearer,
            Some(json!({
                "noteIds": note_ids,
                "action": action,
            })),
        )
    }

    // --- 5. Tags, Colors, Shortcuts, Reminders, Settings ---
    pub fn list_tags(&self) -> ApiResult<Vec<Tag>> {
        let wrapped: WrappedTags =
            self.request_json(HttpMethod::Get, "tags", AuthMode::Bearer, None)?;
        Ok(match wrapped {
            WrappedTags::Wrapped { tags } | WrappedTags::Direct(tags) => tags,
        })
    }

    pub fn create_tag(&self, id: Option<&str>, title: &str) -> ApiResult<Tag> {
        let wrapped: WrappedTag = self.request_json(
            HttpMethod::Post,
            "tags",
            AuthMode::Bearer,
            Some(json!({ "id": id, "title": title })),
        )?;
        Ok(match wrapped {
            WrappedTag::Wrapped { tag } | WrappedTag::Direct(tag) => tag,
        })
    }

    pub fn rename_tag(&self, tag_id: &str, title: &str) -> ApiResult<Tag> {
        let wrapped: WrappedTag = self.request_json(
            HttpMethod::Patch,
            &format!("tags/{tag_id}"),
            AuthMode::Bearer,
            Some(json!({ "title": title })),
        )?;
        Ok(match wrapped {
            WrappedTag::Wrapped { tag } | WrappedTag::Direct(tag) => tag,
        })
    }

    pub fn delete_tag(&self, tag_id: &str) -> ApiResult<()> {
        self.request_empty(
            HttpMethod::Delete,
            &format!("tags/{tag_id}"),
            AuthMode::Bearer,
            None,
        )
    }

    pub fn list_colors(&self) -> ApiResult<Vec<Color>> {
        let wrapped: WrappedColors =
            self.request_json(HttpMethod::Get, "colors", AuthMode::Bearer, None)?;
        Ok(match wrapped {
            WrappedColors::Wrapped { colors } | WrappedColors::Direct(colors) => colors,
        })
    }

    pub fn create_color(
        &self,
        id: Option<&str>,
        title: &str,
        color_code: &str,
    ) -> ApiResult<Color> {
        let wrapped: WrappedColor = self.request_json(
            HttpMethod::Post,
            "colors",
            AuthMode::Bearer,
            Some(json!({ "id": id, "title": title, "colorCode": color_code })),
        )?;
        Ok(match wrapped {
            WrappedColor::Wrapped { color } | WrappedColor::Direct(color) => color,
        })
    }

    pub fn update_color(
        &self,
        color_id: &str,
        title: Option<&str>,
        color_code: Option<&str>,
    ) -> ApiResult<Color> {
        let wrapped: WrappedColor = self.request_json(
            HttpMethod::Patch,
            &format!("colors/{color_id}"),
            AuthMode::Bearer,
            Some(json!({ "title": title, "colorCode": color_code })),
        )?;
        Ok(match wrapped {
            WrappedColor::Wrapped { color } | WrappedColor::Direct(color) => color,
        })
    }

    pub fn delete_color(&self, color_id: &str) -> ApiResult<()> {
        self.request_empty(
            HttpMethod::Delete,
            &format!("colors/{color_id}"),
            AuthMode::Bearer,
            None,
        )
    }

    pub fn list_shortcuts(&self) -> ApiResult<Vec<Shortcut>> {
        let wrapped: WrappedShortcuts =
            self.request_json(HttpMethod::Get, "shortcuts", AuthMode::Bearer, None)?;
        Ok(match wrapped {
            WrappedShortcuts::Wrapped { shortcuts } | WrappedShortcuts::Direct(shortcuts) => {
                shortcuts
            }
        })
    }

    pub fn create_shortcut(
        &self,
        id: Option<&str>,
        item_id: &str,
        item_type: ShortcutItemType,
        sort_index: Option<i32>,
    ) -> ApiResult<Shortcut> {
        let wrapped: WrappedShortcut = self.request_json(
            HttpMethod::Post,
            "shortcuts",
            AuthMode::Bearer,
            Some(json!({
                "id": id,
                "itemId": item_id,
                "itemType": item_type,
                "sortIndex": sort_index,
            })),
        )?;
        Ok(match wrapped {
            WrappedShortcut::Wrapped { shortcut } | WrappedShortcut::Direct(shortcut) => shortcut,
        })
    }

    pub fn update_shortcut(&self, shortcut_id: &str, sort_index: i32) -> ApiResult<Shortcut> {
        let wrapped: WrappedShortcut = self.request_json(
            HttpMethod::Patch,
            &format!("shortcuts/{shortcut_id}"),
            AuthMode::Bearer,
            Some(json!({ "sortIndex": sort_index })),
        )?;
        Ok(match wrapped {
            WrappedShortcut::Wrapped { shortcut } | WrappedShortcut::Direct(shortcut) => shortcut,
        })
    }

    pub fn delete_shortcut(&self, shortcut_id: &str) -> ApiResult<()> {
        self.request_empty(
            HttpMethod::Delete,
            &format!("shortcuts/{shortcut_id}"),
            AuthMode::Bearer,
            None,
        )
    }

    pub fn list_reminders(&self) -> ApiResult<Vec<Reminder>> {
        let wrapped: WrappedReminders =
            self.request_json(HttpMethod::Get, "reminders", AuthMode::Bearer, None)?;
        Ok(match wrapped {
            WrappedReminders::Wrapped { reminders } | WrappedReminders::Direct(reminders) => {
                reminders
            }
        })
    }

    pub fn create_reminder(&self, req: &CreateReminderRequest) -> ApiResult<Reminder> {
        let wrapped: WrappedReminder = self.request_json(
            HttpMethod::Post,
            "reminders",
            AuthMode::Bearer,
            Some(serde_json::to_value(req)?),
        )?;
        Ok(match wrapped {
            WrappedReminder::Wrapped { reminder } | WrappedReminder::Direct(reminder) => reminder,
        })
    }

    pub fn update_reminder(
        &self,
        reminder_id: &str,
        req: &UpdateReminderRequest,
    ) -> ApiResult<Reminder> {
        let wrapped: WrappedReminder = self.request_json(
            HttpMethod::Patch,
            &format!("reminders/{reminder_id}"),
            AuthMode::Bearer,
            Some(serde_json::to_value(req)?),
        )?;
        Ok(match wrapped {
            WrappedReminder::Wrapped { reminder } | WrappedReminder::Direct(reminder) => reminder,
        })
    }

    pub fn delete_reminder(&self, reminder_id: &str) -> ApiResult<()> {
        self.request_empty(
            HttpMethod::Delete,
            &format!("reminders/{reminder_id}"),
            AuthMode::Bearer,
            None,
        )
    }

    pub fn list_settings(&self) -> ApiResult<Vec<SettingItem>> {
        let wrapped: WrappedSettings =
            self.request_json(HttpMethod::Get, "settings", AuthMode::Bearer, None)?;
        Ok(match wrapped {
            WrappedSettings::Wrapped { settings } | WrappedSettings::Direct(settings) => settings,
            WrappedSettings::Map { settings } => {
                let now = Utc::now();
                settings
                    .into_iter()
                    .map(|(k, v)| SettingItem {
                        id: k.clone(),
                        user_id: None,
                        key: k,
                        value: v,
                        date_modified: now,
                        deleted: false,
                        revision: 0,
                    })
                    .collect()
            }
        })
    }

    pub fn put_setting(&self, key: &str, value: Value) -> ApiResult<()> {
        self.request_empty(
            HttpMethod::Put,
            &format!("settings/{key}"),
            AuthMode::Bearer,
            Some(json!({ "value": value })),
        )
    }

    // --- 6. Trash ---
    pub fn get_trash(&self) -> ApiResult<TrashResponse> {
        self.request_json(HttpMethod::Get, "trash", AuthMode::Bearer, None)
    }

    pub fn restore_trash(&self, ids: &[String]) -> ApiResult<usize> {
        let val: Value = self.request_json(
            HttpMethod::Post,
            "trash/restore",
            AuthMode::Bearer,
            Some(json!({ "ids": ids })),
        )?;
        Ok(val
            .get("restored")
            .and_then(Value::as_u64)
            .unwrap_or(ids.len() as u64) as usize)
    }

    pub fn cleanup_trash(&self) -> ApiResult<usize> {
        let val: Value = self.request_json(
            HttpMethod::Post,
            "trash/cleanup",
            AuthMode::Bearer,
            Some(json!({})),
        )?;
        Ok(val.get("purged").and_then(Value::as_u64).unwrap_or(0) as usize)
    }

    pub fn empty_trash(&self, vault_verified: bool) -> ApiResult<()> {
        let path = build_path_with_query("trash", &[("vaultVerified", vault_verified.to_string())]);
        self.request_empty(HttpMethod::Delete, &path, AuthMode::Bearer, None)
    }

    // --- 7. Vault ---
    pub fn get_vault(&self) -> ApiResult<Option<Vault>> {
        let url = self.endpoint_url("vault");
        let resp = self.execute_raw(HttpMethod::Get, &url, AuthMode::Bearer, None)?;
        let status = resp.status().as_u16();
        let bytes = read_body_bytes(resp)?;
        if status == 404 {
            return Ok(None);
        }
        if !(200..300).contains(&status) {
            return Err(map_error_status(status, &bytes));
        }
        if bytes.is_empty() || bytes == b"null" {
            return Ok(None);
        }
        let wrapped: WrappedVault = serde_json::from_slice(&bytes)?;
        Ok(match wrapped {
            WrappedVault::Wrapped { vault } => vault,
            WrappedVault::Direct(vault) => Some(vault),
        })
    }

    pub fn put_vault(&self, req: &PutVaultRequest) -> ApiResult<Vault> {
        let wrapped: WrappedVault = self.request_json(
            HttpMethod::Put,
            "vault",
            AuthMode::Bearer,
            Some(serde_json::to_value(req)?),
        )?;
        match wrapped {
            WrappedVault::Wrapped { vault: Some(v) } | WrappedVault::Direct(v) => Ok(v),
            WrappedVault::Wrapped { vault: None } => Err(ApiClientError::NotFound {
                message: "Vault not returned by PUT /vault".to_owned(),
            }),
        }
    }

    pub fn delete_vault(&self, req: &DeleteVaultRequest) -> ApiResult<()> {
        // Uses HttpMethod::Delete with JSON body -> `.force_send_body().send_json(req)`!
        self.request_empty(
            HttpMethod::Delete,
            "vault",
            AuthMode::Bearer,
            Some(serde_json::to_value(req)?),
        )
    }

    // --- 8. Attachments ---
    pub fn upload_attachment_json(&self, req: &UploadAttachmentJsonRequest) -> ApiResult<Attachment> {
        let wrapped: WrappedAttachment = self.request_json(
            HttpMethod::Post,
            "attachments",
            AuthMode::Bearer,
            Some(serde_json::to_value(req)?),
        )?;
        Ok(match wrapped {
            WrappedAttachment::Wrapped { attachment } | WrappedAttachment::Direct(attachment) => {
                attachment
            }
        })
    }

    pub fn upload_attachment_multipart(
        &self,
        filename: &str,
        mime_type: &str,
        bytes: &[u8],
        hash: &str,
        note_id: Option<&str>,
    ) -> ApiResult<Attachment> {
        let url = self.endpoint_url("attachments");
        let sess = self.current_tokens().ok_or_else(|| ApiClientError::Unauthorized {
            message: "No active authentication token.".to_owned(),
        })?;

        let send_once = |token: &str| -> ApiResult<Response<Body>> {
            let file_part = Part::bytes(bytes)
                .file_name(filename)
                .mime_str(mime_type)
                .map_err(|e| ApiClientError::Transport(e.to_string()))?;
            let mut form = Form::new()
                .text("hash", hash)
                .text("filename", filename)
                .text("mimeType", mime_type)
                .part("file", file_part);
            if let Some(nid) = note_id {
                form = form.text("noteId", nid);
            }
            self.rest_agent
                .post(&url)
                .header("Authorization", &format!("Bearer {token}"))
                .send(form)
                .map_err(|e| ApiClientError::Transport(e.to_string()))
        };

        let mut resp = send_once(&sess.access_token)?;
        if resp.status().as_u16() == 401 {
            let fresh = self.refresh_single_flight(&sess.access_token)?;
            resp = send_once(&fresh)?;
        }

        let status = resp.status().as_u16();
        let resp_bytes = read_body_bytes(resp)?;
        if !(200..300).contains(&status) {
            return Err(map_error_status(status, &resp_bytes));
        }

        let wrapped: WrappedAttachment = serde_json::from_slice(&resp_bytes)?;
        Ok(match wrapped {
            WrappedAttachment::Wrapped { attachment } | WrappedAttachment::Direct(attachment) => {
                attachment
            }
        })
    }

    pub fn list_attachments(&self) -> ApiResult<Vec<Attachment>> {
        let wrapped: WrappedAttachments =
            self.request_json(HttpMethod::Get, "attachments", AuthMode::Bearer, None)?;
        Ok(match wrapped {
            WrappedAttachments::Wrapped { attachments }
            | WrappedAttachments::Direct(attachments) => attachments,
        })
    }

    pub fn download_attachment_bytes(&self, hash: &str) -> ApiResult<Vec<u8>> {
        let url = self.endpoint_url(&format!("attachments/{hash}"));
        let resp = self.execute_raw(HttpMethod::Get, &url, AuthMode::Bearer, None)?;
        let status = resp.status().as_u16();
        let bytes = read_body_bytes(resp)?;
        if !(200..300).contains(&status) {
            return Err(map_error_status(status, &bytes));
        }
        Ok(bytes)
    }

    pub fn get_attachment_meta(&self, hash: &str) -> ApiResult<Attachment> {
        let wrapped: WrappedAttachment = self.request_json(
            HttpMethod::Get,
            &format!("attachments/{hash}?meta=true"),
            AuthMode::Bearer,
            None,
        )?;
        Ok(match wrapped {
            WrappedAttachment::Wrapped { attachment } | WrappedAttachment::Direct(attachment) => {
                attachment
            }
        })
    }

    pub fn delete_attachment(&self, hash: &str) -> ApiResult<()> {
        self.request_empty(
            HttpMethod::Delete,
            &format!("attachments/{hash}"),
            AuthMode::Bearer,
            None,
        )
    }

    // --- 9. Monographs (Authenticated + Unauthenticated Public) ---
    pub fn list_monographs(&self) -> ApiResult<Vec<Monograph>> {
        let wrapped: WrappedMonographs =
            self.request_json(HttpMethod::Get, "monographs", AuthMode::Bearer, None)?;
        Ok(match wrapped {
            WrappedMonographs::Wrapped { monographs } | WrappedMonographs::Direct(monographs) => {
                monographs
            }
        })
    }

    pub fn publish_monograph(&self, req: &PublishMonographRequest) -> ApiResult<Monograph> {
        let wrapped: WrappedMonograph = self.request_json(
            HttpMethod::Post,
            "monographs",
            AuthMode::Bearer,
            Some(serde_json::to_value(req)?),
        )?;
        Ok(match wrapped {
            WrappedMonograph::Wrapped { monograph } | WrappedMonograph::Direct(monograph) => {
                monograph
            }
        })
    }

    pub fn unpublish_monograph(&self, note_id: &str) -> ApiResult<()> {
        self.request_empty(
            HttpMethod::Delete,
            &format!("monographs/{note_id}"),
            AuthMode::Bearer,
            None,
        )
    }

    pub fn get_public_monograph(&self, monograph_id: &str) -> ApiResult<PublicMonographDto> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum WrappedPublic {
            Wrapped { monograph: PublicMonographDto },
            Direct(PublicMonographDto),
        }
        let wrapped: WrappedPublic = self.request_json(
            HttpMethod::Get,
            &format!("public/monographs/{monograph_id}"),
            AuthMode::None,
            None,
        )?;
        Ok(match wrapped {
            WrappedPublic::Wrapped { monograph } | WrappedPublic::Direct(monograph) => monograph,
        })
    }

    pub fn consume_public_monograph(&self, monograph_id: &str) -> ApiResult<()> {
        self.request_empty(
            HttpMethod::Post,
            &format!("public/monographs/{monograph_id}/consume"),
            AuthMode::None,
            Some(json!({})),
        )
    }

    // --- 10. Backups ---
    pub fn export_backup(&self, password: Option<&str>) -> ApiResult<BackupBundle> {
        self.request_json(
            HttpMethod::Post,
            "backups/export",
            AuthMode::Bearer,
            Some(json!({ "password": password })),
        )
    }

    pub fn import_backup(
        &self,
        bundle: &BackupBundle,
        password: Option<&str>,
    ) -> ApiResult<BackupImportResponse> {
        self.request_json(
            HttpMethod::Post,
            "backups/import",
            AuthMode::Bearer,
            Some(json!({
                "bundle": bundle,
                "password": password,
            })),
        )
    }

    // --- 11. Delta Sync ---
    pub fn sync_pull(&self, cursor: i64, device_id: Option<&str>) -> ApiResult<SyncPullResponse> {
        let req = SyncPullRequest {
            cursor,
            device_id: device_id.map(str::to_owned),
        };
        self.request_json(
            HttpMethod::Post,
            "sync/pull",
            AuthMode::Bearer,
            Some(serde_json::to_value(req)?),
        )
    }

    pub fn sync_push(&self, req: &SyncPushRequest) -> ApiResult<SyncPushResponse> {
        self.request_json(
            HttpMethod::Post,
            "sync/push",
            AuthMode::Bearer,
            Some(serde_json::to_value(req)?),
        )
    }
}

pub(crate) fn read_body_bytes(resp: Response<Body>) -> ApiResult<Vec<u8>> {
    resp.into_body()
        .with_config()
        .limit(MAX_API_BODY_BYTES)
        .read_to_vec()
        .map_err(|e| ApiClientError::Transport(e.to_string()))
}

pub(crate) fn map_error_status(status: u16, body_bytes: &[u8]) -> ApiClientError {
    let (message, code) = extract_error_message_and_code(status, body_bytes);
    match status {
        400 => ApiClientError::BadRequest { message, code },
        401 => ApiClientError::Unauthorized { message },
        403 => ApiClientError::Forbidden { message },
        404 => ApiClientError::NotFound { message },
        409 => ApiClientError::Conflict { message, code },
        410 => ApiClientError::Gone { message },
        413 => ApiClientError::PayloadTooLarge { message },
        422 => ApiClientError::Unprocessable { message },
        other => ApiClientError::ServerError {
            status: other,
            message,
        },
    }
}

fn extract_error_message_and_code(status: u16, body_bytes: &[u8]) -> (String, Option<String>) {
    if let Ok(val) = serde_json::from_slice::<Value>(body_bytes) {
        if let Some(err_obj) = val.get("error") {
            if let Some(s) = err_obj.as_str() {
                let code = val.get("code").and_then(Value::as_str).map(str::to_owned);
                return (s.to_owned(), code);
            }
            if let Some(msg) = err_obj.get("message").and_then(Value::as_str) {
                let code = err_obj
                    .get("code")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                return (msg.to_owned(), code);
            }
        }
        if let Some(msg) = val.get("message").and_then(Value::as_str) {
            let code = val.get("code").and_then(Value::as_str).map(str::to_owned);
            return (msg.to_owned(), code);
        }
    }

    let raw = String::from_utf8_lossy(body_bytes).trim().to_owned();
    if raw.is_empty() {
        (format!("HTTP {status}"), None)
    } else {
        (raw, None)
    }
}

fn build_path_with_query(base_path: &str, pairs: &[(&str, String)]) -> String {
    if pairs.is_empty() {
        return base_path.to_owned();
    }
    let mut dummy = Url::parse("http://localhost/").expect("valid base url");
    {
        let mut qp = dummy.query_pairs_mut();
        for (k, v) in pairs {
            qp.append_pair(k, v);
        }
    }
    match dummy.query() {
        Some(q) if !q.is_empty() => format!("{base_path}?{q}"),
        _ => base_path.to_owned(),
    }
}
