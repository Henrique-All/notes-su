use std::collections::HashMap;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::prelude::*;
use gpui_kit::*;
use notes_core::{GroupBy, NoteSummary, SortBy, SortDirection};

use crate::{NotesWorkspace, SidebarSection};

impl NotesWorkspace {
    fn active_list_header_title(&self) -> String {
        match self.active_section {
            SidebarSection::Notebooks => {
                if let Some(ref nb_id) = self.selected_notebook_id
                    && let Some(nb) = self.flat_notebooks.iter().find(|n| &n.id == nb_id)
                {
                    return format!("Notebook: {}", nb.title);
                }
                "All Notebooks".to_owned()
            }
            SidebarSection::Tags => {
                if let Some(ref tid) = self.selected_tag_id
                    && let Some(tag) = self.tags.iter().find(|t| &t.id == tid)
                {
                    return format!("#{}", tag.title);
                }
                "All Tags".to_owned()
            }
            sec => sec.label().to_owned(),
        }
    }

    fn render_note_card(
        &self,
        idx: usize,
        note: &NoteSummary,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let border_col = theme.border;
        let muted_fg = theme.muted_foreground;
        let secondary_bg = theme.secondary;

        let nid = note.id.clone();
        let is_selected = self.selected_note_id.as_deref() == Some(note.id.as_str());

        let preview = if note.locked {
            "🔒 Encrypted in Personal Vault".to_owned()
        } else if let Some(ref snippet) = note.snippet
            && !snippet.trim().is_empty()
        {
            snippet
                .replace("<mark>", "[")
                .replace("</mark>", "]")
                .chars()
                .take(100)
                .collect()
        } else if note.headline.is_empty() {
            "No additional text".to_owned()
        } else {
            note.headline.chars().take(100).collect()
        };

        let mut badges = Vec::new();
        if note.pinned {
            badges.push("📌");
        }
        if note.favorite {
            badges.push("★");
        }
        if note.locked {
            badges.push("🔒");
        }
        if note.archived {
            badges.push("🗄");
        }
        let badge_prefix = if badges.is_empty() {
            String::new()
        } else {
            format!("{} ", badges.join(" "))
        };

        let date_label = match self.group_options.sort_by {
            SortBy::DateCreated => note.date_created.format("%Y-%m-%d %H:%M").to_string(),
            SortBy::DateModified => note.date_modified.format("%Y-%m-%d %H:%M").to_string(),
            SortBy::DateEdited | SortBy::Title => {
                note.date_edited.format("%Y-%m-%d %H:%M").to_string()
            }
        };

        let tag_summary: String = note
            .tags
            .iter()
            .take(3)
            .map(|t| format!("#{}", t.title))
            .collect::<Vec<_>>()
            .join(" ");

        div()
            .id(SharedString::from(format!("note-card-{idx}")))
            .test_support()
            .p_2()
            .rounded(px(6.0))
            .border_1()
            .border_color(border_col)
            .cursor_pointer()
            .flex()
            .flex_col()
            .gap_0p5()
            .when(is_selected, |el| el.bg(secondary_bg))
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .truncate()
                    .child(format!("{badge_prefix}{}", note.title)),
            )
            .child(div().text_xs().text_color(muted_fg).child(preview))
            .child(
                div()
                    .pt_0p5()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .text_xs()
                    .text_color(muted_fg)
                    .child(date_label)
                    .when(!tag_summary.is_empty(), |el| {
                        el.child(div().truncate().child(tag_summary))
                    }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.select_note(&nid, window, cx);
            }))
    }

    pub(crate) fn render_note_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let border_col = theme.border;
        let muted_fg = theme.muted_foreground;

        let header_title = self.active_list_header_title();
        let sort_by = self.group_options.sort_by;
        let sort_dir = self.group_options.sort_direction;
        let group_by = self.group_options.group_by;

        let dir_label = match sort_dir {
            SortDirection::Desc => "↓ Desc",
            SortDirection::Asc => "↑ Asc",
        };
        let group_label = match group_by {
            GroupBy::Default => "Grp: Date",
            GroupBy::Month => "Grp: Month",
            GroupBy::Abc => "Grp: A-Z",
            GroupBy::Year => "Grp: Year",
            GroupBy::Week => "Grp: Week",
            GroupBy::None => "Grp: Off",
        };

        let is_trash_view = self.active_section == SidebarSection::Trash;
        let has_any_trash = !self.notes.is_empty() || !self.trashed_notebooks.is_empty();

        // Map note IDs to deterministic 0..N index matching `self.notes` order
        let idx_by_id: HashMap<&str, usize> = self
            .notes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.as_str(), i))
            .collect();
        let note_by_id: HashMap<&str, &NoteSummary> =
            self.notes.iter().map(|n| (n.id.as_str(), n)).collect();

        let mut list_elements: Vec<AnyElement> = Vec::new();
        if group_by != GroupBy::None && self.sections.len() > 1 {
            for sec in &self.sections {
                list_elements.push(
                    div()
                        .pt_1()
                        .px_1()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(muted_fg)
                        .child(format!("{} ({})", sec.title, sec.note_ids.len()))
                        .into_any_element(),
                );
                for nid in &sec.note_ids {
                    if let (Some(&idx), Some(&note)) =
                        (idx_by_id.get(nid.as_str()), note_by_id.get(nid.as_str()))
                    {
                        list_elements.push(self.render_note_card(idx, note, cx).into_any_element());
                    }
                }
            }
        } else {
            for (idx, note) in self.notes.iter().enumerate() {
                list_elements.push(self.render_note_card(idx, note, cx).into_any_element());
            }
        }

        div()
            .id("pane-note-list")
            .test_support()
            .w(px(310.0))
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(border_col)
            .p_3()
            .gap_2()
            // Header + Trash Empty Action
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(format!("{header_title} ({})", self.notes.len())),
                    )
                    .when(is_trash_view && has_any_trash, |el| {
                        el.child(
                            Button::new("empty-trash-btn")
                                .xsmall()
                                .danger()
                                .label("Empty Trash")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.empty_all_trash(window, cx);
                                })),
                        )
                    }),
            )
            // Quick Search Input (SQLite FTS5 + Search DSL)
            .child(
                Input::new(&self.search_input)
                    .id("search-notes-input")
                    .small()
                    .cleanable(true),
            )
            // Sort & Group Toolbar
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child({
                                let b = Button::new("sort-edited-btn").xsmall().label("Edited");
                                let b = if sort_by == SortBy::DateEdited {
                                    b.secondary()
                                } else {
                                    b.ghost()
                                };
                                b.on_click(cx.listener(|this, _, window, cx| {
                                    this.set_sort_by(SortBy::DateEdited, window, cx);
                                }))
                            })
                            .child({
                                let b = Button::new("sort-created-btn").xsmall().label("Created");
                                let b = if sort_by == SortBy::DateCreated {
                                    b.secondary()
                                } else {
                                    b.ghost()
                                };
                                b.on_click(cx.listener(|this, _, window, cx| {
                                    this.set_sort_by(SortBy::DateCreated, window, cx);
                                }))
                            })
                            .child({
                                let b = Button::new("sort-title-btn").xsmall().label("Title");
                                let b = if sort_by == SortBy::Title {
                                    b.secondary()
                                } else {
                                    b.ghost()
                                };
                                b.on_click(cx.listener(|this, _, window, cx| {
                                    this.set_sort_by(SortBy::Title, window, cx);
                                }))
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Button::new("sort-dir-btn")
                                    .xsmall()
                                    .ghost()
                                    .label(dir_label)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_sort_direction(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("group-toggle-btn")
                                    .xsmall()
                                    .ghost()
                                    .label(group_label)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.cycle_group_by(window, cx);
                                    })),
                            ),
                    ),
            )
            // Optional Trashed Notebooks Recovery Strip in Trash View
            .when(is_trash_view && !self.trashed_notebooks.is_empty(), |el| {
                el.child(
                    div()
                        .p_2()
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(border_col)
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(muted_fg)
                                .child(format!(
                                    "Trashed Notebooks ({})",
                                    self.trashed_notebooks.len()
                                )),
                        )
                        .children(self.trashed_notebooks.iter().map(|nb| {
                            let nb_restore = nb.id.clone();
                            let nb_purge = nb.id.clone();
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_1()
                                .child(div().text_xs().truncate().child(nb.title.clone()))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "restore-nb-{}",
                                                nb.id
                                            )))
                                            .xsmall()
                                            .secondary()
                                            .label("Restore")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.restore_trashed_notebook(
                                                    &nb_restore,
                                                    window,
                                                    cx,
                                                );
                                            })),
                                        )
                                        .child(
                                            Button::new(SharedString::from(format!(
                                                "purge-nb-{}",
                                                nb.id
                                            )))
                                            .xsmall()
                                            .danger()
                                            .label("Delete")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.purge_trashed_notebook(&nb_purge, window, cx);
                                            })),
                                        ),
                                )
                        })),
                )
            })
            // Filtered & Sorted Note Cards
            .child(
                div()
                    .id("note-cards-scroll")
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .overflow_y_scroll()
                    .when(self.notes.is_empty(), |el| {
                        el.child(
                            div()
                                .py_6()
                                .text_center()
                                .text_xs()
                                .text_color(muted_fg)
                                .child("No matching notes in this view."),
                        )
                    })
                    .children(list_elements),
            )
    }
}
