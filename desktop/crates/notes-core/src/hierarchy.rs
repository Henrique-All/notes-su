use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Datelike, Utc};

use crate::models::{
    BreadcrumbItem, GroupBy, GroupOptions, GroupSection, NoteSummary, Notebook, SortBy,
    SortDirection,
};

pub fn detect_notebook_cycle(
    notebook_id: &str,
    proposed_parent_id: Option<&str>,
    parent_map: &HashMap<String, Option<String>>,
) -> bool {
    let Some(mut cur) = proposed_parent_id else {
        return false;
    };
    if cur == notebook_id {
        return true;
    }
    let mut visited = HashSet::new();
    visited.insert(notebook_id);
    loop {
        if cur == notebook_id {
            return true;
        }
        if !visited.insert(cur) {
            return true;
        }
        match parent_map.get(cur).and_then(|p| p.as_deref()) {
            Some(next_parent) => cur = next_parent,
            None => return false,
        }
    }
}

pub fn compute_breadcrumbs(notebook_id: &str, notebooks: &[Notebook]) -> Vec<BreadcrumbItem> {
    let by_id: HashMap<&str, &Notebook> = notebooks
        .iter()
        .filter(|nb| !nb.deleted)
        .map(|nb| (nb.id.as_str(), nb))
        .collect();

    let mut trail = Vec::new();
    let mut visited = HashSet::new();
    let mut cur_opt = Some(notebook_id);

    while let Some(cur_id) = cur_opt {
        if !visited.insert(cur_id) {
            break;
        }
        let Some(nb) = by_id.get(cur_id) else {
            break;
        };
        trail.push(BreadcrumbItem {
            id: nb.id.clone(),
            title: nb.title.clone(),
        });
        cur_opt = nb.parent_id.as_deref();
    }

    trail.reverse();
    trail
}

pub fn compare_notes(a: &NoteSummary, b: &NoteSummary, options: GroupOptions) -> Ordering {
    let pin_cmp = b.pinned.cmp(&a.pinned);
    if pin_cmp != Ordering::Equal {
        return pin_cmp;
    }

    let base_cmp = match options.sort_by {
        SortBy::DateEdited => a
            .date_edited
            .timestamp_millis()
            .cmp(&b.date_edited.timestamp_millis()),
        SortBy::DateCreated => a
            .date_created
            .timestamp_millis()
            .cmp(&b.date_created.timestamp_millis()),
        SortBy::DateModified => a
            .date_modified
            .timestamp_millis()
            .cmp(&b.date_modified.timestamp_millis()),
        SortBy::Title => a
            .title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.title.cmp(&b.title)),
    };

    let directed = match options.sort_direction {
        SortDirection::Asc => base_cmp,
        SortDirection::Desc => base_cmp.reverse(),
    };

    directed.then_with(|| a.id.cmp(&b.id))
}

fn note_relevant_timestamp(note: &NoteSummary, sort_by: SortBy) -> DateTime<Utc> {
    match sort_by {
        SortBy::DateCreated => note.date_created,
        SortBy::DateModified => note.date_modified,
        SortBy::DateEdited | SortBy::Title => note.date_edited,
    }
}

fn bucket_for_unpinned_note(
    note: &NoteSummary,
    options: GroupOptions,
    now: DateTime<Utc>,
) -> (String, String) {
    match options.group_by {
        GroupBy::None => ("all".to_owned(), "Notes".to_owned()),
        GroupBy::Abc => {
            let first = note
                .title
                .trim()
                .chars()
                .next()
                .filter(|c| c.is_alphabetic())
                .and_then(|c| c.to_uppercase().next());
            match first {
                Some(ch) => {
                    let s = ch.to_string();
                    (s.clone(), s)
                }
                None => ("#".to_owned(), "#".to_owned()),
            }
        }
        GroupBy::Year => {
            let ts = note_relevant_timestamp(note, options.sort_by);
            let y = ts.year().to_string();
            (y.clone(), y)
        }
        GroupBy::Month => {
            let ts = note_relevant_timestamp(note, options.sort_by);
            let key = format!("{:04}-{:02}", ts.year(), ts.month());
            let title = ts.format("%B %Y").to_string();
            (key, title)
        }
        GroupBy::Week => {
            let ts = note_relevant_timestamp(note, options.sort_by);
            let iso = ts.iso_week();
            let key = format!("{:04}-W{:02}", iso.year(), iso.week());
            let title = format!("Week {}, {}", iso.week(), iso.year());
            (key, title)
        }
        GroupBy::Default => {
            let ts = note_relevant_timestamp(note, options.sort_by);
            let today = now.date_naive();
            let note_day = ts.date_naive();
            let days_diff = (today - note_day).num_days();
            if days_diff <= 0 {
                ("today".to_owned(), "Today".to_owned())
            } else if days_diff == 1 {
                ("yesterday".to_owned(), "Yesterday".to_owned())
            } else if days_diff < 7 {
                ("this_week".to_owned(), "This Week".to_owned())
            } else if ts.year() == now.year() && ts.month() == now.month() {
                ("this_month".to_owned(), "This Month".to_owned())
            } else {
                ("older".to_owned(), "Older".to_owned())
            }
        }
    }
}

pub fn group_and_sort_notes(
    mut notes: Vec<NoteSummary>,
    options: GroupOptions,
    now: DateTime<Utc>,
) -> (Vec<NoteSummary>, Vec<GroupSection>) {
    notes.sort_by(|a, b| compare_notes(a, b, options));

    let mut sections: Vec<GroupSection> = Vec::new();
    let mut section_index: HashMap<String, usize> = HashMap::new();
    let mut bucketed_notes: Vec<Vec<NoteSummary>> = Vec::new();

    for note in notes {
        let (key, title) = if note.pinned {
            ("pinned".to_owned(), "Pinned".to_owned())
        } else {
            bucket_for_unpinned_note(&note, options, now)
        };

        let idx = if let Some(&existing) = section_index.get(&key) {
            existing
        } else {
            let new_idx = sections.len();
            section_index.insert(key.clone(), new_idx);
            sections.push(GroupSection {
                key,
                title,
                note_ids: Vec::new(),
            });
            bucketed_notes.push(Vec::new());
            new_idx
        };

        sections[idx].note_ids.push(note.id.clone());
        bucketed_notes[idx].push(note);
    }

    let ordered_notes: Vec<NoteSummary> = bucketed_notes.into_iter().flatten().collect();
    (ordered_notes, sections)
}
