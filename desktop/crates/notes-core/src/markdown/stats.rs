use serde::{Deserialize, Serialize};

use crate::markdown::title::detect_fence_marker;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct NoteStats {
    pub words: usize,
    pub characters: usize,
    pub characters_no_spaces: usize,
    pub spaces: usize,
    pub paragraphs: usize,
    pub reading_time_minutes: usize,
}

pub fn compute_stats(markdown: &str) -> NoteStats {
    let characters = markdown.chars().count();
    let spaces = markdown.chars().filter(|c| c.is_whitespace()).count();
    let characters_no_spaces = characters - spaces;

    let mut paragraphs = 0usize;
    let mut in_paragraph = false;
    for line in markdown.lines() {
        if line.trim().is_empty() {
            in_paragraph = false;
        } else if !in_paragraph {
            paragraphs += 1;
            in_paragraph = true;
        }
    }

    let mut words = 0usize;
    let mut in_fence: Option<(char, usize)> = None;

    for raw_line in markdown.lines() {
        let trimmed_start = raw_line.trim_start();
        if let Some((fence_ch, fence_len)) = detect_fence_marker(trimmed_start) {
            match in_fence {
                None => {
                    in_fence = Some((fence_ch, fence_len));
                    continue;
                }
                Some((open_ch, open_len))
                    if open_ch == fence_ch
                        && fence_len >= open_len
                        && trimmed_start[fence_len..].trim().is_empty() =>
                {
                    in_fence = None;
                    continue;
                }
                _ => {}
            }
        }

        let effective_line = if in_fence.is_none() {
            strip_task_prefix(trimmed_start)
        } else {
            raw_line
        };

        for segment in effective_line.split(|c: char| c.is_whitespace() || c == '|') {
            if segment.chars().any(char::is_alphanumeric) {
                words += 1;
            }
        }
    }

    let reading_time_minutes = if words == 0 { 0 } else { words.div_ceil(200) };

    NoteStats {
        words,
        characters,
        characters_no_spaces,
        spaces,
        paragraphs,
        reading_time_minutes,
    }
}

fn strip_task_prefix(trimmed_start: &str) -> &str {
    let mut chars = trimmed_start.chars();
    let Some(bullet) = chars.next() else {
        return trimmed_start;
    };
    if !matches!(bullet, '-' | '*' | '+') {
        return trimmed_start;
    }
    let after_bullet = chars.as_str();
    let after_ws = after_bullet.trim_start_matches([' ', '\t']);
    if after_ws.len() == after_bullet.len() {
        return trimmed_start;
    }
    if let Some(rest) = after_ws
        .strip_prefix("[ ]")
        .or_else(|| after_ws.strip_prefix("[x]"))
        .or_else(|| after_ws.strip_prefix("[X]"))
        && (rest.is_empty() || rest.starts_with([' ', '\t']))
    {
        return rest.trim_start_matches([' ', '\t']);
    }
    trimmed_start
}
