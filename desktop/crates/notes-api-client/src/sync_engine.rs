use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STD};
use chrono::Utc;
use notes_core::SyncPushRequest;
use notes_store::NotesStore;

use crate::client::NotesApiClient;
use crate::dto::UploadAttachmentJsonRequest;
use crate::error::ApiResult;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SyncOutcome {
    pub pulled_cursor: i64,
    pub pushed_cursor: i64,
    pub pushed_items: usize,
    pub conflicts: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct SyncEngine {
    store: NotesStore,
    client: NotesApiClient,
}

impl SyncEngine {
    pub fn new(store: NotesStore, client: NotesApiClient) -> Self {
        Self { store, client }
    }

    pub fn store(&self) -> &NotesStore {
        &self.store
    }

    pub fn client(&self) -> &NotesApiClient {
        &self.client
    }

    /// Runs a complete offline-first delta sync cycle:
    /// 1. Sweeps expired local notes to Trash (`deleted_by = 'expired'`).
    /// 2. Pulls remote delta changes since `sync_cursor` and merges them via LWW + 60s conflict check.
    /// 3. Uploads any pending local binary attachment blobs so server has attachment rows before join links.
    /// 4. Collects unsynced non-`local_only` mutations and pushes them with `(id, date_modified)` CAS ack!
    pub fn sync_once(&self) -> ApiResult<SyncOutcome> {
        self.store.sweep_expired_notes(Utc::now())?;

        let cursor = self.store.get_sync_cursor()?;
        let device_id = self.store.get_device_id()?;

        let pull_resp = self.client.sync_pull(cursor, Some(&device_id))?;
        let mut conflicts = self
            .store
            .merge_remote_changes(&pull_resp.changes, pull_resp.next_cursor)?;

        let (mutations, ack_tokens) = self.store.collect_unsynced_changes()?;
        if mutations.is_empty() {
            return Ok(SyncOutcome {
                pulled_cursor: pull_resp.next_cursor,
                pushed_cursor: pull_resp.next_cursor,
                pushed_items: 0,
                conflicts,
            });
        }

        // Upload any new binary attachments that have a local_blob before pushing metadata
        for att in &mutations.attachments {
            if !att.deleted
                && att.date_uploaded.is_none()
                && let Some(blob) = self.store.get_attachment_blob(&att.hash)?
            {
                let _ = self.client.upload_attachment_json(&UploadAttachmentJsonRequest {
                    filename: att.filename.clone(),
                    mime_type: att.mime_type.clone(),
                    hash: att.hash.clone(),
                    data_base64: BASE64_STD.encode(&blob),
                    note_id: att.note_ids.first().cloned(),
                    encrypted: att.encrypted,
                    encryption_meta: att.encryption_meta.clone(),
                });
            }
        }

        let push_resp = self.client.sync_push(&SyncPushRequest {
            device_id,
            base_cursor: Some(pull_resp.next_cursor),
            mutations,
        })?;

        for cr in &push_resp.conflicts {
            if !conflicts.contains(&cr.note_id) {
                conflicts.push(cr.note_id.clone());
            }
        }

        let pushed_items = self.store.mark_pushed_synced(
            &ack_tokens,
            &push_resp.conflicts,
            push_resp.new_cursor,
        )?;

        Ok(SyncOutcome {
            pulled_cursor: pull_resp.next_cursor,
            pushed_cursor: push_resp.new_cursor,
            pushed_items,
            conflicts,
        })
    }
}
