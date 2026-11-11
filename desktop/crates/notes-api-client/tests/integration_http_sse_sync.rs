use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use chrono::Utc;
use notes_api_client::*;
use notes_core::{ContentFormat, Note, NoteContent, SyncChangeset};
use notes_crypto::create_vault_canary;
use notes_store::{CreateNoteInput, NotesStore};
use serde_json::{Value, json};

#[derive(Default)]
struct MockServerState {
    refresh_calls: AtomicUsize,
    current_refresh_token: Mutex<String>,
    current_access_token: Mutex<String>,
    deleted_vault_bodies: Mutex<Vec<Value>>,
    pushed_mutations: Mutex<Vec<SyncChangeset>>,
    monograph_consumed: AtomicBool,
}

fn spawn_real_tcp_api_server() -> (String, Arc<MockServerState>, Arc<AtomicBool>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");

    let state = Arc::new(MockServerState {
        refresh_calls: AtomicUsize::new(0),
        current_refresh_token: Mutex::new("rt-initial".to_owned()),
        current_access_token: Mutex::new("at-expired".to_owned()),
        deleted_vault_bodies: Mutex::new(Vec::new()),
        pushed_mutations: Mutex::new(Vec::new()),
        monograph_consumed: AtomicBool::new(false),
    });
    let stop = Arc::new(AtomicBool::new(false));

    let st_clone = Arc::clone(&state);
    let stop_clone = Arc::clone(&stop);

    thread::spawn(move || {
        for stream_res in listener.incoming() {
            if stop_clone.load(Ordering::Relaxed) {
                break;
            }
            if let Ok(stream) = stream_res {
                let st = Arc::clone(&st_clone);
                thread::spawn(move || {
                    let _ = handle_http_connection(stream, st);
                });
            }
        }
    });

    (base_url, state, stop)
}

fn handle_http_connection(mut stream: TcpStream, state: Arc<MockServerState>) -> std::io::Result<()> {
    let cloned = stream.try_clone()?;
    let mut reader = BufReader::new(cloned);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let method = parts[0].to_uppercase();
    let path = parts[1].to_owned();

    let mut headers: HashMap<String, String> = HashMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
        }
    }

    let content_len: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body_bytes = vec![0u8; content_len];
    if content_len > 0 {
        reader.read_exact(&mut body_bytes)?;
    }

    let auth_header = headers.get("authorization").cloned().unwrap_or_default();
    let valid_access = state.current_access_token.lock().unwrap().clone();
    let is_authed = auth_header == format!("Bearer {valid_access}") && valid_access != "at-expired";

    match (method.as_str(), path.as_str()) {
        ("GET", "/api/health") => {
            write_json_response(&mut stream, 200, &json!({ "status": "ok", "version": "0.1.0" }))
        }
        ("POST", "/api/auth/login") => {
            *state.current_access_token.lock().unwrap() = "at-expired".to_owned();
            *state.current_refresh_token.lock().unwrap() = "rt-initial".to_owned();
            write_json_response(
                &mut stream,
                200,
                &json!({
                    "user": {
                        "id": "u-1",
                        "email": "user@notes.su",
                        "name": "Test User",
                        "createdAt": "2026-11-10T12:00:00Z",
                        "updatedAt": "2026-11-10T12:00:00Z"
                    },
                    "accessToken": "at-expired",
                    "refreshToken": "rt-initial",
                    "deviceId": "dev-client-1"
                }),
            )
        }
        ("POST", "/api/auth/refresh") => {
            state.refresh_calls.fetch_add(1, Ordering::SeqCst);
            let req_val: Value = serde_json::from_slice(&body_bytes).unwrap_or_default();
            let sent_rt = req_val
                .get("refreshToken")
                .and_then(Value::as_str)
                .unwrap_or("");
            let mut rt_guard = state.current_refresh_token.lock().unwrap();
            // Single-use refresh token rotation!
            if sent_rt != rt_guard.as_str() {
                return write_json_response(
                    &mut stream,
                    401,
                    &json!({ "error": "Refresh token reuse or invalid" }),
                );
            }
            *rt_guard = "rt-rotated-2".to_owned();
            *state.current_access_token.lock().unwrap() = "at-fresh-2".to_owned();
            write_json_response(
                &mut stream,
                200,
                &json!({
                    "accessToken": "at-fresh-2",
                    "refreshToken": "rt-rotated-2"
                }),
            )
        }
        ("GET", "/api/auth/me") => {
            if !is_authed {
                return write_json_response(&mut stream, 401, &json!({ "error": "Token expired" }));
            }
            write_json_response(
                &mut stream,
                200,
                &json!({
                    "user": {
                        "id": "u-1",
                        "email": "user@notes.su",
                        "name": "Test User",
                        "createdAt": "2026-11-10T12:00:00Z",
                        "updatedAt": "2026-11-10T12:00:00Z"
                    },
                    "syncRevision": 5
                }),
            )
        }
        ("DELETE", "/api/vault") => {
            if !is_authed {
                return write_json_response(&mut stream, 401, &json!({ "error": "Unauthorized" }));
            }
            let body_val: Value = serde_json::from_slice(&body_bytes).unwrap_or_default();
            state.deleted_vault_bodies.lock().unwrap().push(body_val);
            write_json_response(&mut stream, 200, &json!({ "ok": true }))
        }
        ("DELETE", "/api/attachments/lockedhash") => {
            if !is_authed {
                return write_json_response(&mut stream, 401, &json!({ "error": "Unauthorized" }));
            }
            write_json_response(
                &mut stream,
                409,
                &json!({ "error": "This attachment is inside a locked note." }),
            )
        }
        ("GET", "/api/public/monographs/mono-1") => {
            if state.monograph_consumed.load(Ordering::SeqCst) {
                return write_json_response(
                    &mut stream,
                    410,
                    &json!({ "error": "Monograph self-destructed" }),
                );
            }
            write_json_response(
                &mut stream,
                200,
                &json!({
                    "id": "mono-1",
                    "title": "Public Self-Destruct Note",
                    "content": "# Read once",
                    "selfDestruct": true,
                    "datePublished": "2026-11-10T12:00:00Z"
                }),
            )
        }
        ("POST", "/api/public/monographs/mono-1/consume") => {
            state.monograph_consumed.store(true, Ordering::SeqCst);
            write_json_response(&mut stream, 200, &json!({ "ok": true }))
        }
        ("POST", "/api/sync/pull") => {
            if !is_authed {
                return write_json_response(&mut stream, 401, &json!({ "error": "Unauthorized" }));
            }
            let now = Utc::now();
            let remote_note = Note {
                id: "remote-note-1".to_owned(),
                user_id: Some("u-1".to_owned()),
                title: "Pulled From Server".to_owned(),
                headline: "Server body".to_owned(),
                is_generated_title: false,
                pinned: true,
                favorite: false,
                archived: false,
                readonly: false,
                local_only: false,
                conflicted: false,
                locked: false,
                color_id: None,
                expiry_date: None,
                expiry_modified_at: None,
                trashed_at: None,
                deleted_by: None,
                deleted: false,
                date_created: now,
                date_edited: now,
                date_modified: now,
                revision: 5,
            };
            let remote_nc = NoteContent {
                id: "remote-nc-1".to_owned(),
                note_id: "remote-note-1".to_owned(),
                format: ContentFormat::Markdown,
                data: "Server body".to_owned(),
                locked: false,
                conflicted_data: None,
                date_edited: now,
                date_modified: now,
                date_resolved: None,
                deleted: false,
                revision: 5,
            };
            let mut cs = SyncChangeset::default();
            cs.notes.push(remote_note);
            cs.contents.push(remote_nc);
            write_json_response(
                &mut stream,
                200,
                &json!({
                    "nextCursor": 5,
                    "changes": cs
                }),
            )
        }
        ("POST", "/api/sync/push") => {
            if !is_authed {
                return write_json_response(&mut stream, 401, &json!({ "error": "Unauthorized" }));
            }
            let req: notes_core::SyncPushRequest = serde_json::from_slice(&body_bytes).unwrap();
            state
                .pushed_mutations
                .lock()
                .unwrap()
                .push(req.mutations.clone());
            write_json_response(
                &mut stream,
                200,
                &json!({
                    "newCursor": 6,
                    "appliedIds": req.mutations.notes.iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
                    "conflicts": []
                }),
            )
        }
        ("GET", "/api/sync/events") => {
            if !is_authed {
                return write_json_response(&mut stream, 401, &json!({ "error": "Unauthorized" }));
            }
            stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
            )?;
            stream.write_all(b": keepalive\n\n")?;
            stream.flush()?;
            stream.write_all(
                b"event: sync\ndata: {\"revision\":7,\"sourceDeviceId\":\"web-browser-99\"}\n\n",
            )?;
            stream.flush()?;
            stream.write_all(
                b"event: sync\ndata: {\"revision\":8,\"sourceDeviceId\":\"dev-client-1\"}\n\n",
            )?;
            stream.flush()
        }
        _ => write_json_response(&mut stream, 404, &json!({ "error": "Route not found" })),
    }
}

fn write_json_response(stream: &mut TcpStream, status: u16, body: &Value) -> std::io::Result<()> {
    let payload = serde_json::to_vec(body).unwrap();
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        409 => "Conflict",
        410 => "Gone",
        _ => "Response",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    )?;
    stream.write_all(&payload)?;
    stream.flush()
}

#[test]
fn single_flight_401_refresh_delete_body_error_mapping_sse_and_sync_engine() {
    let _ = create_vault_canary("test").unwrap();
    let (base_url, server_state, stop) = spawn_real_tcp_api_server();
    let store = NotesStore::in_memory().unwrap();
    let client = NotesApiClient::new(&base_url, Some(store.clone())).unwrap();

    assert_eq!(client.health().unwrap().status, "ok");

    // Login issues `at-expired` + `rt-initial` and persists auth_session in SQLite
    let auth = client.login("user@notes.su", "secret").unwrap();
    assert_eq!(auth.user.email, "user@notes.su");

    // Fire 4 CONCURRENT authenticated `me()` calls while access_token is expired!
    // Single-flight `refresh_lock` MUST invoke `POST /api/auth/refresh` AT MOST ONCE!
    let mut handles = Vec::new();
    for _ in 0..4 {
        let c = client.clone();
        handles.push(thread::spawn(move || c.me()));
    }
    for h in handles {
        let me = h.join().unwrap().expect("concurrent me() after single-flight refresh");
        assert_eq!(me.sync_revision, 5);
    }
    assert_eq!(server_state.refresh_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.get_auth_session().unwrap().unwrap().access_token,
        "at-fresh-2"
    );

    // Verify `DELETE /api/vault` actually transmits its JSON body via `.force_send_body()`!
    client
        .delete_vault(&DeleteVaultRequest {
            mode: "unlock_notes".to_owned(),
            unlocked_notes: vec![ReencryptedNoteItem {
                note_id: "n-1".to_owned(),
                data: "Plaintext".to_owned(),
            }],
        })
        .unwrap();
    let del_bodies = server_state.deleted_vault_bodies.lock().unwrap();
    assert_eq!(del_bodies.len(), 1);
    assert_eq!(del_bodies[0]["mode"], "unlock_notes");
    drop(del_bodies);

    // Verify 409 Conflict mapping for locked-note attachment deletion
    let err_409 = client.delete_attachment("lockedhash").unwrap_err();
    assert!(matches!(
        err_409,
        ApiClientError::Conflict { ref message, .. } if message == "This attachment is inside a locked note."
    ));

    // Verify Public Monograph read + consume + 410 Gone
    let pub_mono = client.get_public_monograph("mono-1").unwrap();
    assert!(pub_mono.self_destruct);
    client.consume_public_monograph("mono-1").unwrap();
    assert!(matches!(
        client.get_public_monograph("mono-1"),
        Err(ApiClientError::Gone { .. })
    ));

    // Verify SSE event streaming (`GET /api/sync/events`)
    let mut events = Vec::new();
    let shutdown = AtomicBool::new(false);
    client
        .subscribe_sync_events(&shutdown, |ev| {
            events.push(ev);
            if events.len() >= 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].revision, 7);
    assert_eq!(
        events[0].source_device_id.as_deref(),
        Some("web-browser-99")
    );

    // Verify `SyncEngine::sync_once()` pulls remote note AND pushes non-local_only local note!
    let _local_synced = store
        .create_note(CreateNoteInput {
            title: Some("Created On Desktop".to_owned()),
            content: Some("Sync me to server".to_owned()),
            ..Default::default()
        })
        .unwrap();
    let _local_private = store
        .create_note(CreateNoteInput {
            title: Some("Local Only Secret".to_owned()),
            content: Some("Stay offline".to_owned()),
            local_only: true,
            ..Default::default()
        })
        .unwrap();

    let engine = SyncEngine::new(store.clone(), client.clone());
    let outcome = engine.sync_once().unwrap();
    assert_eq!(outcome.pulled_cursor, 5);
    assert_eq!(outcome.pushed_cursor, 6);
    assert!(outcome.conflicts.is_empty());

    // Remote note is now in local SQLite, and pushed mutations excluded `local_only = true`!
    let pulled = store.get_note("remote-note-1").unwrap();
    assert_eq!(pulled.title, "Pulled From Server");
    let pushed_batches = server_state.pushed_mutations.lock().unwrap();
    assert_eq!(pushed_batches.len(), 1);
    assert_eq!(pushed_batches[0].notes.len(), 1);
    assert_eq!(pushed_batches[0].notes[0].title, "Created On Desktop");

    stop.store(true, Ordering::Relaxed);
}
