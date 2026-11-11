use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::prelude::*;
use gpui_kit::*;
use notes_core::NotebookDto;

use crate::{NotesWorkspace, SidebarSection};

fn flatten_notebook_tree<'a>(
    nodes: &'a [NotebookDto],
    depth: usize,
    out: &mut Vec<(&'a NotebookDto, usize)>,
) {
    for nb in nodes {
        out.push((nb, depth));
        flatten_notebook_tree(&nb.children, depth + 1, out);
    }
}

impl NotesWorkspace {
    pub(crate) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let border_col = theme.border;
        let sidebar_bg = theme.sidebar;
        let muted_fg = theme.muted_foreground;
        let secondary_bg = theme.secondary;

        let mut tree_rows = Vec::new();
        flatten_notebook_tree(&self.notebooks, 0, &mut tree_rows);

        let vault_unlocked = self.session_vault_password.is_some();
        let vault_status_label = if !self.vault_initialized {
            "Setup password to initialize"
        } else if vault_unlocked {
            "Unlocked in session"
        } else {
            "Locked (Argon2id)"
        };

        let selected_nb_for_sub = self.selected_notebook_id.clone();

        div()
            .id("pane-sidebar")
            .test_support()
            .w(px(248.0))
            .h_full()
            .flex()
            .flex_col()
            .bg(sidebar_bg)
            .border_r_1()
            .border_color(border_col)
            .p_3()
            .gap_2()
            // Top Brand & New Note Action
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pb_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .child("notes-su"),
                    )
                    .child(
                        Button::new("new-note-btn")
                            .primary()
                            .small()
                            .label("+ New")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.create_new_note(window, cx);
                            })),
                    ),
            )
            // Scrollable Navigation, Notebooks, Tags & Vault Controls
            .child(
                div()
                    .id("sidebar-scroll")
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .overflow_y_scroll()
                    // Primary Navigation (Notes, Favorites, Vault, Reminders, Archive, Monographs, Trash)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .children(SidebarSection::PRIMARY_NAV.into_iter().map(|sec| {
                                let is_active = self.active_section == sec;
                                div()
                                    .id(sec.element_id())
                                    .test_support()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(6.0))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .when(is_active, |el| el.bg(secondary_bg))
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(if is_active {
                                                FontWeight::SEMIBOLD
                                            } else {
                                                FontWeight::NORMAL
                                            })
                                            .child(format!("{}  {}", sec.icon(), sec.label())),
                                    )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.select_section(sec, window, cx);
                                    }))
                            })),
                    )
                    // Personal Vault Box (Cofre)
                    .child(
                        div()
                            .p_2()
                            .rounded(px(6.0))
                            .border_1()
                            .border_color(border_col)
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("Personal Vault"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted_fg)
                                            .child(vault_status_label),
                                    ),
                            )
                            .when(!vault_unlocked, |el| {
                                el.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            div().flex_1().child(
                                                Input::new(&self.vault_password_input)
                                                    .id("vault-password-input")
                                                    .xsmall()
                                                    .mask_toggle(),
                                            ),
                                        )
                                        .child(
                                            Button::new("vault-action-btn")
                                                .xsmall()
                                                .secondary()
                                                .label(if self.vault_initialized {
                                                    "Unlock"
                                                } else {
                                                    "Init"
                                                })
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.submit_vault_password(window, cx);
                                                })),
                                        ),
                                )
                            })
                            .when(vault_unlocked, |el| {
                                el.child(
                                    Button::new("vault-lock-session-btn")
                                        .xsmall()
                                        .ghost()
                                        .label("Lock Vault Session")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.lock_vault_session(window, cx);
                                        })),
                                )
                            }),
                    )
                    // Notebooks Section (Cadernos)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .id(SidebarSection::Notebooks.element_id())
                                    .test_support()
                                    .px_1()
                                    .py_0p5()
                                    .rounded(px(4.0))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .when(
                                        self.active_section == SidebarSection::Notebooks
                                            && self.selected_notebook_id.is_none(),
                                        |el| el.bg(secondary_bg),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(muted_fg)
                                            .child(format!(
                                                "NOTEBOOKS ({})",
                                                self.flat_notebooks.len()
                                            )),
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.select_section(SidebarSection::Notebooks, window, cx);
                                    })),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div().flex_1().child(
                                            Input::new(&self.new_notebook_input)
                                                .id("new-notebook-input")
                                                .xsmall(),
                                        ),
                                    )
                                    .child(
                                        Button::new("add-notebook-btn")
                                            .xsmall()
                                            .secondary()
                                            .label("+")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.create_notebook_from_input(None, window, cx);
                                            })),
                                    )
                                    .when_some(selected_nb_for_sub, |el, parent_id| {
                                        el.child(
                                            Button::new("add-subnotebook-btn")
                                                .xsmall()
                                                .ghost()
                                                .label("+Sub")
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.create_notebook_from_input(
                                                            Some(parent_id.clone()),
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                        )
                                    }),
                            )
                            .children(tree_rows.into_iter().map(|(nb, depth)| {
                                let nb_id = nb.id.clone();
                                let nb_id_trash = nb.id.clone();
                                let is_selected = self.active_section == SidebarSection::Notebooks
                                    && self.selected_notebook_id.as_deref() == Some(nb.id.as_str());
                                let indent = 6.0 + (depth.min(5) as f32) * 12.0;
                                let prefix = if !nb.children.is_empty() { "▾" } else { "•" };

                                div()
                                    .px_1()
                                    .py_0p5()
                                    .pl(px(indent))
                                    .rounded(px(5.0))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_1()
                                    .when(is_selected, |el| el.bg(secondary_bg))
                                    .child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "notebook-item-{}",
                                                nb.id
                                            )))
                                            .test_support()
                                            .flex_1()
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .truncate()
                                                    .child(format!("{prefix} {}", nb.title)),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(muted_fg)
                                                    .child(nb.total_notes.to_string()),
                                            )
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.select_notebook(&nb_id, window, cx);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "trash-notebook-{}",
                                                nb.id
                                            )))
                                            .test_support()
                                            .px_1()
                                            .cursor_pointer()
                                            .text_xs()
                                            .text_color(muted_fg)
                                            .child("×")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.trash_notebook_by_id(&nb_id_trash, window, cx);
                                            })),
                                    )
                            })),
                    )
                    // Tags Section
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .id(SidebarSection::Tags.element_id())
                                    .test_support()
                                    .px_1()
                                    .py_0p5()
                                    .rounded(px(4.0))
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .when(
                                        self.active_section == SidebarSection::Tags
                                            && self.selected_tag_id.is_none(),
                                        |el| el.bg(secondary_bg),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(muted_fg)
                                            .child(format!("TAGS ({})", self.tags.len())),
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.select_section(SidebarSection::Tags, window, cx);
                                    })),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div().flex_1().child(
                                            Input::new(&self.new_tag_input)
                                                .id("new-tag-input")
                                                .xsmall(),
                                        ),
                                    )
                                    .child(
                                        Button::new("add-tag-btn")
                                            .xsmall()
                                            .secondary()
                                            .label("+")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.create_tag_from_input(window, cx);
                                            })),
                                    ),
                            )
                            .children(self.tags.iter().map(|tag| {
                                let tid = tag.id.clone();
                                let tid_del = tag.id.clone();
                                let is_selected = self.active_section == SidebarSection::Tags
                                    && self.selected_tag_id.as_deref() == Some(tag.id.as_str());
                                let count = tag.note_count.unwrap_or(0);

                                div()
                                    .px_2()
                                    .py_0p5()
                                    .rounded(px(5.0))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_1()
                                    .when(is_selected, |el| el.bg(secondary_bg))
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("tag-item-{}", tag.id)))
                                            .test_support()
                                            .flex_1()
                                            .cursor_pointer()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .truncate()
                                                    .child(format!("#{}", tag.title)),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(muted_fg)
                                                    .child(count.to_string()),
                                            )
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.select_tag(&tid, window, cx);
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "delete-tag-{}",
                                                tag.id
                                            )))
                                            .test_support()
                                            .px_1()
                                            .cursor_pointer()
                                            .text_xs()
                                            .text_color(muted_fg)
                                            .child("×")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.delete_tag_by_id(&tid_del, window, cx);
                                            })),
                                    )
                            })),
                    ),
            )
            // Sidebar Footer Summary
            .child(
                div()
                    .pt_2()
                    .border_t_1()
                    .border_color(border_col)
                    .text_xs()
                    .text_color(muted_fg)
                    .child(format!(
                        "{} notebooks • {} tags",
                        self.flat_notebooks.len(),
                        self.tags.len()
                    )),
            )
    }
}
