use chrono::{DateTime, Utc};

use crate::error::CoreError;
use crate::markdown::frontmatter::parse_frontmatter;
use crate::markdown::headline::extract_headline;

pub fn validate_note_not_blank(title: Option<&str>, content: Option<&str>) -> Result<(), CoreError> {
    let title_blank = title.unwrap_or("").trim().is_empty();
    let content_blank = content.unwrap_or("").trim().is_empty();
    if title_blank && content_blank {
        return Err(CoreError::BlankNote);
    }
    Ok(())
}

pub fn sanitize_title(raw: &str) -> String {
    let no_newlines = raw.replace("\r\n", " ").replace(['\n', '\r'], " ");
    let mut out = String::with_capacity(no_newlines.len());
    let mut prev_space = false;
    for ch in no_newlines.chars() {
        if ch.is_whitespace() {
            if !prev_space && !out.is_empty() {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out.trim().to_owned()
}

fn extract_first_line_headline(content: &str, locked: bool) -> String {
    if locked {
        return String::new();
    }
    let (_, body) = parse_frontmatter(content);
    let mut in_fence: Option<(char, usize)> = None;

    for raw_line in body.lines() {
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
        if in_fence.is_some() {
            continue;
        }
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || is_thematic_break(trimmed) {
            continue;
        }
        let plain = extract_headline(trimmed, false);
        if !plain.is_empty() {
            return plain.chars().take(64).collect::<String>().trim().to_owned();
        }
    }
    String::new()
}

pub(crate) fn detect_fence_marker(trimmed_start: &str) -> Option<(char, usize)> {
    let first = trimmed_start.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let count = trimmed_start.chars().take_while(|&c| c == first).count();
    if count >= 3 {
        Some((first, count))
    } else {
        None
    }
}

fn is_thematic_break(trimmed: &str) -> bool {
    let no_spaces: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    if no_spaces.len() < 3 {
        return false;
    }
    no_spaces.chars().all(|c| c == '-')
        || no_spaces.chars().all(|c| c == '*')
        || no_spaces.chars().all(|c| c == '_')
}

pub fn resolve_note_title(
    explicit_title: Option<&str>,
    content: &str,
    locked: bool,
    title_format: Option<&str>,
    now: DateTime<Utc>,
    existing_note_count: usize,
) -> (String, bool) {
    if let Some(raw_title) = explicit_title {
        let clean = sanitize_title(raw_title);
        if !clean.is_empty() {
            return (clean, false);
        }
    }

    let template = title_format
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .unwrap_or("$headline$");

    let first_line = extract_first_line_headline(content, locked);
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%H:%M").to_string();
    let count_str = (existing_note_count + 1).to_string();

    let substituted = template
        .replace("$headline$", &first_line)
        .replace("$date$", &date_str)
        .replace("$time$", &time_str)
        .replace("$count$", &count_str);

    let clean_generated = sanitize_title(&substituted);
    if !clean_generated.is_empty() {
        (clean_generated, true)
    } else {
        (format!("Note {}", now.format("%Y-%m-%d %H:%M")), true)
    }
}
