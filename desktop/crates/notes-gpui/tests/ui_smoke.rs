extern crate gpui_kit as gpui;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, TestAppContext, WindowOptions};
use notes_core::{SortBy, SortDirection, parse_tasks};
use notes_gpui::{EditorMode, NotesWorkspace, SidebarSection};
use notes_store::{CreateNoteInput, NoteListFilter, NotesStore};

#[gpui_kit::test]
fn three_pane_workspace_renders_and_creates_notes_in_sqlite(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let store = NotesStore::in_memory().expect("in-memory sqlite store");
    store
        .create_note(CreateNoteInput {
            title: Some("Initial Seed Note".to_owned()),
            content: Some("# Seed\n\nHello GPUI!".to_owned()),
            favorite: true,
            ..Default::default()
        })
        .expect("seed note");

    let store_for_win = store.clone();
    let (handle, workspace) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
            cx.new(|cx| NotesWorkspace::new(store_for_win, None, window, cx))
        })
        .expect("open headless test window")
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);

        // Verify all 3 panes and the initial note card are visible
        assert!(window.find("notes-workspace-root").visible());
        assert!(window.find("pane-sidebar").visible());
        assert!(window.find("pane-note-list").visible());
        assert!(window.find("pane-editor").visible());
        assert!(window.find("note-card-0").visible());
        assert_eq!(workspace.read(cx).visible_note_count(), 1);

        // Click "+ New" button in the sidebar and verify SQLite + middle pane update!
        window.click("new-note-btn", cx);
        window.render_frame(cx);
        assert_eq!(workspace.read(cx).visible_note_count(), 2);
        assert!(window.find("note-card-1").visible());

        // Switch to Favorites section via sidebar click -> only 1 favorite note!
        window.click("nav-favorites", cx);
        window.render_frame(cx);
        assert_eq!(workspace.read(cx).active_section(), SidebarSection::Favorites);
        assert_eq!(workspace.read(cx).visible_note_count(), 1);
    })
    .unwrap();

    // Confirm both notes are genuinely persisted in SQLite
    let (persisted, _) = store.list_notes(NoteListFilter::default()).unwrap();
    assert_eq!(persisted.len(), 2);
}

#[gpui_kit::test]
fn sidebar_notebooks_tags_vault_and_trash_wire_to_store(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let store = NotesStore::in_memory().expect("in-memory sqlite store");
    let seed = store
        .create_note(CreateNoteInput {
            title: Some("Architecture Plan".to_owned()),
            content: Some("# Confidential\n\nZero-knowledge local-first design.\n".to_owned()),
            ..Default::default()
        })
        .expect("seed note");

    let store_for_win = store.clone();
    let (handle, workspace) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
            cx.new(|cx| NotesWorkspace::new(store_for_win, None, window, cx))
        })
        .expect("open headless test window")
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);

        // 1. Create a Notebook from Column 1 Sidebar
        window.click("new-notebook-input", cx);
        window.input("Engineering", cx);
        window.click("add-notebook-btn", cx);
        window.render_frame(cx);
        assert_eq!(workspace.read(cx).active_section(), SidebarSection::Notebooks);
        assert_eq!(store.list_notebooks(false).unwrap().len(), 1);

        // 2. Create a Tag from Column 1 Sidebar
        window.click("new-tag-input", cx);
        window.input("rust", cx);
        window.click("add-tag-btn", cx);
        window.render_frame(cx);
        assert_eq!(workspace.read(cx).active_section(), SidebarSection::Tags);
        assert_eq!(store.list_tags().unwrap().len(), 1);

        // 3. Initialize Personal Vault from Sidebar and lock the seed note from Column 3
        window.click("nav-notes", cx);
        window.render_frame(cx);
        window.click("vault-password-input", cx);
        window.input("correct-horse-battery", cx);
        window.click("vault-action-btn", cx);
        window.render_frame(cx);
        assert!(workspace.read(cx).is_vault_session_unlocked());

        window.click("toggle-lock-btn", cx);
        window.render_frame(cx);
        assert!(store.get_note(&seed.id).unwrap().locked);

        // Filter Vault section in Column 1 -> shows locked note
        window.click("nav-vault", cx);
        window.render_frame(cx);
        assert_eq!(workspace.read(cx).active_section(), SidebarSection::Vault);
        assert_eq!(workspace.read(cx).visible_note_count(), 1);

        // Lock Vault session -> Column 3 displays encrypted Vault gate
        window.click("vault-lock-session-btn", cx);
        window.render_frame(cx);
        assert!(window.find("vault-locked-gate").visible());

        // Unlock note in session from Column 3 gate
        window.click("unlock-note-password-input", cx);
        window.input("correct-horse-battery", cx);
        window.click("unlock-note-temp-btn", cx);
        window.render_frame(cx);
        assert!(workspace.read(cx).is_vault_session_unlocked());

        // 4. Move note to Trash & Restore from Trash
        window.click("trash-note-btn", cx);
        window.render_frame(cx);
        assert_eq!(workspace.read(cx).visible_note_count(), 0);

        window.click("nav-trash", cx);
        window.render_frame(cx);
        assert_eq!(workspace.read(cx).active_section(), SidebarSection::Trash);
        assert_eq!(workspace.read(cx).visible_note_count(), 1);

        window.click("restore-note-btn", cx);
        window.render_frame(cx);
        assert_eq!(workspace.read(cx).visible_note_count(), 0);
    })
    .unwrap();

    assert!(store.get_note(&seed.id).unwrap().trashed_at.is_none());
}

#[gpui_kit::test]
fn list_view_fts5_search_sort_and_editor_preview_tasks_wire_to_store(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let store = NotesStore::in_memory().expect("in-memory sqlite store");
    let z_note = store
        .create_note(CreateNoteInput {
            title: Some("Zeta Roadmap".to_owned()),
            content: Some("# Zeta\n\n- [ ] Ship GPUI client\nQuantum indexable keyword\n".to_owned()),
            ..Default::default()
        })
        .expect("zeta note");
    store
        .create_note(CreateNoteInput {
            title: Some("Alpha Journal".to_owned()),
            content: Some("# Alpha\n\nDaily personal reflections.\n".to_owned()),
            ..Default::default()
        })
        .expect("alpha note");

    let store_for_win = store.clone();
    let (handle, workspace) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
            cx.new(|cx| NotesWorkspace::new(store_for_win, None, window, cx))
        })
        .expect("open headless test window")
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(workspace.read(cx).visible_note_count(), 2);

        // 1. Sort by Title + toggle direction to Ascending -> Alpha first, Zeta second
        window.click("sort-title-btn", cx);
        window.click("sort-dir-btn", cx);
        window.render_frame(cx);
        assert_eq!(workspace.read(cx).group_options().sort_by, SortBy::Title);
        assert_eq!(
            workspace.read(cx).group_options().sort_direction,
            SortDirection::Asc
        );
        assert_eq!(
            workspace.read(cx).visible_note_titles(),
            vec!["Alpha Journal".to_owned(), "Zeta Roadmap".to_owned()]
        );

        // 2. Quick Search via SQLite FTS5 input filters live to "Quantum" -> only Zeta Roadmap!
        window.click("search-notes-input", cx);
        window.input("Quantum", cx);
        window.render_frame(cx);
        assert_eq!(window.find("search-notes-input").value(), Some("Quantum"));
        assert_eq!(workspace.read(cx).visible_note_count(), 1);
        assert_eq!(
            workspace.read(cx).selected_note_id(),
            Some(z_note.id.as_str())
        );

        // 3. Switch Column 3 to View (Preview) mode and click Markdown task checkbox!
        window.click("mode-view-btn", cx);
        window.render_frame(cx);
        assert_eq!(workspace.read(cx).editor_mode(), EditorMode::Preview);
        assert!(window.find("editor-preview-scroll").visible());

        window.click("preview-task-line-3", cx);
        window.render_frame(cx);
    })
    .unwrap();

    // Verify task toggle persisted to SQLite and SortOptions persisted in settings!
    let updated_z = store.get_note(&z_note.id).unwrap();
    let parsed = parse_tasks(&updated_z.content.unwrap().data);
    assert_eq!(parsed.len(), 1);
    assert!(parsed[0].checked);

    let saved_opts = store.get_view_group_options("notes").unwrap();
    assert_eq!(saved_opts.sort_by, SortBy::Title);
    assert_eq!(saved_opts.sort_direction, SortDirection::Asc);
}
