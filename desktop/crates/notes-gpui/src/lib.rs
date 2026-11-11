extern crate gpui_kit as gpui;

mod editor_view;
mod list_view;
mod sidebar;

use std::collections::HashSet;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::input::{EditorState, InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::*;
use notes_api_client::NotesApiClient;
use notes_core::{
    GroupBy, GroupOptions, GroupSection, NoteDetail, NoteSummary, NotebookDto, SortBy,
    SortDirection, Tag, toggle_task_at_line,
};
use notes_store::{
    CreateNoteInput, CreateNotebookInput, NoteListFilter, NotesStore, UpdateNoteInput,
};

gpui_kit::actions!(
    notes_gpui,
    [QuitApp, CreateNewNote, SaveActiveNote, ToggleEditorPreview]
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarSection {
    Notes,
    Favorites,
    Vault,
    Notebooks,
    Tags,
    Reminders,
    Archive,
    Trash,
    Monographs,
}

impl SidebarSection {
    pub const ALL: [Self; 9] = [
        Self::Notes,
        Self::Favorites,
        Self::Vault,
        Self::Notebooks,
        Self::Tags,
        Self::Reminders,
        Self::Archive,
        Self::Trash,
        Self::Monographs,
    ];

    pub const PRIMARY_NAV: [Self; 7] = [
        Self::Notes,
        Self::Favorites,
        Self::Vault,
        Self::Reminders,
        Self::Archive,
        Self::Monographs,
        Self::Trash,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Notes => "Notes",
            Self::Favorites => "Favorites",
            Self::Vault => "Vault",
            Self::Notebooks => "Notebooks",
            Self::Tags => "Tags",
            Self::Reminders => "Reminders",
            Self::Archive => "Archive",
            Self::Trash => "Trash",
            Self::Monographs => "Monographs",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Notes => "📝",
            Self::Favorites => "★",
            Self::Vault => "🔒",
            Self::Notebooks => "📓",
            Self::Tags => "#",
            Self::Reminders => "⏰",
            Self::Archive => "🗄",
            Self::Trash => "🗑",
            Self::Monographs => "🌐",
        }
    }

    pub fn element_id(self) -> &'static str {
        match self {
            Self::Notes => "nav-notes",
            Self::Favorites => "nav-favorites",
            Self::Vault => "nav-vault",
            Self::Notebooks => "nav-notebooks",
            Self::Tags => "nav-tags",
            Self::Reminders => "nav-reminders",
            Self::Archive => "nav-archive",
            Self::Trash => "nav-trash",
            Self::Monographs => "nav-monographs",
        }
    }

    pub fn view_key(self) -> &'static str {
        match self {
            Self::Notes => "notes",
            Self::Favorites => "favorites",
            Self::Vault => "vault",
            Self::Notebooks => "notebooks",
            Self::Tags => "tags",
            Self::Reminders => "reminders",
            Self::Archive => "archive",
            Self::Trash => "trash",
            Self::Monographs => "monographs",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorMode {
    #[default]
    Edit,
    Preview,
}

pub struct NotesWorkspace {
    pub(crate) store: NotesStore,
    pub(crate) api_client: Option<NotesApiClient>,
    pub(crate) active_section: SidebarSection,
    pub(crate) active_search_query: String,
    pub(crate) selected_notebook_id: Option<String>,
    pub(crate) selected_tag_id: Option<String>,
    pub(crate) group_options: GroupOptions,
    pub(crate) notes: Vec<NoteSummary>,
    pub(crate) sections: Vec<GroupSection>,
    pub(crate) notebooks: Vec<NotebookDto>,
    pub(crate) flat_notebooks: Vec<NotebookDto>,
    pub(crate) trashed_notebooks: Vec<NotebookDto>,
    pub(crate) tags: Vec<Tag>,
    pub(crate) vault_initialized: bool,
    pub(crate) session_vault_password: Option<String>,
    pub(crate) selected_note_id: Option<String>,
    pub(crate) editor_loaded_note_id: Option<String>,
    pub(crate) active_note: Option<NoteDetail>,
    pub(crate) active_note_unlocked: bool,
    pub(crate) editor_mode: EditorMode,
    pub(crate) is_dirty: bool,
    pub(crate) suppress_input_events: bool,
    pub(crate) search_input: Entity<InputState>,
    pub(crate) new_notebook_input: Entity<InputState>,
    pub(crate) new_tag_input: Entity<InputState>,
    pub(crate) vault_password_input: Entity<InputState>,
    pub(crate) unlock_note_input: Entity<InputState>,
    pub(crate) title_input: Entity<InputState>,
    pub(crate) note_tag_input: Entity<InputState>,
    pub(crate) body_editor: Entity<EditorState>,
    pub(crate) status_message: String,
    _subscriptions: Vec<Subscription>,
}

impl NotesWorkspace {
    pub fn new(
        store: NotesStore,
        api_client: Option<NotesApiClient>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Quick search notes (FTS5 / tag: / in:)...")
        });
        let new_notebook_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("New notebook..."));
        let new_tag_input = cx.new(|cx| InputState::new(window, cx).placeholder("New tag..."));
        let vault_password_input = cx
            .new(|cx| InputState::new(window, cx).masked(true).placeholder("Vault password"));
        let unlock_note_input = cx
            .new(|cx| InputState::new(window, cx).masked(true).placeholder("Enter Vault password"));
        let title_input = cx.new(|cx| InputState::new(window, cx).placeholder("Note title..."));
        let note_tag_input = cx.new(|cx| InputState::new(window, cx).placeholder("Add tag..."));
        let body_editor = cx.new(|cx| EditorState::new(window, cx).language("markdown"));

        let subscriptions = vec![
            cx.subscribe(
                &search_input,
                |this: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) && !this.suppress_input_events {
                        let _ = this.sync_store_snapshot(cx);
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(
                &search_input,
                window,
                |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) && !this.suppress_input_events {
                        this.reload_from_store(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &new_notebook_input,
                window,
                |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.create_notebook_from_input(None, window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &new_tag_input,
                window,
                |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.create_tag_from_input(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &vault_password_input,
                window,
                |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.submit_vault_password(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &unlock_note_input,
                window,
                |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. })
                        && let Some(nid) = this.selected_note_id.clone()
                    {
                        this.unlock_active_note_in_session(&nid, window, cx);
                    }
                },
            ),
            cx.subscribe(
                &title_input,
                |this: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) && !this.suppress_input_events {
                        this.is_dirty = true;
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(
                &title_input,
                window,
                |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) && !this.suppress_input_events
                    {
                        this.save_active_note(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &note_tag_input,
                window,
                |this: &mut Self, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.add_tag_to_active_note(window, cx);
                    }
                },
            ),
            cx.subscribe(
                &body_editor,
                |this: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) && !this.suppress_input_events {
                        this.is_dirty = true;
                        cx.notify();
                    }
                },
            ),
        ];

        let initial_group_options = store
            .get_view_group_options("notes")
            .unwrap_or_default();

        let mut ws = Self {
            store,
            api_client,
            active_section: SidebarSection::Notes,
            active_search_query: String::new(),
            selected_notebook_id: None,
            selected_tag_id: None,
            group_options: initial_group_options,
            notes: Vec::new(),
            sections: Vec::new(),
            notebooks: Vec::new(),
            flat_notebooks: Vec::new(),
            trashed_notebooks: Vec::new(),
            tags: Vec::new(),
            vault_initialized: false,
            session_vault_password: None,
            selected_note_id: None,
            editor_loaded_note_id: None,
            active_note: None,
            active_note_unlocked: true,
            editor_mode: EditorMode::Edit,
            is_dirty: false,
            suppress_input_events: false,
            search_input,
            new_notebook_input,
            new_tag_input,
            vault_password_input,
            unlock_note_input,
            title_input,
            note_tag_input,
            body_editor,
            status_message: "Local-first SQLite FTS5 ready".to_owned(),
            _subscriptions: subscriptions,
        };
        ws.reload_from_store(window, cx);
        ws
    }

    pub fn store(&self) -> &NotesStore {
        &self.store
    }

    pub fn api_client(&self) -> Option<&NotesApiClient> {
        self.api_client.as_ref()
    }

    pub fn active_section(&self) -> SidebarSection {
        self.active_section
    }

    pub fn selected_notebook_id(&self) -> Option<&str> {
        self.selected_notebook_id.as_deref()
    }

    pub fn selected_tag_id(&self) -> Option<&str> {
        self.selected_tag_id.as_deref()
    }

    pub fn group_options(&self) -> GroupOptions {
        self.group_options
    }

    pub fn editor_mode(&self) -> EditorMode {
        self.editor_mode
    }

    pub fn visible_note_count(&self) -> usize {
        self.notes.len()
    }

    pub fn visible_note_titles(&self) -> Vec<String> {
        self.notes.iter().map(|n| n.title.clone()).collect()
    }

    pub fn selected_note_id(&self) -> Option<&str> {
        self.selected_note_id.as_deref()
    }

    pub fn is_vault_session_unlocked(&self) -> bool {
        self.session_vault_password.is_some()
    }

    pub(crate) fn current_view_key(&self) -> String {
        match self.active_section {
            SidebarSection::Notebooks => match self.selected_notebook_id.as_deref() {
                Some(id) => format!("notebook:{id}"),
                None => "notebooks".to_owned(),
            },
            SidebarSection::Tags => match self.selected_tag_id.as_deref() {
                Some(id) => format!("tag:{id}"),
                None => "tags".to_owned(),
            },
            sec => sec.view_key().to_owned(),
        }
    }

    fn refresh_view_group_options(&mut self) {
        let key = self.current_view_key();
        if let Ok(opts) = self.store.get_view_group_options(&key) {
            self.group_options = opts;
        }
    }

    /// Refreshes notes, notebooks, tags, vault, and selection metadata from SQLite without
    /// requiring a mutable `Window` borrow. Returns `Some(next_id)` if the editor inputs
    /// should be reloaded for a newly selected note (or `Some(None)` if cleared).
    fn sync_store_snapshot(&mut self, cx: &mut Context<Self>) -> Option<Option<String>> {
        let query_raw = self.search_input.read(cx).value().to_string();
        self.active_search_query = query_raw.clone();
        let query = (!query_raw.trim().is_empty()).then_some(query_raw);
        let view_key = self.current_view_key();

        let filter = match self.active_section {
            SidebarSection::Notes | SidebarSection::Reminders | SidebarSection::Monographs => {
                NoteListFilter {
                    query,
                    group_options: Some(self.group_options),
                    view_key: Some(view_key),
                    ..Default::default()
                }
            }
            SidebarSection::Favorites => NoteListFilter {
                favorite: Some(true),
                query,
                group_options: Some(self.group_options),
                view_key: Some(view_key),
                ..Default::default()
            },
            SidebarSection::Vault => NoteListFilter {
                locked: Some(true),
                query,
                group_options: Some(self.group_options),
                view_key: Some(view_key),
                ..Default::default()
            },
            SidebarSection::Notebooks => NoteListFilter {
                notebook_id: self.selected_notebook_id.clone(),
                query,
                group_options: Some(self.group_options),
                view_key: Some(view_key),
                ..Default::default()
            },
            SidebarSection::Tags => NoteListFilter {
                tag_id: self.selected_tag_id.clone(),
                query,
                group_options: Some(self.group_options),
                view_key: Some(view_key),
                ..Default::default()
            },
            SidebarSection::Archive => NoteListFilter {
                archived: Some(true),
                query,
                group_options: Some(self.group_options),
                view_key: Some(view_key),
                ..Default::default()
            },
            SidebarSection::Trash => NoteListFilter {
                trashed: Some(true),
                query,
                group_options: Some(self.group_options),
                view_key: Some(view_key),
                ..Default::default()
            },
        };

        if let Ok((mut notes, mut sections)) = self.store.list_notes(filter) {
            if self.active_section == SidebarSection::Reminders {
                notes.retain(|n| n.has_reminders);
                let allowed: HashSet<&str> = notes.iter().map(|n| n.id.as_str()).collect();
                for sec in &mut sections {
                    sec.note_ids.retain(|id| allowed.contains(id.as_str()));
                }
                sections.retain(|s| !s.note_ids.is_empty());
            } else if self.active_section == SidebarSection::Monographs
                && let Ok(monos) = self.store.list_monographs()
            {
                let mono_note_ids: HashSet<String> =
                    monos.into_iter().map(|m| m.note_id).collect();
                notes.retain(|n| mono_note_ids.contains(&n.id));
                let allowed: HashSet<&str> = notes.iter().map(|n| n.id.as_str()).collect();
                for sec in &mut sections {
                    sec.note_ids.retain(|id| allowed.contains(id.as_str()));
                }
                sections.retain(|s| !s.note_ids.is_empty());
            }
            self.notes = notes;
            self.sections = sections;
        }

        if let Ok(tree_nbs) = self.store.list_notebooks(true) {
            self.notebooks = tree_nbs;
        }
        if let Ok(flat_nbs) = self.store.list_notebooks(false) {
            self.flat_notebooks = flat_nbs;
        }
        if let Ok(tags) = self.store.list_tags() {
            self.tags = tags;
        }
        if let Ok(vault_opt) = self.store.get_vault() {
            self.vault_initialized = vault_opt.is_some();
        }
        if self.active_section == SidebarSection::Trash {
            self.trashed_notebooks = self
                .store
                .get_trash()
                .map(|t| t.notebooks)
                .unwrap_or_default();
        } else {
            self.trashed_notebooks.clear();
        }

        let current_still_present = self
            .selected_note_id
            .as_ref()
            .is_some_and(|id| self.notes.iter().any(|n| &n.id == id));

        if current_still_present {
            if let Some(ref id) = self.selected_note_id
                && let Ok(detail) = self.store.get_note(id)
            {
                self.active_note = Some(detail);
            }
            None
        } else {
            let next_id = self.notes.first().map(|n| n.id.clone());
            self.selected_note_id = next_id.clone();
            self.active_note = next_id
                .as_deref()
                .and_then(|id| self.store.get_note(id).ok());
            Some(next_id)
        }
    }

    pub fn reload_from_store(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.sync_store_snapshot(cx);
        if self.editor_loaded_note_id != self.selected_note_id {
            let next_id = self.selected_note_id.clone();
            self.load_note_into_editor(next_id.as_deref(), window, cx);
        }
        cx.notify();
    }

    // --- Column 1 (Sidebar) Handlers ---

    pub fn select_section(
        &mut self,
        section: SidebarSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flush_autosave_if_needed(cx);
        self.active_section = section;
        if section != SidebarSection::Notebooks {
            self.selected_notebook_id = None;
        }
        if section != SidebarSection::Tags {
            self.selected_tag_id = None;
        }
        self.refresh_view_group_options();
        self.reload_from_store(window, cx);
    }

    pub fn select_notebook(
        &mut self,
        notebook_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.flush_autosave_if_needed(cx);
        self.active_section = SidebarSection::Notebooks;
        self.selected_notebook_id = Some(notebook_id.to_owned());
        self.selected_tag_id = None;
        self.refresh_view_group_options();
        self.reload_from_store(window, cx);
    }

    pub fn select_tag(&mut self, tag_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_autosave_if_needed(cx);
        self.active_section = SidebarSection::Tags;
        self.selected_tag_id = Some(tag_id.to_owned());
        self.selected_notebook_id = None;
        self.refresh_view_group_options();
        self.reload_from_store(window, cx);
    }

    pub fn create_notebook_from_input(
        &mut self,
        parent_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let raw_title = self.new_notebook_input.read(cx).value().to_string();
        if raw_title.trim().is_empty() {
            self.status_message = "Notebook title cannot be empty".to_owned();
            cx.notify();
            return;
        }

        match self.store.create_notebook(CreateNotebookInput {
            title: raw_title,
            parent_id,
            ..Default::default()
        }) {
            Ok(nb) => {
                self.suppress_input_events = true;
                self.new_notebook_input
                    .update(cx, |s, cx| s.set_value("", window, cx));
                self.suppress_input_events = false;
                self.active_section = SidebarSection::Notebooks;
                self.selected_notebook_id = Some(nb.id.clone());
                self.status_message = format!("Created notebook '{}'", nb.title);
                self.refresh_view_group_options();
                self.reload_from_store(window, cx);
            }
            Err(err) => {
                self.status_message = format!("Notebook error: {err}");
                cx.notify();
            }
        }
    }

    pub fn trash_notebook_by_id(
        &mut self,
        notebook_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.store.trash_notebook(notebook_id) {
            Ok(nb) => {
                if self.selected_notebook_id.as_deref() == Some(notebook_id) {
                    self.selected_notebook_id = None;
                }
                self.status_message = format!("Moved notebook '{}' to Trash", nb.title);
                self.reload_from_store(window, cx);
            }
            Err(err) => {
                self.status_message = format!("Failed to trash notebook: {err}");
                cx.notify();
            }
        }
    }

    pub fn create_tag_from_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw_title = self.new_tag_input.read(cx).value().to_string();
        let clean = raw_title.trim().trim_start_matches('#');
        if clean.is_empty() {
            self.status_message = "Tag name cannot be empty".to_owned();
            cx.notify();
            return;
        }

        match self.store.create_tag(None, clean) {
            Ok(tag) => {
                self.suppress_input_events = true;
                self.new_tag_input
                    .update(cx, |s, cx| s.set_value("", window, cx));
                self.suppress_input_events = false;
                self.active_section = SidebarSection::Tags;
                self.selected_tag_id = Some(tag.id.clone());
                self.status_message = format!("Created tag #{}", tag.title);
                self.refresh_view_group_options();
                self.reload_from_store(window, cx);
            }
            Err(err) => {
                self.status_message = format!("Tag error: {err}");
                cx.notify();
            }
        }
    }

    pub fn delete_tag_by_id(&mut self, tag_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match self.store.delete_tag(tag_id) {
            Ok(()) => {
                if self.selected_tag_id.as_deref() == Some(tag_id) {
                    self.selected_tag_id = None;
                }
                self.status_message = "Tag deleted".to_owned();
                self.reload_from_store(window, cx);
            }
            Err(err) => {
                self.status_message = format!("Failed to delete tag: {err}");
                cx.notify();
            }
        }
    }

    pub fn submit_vault_password(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pw = self.vault_password_input.read(cx).value().to_string();
        if pw.is_empty() {
            self.status_message = "Enter a Vault password first".to_owned();
            cx.notify();
            return;
        }

        let outcome = if self.vault_initialized {
            self.store.verify_vault(&pw)
        } else {
            self.store.init_vault(&pw, None).map(|_| ())
        };

        match outcome {
            Ok(()) => {
                self.vault_initialized = true;
                self.session_vault_password = Some(pw);
                self.suppress_input_events = true;
                self.vault_password_input
                    .update(cx, |s, cx| s.set_value("", window, cx));
                self.suppress_input_events = false;
                if let Some(id) = self.selected_note_id.clone() {
                    self.load_note_into_editor(Some(&id), window, cx);
                }
                self.status_message = "Personal Vault unlocked for this session".to_owned();
                self.reload_from_store(window, cx);
            }
            Err(err) => {
                self.status_message = format!("Vault authentication failed: {err}");
                cx.notify();
            }
        }
    }

    pub fn lock_vault_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_autosave_if_needed(cx);
        self.session_vault_password = None;
        if let Some(id) = self.selected_note_id.clone() {
            self.load_note_into_editor(Some(&id), window, cx);
        }
        self.status_message = "Vault session locked".to_owned();
        cx.notify();
    }

    // --- Column 2 (List View: Search, Sort & Trash Recovery) Handlers ---

    pub fn set_sort_by(&mut self, sort_by: SortBy, window: &mut Window, cx: &mut Context<Self>) {
        self.group_options.sort_by = sort_by;
        let key = self.current_view_key();
        let _ = self.store.set_view_group_options(&key, self.group_options);
        self.reload_from_store(window, cx);
    }

    pub fn toggle_sort_direction(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.group_options.sort_direction = match self.group_options.sort_direction {
            SortDirection::Desc => SortDirection::Asc,
            SortDirection::Asc => SortDirection::Desc,
        };
        let key = self.current_view_key();
        let _ = self.store.set_view_group_options(&key, self.group_options);
        self.reload_from_store(window, cx);
    }

    pub fn cycle_group_by(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.group_options.group_by = match self.group_options.group_by {
            GroupBy::Default => GroupBy::Month,
            GroupBy::Month => GroupBy::Abc,
            GroupBy::Abc => GroupBy::None,
            GroupBy::Year | GroupBy::Week | GroupBy::None => GroupBy::Default,
        };
        let key = self.current_view_key();
        let _ = self.store.set_view_group_options(&key, self.group_options);
        self.reload_from_store(window, cx);
    }

    pub fn select_note(&mut self, note_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_note_id.as_deref() != Some(note_id) {
            self.flush_autosave_if_needed(cx);
        }
        self.load_note_into_editor(Some(note_id), window, cx);
        cx.notify();
    }

    pub fn empty_all_trash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let vault_ok = self.session_vault_password.is_some();
        match self.store.empty_trash(vault_ok) {
            Ok(purged) => {
                self.status_message = format!("Emptied Trash ({purged} items purged)");
                self.reload_from_store(window, cx);
            }
            Err(err) => {
                self.status_message = format!("Empty Trash requires unlocked Vault: {err}");
                cx.notify();
            }
        }
    }

    pub fn restore_trashed_notebook(
        &mut self,
        notebook_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Ok(nb) = self.store.restore_notebook(notebook_id) {
            self.status_message = format!("Restored notebook '{}'", nb.title);
            self.reload_from_store(window, cx);
        }
    }

    pub fn purge_trashed_notebook(
        &mut self,
        notebook_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.store.delete_notebook(notebook_id, true).is_ok() {
            self.status_message = "Permanently deleted notebook".to_owned();
            self.reload_from_store(window, cx);
        }
    }

    // --- Column 3 (Editor & Preview View) Handlers ---

    pub fn set_editor_mode(&mut self, mode: EditorMode, cx: &mut Context<Self>) {
        self.editor_mode = mode;
        cx.notify();
    }

    pub fn toggle_editor_mode(&mut self, cx: &mut Context<Self>) {
        self.editor_mode = match self.editor_mode {
            EditorMode::Edit => EditorMode::Preview,
            EditorMode::Preview => EditorMode::Edit,
        };
        cx.notify();
    }

    pub fn create_new_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_autosave_if_needed(cx);

        let mut notebook_ids = Vec::new();
        let mut tags = Vec::new();
        let mut favorite = false;
        let lock_in_vault =
            self.active_section == SidebarSection::Vault && self.session_vault_password.is_some();

        match self.active_section {
            SidebarSection::Notebooks => {
                if let Some(ref nb_id) = self.selected_notebook_id {
                    notebook_ids.push(nb_id.clone());
                }
            }
            SidebarSection::Tags => {
                if let Some(ref tid) = self.selected_tag_id
                    && let Some(tag) = self.tags.iter().find(|t| &t.id == tid)
                {
                    tags.push(tag.title.clone());
                }
            }
            SidebarSection::Favorites => {
                favorite = true;
            }
            SidebarSection::Vault if lock_in_vault => {}
            _ => {
                self.active_section = SidebarSection::Notes;
                self.selected_notebook_id = None;
                self.selected_tag_id = None;
                self.refresh_view_group_options();
            }
        }

        let created = self.store.create_note(CreateNoteInput {
            title: Some("Untitled Note".to_owned()),
            content: Some("# New Note\n\nStart writing in Markdown...\n".to_owned()),
            favorite,
            notebook_ids,
            tags,
            ..Default::default()
        });

        if let Ok(mut detail) = created {
            if lock_in_vault
                && let Some(ref pw) = self.session_vault_password
                && let Ok(locked_detail) = self.store.lock_note(&detail.id, pw)
            {
                detail = locked_detail;
            }
            self.selected_note_id = Some(detail.id.clone());
            self.reload_from_store(window, cx);
            self.load_note_into_editor(Some(&detail.id), window, cx);
            self.status_message = format!("Created '{}'", detail.title);
            cx.notify();
        }
    }

    pub fn save_active_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note_id) = self.selected_note_id.clone() else {
            return;
        };
        let Some(ref cur) = self.active_note else {
            return;
        };
        if cur.trashed_at.is_some() || (cur.locked && !self.active_note_unlocked) {
            return;
        }

        let new_title = self.title_input.read(cx).value().to_string();
        let new_body = self.body_editor.read(cx).value().to_string();

        match self.store.update_note(
            &note_id,
            UpdateNoteInput {
                title: Some(new_title),
                content: Some(new_body),
                vault_password: self.session_vault_password.clone(),
                ..Default::default()
            },
        ) {
            Ok(updated) => {
                self.is_dirty = false;
                self.status_message = format!("Saved '{}'", updated.title);
                self.active_note = Some(updated);
                self.reload_from_store(window, cx);
            }
            Err(err) => {
                self.status_message = format!("Save error: {err}");
                cx.notify();
            }
        }
    }

    fn flush_autosave_if_needed(&mut self, cx: &mut Context<Self>) {
        if !self.is_dirty {
            return;
        }
        let Some(note_id) = self.selected_note_id.clone() else {
            return;
        };
        let Some(ref cur) = self.active_note else {
            return;
        };
        if cur.readonly || cur.trashed_at.is_some() || (cur.locked && !self.active_note_unlocked) {
            return;
        }

        let new_title = self.title_input.read(cx).value().to_string();
        let new_body = self.body_editor.read(cx).value().to_string();
        if new_title.trim().is_empty() && new_body.trim().is_empty() {
            return;
        }

        if let Ok(updated) = self.store.update_note(
            &note_id,
            UpdateNoteInput {
                title: Some(new_title),
                content: Some(new_body),
                vault_password: self.session_vault_password.clone(),
                ..Default::default()
            },
        ) {
            self.is_dirty = false;
            self.active_note = Some(updated);
        }
    }

    pub fn toggle_active_note_pinned(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ref cur) = self.active_note else {
            return;
        };
        let nid = cur.id.clone();
        let next = !cur.pinned;
        if let Ok(updated) = self.store.update_note(
            &nid,
            UpdateNoteInput {
                pinned: Some(next),
                ..Default::default()
            },
        ) {
            self.active_note = Some(updated);
            self.status_message = if next {
                "Note pinned".to_owned()
            } else {
                "Note unpinned".to_owned()
            };
            self.reload_from_store(window, cx);
        }
    }

    pub fn toggle_active_note_favorite(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ref cur) = self.active_note else {
            return;
        };
        let nid = cur.id.clone();
        let next = !cur.favorite;
        if let Ok(updated) = self.store.update_note(
            &nid,
            UpdateNoteInput {
                favorite: Some(next),
                ..Default::default()
            },
        ) {
            self.active_note = Some(updated);
            self.status_message = if next {
                "Added to Favorites".to_owned()
            } else {
                "Removed from Favorites".to_owned()
            };
            self.reload_from_store(window, cx);
        }
    }

    pub fn toggle_active_note_archived(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ref cur) = self.active_note else {
            return;
        };
        let nid = cur.id.clone();
        let next = !cur.archived;
        if let Ok(updated) = self.store.update_note(
            &nid,
            UpdateNoteInput {
                archived: Some(next),
                ..Default::default()
            },
        ) {
            self.active_note = Some(updated);
            self.status_message = if next {
                "Note archived".to_owned()
            } else {
                "Note restored from Archive".to_owned()
            };
            self.reload_from_store(window, cx);
        }
    }

    pub fn toggle_active_note_vault_lock(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ref cur) = self.active_note.clone() else {
            return;
        };

        if cur.locked {
            let Some(pw) = self.session_vault_password.clone() else {
                self.status_message =
                    "Unlock the note or Personal Vault first to remove encryption".to_owned();
                cx.notify();
                return;
            };
            match self.store.unlock_note_permanent(&cur.id, &pw) {
                Ok(detail) => {
                    self.status_message = format!("Removed Vault lock from '{}'", detail.title);
                    self.load_note_into_editor(Some(&detail.id), window, cx);
                    self.reload_from_store(window, cx);
                }
                Err(err) => {
                    self.status_message = format!("Vault unlock error: {err}");
                    cx.notify();
                }
            }
        } else {
            self.flush_autosave_if_needed(cx);
            let pw_opt = self.session_vault_password.clone().or_else(|| {
                let typed = self.vault_password_input.read(cx).value().to_string();
                (!typed.is_empty()).then_some(typed)
            });
            let Some(pw) = pw_opt else {
                self.status_message =
                    "Initialize or unlock Personal Vault in the Sidebar first".to_owned();
                cx.notify();
                return;
            };

            if !self.vault_initialized {
                if let Err(err) = self.store.init_vault(&pw, None) {
                    self.status_message = format!("Vault init error: {err}");
                    cx.notify();
                    return;
                }
                self.vault_initialized = true;
            }

            match self.store.lock_note(&cur.id, &pw) {
                Ok(detail) => {
                    self.session_vault_password = Some(pw);
                    self.suppress_input_events = true;
                    self.vault_password_input
                        .update(cx, |s, cx| s.set_value("", window, cx));
                    self.suppress_input_events = false;
                    self.status_message = format!("Encrypted '{}' in Personal Vault", detail.title);
                    self.load_note_into_editor(Some(&detail.id), window, cx);
                    self.reload_from_store(window, cx);
                }
                Err(err) => {
                    self.status_message = format!("Failed to lock note: {err}");
                    cx.notify();
                }
            }
        }
    }

    pub fn unlock_active_note_in_session(
        &mut self,
        note_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pw = self.unlock_note_input.read(cx).value().to_string();
        if pw.is_empty() {
            self.status_message = "Enter Vault password to unlock".to_owned();
            cx.notify();
            return;
        }

        match self.store.unlock_note_temporary(note_id, &pw) {
            Ok(plain) => {
                self.session_vault_password = Some(pw);
                self.active_note_unlocked = true;
                self.suppress_input_events = true;
                self.unlock_note_input
                    .update(cx, |s, cx| s.set_value("", window, cx));
                self.body_editor
                    .update(cx, |s, cx| s.set_value(plain.to_string(), window, cx));
                self.suppress_input_events = false;
                self.is_dirty = false;
                self.status_message = "Decrypted note in session".to_owned();
                cx.notify();
            }
            Err(err) => {
                self.status_message = format!("Wrong Vault password: {err}");
                cx.notify();
            }
        }
    }

    pub fn unlock_active_note_permanently(
        &mut self,
        note_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pw = self.unlock_note_input.read(cx).value().to_string();
        if pw.is_empty() {
            self.status_message = "Enter Vault password to decrypt permanently".to_owned();
            cx.notify();
            return;
        }

        match self.store.unlock_note_permanent(note_id, &pw) {
            Ok(detail) => {
                self.session_vault_password = Some(pw);
                self.suppress_input_events = true;
                self.unlock_note_input
                    .update(cx, |s, cx| s.set_value("", window, cx));
                self.suppress_input_events = false;
                self.status_message = format!("Decrypted '{}' permanently", detail.title);
                self.load_note_into_editor(Some(&detail.id), window, cx);
                self.reload_from_store(window, cx);
            }
            Err(err) => {
                self.status_message = format!("Decryption failed: {err}");
                cx.notify();
            }
        }
    }

    pub fn cycle_active_note_notebook(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.flat_notebooks.is_empty() {
            self.status_message = "Create a notebook in the Sidebar first".to_owned();
            cx.notify();
            return;
        }
        let Some(ref cur) = self.active_note else {
            return;
        };
        let nid = cur.id.clone();

        let next_nb_ids = match cur.notebook_ids.first() {
            None => vec![self.flat_notebooks[0].id.clone()],
            Some(current_id) => {
                let pos = self
                    .flat_notebooks
                    .iter()
                    .position(|n| &n.id == current_id);
                match pos {
                    Some(idx) if idx + 1 < self.flat_notebooks.len() => {
                        vec![self.flat_notebooks[idx + 1].id.clone()]
                    }
                    _ => Vec::new(),
                }
            }
        };

        if let Ok(updated) = self.store.update_note(
            &nid,
            UpdateNoteInput {
                notebook_ids: Some(next_nb_ids),
                ..Default::default()
            },
        ) {
            let msg = match updated.notebook_ids.first() {
                Some(nb_id) => {
                    let nb_name = self
                        .flat_notebooks
                        .iter()
                        .find(|n| &n.id == nb_id)
                        .map(|n| n.title.as_str())
                        .unwrap_or("Notebook");
                    format!("Assigned to '{nb_name}'")
                }
                None => "Removed notebook assignment".to_owned(),
            };
            self.active_note = Some(updated);
            self.status_message = msg;
            self.reload_from_store(window, cx);
        }
    }

    pub fn add_tag_to_active_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw_tag = self.note_tag_input.read(cx).value().to_string();
        let clean = raw_tag.trim().trim_start_matches('#').to_owned();
        if clean.is_empty() {
            return;
        }
        let Some(ref cur) = self.active_note else {
            return;
        };
        let nid = cur.id.clone();
        let mut tag_titles: Vec<String> = cur.tags.iter().map(|t| t.title.clone()).collect();
        if !tag_titles.iter().any(|t| t == &clean) {
            tag_titles.push(clean.clone());
        }

        if let Ok(updated) = self.store.update_note(
            &nid,
            UpdateNoteInput {
                tags: Some(tag_titles),
                ..Default::default()
            },
        ) {
            self.suppress_input_events = true;
            self.note_tag_input
                .update(cx, |s, cx| s.set_value("", window, cx));
            self.suppress_input_events = false;
            self.active_note = Some(updated);
            self.status_message = format!("Tagged #{clean}");
            self.reload_from_store(window, cx);
        }
    }

    pub fn remove_tag_from_active_note(
        &mut self,
        tag_title: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(ref cur) = self.active_note else {
            return;
        };
        let nid = cur.id.clone();
        let next_tags: Vec<String> = cur
            .tags
            .iter()
            .filter(|t| t.title != tag_title)
            .map(|t| t.title.clone())
            .collect();

        if let Ok(updated) = self.store.update_note(
            &nid,
            UpdateNoteInput {
                tags: Some(next_tags),
                ..Default::default()
            },
        ) {
            self.active_note = Some(updated);
            self.status_message = format!("Removed #{tag_title}");
            self.reload_from_store(window, cx);
        }
    }

    pub fn toggle_markdown_task_line(
        &mut self,
        line_1_based: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(note_id) = self.selected_note_id.clone() else {
            return;
        };
        let current_body = self.body_editor.read(cx).value().to_string();
        let Ok(toggled_body) = toggle_task_at_line(&current_body, line_1_based) else {
            return;
        };

        self.suppress_input_events = true;
        self.body_editor
            .update(cx, |s, cx| s.set_value(toggled_body.clone(), window, cx));
        self.suppress_input_events = false;

        if let Ok(updated) = self.store.update_note(
            &note_id,
            UpdateNoteInput {
                content: Some(toggled_body),
                vault_password: self.session_vault_password.clone(),
                ..Default::default()
            },
        ) {
            self.is_dirty = false;
            self.active_note = Some(updated);
            self.status_message = "Updated task item".to_owned();
            self.reload_from_store(window, cx);
        }
    }

    pub fn trash_active_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note_id) = self.selected_note_id.clone() else {
            return;
        };
        self.is_dirty = false;
        if let Ok(trashed) = self.store.trash_note(&note_id) {
            self.status_message = format!("Moved '{}' to Trash", trashed.title);
            self.reload_from_store(window, cx);
        }
    }

    pub fn restore_active_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note_id) = self.selected_note_id.clone() else {
            return;
        };
        if let Ok(restored) = self.store.restore_note(&note_id) {
            self.status_message = format!("Restored '{}'", restored.title);
            self.reload_from_store(window, cx);
        }
    }

    pub fn purge_active_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(note_id) = self.selected_note_id.clone() else {
            return;
        };
        let vault_ok = self.session_vault_password.is_some();
        match self.store.delete_note(&note_id, true, vault_ok) {
            Ok(_) => {
                self.is_dirty = false;
                self.status_message = "Permanently deleted note".to_owned();
                self.reload_from_store(window, cx);
            }
            Err(err) => {
                self.status_message = format!("Cannot purge locked note without Vault unlock: {err}");
                cx.notify();
            }
        }
    }

    pub(crate) fn load_note_into_editor(
        &mut self,
        note_id: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.suppress_input_events = true;

        let Some(id) = note_id else {
            self.selected_note_id = None;
            self.editor_loaded_note_id = None;
            self.active_note = None;
            self.active_note_unlocked = true;
            self.is_dirty = false;
            self.title_input
                .update(cx, |s, cx| s.set_value("", window, cx));
            self.body_editor
                .update(cx, |s, cx| s.set_value("", window, cx));
            self.suppress_input_events = false;
            return;
        };

        if let Ok(detail) = self.store.get_note(id) {
            let title_val = detail.title.clone();
            let (unlocked, body_val) = if detail.locked {
                if let Some(ref pw) = self.session_vault_password
                    && let Ok(plain) = self.store.unlock_note_temporary(&detail.id, pw)
                {
                    (true, plain.to_string())
                } else {
                    (false, String::new())
                }
            } else {
                let raw = detail
                    .content
                    .as_ref()
                    .map(|c| c.data.clone())
                    .unwrap_or_default();
                (true, raw)
            };

            self.selected_note_id = Some(detail.id.clone());
            self.editor_loaded_note_id = Some(detail.id.clone());
            self.active_note = Some(detail);
            self.active_note_unlocked = unlocked;
            self.is_dirty = false;

            self.title_input
                .update(cx, |s, cx| s.set_value(title_val, window, cx));
            self.body_editor
                .update(cx, |s, cx| s.set_value(body_val, window, cx));
        }

        self.suppress_input_events = false;
    }
}

impl Render for NotesWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.search_input.read(cx).value().as_str() != self.active_search_query {
            let _ = self.sync_store_snapshot(cx);
        }
        if self.editor_loaded_note_id != self.selected_note_id {
            let next_id = self.selected_note_id.clone();
            self.load_note_into_editor(next_id.as_deref(), window, cx);
        }
        let theme = cx.theme();
        div()
            .id("notes-workspace-root")
            .test_support()
            .size_full()
            .flex()
            .flex_row()
            .bg(theme.background)
            .text_color(theme.foreground)
            .on_action(cx.listener(|_, _: &QuitApp, _, cx| cx.quit()))
            .on_action(cx.listener(|this, _: &CreateNewNote, window, cx| {
                this.create_new_note(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SaveActiveNote, window, cx| {
                this.save_active_note(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleEditorPreview, _, cx| {
                this.toggle_editor_mode(cx);
            }))
            .child(self.render_sidebar(cx))
            .child(self.render_note_list(cx))
            .child(self.render_editor_pane(cx))
    }
}
