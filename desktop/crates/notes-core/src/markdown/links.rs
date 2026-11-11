use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::markdown::title::detect_fence_marker;

static MD_INTERNAL_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[([^\]]*)\]\((nn://note/[^\s\)]+)\)")
        .expect("valid markdown internal link regex")
});

static BARE_INTERNAL_LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<?(nn://note/[A-Za-z0-9_-]+(?:\?[^\s>\)\]]+)?)>?")
        .expect("valid bare internal link regex")
});

static WIKILINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[\[([^\[\]\n]+)\]\]").expect("valid wikilink regex")
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InternalNoteLink {
    pub note_id: String,
    #[serde(default)]
    pub block_id: Option<String>,
    pub label: String,
    pub snippet: String,
    pub line: usize,
}

#[derive(Debug)]
struct RawMatchedLink {
    byte_start: usize,
    byte_end: usize,
    note_id: String,
    block_id: Option<String>,
    label: String,
}

pub fn extract_internal_links(markdown: &str, locked: bool) -> Vec<InternalNoteLink> {
    if locked || markdown.is_empty() {
        return Vec::new();
    }

    let masked = mask_code_fences_and_inline_code(markdown);
    let mut raw_matches: Vec<RawMatchedLink> = Vec::new();
    let mut covered_ranges: Vec<Range<usize>> = Vec::new();

    for caps in MD_INTERNAL_LINK_RE.captures_iter(&masked) {
        let Some(full) = caps.get(0) else { continue };
        let label_raw = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let url_raw = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        if let Some((note_id, block_id)) = parse_nn_note_url(url_raw) {
            let label = if label_raw.trim().is_empty() {
                note_id.clone()
            } else {
                label_raw.trim().to_owned()
            };
            covered_ranges.push(full.range());
            raw_matches.push(RawMatchedLink {
                byte_start: full.start(),
                byte_end: full.end(),
                note_id,
                block_id,
                label,
            });
        }
    }

    for caps in BARE_INTERNAL_LINK_RE.captures_iter(&masked) {
        let Some(full) = caps.get(0) else { continue };
        if covered_ranges
            .iter()
            .any(|r| full.start() < r.end && full.end() > r.start)
        {
            continue;
        }
        let url_raw = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        if let Some((note_id, block_id)) = parse_nn_note_url(url_raw) {
            let label = note_id.clone();
            covered_ranges.push(full.range());
            raw_matches.push(RawMatchedLink {
                byte_start: full.start(),
                byte_end: full.end(),
                note_id,
                block_id,
                label,
            });
        }
    }

    for caps in WIKILINK_RE.captures_iter(&masked) {
        let Some(full) = caps.get(0) else { continue };
        if covered_ranges
            .iter()
            .any(|r| full.start() < r.end && full.end() > r.start)
        {
            continue;
        }
        let inner = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        if let Some((note_id, block_id, label)) = parse_wikilink_inner(inner) {
            covered_ranges.push(full.range());
            raw_matches.push(RawMatchedLink {
                byte_start: full.start(),
                byte_end: full.end(),
                note_id,
                block_id,
                label,
            });
        }
    }

    raw_matches.sort_by_key(|m| m.byte_start);

    raw_matches
        .into_iter()
        .map(|m| {
            let line = 1 + markdown[..m.byte_start]
                .bytes()
                .filter(|&b| b == b'\n')
                .count();
            let snippet = build_line_snippet(markdown, m.byte_start, m.byte_end);
            InternalNoteLink {
                note_id: m.note_id,
                block_id: m.block_id,
                label: m.label,
                snippet,
                line,
            }
        })
        .collect()
}

fn parse_nn_note_url(raw_url: &str) -> Option<(String, Option<String>)> {
    let parsed = Url::parse(raw_url).ok()?;
    if parsed.scheme() != "nn" || parsed.host_str() != Some("note") {
        return None;
    }
    let note_id = parsed.path().trim_matches('/').trim().to_owned();
    if note_id.is_empty() {
        return None;
    }
    let mut block_id = parsed
        .query_pairs()
        .find(|(k, _)| k == "blockId" || k == "block_id")
        .map(|(_, v)| v.into_owned())
        .filter(|v| !v.is_empty());

    if block_id.is_none()
        && let Some(frag) = parsed.fragment()
        && !frag.trim().is_empty()
    {
        block_id = Some(frag.trim().to_owned());
    }

    Some((note_id, block_id))
}

fn parse_wikilink_inner(inner: &str) -> Option<(String, Option<String>, String)> {
    let (target_raw, explicit_label) = match inner.split_once('|') {
        Some((t, l)) => (t.trim(), Some(l.trim())),
        None => (inner.trim(), None),
    };
    if target_raw.is_empty() {
        return None;
    }

    let (note_id_str, block_id) = if let Some((id_part, query_part)) = target_raw.split_once('?') {
        let bid = Url::parse(&format!("nn://note/x?{query_part}"))
            .ok()
            .and_then(|u| {
                u.query_pairs()
                    .find(|(k, _)| k == "blockId" || k == "block_id")
                    .map(|(_, v)| v.into_owned())
            })
            .filter(|v| !v.is_empty());
        (id_part.trim(), bid)
    } else if let Some((id_part, hash_part)) = target_raw.split_once('#') {
        let bid = (!hash_part.trim().is_empty()).then(|| hash_part.trim().to_owned());
        (id_part.trim(), bid)
    } else {
        (target_raw, None)
    };

    if note_id_str.is_empty() {
        return None;
    }

    let label = explicit_label
        .filter(|l| !l.is_empty())
        .unwrap_or(note_id_str)
        .to_owned();

    Some((note_id_str.to_owned(), block_id, label))
}

fn build_line_snippet(markdown: &str, byte_start: usize, byte_end: usize) -> String {
    let line_start = markdown[..byte_start]
        .rfind('\n')
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let line_end = markdown[byte_end..]
        .find('\n')
        .map(|rel| byte_end + rel)
        .unwrap_or(markdown.len());

    let raw_line = markdown[line_start..line_end].trim_end_matches('\r').trim();
    let readable = MD_INTERNAL_LINK_RE
        .replace_all(raw_line, "$1")
        .into_owned();
    let readable = WIKILINK_RE
        .replace_all(&readable, |caps: &regex::Captures| {
            let inner = &caps[1];
            if let Some((_, label)) = inner.split_once('|') {
                label.trim().to_owned()
            } else {
                inner.split(['#', '?']).next().unwrap_or(inner).trim().to_owned()
            }
        })
        .into_owned();

    let chars: Vec<char> = readable.chars().collect();
    if chars.len() <= 120 {
        readable
    } else {
        let mut truncated: String = chars.into_iter().take(120).collect();
        truncated.push('…');
        truncated
    }
}

/// Replaces fenced code block contents and inline code spans with ASCII spaces
/// of the exact same byte length (preserving `\n` and `\r`) so regex matches
/// never trigger inside code while byte offsets and 1-based line numbers remain exact.
fn mask_code_fences_and_inline_code(markdown: &str) -> String {
    let mut bytes = markdown.as_bytes().to_vec();
    let mut in_fence: Option<(char, usize)> = None;

    let mut line_start = 0usize;
    while line_start < markdown.len() {
        let line_end = markdown[line_start..]
            .find('\n')
            .map(|rel| line_start + rel + 1)
            .unwrap_or(markdown.len());
        let line_slice = &markdown[line_start..line_end];
        let line_no_nl = line_slice.trim_end_matches(['\r', '\n']);
        let trimmed_start = line_no_nl.trim_start();

        if let Some((fence_ch, fence_len)) = detect_fence_marker(trimmed_start) {
            match in_fence {
                None => {
                    in_fence = Some((fence_ch, fence_len));
                    mask_non_newline_bytes(&mut bytes[line_start..line_end]);
                    line_start = line_end;
                    continue;
                }
                Some((open_ch, open_len))
                    if open_ch == fence_ch
                        && fence_len >= open_len
                        && trimmed_start[fence_len..].trim().is_empty() =>
                {
                    in_fence = None;
                    mask_non_newline_bytes(&mut bytes[line_start..line_end]);
                    line_start = line_end;
                    continue;
                }
                _ => {}
            }
        }

        if in_fence.is_some() {
            mask_non_newline_bytes(&mut bytes[line_start..line_end]);
        } else {
            mask_inline_code_in_line(&mut bytes[line_start..line_end]);
        }
        line_start = line_end;
    }

    String::from_utf8(bytes).unwrap_or_else(|_| markdown.to_owned())
}

fn mask_non_newline_bytes(slice: &mut [u8]) {
    for b in slice.iter_mut() {
        if *b != b'\n' && *b != b'\r' {
            *b = b' ';
        }
    }
}

fn mask_inline_code_in_line(slice: &mut [u8]) {
    let mut i = 0;
    while i < slice.len() {
        if slice[i] == b'`' {
            let open_start = i;
            while i < slice.len() && slice[i] == b'`' {
                i += 1;
            }
            let tick_len = i - open_start;
            let mut j = i;
            let mut closed_at = None;
            while j < slice.len() {
                if slice[j] == b'`' {
                    let close_start = j;
                    while j < slice.len() && slice[j] == b'`' {
                        j += 1;
                    }
                    if j - close_start == tick_len {
                        closed_at = Some(j);
                        break;
                    }
                } else {
                    j += 1;
                }
            }
            if let Some(close_end) = closed_at {
                mask_non_newline_bytes(&mut slice[open_start..close_end]);
                i = close_end;
            }
        } else {
            i += 1;
        }
    }
}
