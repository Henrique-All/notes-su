use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::markdown::frontmatter::parse_frontmatter;
use crate::markdown::title::detect_fence_marker;

pub const MAX_HEADLINE_CHARS: usize = 350;

pub fn extract_headline(markdown: &str, locked: bool) -> String {
    if locked {
        return String::new();
    }

    let (_, body) = parse_frontmatter(markdown);
    let expanded = replace_wikilinks_outside_fences(body);

    let opts = Options::ENABLE_GFM
        | Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_MATH;

    let mut raw_buf = String::with_capacity(expanded.len().min(1024));
    let mut image_depth: usize = 0;

    for event in Parser::new_ext(&expanded, opts) {
        match event {
            Event::Start(Tag::Image { .. }) => {
                image_depth += 1;
            }
            Event::End(TagEnd::Image) => {
                image_depth = image_depth.saturating_sub(1);
            }
            _ if image_depth > 0 => {}
            Event::Text(t) | Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t) => {
                raw_buf.push_str(&t);
            }
            Event::SoftBreak
            | Event::HardBreak
            | Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::Item
                | TagEnd::TableCell
                | TagEnd::CodeBlock
                | TagEnd::BlockQuote(_),
            ) => {
                raw_buf.push(' ');
            }
            Event::Html(_) | Event::InlineHtml(_) => {}
            _ => {}
        }
    }

    let mut collapsed = String::with_capacity(raw_buf.len().min(MAX_HEADLINE_CHARS + 16));
    let mut prev_ws = false;
    let mut char_count = 0usize;

    for ch in raw_buf.chars() {
        if ch.is_whitespace() {
            if !prev_ws && !collapsed.is_empty() {
                prev_ws = true;
            }
        } else {
            if prev_ws {
                if char_count + 1 >= MAX_HEADLINE_CHARS {
                    break;
                }
                collapsed.push(' ');
                char_count += 1;
                prev_ws = false;
            }
            if char_count >= MAX_HEADLINE_CHARS {
                break;
            }
            collapsed.push(ch);
            char_count += 1;
        }
    }

    collapsed.trim().to_owned()
}

pub(crate) fn replace_wikilinks_outside_fences(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut in_fence: Option<(char, usize)> = None;

    for chunk in body.split_inclusive('\n') {
        let line_no_nl = chunk.trim_end_matches(['\r', '\n']);
        let trimmed_start = line_no_nl.trim_start();
        if let Some((fence_ch, fence_len)) = detect_fence_marker(trimmed_start) {
            match in_fence {
                None => {
                    in_fence = Some((fence_ch, fence_len));
                    out.push_str(chunk);
                    continue;
                }
                Some((open_ch, open_len))
                    if open_ch == fence_ch
                        && fence_len >= open_len
                        && trimmed_start[fence_len..].trim().is_empty() =>
                {
                    in_fence = None;
                    out.push_str(chunk);
                    continue;
                }
                _ => {}
            }
        }
        if in_fence.is_some() {
            out.push_str(chunk);
        } else {
            out.push_str(&replace_wikilinks_in_line(chunk));
        }
    }
    out
}

fn replace_wikilinks_in_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'`' {
            let tick_start = i;
            while i < bytes.len() && bytes[i] == b'`' {
                i += 1;
            }
            let tick_len = i - tick_start;
            out.push_str(&line[tick_start..i]);
            while i < bytes.len() {
                if bytes[i] == b'`' {
                    let close_start = i;
                    while i < bytes.len() && bytes[i] == b'`' {
                        i += 1;
                    }
                    out.push_str(&line[close_start..i]);
                    if i - close_start == tick_len {
                        break;
                    }
                } else {
                    let ch = line[i..].chars().next().unwrap();
                    out.push(ch);
                    i += ch.len_utf8();
                }
            }
            continue;
        }

        if i + 1 < bytes.len() && bytes[i] == b'[' && bytes[i + 1] == b'['
            && let Some(rel_end) = line[i + 2..].find("]]")
        {
            let inner = &line[i + 2..i + 2 + rel_end];
            if !inner.is_empty() && !inner.contains('\n') {
                let display = if let Some((_, label)) = inner.split_once('|') {
                    label.trim()
                } else {
                    inner
                        .split(['#', '?'])
                        .next()
                        .unwrap_or(inner)
                        .trim()
                };
                out.push_str(display);
                i = i + 2 + rel_end + 2;
                continue;
            }
        }

        let ch = line[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}
