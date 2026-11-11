use chrono::{Duration, Utc};
use notes_core::{
    ConflictResolution, ContentFormat, DeletedBy, GroupBy, GroupOptions, Note, NoteContent,
    RecurringMode, ReminderMode, ReminderPriority, SortBy, SortDirection, SyncChangeset,
};
use notes_store::*;
use tempfile::TempDir;

#[test]
fn wal_fk_fts5_bootstrap_and_7_default_colors() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("notes.db");
    let store = NotesStore::open(&db_path).expect("open store");

    assert_eq!(store.journal_mode().unwrap().to_lowercase(), "wal");
    assert!(store.foreign_keys_enabled().unwrap());

    let colors = store.list_colors().unwrap();
    assert_eq!(colors.len(), 7);
    assert_eq!(colors[0].title, "red");
    assert_eq!(colors[0].color_code, "#f44336");

    // Seeded colors start synced = 1 so they don't pollute initial sync push
    let (unsynced, _) = store.collect_unsynced_changes().unwrap();
    assert!(unsynced.colors.is_empty());
}

#[test]
fn notebook_hierarchy_cycles_breadcrumbs_distinct_counts_and_cascading_trash() {
    let store = NotesStore::in_memory().unwrap();

    let root = store
        .create_notebook(CreateNotebookInput {
            title: "Engineering".to_owned(),
            ..Default::default()
        })
        .unwrap();
    let child = store
        .create_notebook(CreateNotebookInput {
            title: "Rust".to_owned(),
            parent_id: Some(root.id.clone()),
            ..Default::default()
        })
        .unwrap();
    let grandchild = store
        .create_notebook(CreateNotebookInput {
            title: "GPUI".to_owned(),
            parent_id: Some(child.id.clone()),
            ..Default::default()
        })
        .unwrap();

    // Reject self-parent and ancestor cycle
    assert!(matches!(
        store.update_notebook(
            &root.id,
            UpdateNotebookInput {
                parent_id: Some(Some(root.id.clone())),
                ..Default::default()
            }
        ),
        Err(StoreError::CycleDetected(_))
    ));
    assert!(matches!(
        store.update_notebook(
            &root.id,
            UpdateNotebookInput {
                parent_id: Some(Some(grandchild.id.clone())),
                ..Default::default()
            }
        ),
        Err(StoreError::CycleDetected(_))
    ));

    // Breadcrumbs ordered Root -> Child -> Grandchild
    let crumbs = store.get_notebook_breadcrumbs(&grandchild.id).unwrap();
    let titles: Vec<&str> = crumbs.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, vec!["Engineering", "Rust", "GPUI"]);

    // Assign a single note to BOTH root and grandchild -> COUNT(DISTINCT) must be 1, not 2!
    let note = store
        .create_note(CreateNoteInput {
            title: Some("Shared Note".to_owned()),
            content: Some("Body".to_owned()),
            notebook_ids: vec![root.id.clone(), grandchild.id.clone()],
            ..Default::default()
        })
        .unwrap();

    let tree = store.list_notebooks(true).unwrap();
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].total_notes, 1);
    assert_eq!(tree[0].children[0].total_notes, 1);

    // Trash `child` explicitly (`user`), then trash `root` (`user` -> cascades `app` to `grandchild`? Wait: grandchild is under child which is already `user`!)
    // Let's test trashing `root` directly so `child` and `grandchild` get `deleted_by = 'app'`!
    store.trash_notebook(&root.id).unwrap();

    // Member note remains active in global Notes!
    let (active_notes, _) = store.list_notes(NoteListFilter::default()).unwrap();
    assert_eq!(active_notes.len(), 1);
    assert_eq!(active_notes[0].id, note.id);

    // Trashed notebook cannot be edited
    assert!(matches!(
        store.update_notebook(
            &root.id,
            UpdateNotebookInput {
                title: Some("Nope".to_owned()),
                ..Default::default()
            }
        ),
        Err(StoreError::NotebookInTrash(_))
    ));

    // Trash view shows only top-level `user` notebook with `app` children nested
    let trash = store.get_trash().unwrap();
    assert_eq!(trash.notebooks.len(), 1);
    assert_eq!(trash.notebooks[0].id, root.id);
    assert_eq!(trash.notebooks[0].deleted_by, Some(DeletedBy::User));
    assert_eq!(trash.notebooks[0].children.len(), 1);
    assert_eq!(
        trash.notebooks[0].children[0].deleted_by,
        Some(DeletedBy::App)
    );

    // Restore `root` -> cascades restore to `child` and `grandchild`!
    store.restore_notebook(&root.id).unwrap();
    let restored_flat = store.list_notebooks(false).unwrap();
    assert_eq!(restored_flat.len(), 3);
}

#[test]
fn search_fts5_15_operators_diacritics_short_tokens_xss_and_zero_locked_leakage() {
    let store = NotesStore::in_memory().unwrap();
    let colors = store.list_colors().unwrap();
    let red_id = colors[0].id.clone();

    let nb = store
        .create_notebook(CreateNotebookInput {
            title: "Research".to_owned(),
            ..Default::default()
        })
        .unwrap();

    // Case-sensitive tags: "Work" vs "work"
    store.create_tag(None, "Work").unwrap();
    store.create_tag(None, "work").unwrap();
    assert!(matches!(
        store.create_tag(None, "Work"),
        Err(StoreError::DuplicateTag(_))
    ));

    let n_open = store
        .create_note(CreateNoteInput {
            title: Some("Café <img src=x onerror=1> UI".to_owned()),
            content: Some("Reunião sobre C# e orçamento anual.".to_owned()),
            pinned: true,
            favorite: true,
            color_id: Some(red_id.clone()),
            notebook_ids: vec![nb.id.clone()],
            tags: vec!["Work".to_owned()],
            ..Default::default()
        })
        .unwrap();

    // Create vault and lock a second note
    store.init_vault("vault-pass-123", Some(300)).unwrap();
    let n_secret = store
        .create_note(CreateNoteInput {
            title: Some("Encrypted Diary".to_owned()),
            content: Some("ULTRASECRET_TOKEN_XYZ e Café".to_owned()),
            ..Default::default()
        })
        .unwrap();
    // Record a plaintext history session BEFORE locking, then lock and verify history is purged!
    store
        .record_history_session(&n_secret.id, Some("s1"), None)
        .unwrap();
    assert!(!store.list_history_sessions(&n_secret.id).unwrap().is_empty());

    let locked_detail = store.lock_note(&n_secret.id, "vault-pass-123").unwrap();
    assert!(locked_detail.locked);
    assert_eq!(locked_detail.headline, "");
    assert!(store.list_history_sessions(&n_secret.id).unwrap().is_empty());

    // Locked note MUST NEVER match plaintext or CipherEnvelope substrings!
    assert!(
        store
            .search_notes("ULTRASECRET_TOKEN_XYZ", SearchScope::Default)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .search_notes("xchacha20poly1305", SearchScope::Default)
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .search_notes("content:Diary", SearchScope::Default)
            .unwrap()
            .is_empty()
    );

    // Locked note matches title + `locked:true`, with EMPTY snippet
    let locked_hits = store
        .search_notes("Diary locked:true", SearchScope::Default)
        .unwrap();
    assert_eq!(locked_hits.len(), 1);
    assert_eq!(locked_hits[0].note.id, n_secret.id);
    assert_eq!(locked_hits[0].highlighted_snippet, "");

    // Diacritic-insensitive + XSS-safe highlight + <3-char ("UI", "C#") + 15-operator query
    let hits = store
        .search_notes(
            r#"cafe "C#" UI tag:Work color:red pinned:true favorite:true locked:false readonly:false archived:false tagged:true colored:true in_notebook:Research"#,
            SearchScope::Default,
        )
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].note.id, n_open.id);
    assert!(hits[0].highlighted_title.contains("<mark>Café</mark>"));
    assert!(hits[0].highlighted_title.contains("&lt;img src=x onerror=1&gt;"));
    assert!(!hits[0].highlighted_title.contains("<img"));

    // Case-sensitive tag distinction
    assert!(
        store
            .search_notes("tag:work", SearchScope::Default)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn vault_atomic_rotation_rollback_attachment_409_guard_and_history_restore() {
    let store = NotesStore::in_memory().unwrap();
    store.init_vault("old-pw", Some(300)).unwrap();

    let n1 = store
        .create_note(CreateNoteInput {
            title: Some("Note 1".to_owned()),
            content: Some("Secret Alpha".to_owned()),
            ..Default::default()
        })
        .unwrap();
    let n2 = store
        .create_note(CreateNoteInput {
            title: Some("Note 2".to_owned()),
            content: Some("Secret Beta".to_owned()),
            ..Default::default()
        })
        .unwrap();

    // Attach binary file to n1 BEFORE locking, then lock n1 -> deleting attachment must return AttachmentInLockedNote!
    let att = store
        .upsert_attachment(UpsertAttachmentInput {
            id: None,
            filename: "doc.pdf".to_owned(),
            mime_type: "application/pdf".to_owned(),
            bytes: b"%PDF-1.7 sample bytes",
            explicit_hash: None,
            note_id: Some(n1.id.clone()),
            encrypted: false,
            encryption_meta: None,
        })
        .unwrap();

    store.lock_note(&n1.id, "old-pw").unwrap();
    store.lock_note(&n2.id, "old-pw").unwrap();

    assert!(matches!(
        store.delete_attachment_by_hash(&att.hash),
        Err(StoreError::AttachmentInLockedNote)
    ));
    assert_eq!(
        StoreError::AttachmentInLockedNote.to_string(),
        "This attachment is inside a locked note."
    );

    // Valid atomic rotation succeeds
    store.rotate_vault_atomic("old-pw", "new-pw").unwrap();
    assert_eq!(
        store.unlock_note_temporary(&n1.id, "new-pw").unwrap().as_str(),
        "Secret Alpha"
    );
    assert_eq!(
        store.unlock_note_temporary(&n2.id, "new-pw").unwrap().as_str(),
        "Secret Beta"
    );

    // Permanently unlock n1 -> attachment deletion is now permitted!
    store.unlock_note_permanent(&n1.id, "new-pw").unwrap();
    store.delete_attachment_by_hash(&att.hash).unwrap();
    assert!(store.list_attachments().unwrap().is_empty());

    // Version history pre-restore snapshot test on unlocked n1
    store
        .update_note(
            &n1.id,
            UpdateNoteInput {
                content: Some("Updated Alpha v2".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();
    let sessions_before = store.list_history_sessions(&n1.id).unwrap();
    assert_eq!(sessions_before.len(), 1);
    assert_eq!(sessions_before[0].data, "Secret Alpha");

    let restored = store
        .restore_history_session(&n1.id, &sessions_before[0].id)
        .unwrap();
    assert_eq!(restored.content.unwrap().data, "Secret Alpha");

    // Restoring created a snapshot of "Updated Alpha v2" so nothing was lost!
    let sessions_after = store.list_history_sessions(&n1.id).unwrap();
    assert_eq!(sessions_after.len(), 2);
    assert!(sessions_after.iter().any(|s| s.data == "Updated Alpha v2"));
}

#[test]
fn sync_local_only_isolation_cas_race_60s_conflict_keep_both_and_expiry_restore() {
    let store = NotesStore::in_memory().unwrap();
    let nb = store
        .create_notebook(CreateNotebookInput {
            title: "General".to_owned(),
            ..Default::default()
        })
        .unwrap();

    // Local-only note linked to notebook & tag MUST NOT leak into collect_unsynced_changes!
    let _local_note = store
        .create_note(CreateNoteInput {
            title: Some("Strictly Local".to_owned()),
            content: Some("Never leaves device".to_owned()),
            local_only: true,
            notebook_ids: vec![nb.id.clone()],
            tags: vec!["SharedTag".to_owned()],
            ..Default::default()
        })
        .unwrap();

    let synced_note = store
        .create_note(CreateNoteInput {
            title: Some("Synced Note".to_owned()),
            content: Some("Initial v1".to_owned()),
            ..Default::default()
        })
        .unwrap();

    let (changeset, ack_tokens) = store.collect_unsynced_changes().unwrap();
    assert_eq!(changeset.notes.len(), 1);
    assert_eq!(changeset.notes[0].id, synced_note.id);
    assert!(changeset.note_notebooks.is_empty());
    assert!(changeset.note_tags.is_empty());

    // Mid-sync CAS race: mutate `synced_note` locally BEFORE calling `mark_pushed_synced`!
    store
        .update_note(
            &synced_note.id,
            UpdateNoteInput {
                content: Some("Edited mid-flight v2".to_owned()),
                ..Default::default()
            },
        )
        .unwrap();

    store.mark_pushed_synced(&ack_tokens, &[], 10).unwrap();

    // Because `date_modified` changed mid-flight, `synced_note` MUST still be unsynced (`synced = 0`)!
    let (after_race_cs, _after_race_tokens) = store.collect_unsynced_changes().unwrap();
    assert_eq!(after_race_cs.notes.len(), 1);
    assert_eq!(after_race_cs.contents.len(), 1);
    assert_eq!(after_race_cs.contents[0].data, "Edited mid-flight v2");

    // Merge remote edit >= 60s newer while local is unsynced -> triggers conflict!
    let remote_time = Utc::now() + Duration::seconds(120);
    let mut remote_pull = SyncChangeset::default();
    remote_pull.contents.push(NoteContent {
        id: synced_note.content.as_ref().unwrap().id.clone(),
        note_id: synced_note.id.clone(),
        format: ContentFormat::Markdown,
        data: "Conflicting Remote Version".to_owned(),
        locked: false,
        conflicted_data: None,
        date_edited: remote_time,
        date_modified: remote_time,
        date_resolved: None,
        deleted: false,
        revision: 11,
    });

    let conflicts = store.merge_remote_changes(&remote_pull, 11).unwrap();
    assert_eq!(conflicts, vec![synced_note.id.clone()]);
    let conflicted_detail = store.get_note(&synced_note.id).unwrap();
    assert!(conflicted_detail.conflicted);
    assert_eq!(
        conflicted_detail.content.as_ref().unwrap().data,
        "Edited mid-flight v2"
    );
    assert_eq!(
        conflicted_detail
            .content
            .as_ref()
            .unwrap()
            .conflicted_data
            .as_ref()
            .unwrap()
            .data,
        "Conflicting Remote Version"
    );

    // Resolve with KeepBoth -> clears conflict and creates "(Conflict Copy)" note!
    let (resolved_main, copy_opt) = store
        .resolve_conflict(&synced_note.id, ConflictResolution::KeepBoth, None)
        .unwrap();
    assert!(!resolved_main.conflicted);
    let copy = copy_opt.expect("KeepBoth creates copy");
    assert_eq!(copy.title, "Synced Note (Conflict Copy)");
    assert_eq!(copy.content.unwrap().data, "Conflicting Remote Version");

    // Acknowledge remaining tokens cleanly when no mid-sync race occurs
    let (_, clean_tokens) = store.collect_unsynced_changes().unwrap();
    store.mark_pushed_synced(&clean_tokens, &[], 12).unwrap();
    assert!(store.collect_unsynced_changes().unwrap().0.is_empty());

    // Edge Case #7: Locally expired note auto-restores when remote extends expiryDate!
    let past = Utc::now() - Duration::minutes(10);
    let exp_note = store
        .create_note(CreateNoteInput {
            title: Some("Ephemeral".to_owned()),
            content: Some("Temp".to_owned()),
            expiry_date: Some(past),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(store.sweep_expired_notes(Utc::now()).unwrap(), 1);
    assert_eq!(
        store.get_note(&exp_note.id).unwrap().deleted_by,
        Some(DeletedBy::Expired)
    );

    let future = Utc::now() + Duration::days(7);
    let bump = Utc::now() + Duration::seconds(5);
    let mut restore_cs = SyncChangeset::default();
    restore_cs.notes.push(Note {
        id: exp_note.id.clone(),
        user_id: None,
        title: "Ephemeral".to_owned(),
        headline: "Temp".to_owned(),
        is_generated_title: false,
        pinned: false,
        favorite: false,
        archived: false,
        readonly: false,
        local_only: false,
        conflicted: false,
        locked: false,
        color_id: None,
        expiry_date: Some(future),
        expiry_modified_at: Some(bump),
        trashed_at: None,
        deleted_by: None,
        deleted: false,
        date_created: exp_note.date_created,
        date_edited: exp_note.date_edited,
        date_modified: bump,
        revision: 20,
    });
    store.merge_remote_changes(&restore_cs, 20).unwrap();
    let auto_restored = store.get_note(&exp_note.id).unwrap();
    assert!(auto_restored.trashed_at.is_none());
    assert!(auto_restored.deleted_by.is_none());

    // Backup export + import into a brand-new store
    let _rem = store
        .create_reminder(CreateReminderInput {
            id: None,
            note_id: Some(synced_note.id.clone()),
            title: "Check sync".to_owned(),
            description: None,
            date: future,
            mode: ReminderMode::Repeat,
            recurring_mode: Some(RecurringMode::Day),
            selected_days: vec![],
            priority: ReminderPriority::Vibrate,
            disabled: false,
            snooze_until: None,
            local_only: false,
        })
        .unwrap();
    store
        .set_view_group_options(
            "notes",
            GroupOptions {
                group_by: GroupBy::Abc,
                sort_by: SortBy::Title,
                sort_direction: SortDirection::Asc,
            },
        )
        .unwrap();

    let bundle = store.export_local_backup(Some("backup-pw")).unwrap();
    let dest_store = NotesStore::in_memory().unwrap();
    let counts = dest_store
        .import_local_backup(&bundle, Some("backup-pw"))
        .unwrap();
    assert!(*counts.get("notes").unwrap() >= 3);
    assert_eq!(dest_store.get_view_group_options("notes").unwrap().group_by, GroupBy::Abc);
}
