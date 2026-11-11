use std::collections::HashMap;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

use crate::markdown::frontmatter::parse_frontmatter;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TocHeading {
    pub level: u8,
    pub text: String,
    pub slug: String,
    pub line: usize,
}

pub fn extract_toc(markdown: &str) -> Vec<TocHeading> {
    let (_, body) = parse_frontmatter(markdown);
    let body_offset = markdown.len().saturating_sub(body.len());
    let line_offset = markdown[..body_offset]
        .bytes()
        .filter(|&b| b == b'\n')
        .count();

    let opts = Options::ENABLE_GFM | Options::ENABLE_HEADING_ATTRIBUTES | Options::ENABLE_MATH;
    let mut headings = Vec::new();
    let mut slug_counts: HashMap<String, usize> = HashMap::new();

    let mut current_level: Option<u8> = None;
    let mut current_line: usize = 1;
    let mut current_text = String::new();

    for (event, range) in Parser::new_ext(body, opts).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current_level = Some(heading_level_to_u8(level));
                let safe_start = range.start.min(body.len());
                current_line = line_offset
                    + 1
                    + body[..safe_start].bytes().filter(|&b| b == b'\n').count();
                current_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = current_level.take() {
                    let trimmed = current_text.trim();
                    if !trimmed.is_empty() {
                        let base_slug = slugify_heading(trimmed);
                        let count = slug_counts.entry(base_slug.clone()).or_insert(0);
                        let slug = if *count == 0 {
                            base_slug
                        } else {
                            format!("{base_slug}-{count}")
                        };
                        *count += 1;

                        headings.push(TocHeading {
                            level,
                            text: trimmed.to_owned(),
                            slug,
                            line: current_line,
                        });
                    }
                }
                current_text.clear();
            }
            Event::Text(t) | Event::Code(t) | Event::InlineMath(t) if current_level.is_some() => {
                current_text.push_str(&t);
            }
            Event::SoftBreak | Event::HardBreak if current_level.is_some() => {
                current_text.push(' ');
            }
            _ => {}
        }
    }

    headings
}

fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

pub fn slugify_heading(text: &str) -> String {
    let mut slug = String::with_capacity(text.len());
    let mut prev_dash = false;

    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for lc in ch.to_lowercase() {
                slug.push(lc);
            }
            prev_dash = false;
        } else if (ch.is_whitespace() || ch == '-' || ch == '_') && !slug.is_empty() && !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }

    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "section".to_owned()
    } else {
        trimmed.to_owned()
    }
}
