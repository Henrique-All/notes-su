use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

use crate::markdown::escape_html;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiffTag {
    Equal,
    Delete,
    Insert,
}

impl From<ChangeTag> for DiffTag {
    fn from(tag: ChangeTag) -> Self {
        match tag {
            ChangeTag::Equal => Self::Equal,
            ChangeTag::Delete => Self::Delete,
            ChangeTag::Insert => Self::Insert,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffSpan {
    pub tag: DiffTag,
    pub emphasized: bool,
    pub value: String,
    pub escaped_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub tag: DiffTag,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub value: String,
    pub escaped_value: String,
    pub inline_spans: Vec<DiffSpan>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NoteDiff {
    pub old_title: String,
    pub new_title: String,
    pub escaped_old_title: String,
    pub escaped_new_title: String,
    pub title_changed: bool,
    pub title_spans: Vec<DiffSpan>,
    pub additions: usize,
    pub deletions: usize,
    pub hunks: Vec<DiffHunk>,
    pub unified_diff: String,
}

pub fn compute_diff(old_title: &str, old_data: &str, new_title: &str, new_data: &str) -> NoteDiff {
    let escaped_old_title = escape_html(old_title);
    let escaped_new_title = escape_html(new_title);
    let title_changed = old_title != new_title;

    let title_diff = TextDiff::from_words(old_title, new_title);
    let title_spans = title_diff
        .iter_all_changes()
        .map(|change| {
            let tag = DiffTag::from(change.tag());
            let val = change.value().to_owned();
            let escaped_value = escape_html(&val);
            DiffSpan {
                tag,
                emphasized: tag != DiffTag::Equal,
                value: val,
                escaped_value,
            }
        })
        .collect();

    let body_diff = TextDiff::from_lines(old_data, new_data);
    let mut additions = 0usize;
    let mut deletions = 0usize;
    for change in body_diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => additions += 1,
            ChangeTag::Delete => deletions += 1,
            ChangeTag::Equal => {}
        }
    }

    let mut hunks = Vec::new();
    for group in body_diff.grouped_ops(3) {
        let mut lines = Vec::new();
        for op in &group {
            for change in body_diff.iter_inline_changes(op) {
                let tag = DiffTag::from(change.tag());
                let old_line = change.old_index().map(|idx| idx + 1);
                let new_line = change.new_index().map(|idx| idx + 1);

                let mut full_line = String::new();
                let mut spans = Vec::new();
                for (emphasized, piece) in change.iter_strings_lossy() {
                    let s = piece.into_owned();
                    full_line.push_str(&s);
                    let escaped = escape_html(&s);
                    spans.push(DiffSpan {
                        tag,
                        emphasized,
                        value: s,
                        escaped_value: escaped,
                    });
                }

                let escaped_line = escape_html(&full_line);
                lines.push(DiffLine {
                    tag,
                    old_line,
                    new_line,
                    value: full_line,
                    escaped_value: escaped_line,
                    inline_spans: spans,
                });
            }
        }
        if !lines.is_empty() {
            hunks.push(DiffHunk { lines });
        }
    }

    let unified_diff = body_diff
        .unified_diff()
        .context_radius(3)
        .header("old", "new")
        .to_string();

    NoteDiff {
        old_title: old_title.to_owned(),
        new_title: new_title.to_owned(),
        escaped_old_title,
        escaped_new_title,
        title_changed,
        title_spans,
        additions,
        deletions,
        hunks,
        unified_diff,
    }
}
