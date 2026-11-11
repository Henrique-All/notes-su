use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag, TagEnd, html};

use crate::error::CoreError;
use crate::markdown::frontmatter::parse_frontmatter;
use crate::markdown::headline::replace_wikilinks_outside_fences;

pub fn escape_html(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() + 16);
    for ch in s.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn is_allowed_attribute_free_html_tag(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "<u>"
            | "</u>"
            | "<mark>"
            | "</mark>"
            | "<sub>"
            | "</sub>"
            | "<sup>"
            | "</sup>"
            | "<kbd>"
            | "</kbd>"
            | "<br>"
            | "<br/>"
            | "<br />"
    )
}

fn classify_dangerous_container_tag(raw: &str) -> Option<(bool, &'static str)> {
    let trimmed = raw.trim().strip_prefix('<')?.trim_start();
    let (is_closing, rest) = if let Some(after_slash) = trimmed.strip_prefix('/') {
        (true, after_slash.trim_start())
    } else {
        (false, trimmed)
    };
    let self_closing = rest.trim_end_matches('>').trim_end().ends_with('/');
    if self_closing && !is_closing {
        return None;
    }
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let matched = match name.as_str() {
        "script" => "script",
        "style" => "style",
        "iframe" => "iframe",
        "object" => "object",
        "embed" => "embed",
        "svg" => "svg",
        "template" => "template",
        _ => return None,
    };
    // If block HTML already contains both opening and closing tag on the same chunk, depth stays 0
    if !is_closing && raw.to_ascii_lowercase().contains(&format!("</{matched}")) {
        return None;
    }
    Some((is_closing, matched))
}

pub fn is_safe_export_url(raw_url: &str) -> bool {
    let cleaned: String = raw_url
        .chars()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .collect();

    if cleaned.is_empty() || cleaned.starts_with('#') {
        return true;
    }

    let lower = cleaned.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("nn://note/")
        || lower.starts_with("attachment://")
    {
        return true;
    }

    if lower.starts_with("data:image/png;base64,")
        || lower.starts_with("data:image/jpeg;base64,")
        || lower.starts_with("data:image/jpg;base64,")
        || lower.starts_with("data:image/gif;base64,")
        || lower.starts_with("data:image/webp;base64,")
    {
        return true;
    }

    !lower.contains(':') && !lower.starts_with("//")
}

fn sanitize_code_fence_info(info: &str) -> String {
    let lang = info.split_whitespace().next().unwrap_or("");
    lang.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+'))
        .collect()
}

pub fn export_xss_safe_html(
    title: &str,
    headline: Option<&str>,
    markdown: &str,
    locked: bool,
) -> Result<String, CoreError> {
    if locked {
        return Err(CoreError::NoteLocked);
    }

    let escaped_title = escape_html(title);
    let escaped_headline = headline.map(escape_html);

    let (_, body_md) = parse_frontmatter(markdown);
    let opts = Options::ENABLE_GFM
        | Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_MATH;

    let mut dangerous_depth: usize = 0;
    let mut sanitized_events: Vec<Event<'_>> = Vec::new();

    for event in Parser::new_ext(body_md, opts) {
        match event {
            Event::Html(raw) | Event::InlineHtml(raw) => {
                if let Some((is_closing, _)) = classify_dangerous_container_tag(&raw) {
                    if is_closing {
                        dangerous_depth = dangerous_depth.saturating_sub(1);
                    } else {
                        dangerous_depth += 1;
                    }
                    continue;
                }
                if dangerous_depth == 0 && is_allowed_attribute_free_html_tag(&raw) {
                    sanitized_events.push(Event::InlineHtml(raw));
                }
            }
            _ if dangerous_depth > 0 => {}
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let safe_url: CowStr<'_> = if is_safe_export_url(&dest_url) {
                    dest_url
                } else {
                    CowStr::Borrowed("#blocked")
                };
                sanitized_events.push(Event::Start(Tag::Link {
                    link_type,
                    dest_url: safe_url,
                    title,
                    id,
                }));
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let safe_url: CowStr<'_> = if is_safe_export_url(&dest_url) {
                    dest_url
                } else {
                    CowStr::Borrowed("")
                };
                sanitized_events.push(Event::Start(Tag::Image {
                    link_type,
                    dest_url: safe_url,
                    title,
                    id,
                }));
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                let safe_info = sanitize_code_fence_info(&info);
                sanitized_events.push(Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(
                    CowStr::Boxed(safe_info.into_boxed_str()),
                ))));
            }
            other => sanitized_events.push(other),
        }
    }

    let mut rendered_body = String::with_capacity(body_md.len() + 256);
    html::push_html(&mut rendered_body, sanitized_events.into_iter());

    let meta_desc = escaped_headline
        .filter(|h| !h.is_empty())
        .map(|h| format!("\n  <meta name=\"description\" content=\"{h}\">"))
        .unwrap_or_default();

    Ok(format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data: https: http:; style-src 'unsafe-inline';\">{meta_desc}\n  <title>{escaped_title}</title>\n</head>\n<body>\n  <article>\n    <h1>{escaped_title}</h1>\n    {rendered_body}\n  </article>\n</body>\n</html>\n"
    ))
}

pub fn export_plain_text(title: &str, markdown: &str, locked: bool) -> Result<String, CoreError> {
    if locked {
        return Err(CoreError::NoteLocked);
    }

    let (_, body_md) = parse_frontmatter(markdown);
    let expanded = replace_wikilinks_outside_fences(body_md);

    let opts = Options::ENABLE_GFM
        | Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_MATH;

    let mut out = String::with_capacity(title.len() + expanded.len() + 16);
    if !title.trim().is_empty() {
        out.push_str(title.trim());
        out.push_str("\n\n");
    }

    for event in Parser::new_ext(&expanded, opts) {
        match event {
            Event::TaskListMarker(checked) => {
                out.push_str(if checked { "✅ " } else { "☐ " });
            }
            Event::Text(t) | Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t) => {
                out.push_str(&t);
            }
            Event::SoftBreak | Event::HardBreak => {
                out.push('\n');
            }
            Event::End(TagEnd::TableCell) => {
                out.push('\t');
            }
            Event::End(TagEnd::TableRow | TagEnd::TableHead | TagEnd::Item) => {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::CodeBlock
                | TagEnd::BlockQuote(_)
                | TagEnd::Table,
            ) => {
                if !out.ends_with("\n\n") {
                    if out.ends_with('\n') {
                        out.push('\n');
                    } else {
                        out.push_str("\n\n");
                    }
                }
            }
            Event::Rule => {
                out.push_str("---\n\n");
            }
            _ => {}
        }
    }

    Ok(out.trim_end().to_owned())
}
