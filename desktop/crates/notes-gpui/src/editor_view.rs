use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Editor, Input};
use gpui_kit::prelude::*;
use gpui_kit::*;
use notes_core::{NoteDetail, compute_stats, parse_tasks, task_counts};

use crate::{EditorMode, NotesWorkspace};

impl NotesWorkspace {
    fn render_markdown_preview(
        &self,
        markdown: &str,
        can_toggle_tasks: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let border_col = theme.border;
        let muted_fg = theme.muted_foreground;
        let code_bg = theme.secondary;

        let tasks_by_line: std::collections::HashMap<usize, (bool, String, usize)> =
            parse_tasks(markdown)
                .into_iter()
                .map(|t| (t.line, (t.checked, t.text, t.depth)))
                .collect();

        let mut elements: Vec<AnyElement> = Vec::new();
        let mut in_fence = false;
        let mut fence_buf: Vec<String> = Vec::new();

        for (zero_idx, raw_line) in markdown.lines().enumerate() {
            let line_1 = zero_idx + 1;
            let trimmed_start = raw_line.trim_start();

            if trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~") {
                if in_fence {
                    let block_text = fence_buf.join("\n");
                    fence_buf.clear();
                    in_fence = false;
                    elements.push(
                        div()
                            .p_2p5()
                            .rounded(px(6.0))
                            .bg(code_bg)
                            .border_1()
                            .border_color(border_col)
                            .text_xs()
                            .child(block_text)
                            .into_any_element(),
                    );
                } else {
                    in_fence = true;
                }
                continue;
            }

            if in_fence {
                fence_buf.push(raw_line.to_owned());
                continue;
            }

            if let Some((checked, task_text, depth)) = tasks_by_line.get(&line_1) {
                let checked = *checked;
                let indent_px = ((*depth).min(5) as f32) * 16.0;
                let box_mark = if checked { "☑" } else { "☐" };
                let label = format!("{box_mark}  {task_text}");
                elements.push(
                    div()
                        .id(SharedString::from(format!("preview-task-line-{line_1}")))
                        .test_support()
                        .pl(px(indent_px))
                        .py_0p5()
                        .rounded(px(4.0))
                        .when(can_toggle_tasks, |el| {
                            el.cursor_pointer().on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.toggle_markdown_task_line(line_1, window, cx);
                                },
                            ))
                        })
                        .text_sm()
                        .when(checked, |el| el.text_color(muted_fg))
                        .child(label)
                        .into_any_element(),
                );
                continue;
            }

            if trimmed_start.is_empty() {
                elements.push(div().h(px(6.0)).into_any_element());
            } else if let Some(h1) = trimmed_start.strip_prefix("# ") {
                elements.push(
                    div()
                        .pt_2()
                        .pb_1()
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .child(h1.to_owned())
                        .into_any_element(),
                );
            } else if let Some(h2) = trimmed_start.strip_prefix("## ") {
                elements.push(
                    div()
                        .pt_1p5()
                        .pb_0p5()
                        .text_base()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(h2.to_owned())
                        .into_any_element(),
                );
            } else if let Some(h3) = trimmed_start.strip_prefix("### ") {
                elements.push(
                    div()
                        .pt_1()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(h3.to_owned())
                        .into_any_element(),
                );
            } else if let Some(quote) = trimmed_start.strip_prefix("> ") {
                elements.push(
                    div()
                        .pl_3()
                        .py_0p5()
                        .border_l_2()
                        .border_color(border_col)
                        .text_sm()
                        .text_color(muted_fg)
                        .child(quote.to_owned())
                        .into_any_element(),
                );
            } else if let Some(item) = trimmed_start
                .strip_prefix("- ")
                .or_else(|| trimmed_start.strip_prefix("* "))
            {
                elements.push(
                    div()
                        .pl_2()
                        .text_sm()
                        .child(format!("•  {item}"))
                        .into_any_element(),
                );
            } else {
                elements.push(div().text_sm().child(raw_line.to_owned()).into_any_element());
            }
        }

        if !fence_buf.is_empty() {
            elements.push(
                div()
                    .p_2p5()
                    .rounded(px(6.0))
                    .bg(code_bg)
                    .border_1()
                    .border_color(border_col)
                    .text_xs()
                    .child(fence_buf.join("\n"))
                    .into_any_element(),
            );
        }

        div()
            .id("editor-preview-scroll")
            .test_support()
            .size_full()
            .p_3()
            .rounded(px(6.0))
            .border_1()
            .border_color(border_col)
            .flex()
            .flex_col()
            .gap_1()
            .overflow_y_scroll()
            .children(elements)
    }

    fn render_locked_vault_gate(&self, note: &NoteDetail, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let border_col = theme.border;
        let muted_fg = theme.muted_foreground;
        let nid_temp = note.id.clone();
        let nid_perm = note.id.clone();

        div()
            .id("vault-locked-gate")
            .test_support()
            .flex_1()
            .w_full()
            .rounded(px(8.0))
            .border_1()
            .border_color(border_col)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .p_6()
            .gap_3()
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("🔒 Locked in Personal Vault"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted_fg)
                    .child("Protected with Argon2id + XChaCha20-Poly1305. Enter Vault password to view or edit."),
            )
            .child(
                div()
                    .w(px(280.0))
                    .child(
                        Input::new(&self.unlock_note_input)
                            .id("unlock-note-password-input")
                            .small()
                            .mask_toggle(),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("unlock-note-temp-btn")
                            .small()
                            .primary()
                            .label("Unlock in Session")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.unlock_active_note_in_session(&nid_temp, window, cx);
                            })),
                    )
                    .child(
                        Button::new("unlock-note-perm-btn")
                            .small()
                            .secondary()
                            .label("Decrypt Permanently")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.unlock_active_note_permanently(&nid_perm, window, cx);
                            })),
                    ),
            )
    }

    pub(crate) fn render_editor_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let border_col = theme.border;
        let muted_fg = theme.muted_foreground;
        let secondary_bg = theme.secondary;

        let Some(ref note) = self.active_note else {
            return div()
                .id("pane-editor")
                .test_support()
                .flex_1()
                .h_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .p_6()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .text_color(muted_fg)
                        .child("No note selected."),
                )
                .child(
                    Button::new("empty-create-note-btn")
                        .primary()
                        .small()
                        .label("+ Create New Note")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.create_new_note(window, cx);
                        })),
                )
                .child(div().text_xs().text_color(muted_fg).child(self.status_message.clone()));
        };

        let is_trashed = note.trashed_at.is_some();
        let is_locked_closed = note.locked && !self.active_note_unlocked;
        let readonly_inputs = is_trashed || note.readonly || is_locked_closed;

        let body_str = if is_locked_closed {
            String::new()
        } else {
            self.body_editor.read(cx).value().to_string()
        };
        let stats = compute_stats(&body_str);
        let tasks = task_counts(&body_str);

        let assigned_nb_label = note
            .notebook_ids
            .first()
            .and_then(|nb_id| self.flat_notebooks.iter().find(|n| &n.id == nb_id))
            .map(|nb| format!("📓 {}", nb.title))
            .unwrap_or_else(|| "📓 + Notebook".to_owned());

        let stats_label = if tasks.total > 0 {
            format!(
                "{} words • {} chars • {}/{} tasks • ~{} min read",
                stats.words,
                stats.characters,
                tasks.completed,
                tasks.total,
                stats.reading_time_minutes
            )
        } else {
            format!(
                "{} words • {} chars • ~{} min read",
                stats.words, stats.characters, stats.reading_time_minutes
            )
        };

        let state_badge = if self.is_dirty { "● Unsaved" } else { "✓ Saved" };

        div()
            .id("pane-editor")
            .test_support()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .p_4()
            .gap_2p5()
            // Top Title + Action Bar
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(
                        div().flex_1().child(
                            Input::new(&self.title_input)
                                .id("note-title-input")
                                .readonly(readonly_inputs),
                        ),
                    )
                    // Edit / View mode switcher
                    .child({
                        let b = Button::new("mode-edit-btn").small().label("Edit");
                        let b = if self.editor_mode == EditorMode::Edit {
                            b.secondary()
                        } else {
                            b.ghost()
                        };
                        b.on_click(cx.listener(|this, _, _, cx| {
                            this.set_editor_mode(EditorMode::Edit, cx);
                        }))
                    })
                    .child({
                        let b = Button::new("mode-view-btn").small().label("View");
                        let b = if self.editor_mode == EditorMode::Preview {
                            b.secondary()
                        } else {
                            b.ghost()
                        };
                        b.on_click(cx.listener(|this, _, _, cx| {
                            this.set_editor_mode(EditorMode::Preview, cx);
                        }))
                    })
                    // Active Note Actions (Non-trashed)
                    .when(!is_trashed, |el| {
                        el.child({
                            let b = Button::new("toggle-pin-btn").small().label(if note.pinned {
                                "Pinned"
                            } else {
                                "Pin"
                            });
                            let b = if note.pinned { b.secondary() } else { b.ghost() };
                            b.on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_active_note_pinned(window, cx);
                            }))
                        })
                        .child({
                            let b = Button::new("toggle-fav-btn").small().label(if note.favorite {
                                "★ Fav"
                            } else {
                                "☆ Fav"
                            });
                            let b = if note.favorite { b.secondary() } else { b.ghost() };
                            b.on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_active_note_favorite(window, cx);
                            }))
                        })
                        .child({
                            let b = Button::new("toggle-lock-btn").small().label(if note.locked {
                                "🔒 Vault"
                            } else {
                                "Lock"
                            });
                            let b = if note.locked { b.secondary() } else { b.ghost() };
                            b.on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_active_note_vault_lock(window, cx);
                            }))
                        })
                        .child({
                            let b = Button::new("save-note-btn").small().label("Save");
                            let b = if self.is_dirty {
                                b.primary()
                            } else {
                                b.secondary()
                            };
                            b.on_click(cx.listener(|this, _, window, cx| {
                                this.save_active_note(window, cx);
                            }))
                        })
                        .child(
                            Button::new("trash-note-btn")
                                .small()
                                .danger()
                                .label("Trash")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.trash_active_note(window, cx);
                                })),
                        )
                    })
                    // Trash Recovery Actions
                    .when(is_trashed, |el| {
                        el.child(
                            Button::new("restore-note-btn")
                                .small()
                                .primary()
                                .label("Restore")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.restore_active_note(window, cx);
                                })),
                        )
                        .child(
                            Button::new("purge-note-btn")
                                .small()
                                .danger()
                                .label("Delete Forever")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.purge_active_note(window, cx);
                                })),
                        )
                    }),
            )
            // Organization Metadata Bar: Notebook assignment + Tags + Archive toggle
            .when(!is_trashed, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .pb_1()
                        .border_b_1()
                        .border_color(border_col)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .flex_wrap()
                                .child(
                                    Button::new("cycle-notebook-btn")
                                        .xsmall()
                                        .ghost()
                                        .label(assigned_nb_label)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.cycle_active_note_notebook(window, cx);
                                        })),
                                )
                                .children(note.tags.iter().map(|t| {
                                    let tag_title = t.title.clone();
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded(px(4.0))
                                        .bg(secondary_bg)
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .text_xs()
                                        .child(format!("#{}", t.title))
                                        .child(
                                            div()
                                                .id(SharedString::from(format!(
                                                    "remove-note-tag-{}",
                                                    t.title
                                                )))
                                                .test_support()
                                                .cursor_pointer()
                                                .text_color(muted_fg)
                                                .child("×")
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.remove_tag_from_active_note(
                                                            &tag_title, window, cx,
                                                        );
                                                    },
                                                )),
                                        )
                                }))
                                .child(
                                    div()
                                        .w(px(110.0))
                                        .child(
                                            Input::new(&self.note_tag_input)
                                                .id("note-tag-input")
                                                .xsmall(),
                                        ),
                                )
                                .child(
                                    Button::new("add-note-tag-btn")
                                        .xsmall()
                                        .ghost()
                                        .label("+Tag")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.add_tag_to_active_note(window, cx);
                                        })),
                                ),
                        )
                        .child({
                            let b = Button::new("toggle-archive-btn").xsmall().label(
                                if note.archived {
                                    "Unarchive"
                                } else {
                                    "Archive"
                                },
                            );
                            let b = if note.archived {
                                b.secondary()
                            } else {
                                b.ghost()
                            };
                            b.on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_active_note_archived(window, cx);
                            }))
                        }),
                )
            })
            // Main Body Area: Vault Gate OR Edit Mode OR Preview Mode
            .child(if is_locked_closed {
                self.render_locked_vault_gate(note, cx).into_any_element()
            } else if self.editor_mode == EditorMode::Preview {
                div()
                    .flex_1()
                    .w_full()
                    .child(self.render_markdown_preview(
                        &body_str,
                        !is_trashed && !note.readonly,
                        cx,
                    ))
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .w_full()
                    .child(
                        Editor::new(&self.body_editor)
                            .h(relative(1.0))
                            .readonly(readonly_inputs),
                    )
                    .into_any_element()
            })
            // Footer Status Bar
            .child(
                div()
                    .pt_2()
                    .border_t_1()
                    .border_color(border_col)
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_xs()
                    .text_color(muted_fg)
                    .child(stats_label)
                    .child(format!("{state_badge}  •  {}", self.status_message)),
            )
    }
}
