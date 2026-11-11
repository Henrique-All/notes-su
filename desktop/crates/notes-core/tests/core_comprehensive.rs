use std::collections::HashMap;

use chrono::{TimeZone, Utc};
use notes_core::*;

#[test]
fn serde_cipher_envelope_preserves_mem_kib_and_accepts_aliases() {
    let env = CipherEnvelope {
        alg: CIPHER_ALG_V1.to_owned(),
        kdf: KdfParams::default(),
        salt: "AAECAwQFBgcICQoLDA0ODw==".to_owned(),
        iv: "EBESExQVFhcYGRobHB0eHyAhIiMkJSYn".to_owned(),
        cipher: "BE13bnqsaTK2cm0EOzfKneXdL0SzZM/j8Dl5mmohs8oBKN4i5cti/g==".to_owned(),
        length: 24,
    };

    let json = serde_json::to_string(&env).expect("serialize envelope");
    assert!(json.contains("\"memKiB\":19456"));
    assert!(!json.contains("\"memKib\""));

    let from_camel_alias: CipherEnvelope = serde_json::from_str(
        r#"{"alg":"xchacha20poly1305-argon2id-v1","kdf":{"ops":3,"memKib":19456,"parallelism":1},"salt":"AA==","iv":"AA==","cipher":"AA==","length":0}"#,
    )
    .expect("deserialize memKib alias");
    assert_eq!(from_camel_alias.kdf.mem_kib, 19456);

    let from_snake_alias: CipherEnvelope = serde_json::from_str(
        r#"{"alg":"xchacha20poly1305-argon2id-v1","kdf":{"ops":3,"mem_kib":19456,"parallelism":1},"salt":"AA==","iv":"AA==","cipher":"AA==","length":0}"#,
    )
    .expect("deserialize mem_kib alias");
    assert_eq!(from_snake_alias.kdf.mem_kib, 19456);
}

#[test]
fn serde_flexible_timestamps_envelopes_and_resolutions() {
    // Vault with stringified keyCanary and numeric millisecond timestamps
    let env_str = serde_json::to_string(&CipherEnvelope {
        alg: CIPHER_ALG_V1.to_owned(),
        kdf: KdfParams::default(),
        salt: "c2FsdA==".to_owned(),
        iv: "aXY=".to_owned(),
        cipher: "Y2lwaGVy".to_owned(),
        length: 5,
    })
    .unwrap();

    let vault_json = serde_json::json!({
        "id": "v1",
        "keyCanary": env_str,
        "lockAfter": 120,
        "dateCreated": 1762811361123i64,
        "dateModified": "2026-11-10"
    });
    let vault: Vault = serde_json::from_value(vault_json).expect("deserialize flexible vault");
    assert_eq!(vault.lock_after_seconds, 120);
    assert_eq!(vault.key_canary.length, 5);
    assert_eq!(vault.date_created.timestamp_millis(), 1762811361123);

    let r1: ConflictResolution = serde_json::from_str("\"keep_local\"").unwrap();
    let r2: ConflictResolution = serde_json::from_str("\"keepBoth\"").unwrap();
    assert_eq!(r1, ConflictResolution::KeepLocal);
    assert_eq!(r2, ConflictResolution::KeepBoth);
    assert_eq!(
        serde_json::to_string(&ConflictResolution::KeepRemote).unwrap(),
        "\"keep_remote\""
    );

    // Seeded 7 default colors preserve exact hex casing (#f44336 for red, uppercase others)
    let seeded = Color::seed_defaults(Some("u1"), Utc::now());
    assert_eq!(seeded.len(), 7);
    assert_eq!(seeded[0].title, "red");
    assert_eq!(seeded[0].color_code, "#f44336");
    assert_eq!(seeded[1].color_code, "#FF9800");
    assert_eq!(seeded[6].title, "gray");
}

#[test]
fn title_validation_sanitization_and_template_resolution() {
    assert_eq!(
        validate_note_not_blank(None, Some("   \n\t  ")),
        Err(CoreError::BlankNote)
    );
    assert!(validate_note_not_blank(Some("Hello"), None).is_ok());
    assert!(validate_note_not_blank(None, Some("Body text")).is_ok());

    assert_eq!(sanitize_title("  First\r\nSecond\n\tThird  "), "First Second Third");

    let now = Utc.with_ymd_and_hms(2026, 11, 10, 14, 35, 0).unwrap();
    let (t1, gen1) = resolve_note_title(
        None,
        "# Reunião de Projeto\n\nDetalhes importantes",
        false,
        None,
        now,
        4,
    );
    assert_eq!(t1, "Reunião de Projeto");
    assert!(gen1);

    let (t2, gen2) = resolve_note_title(
        None,
        "Body",
        false,
        Some("Note #$count$ ($date$ $time$) - $headline$"),
        now,
        9,
    );
    assert_eq!(t2, "Note #10 (2026-11-10 14:35) - Body");
    assert!(gen2);

    // Locked note never leaks content into $headline$
    let (t_locked, gen_locked) =
        resolve_note_title(None, "Top secret body", true, Some("$headline$"), now, 0);
    assert_eq!(t_locked, "Note 2026-11-10 14:35");
    assert!(gen_locked);

    let (t_explicit, gen_explicit) =
        resolve_note_title(Some("Custom Title"), "Body", false, None, now, 0);
    assert_eq!(t_explicit, "Custom Title");
    assert!(!gen_explicit);
}

#[test]
fn headline_extraction_utf8_truncation_and_locked_guard() {
    let md = "---\ntitle: Ignored\n---\n# Olá Mundo 🔒\n\nVeja [[note-1|Nota Referenciada]] e ![alt](https://example.com/img.png) com **negrito**.\n";
    let hl = extract_headline(md, false);
    assert_eq!(hl, "Olá Mundo 🔒 Veja Nota Referenciada e com negrito.");
    assert_eq!(extract_headline(md, true), "");

    let long_utf8 = "ãçé🔒".repeat(200); // 800 UTF-8 chars, multi-byte
    let truncated = extract_headline(&long_utf8, false);
    assert_eq!(truncated.chars().count(), 350);
}

#[test]
fn document_stats_and_reading_time() {
    assert_eq!(
        compute_stats(""),
        NoteStats {
            words: 0,
            characters: 0,
            characters_no_spaces: 0,
            spaces: 0,
            paragraphs: 0,
            reading_time_minutes: 0
        }
    );

    let md = "# Heading One\n\n- [x] First task\n- [ ] Second task\n\nParagraph three with words.";
    let stats = compute_stats(md);
    // Heading(1) One(2) First(3) task(4) Second(5) task(6) Paragraph(7) three(8) with(9) words(10)
    assert_eq!(stats.words, 10);
    assert_eq!(stats.paragraphs, 3);
    assert_eq!(stats.reading_time_minutes, 1);

    let two_hundred_one = vec!["palavra"; 201].join(" ");
    assert_eq!(compute_stats(&two_hundred_one).reading_time_minutes, 2);
}

#[test]
fn toc_extraction_skips_fenced_code_and_empty_headings_and_deduplicates_slugs() {
    let md = "---\ntitle: Demo\n---\n# Introduction\n\n```bash\n# comment inside code block\n```\n\n##   \n## Sub Section!\n### Introduction\n";
    let toc = extract_toc(md);
    assert_eq!(toc.len(), 3);
    assert_eq!(
        toc[0],
        TocHeading {
            level: 1,
            text: "Introduction".to_owned(),
            slug: "introduction".to_owned(),
            line: 4
        }
    );
    assert_eq!(
        toc[1],
        TocHeading {
            level: 2,
            text: "Sub Section!".to_owned(),
            slug: "sub-section".to_owned(),
            line: 11
        }
    );
    assert_eq!(
        toc[2],
        TocHeading {
            level: 3,
            text: "Introduction".to_owned(),
            slug: "introduction-1".to_owned(),
            line: 12
        }
    );
}

#[test]
fn task_list_parser_toggle_clear_and_nested_sort() {
    let md = "# Tasks\n- [x] Done root\n  - [ ] Child of done\n- [ ] Active root\n  - [x] Done subtask\n  - [ ] Open subtask\n```\n- [ ] Fenced fake task\n```\n";
    let counts = task_counts(md);
    assert_eq!(counts, TaskCounts { total: 5, completed: 2 });

    let toggled = toggle_task_by_index(md, 2).expect("toggle active root");
    assert_eq!(task_counts(&toggled).completed, 3);

    // Toggling fenced line fails cleanly
    assert!(matches!(
        toggle_task_at_line(md, 8),
        Err(CoreError::TaskLineOutOfRange(8))
    ));

    let sorted = sort_completed_tasks_to_bottom(md);
    let expected_sorted = "# Tasks\n- [ ] Active root\n  - [ ] Open subtask\n  - [x] Done subtask\n- [x] Done root\n  - [ ] Child of done\n```\n- [ ] Fenced fake task\n```\n";
    assert_eq!(sorted, expected_sorted);

    let cleared = clear_completed_tasks(md);
    let expected_cleared = "# Tasks\n- [ ] Active root\n  - [ ] Open subtask\n```\n- [ ] Fenced fake task\n```\n";
    assert_eq!(cleared, expected_cleared);
}

#[test]
fn csv_and_gfm_table_roundtrip_and_malformed_fallbacks() {
    let csv_in = "Name,Notes,PipeCol\n\"Alice, Jr.\",\"Line 1\nLine 2\",a|b\nBob,\"Quoted \"\"hi\"\"\",plain\n";
    let gfm = csv_to_gfm_table(csv_in);
    assert!(gfm.contains("Line 1<br>Line 2"));
    assert!(gfm.contains("a\\|b"));

    let csv_out = gfm_table_to_csv(&gfm);
    let reparsed_gfm = csv_to_gfm_table(&csv_out);
    assert_eq!(gfm, reparsed_gfm);

    // Malformed unclosed CSV quote falls back gracefully
    let bad_csv = "a,b\n\"unclosed,cell";
    assert_eq!(csv_to_gfm_table(bad_csv), bad_csv);

    // Non-table markdown falls back gracefully
    let plain_md = "Just a regular paragraph without pipes.";
    assert_eq!(gfm_table_to_csv(plain_md), plain_md);
}

#[test]
fn internal_links_extraction_skips_code_and_locked_notes() {
    let md = "Intro [Project Spec](nn://note/11111111-1111-4111-8111-111111111111?blockId=blk-9)\nInline `[[ignored-inline]]` and [[22222222-2222-4222-8222-222222222222|Second Note]].\n```\nnn://note/ignored-fenced\n```\nBare: nn://note/33333333-3333-4333-8333-333333333333\n";

    assert!(extract_internal_links(md, true).is_empty());

    let links = extract_internal_links(md, false);
    assert_eq!(links.len(), 3);
    assert_eq!(links[0].note_id, "11111111-1111-4111-8111-111111111111");
    assert_eq!(links[0].block_id.as_deref(), Some("blk-9"));
    assert_eq!(links[0].label, "Project Spec");
    assert_eq!(links[0].line, 1);

    assert_eq!(links[1].note_id, "22222222-2222-4222-8222-222222222222");
    assert_eq!(links[1].label, "Second Note");
    assert_eq!(links[1].line, 2);

    assert_eq!(links[2].note_id, "33333333-3333-4333-8333-333333333333");
    assert_eq!(links[2].line, 6);
}

#[test]
fn frontmatter_and_xss_safe_html_export_ghsa_advisories() {
    let meta = NoteFrontmatter {
        title: Some("Note Title".to_owned()),
        tags: vec!["rust".to_owned(), "gpui".to_owned()],
        pinned: Some(true),
        ..Default::default()
    };
    assert_eq!(
        export_markdown_with_frontmatter(&meta, "Hello", true),
        Err(CoreError::NoteLocked)
    );
    let exported_md = export_markdown_with_frontmatter(&meta, "# Body\n", false).unwrap();
    let (parsed_meta, body) = parse_frontmatter(&exported_md);
    assert_eq!(parsed_meta, Some(meta));
    assert_eq!(body, "# Body\n");

    // GHSA-fjm8-jg78-89h4 XSS payload test
    let malicious_title = "<script>alert('title')</script>";
    let malicious_md = "Hello <script>alert(1)</script> <img src=x onerror=alert(2)> [bad](javascript:alert(3)) [badData](data:text/html,<script>alert(4)</script>) [good](nn://note/abc) <u>underlined</u>";

    assert_eq!(
        export_xss_safe_html(malicious_title, None, malicious_md, true),
        Err(CoreError::NoteLocked)
    );

    let html = export_xss_safe_html(
        malicious_title,
        Some("\"><svg onload=alert(9)>"),
        malicious_md,
        false,
    )
    .expect("html export");

    assert!(!html.contains("<script>"));
    assert!(!html.contains("alert(1)"));
    assert!(!html.contains("onerror="));
    assert!(!html.contains("<svg"));
    assert!(html.contains("&quot;&gt;&lt;svg onload=alert(9)&gt;"));
    assert!(!html.contains("javascript:"));
    assert!(!html.contains("data:text/html"));
    assert!(html.contains("&lt;script&gt;alert(&#39;title&#39;)&lt;/script&gt;"));
    assert!(html.contains("href=\"nn://note/abc\""));
    assert!(html.contains("<u>underlined</u>"));

    // GHSA-45g3-cv93-q59v Diff XSS title test
    let diff = compute_diff(
        "<img src=x onerror=alert(1)>",
        "old word line\nsame\n",
        "<b>Safe?</b>",
        "new word line\nsame\n",
    );
    assert_eq!(diff.escaped_old_title, "&lt;img src=x onerror=alert(1)&gt;");
    assert_eq!(diff.escaped_new_title, "&lt;b&gt;Safe?&lt;/b&gt;");
    assert_eq!(diff.additions, 1);
    assert_eq!(diff.deletions, 1);
    assert!(!diff.hunks.is_empty());
}

#[test]
fn search_dsl_all_15_operators_boolean_precedence_diacritics_and_locked_isolation() {
    let t_created = Utc.with_ymd_and_hms(2026, 11, 5, 10, 0, 0).unwrap();
    let t_edited = Utc.with_ymd_and_hms(2026, 11, 10, 15, 30, 0).unwrap();

    let note = Note {
        id: "n1".to_owned(),
        user_id: None,
        title: "Café & Orçamento".to_owned(),
        headline: "Comprar pão".to_owned(),
        is_generated_title: false,
        pinned: true,
        favorite: true,
        archived: false,
        readonly: false,
        local_only: false,
        conflicted: false,
        locked: false,
        color_id: Some("c1".to_owned()),
        expiry_date: None,
        expiry_modified_at: None,
        trashed_at: None,
        deleted_by: None,
        deleted: false,
        date_created: t_created,
        date_edited: t_edited,
        date_modified: t_edited,
        revision: 1,
    };
    let tag = Tag {
        id: "t1".to_owned(),
        user_id: None,
        title: "Work".to_owned(),
        note_count: None,
        date_created: t_created,
        date_modified: t_created,
        deleted: false,
        revision: 1,
    };
    let color = Color {
        id: "c1".to_owned(),
        user_id: None,
        title: "red".to_owned(),
        color_code: "#f44336".to_owned(),
        sort_index: 0,
        note_count: None,
        date_created: t_created,
        date_modified: t_created,
        deleted: false,
        revision: 1,
    };
    let root_nb = Notebook {
        id: "nb-root".to_owned(),
        user_id: None,
        parent_id: None,
        title: "Engineering".to_owned(),
        description: None,
        pinned: false,
        trashed_at: None,
        deleted_by: None,
        deleted: false,
        date_created: t_created,
        date_edited: t_created,
        date_modified: t_created,
        revision: 1,
    };
    let child_nb = Notebook {
        id: "nb-child".to_owned(),
        parent_id: Some("nb-root".to_owned()),
        title: "Rust GPUI".to_owned(),
        ..root_nb.clone()
    };

    let assigned = vec!["nb-child".to_owned()];
    let notebooks = vec![root_nb, child_nb];
    let tags = vec![tag];

    let ctx = SearchEvalContext {
        note: &note,
        content_data: Some("Comprar pão de açúcar na padaria https://example.com/path"),
        tags: &tags,
        color: Some(&color),
        assigned_notebook_ids: &assigned,
        all_notebooks: &notebooks,
    };

    let complex = parse_search_query(
        r#"title:cafe content:"pao de acucar" tag:Work color:#F44336 pinned:true locked:false readonly:false favorite:true archived:false tagged:true colored:true in_notebook:Engineering created_after:2026-11-01 created_before:2026-11-06 edited_after:2026-11-10 edited_before:2026-11-11 -tag:Personal"#,
    );
    assert!(matches_note(&complex, &ctx));

    // Precedence: OR < AND < NOT
    let prec = parse_search_query("tag:Other OR tag:Work AND NOT archived:true");
    assert!(matches_note(&prec, &ctx));

    // Unknown prefixes / invalid booleans / invalid dates / URLs fall back cleanly to literal text
    let fallback_url = parse_search_query("https://example.com/path");
    assert!(matches_note(&fallback_url, &ctx));
    let fallback_bad = parse_search_query("pinned:maybe created_before:not-a-date");
    assert!(!matches_note(&fallback_bad, &ctx));

    // Locked note NEVER matches content: or body ciphertext
    let mut locked_note = note.clone();
    locked_note.locked = true;
    locked_note.headline = String::new();
    let locked_ctx = SearchEvalContext {
        note: &locked_note,
        content_data: Some(r#"{"alg":"xchacha20poly1305-argon2id-v1","cipher":"SECRET_CIPHERTEXT"}"#),
        tags: &tags,
        color: Some(&color),
        assigned_notebook_ids: &assigned,
        all_notebooks: &notebooks,
    };
    assert!(!matches_note(
        &parse_search_query("content:xchacha20poly1305"),
        &locked_ctx
    ));
    assert!(!matches_note(
        &parse_search_query("SECRET_CIPHERTEXT"),
        &locked_ctx
    ));
    assert!(matches_note(
        &parse_search_query("cafe locked:true"),
        &locked_ctx
    ));
}

#[test]
fn reminders_clamping_hierarchy_cycles_and_conflict_window() {
    let base = Utc.with_ymd_and_hms(2026, 1, 31, 9, 0, 0).unwrap();
    let reminder = Reminder {
        id: "r1".to_owned(),
        user_id: None,
        note_id: None,
        title: "Pay rent".to_owned(),
        description: None,
        date: base,
        mode: ReminderMode::Repeat,
        recurring_mode: Some(RecurringMode::Month),
        selected_days: vec![31],
        priority: ReminderPriority::Urgent,
        disabled: false,
        snooze_until: None,
        next_occurrence: None,
        local_only: false,
        date_created: base,
        date_modified: base,
        deleted: false,
        revision: 0,
    };

    // Non-leap Feb 2026 clamps 31 -> Feb 28
    let feb_2026 = next_occurrence(&reminder, Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).unwrap());
    assert_eq!(
        feb_2026,
        Some(Utc.with_ymd_and_hms(2026, 2, 28, 9, 0, 0).unwrap())
    );

    // Leap Feb 2028 clamps 31 -> Feb 29
    let feb_2028 = next_occurrence(&reminder, Utc.with_ymd_and_hms(2028, 2, 1, 0, 0, 0).unwrap());
    assert_eq!(
        feb_2028,
        Some(Utc.with_ymd_and_hms(2028, 2, 29, 9, 0, 0).unwrap())
    );

    // April 2026 clamps 31 -> Apr 30
    let apr_2026 = next_occurrence(&reminder, Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap());
    assert_eq!(
        apr_2026,
        Some(Utc.with_ymd_and_hms(2026, 4, 30, 9, 0, 0).unwrap())
    );

    // Notebook cycle detection: A -> B -> C, moving A under C creates cycle
    let mut parents = HashMap::new();
    parents.insert("A".to_owned(), None);
    parents.insert("B".to_owned(), Some("A".to_owned()));
    parents.insert("C".to_owned(), Some("B".to_owned()));
    assert!(detect_notebook_cycle("A", Some("A"), &parents));
    assert!(detect_notebook_cycle("A", Some("C"), &parents));
    assert!(!detect_notebook_cycle("C", None, &parents));

    // 60-second conflict window
    let t0 = Utc.with_ymd_and_hms(2026, 11, 10, 12, 0, 0).unwrap();
    let t_59s = t0 + chrono::Duration::seconds(59);
    let t_60s = t0 + chrono::Duration::seconds(60);

    assert!(!is_content_conflicted("a", false, t0, true, "b", false, t_59s));
    assert!(is_content_conflicted("a", false, t0, true, "b", false, t_60s));
    assert!(!is_content_conflicted(
        "same\r\n",
        false,
        t0,
        true,
        "same\n",
        false,
        t_60s
    ));
    assert!(!is_content_conflicted("a", false, t0, false, "b", false, t_60s));
}
