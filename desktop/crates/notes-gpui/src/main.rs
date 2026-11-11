#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use gpui_kit::{AppContext as _, Bounds, KeyBinding, WindowBounds, WindowOptions, px, size};
use notes_api_client::NotesApiClient;
use notes_gpui::{CreateNewNote, NotesWorkspace, QuitApp, SaveActiveNote, ToggleEditorPreview};
use notes_store::{CreateNoteInput, NoteListFilter, NotesStore};

#[derive(Debug, Parser)]
#[command(name = "notes-gpui", about = "Privacy-first zero-knowledge GPUI notes desktop client")]
struct CliArgs {
    /// Path to the local SQLite database file (defaults to LOCALAPPDATA/XDG data directory)
    #[arg(long, env = "NOTES_DB_PATH")]
    db_path: Option<PathBuf>,

    /// Base URL for the notes_api synchronization server
    #[arg(long, env = "NOTES_API_URL", default_value = "http://127.0.0.1:3000")]
    api_url: String,

    /// Launch window, initialize SQLite + 3-pane GPUI workspace, render frame, and exit 0 cleanly
    #[arg(long)]
    smoke_test: bool,
}

fn resolve_default_db_path() -> PathBuf {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA")
        && !local_app_data.trim().is_empty()
    {
        return PathBuf::from(local_app_data).join("notes-su").join("notes.db");
    }
    if let Ok(app_data) = std::env::var("APPDATA")
        && !app_data.trim().is_empty()
    {
        return PathBuf::from(app_data).join("notes-su").join("notes.db");
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME")
        && !xdg.trim().is_empty()
    {
        return PathBuf::from(xdg).join("notes-su").join("notes.db");
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("notes-su")
            .join("notes.db");
    }
    if let Ok(user_profile) = std::env::var("USERPROFILE")
        && !user_profile.trim().is_empty()
    {
        return PathBuf::from(user_profile)
            .join("AppData")
            .join("Local")
            .join("notes-su")
            .join("notes.db");
    }
    PathBuf::from("notes-su.db")
}

fn main() -> anyhow::Result<()> {
    let args = CliArgs::parse();

    let (store, should_seed_welcome) = if let Some(ref explicit_path) = args.db_path {
        let is_new = !explicit_path.exists();
        (NotesStore::open(explicit_path)?, is_new && args.smoke_test)
    } else if args.smoke_test {
        (NotesStore::in_memory()?, true)
    } else {
        let default_path = resolve_default_db_path();
        let is_first_launch = !default_path.exists();
        (NotesStore::open(default_path)?, is_first_launch)
    };

    // Seed welcome note on first launch or smoke-test DB so all 3 panes render populated content
    if should_seed_welcome && store.list_notes(NoteListFilter::default())?.0.is_empty() {
        store.create_note(CreateNoteInput {
            title: Some("Bem-vindo ao notes-su".to_owned()),
            content: Some(
                "# Bem-vindo ao notes-su\n\nSeu bloco de notas privado inspirado no Notesnook (Rust + GPUI).\n\n- [x] Armazenamento local SQLite WAL + busca rápida FTS5\n- [x] Cofre criptografado Argon2id + XChaCha20-Poly1305\n- [ ] Criar seu primeiro caderno na barra lateral\n- [ ] Testar o modo Preview (`Ctrl+E`) e clicar nos checkboxes\n"
                    .to_owned(),
            ),
            tags: vec!["welcome".to_owned()],
            ..Default::default()
        })?;
    }

    let api_client = NotesApiClient::new(&args.api_url, Some(store.clone())).ok();
    let smoke_mode = args.smoke_test;

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-q", QuitApp, None),
            KeyBinding::new("cmd-q", QuitApp, None),
            KeyBinding::new("ctrl-n", CreateNewNote, None),
            KeyBinding::new("cmd-n", CreateNewNote, None),
            KeyBinding::new("ctrl-s", SaveActiveNote, None),
            KeyBinding::new("cmd-s", SaveActiveNote, None),
            KeyBinding::new("ctrl-e", ToggleEditorPreview, None),
            KeyBinding::new("cmd-e", ToggleEditorPreview, None),
        ]);

        let bounds = Bounds::centered(None, size(px(1200.0), px(780.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };

        let store_for_win = store.clone();
        let client_for_win = api_client.clone();
        let (_win_handle, workspace_entity) =
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| NotesWorkspace::new(store_for_win, client_for_win, window, cx))
            })
            .expect("failed to open notes-gpui window");

        if smoke_mode {
            let count = workspace_entity.read(cx).visible_note_count();
            assert!(count >= 1, "smoke test workspace must render at least 1 note");
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                cx.update(|cx| cx.quit());
            })
            .detach();
        }
    });

    Ok(())
}
