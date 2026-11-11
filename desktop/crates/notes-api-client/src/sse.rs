use std::io::{BufRead, BufReader};
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use notes_core::SyncSseEvent;

use crate::client::{NotesApiClient, map_error_status, read_body_bytes};
use crate::error::{ApiClientError, ApiResult};
use crate::sync_engine::SyncEngine;

impl NotesApiClient {
    /// Streams real-time SSE notifications from `GET /api/sync/events`.
    /// Automatically refreshes expired access tokens on `401` before opening the stream.
    pub fn subscribe_sync_events<F>(
        &self,
        shutdown: &AtomicBool,
        mut on_event: F,
    ) -> ApiResult<()>
    where
        F: FnMut(SyncSseEvent) -> ControlFlow<()>,
    {
        let url = self.endpoint_url("sync/events");
        let sess = self.current_tokens().ok_or_else(|| ApiClientError::Unauthorized {
            message: "No active authentication token for SSE.".to_owned(),
        })?;

        let mut resp = self
            .sse_agent
            .get(&url)
            .header("Accept", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Authorization", &format!("Bearer {}", sess.access_token))
            .call()
            .map_err(|e| ApiClientError::Transport(e.to_string()))?;

        if resp.status().as_u16() == 401 {
            let fresh = self.refresh_single_flight(&sess.access_token)?;
            resp = self
                .sse_agent
                .get(&url)
                .header("Accept", "text/event-stream")
                .header("Cache-Control", "no-cache")
                .header("Authorization", &format!("Bearer {fresh}"))
                .call()
                .map_err(|e| ApiClientError::Transport(e.to_string()))?;
        }

        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let err_bytes = read_body_bytes(resp)?;
            return Err(map_error_status(status, &err_bytes));
        }

        let mut reader = BufReader::new(resp.into_body().into_reader());
        let mut current_event: Option<String> = None;
        let mut data_lines: Vec<String> = Vec::new();
        let mut line = String::new();

        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            line.clear();
            let n = reader
                .read_line(&mut line)
                .map_err(|e| ApiClientError::Transport(e.to_string()))?;
            if n == 0 {
                break; // Server closed SSE stream cleanly
            }

            let trimmed = line.trim_end_matches(['\r', '\n']);

            if trimmed.is_empty() {
                if !data_lines.is_empty() {
                    let payload_str = data_lines.join("\n");
                    data_lines.clear();
                    let ev_type = current_event.take();

                    if let Ok(mut parsed) = serde_json::from_str::<SyncSseEvent>(&payload_str) {
                        if parsed.event.is_none() {
                            parsed.event = ev_type;
                        }
                        if on_event(parsed) == ControlFlow::Break(()) {
                            break;
                        }
                    }
                } else {
                    current_event = None;
                }
                continue;
            }

            if trimmed.starts_with(':') {
                continue; // SSE comment / keepalive heartbeat
            }

            if let Some(rest) = trimmed.strip_prefix("event:") {
                current_event = Some(rest.trim().to_owned());
            } else if let Some(rest) = trimmed.strip_prefix("data:") {
                let d = rest.strip_prefix(' ').unwrap_or(rest);
                data_lines.push(d.to_owned());
            }
        }

        Ok(())
    }
}

/// Spawns a background SSE listener thread with exponential backoff reconnect (`100ms` -> `5s`)
/// that triggers `engine.sync_once()` whenever a remote device publishes a higher revision!
pub fn spawn_sse_sync_worker<F>(
    engine: SyncEngine,
    shutdown: Arc<AtomicBool>,
    mut on_synced: F,
) -> JoinHandle<()>
where
    F: FnMut(&crate::sync_engine::SyncOutcome) + Send + 'static,
{
    thread::spawn(move || {
        let mut backoff_ms = 100u64;

        while !shutdown.load(Ordering::Relaxed) {
            let my_device_id = engine.store().get_device_id().ok();
            let res = engine
                .client()
                .subscribe_sync_events(&shutdown, |ev| {
                    backoff_ms = 100;
                    if shutdown.load(Ordering::Relaxed) {
                        return ControlFlow::Break(());
                    }
                    if let (Some(mine), Some(src)) = (&my_device_id, &ev.source_device_id)
                        && mine == src
                    {
                        return ControlFlow::Continue(());
                    }
                    let cur = engine.store().get_sync_cursor().unwrap_or(0);
                    if ev.revision > cur
                        && let Ok(outcome) = engine.sync_once()
                    {
                        on_synced(&outcome);
                    }
                    ControlFlow::Continue(())
                });

            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            if res.is_err() {
                let step_ms = 25u64;
                let mut waited = 0u64;
                while waited < backoff_ms && !shutdown.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(step_ms));
                    waited += step_ms;
                }
                backoff_ms = (backoff_ms * 2).min(5_000);
            }
        }
    })
}
