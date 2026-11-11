use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::error::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct NoteFrontmatter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_frontmatter_tags"
    )]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favorite: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readonly: Option<bool>,
}

fn deserialize_frontmatter_tags<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let val = Option::<Value>::deserialize(deserializer)?;
    match val {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(s
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .collect()),
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                if let Some(s) = item.as_str() {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        out.push(trimmed.to_owned());
                    }
                }
            }
            Ok(out)
        }
        _ => Ok(Vec::new()),
    }
}

pub fn parse_frontmatter(markdown: &str) -> (Option<NoteFrontmatter>, &str) {
    let working = markdown.strip_prefix('\u{feff}').unwrap_or(markdown);

    let after_open = if let Some(rest) = working.strip_prefix("---\r\n") {
        rest
    } else if let Some(rest) = working.strip_prefix("---\n") {
        rest
    } else {
        return (None, markdown);
    };

    let mut search_from = 0usize;
    while let Some(rel_pos) = after_open[search_from..].find("\n---") {
        let dash_pos = search_from + rel_pos + 1; // points to first '-' of closing '---'
        let yaml_end = if dash_pos > 0 && after_open.as_bytes()[dash_pos - 1] == b'\n' {
            if dash_pos >= 2 && after_open.as_bytes()[dash_pos - 2] == b'\r' {
                dash_pos - 2
            } else {
                dash_pos - 1
            }
        } else {
            dash_pos
        };

        let after_close_dashes = &after_open[dash_pos + 3..];
        let body_opt = if after_close_dashes.is_empty() {
            Some("")
        } else if let Some(rest) = after_close_dashes.strip_prefix("\r\n") {
            Some(rest)
        } else {
            after_close_dashes.strip_prefix('\n')
        };

        if let Some(body) = body_opt {
            let yaml_slice = &after_open[..yaml_end];
            if let Ok(meta) = serde_yaml_ng::from_str::<NoteFrontmatter>(yaml_slice) {
                let trimmed_body = body.strip_prefix("\r\n").or_else(|| body.strip_prefix('\n')).unwrap_or(body);
                return (Some(meta), trimmed_body);
            }
            return (None, markdown);
        }

        search_from = dash_pos + 3;
    }

    (None, markdown)
}

pub fn export_markdown_with_frontmatter(
    meta: &NoteFrontmatter,
    body: &str,
    locked: bool,
) -> Result<String, CoreError> {
    if locked {
        return Err(CoreError::NoteLocked);
    }

    let yaml = serde_yaml_ng::to_string(meta).map_err(|e| CoreError::Yaml(e.to_string()))?;
    let clean_yaml = yaml.strip_prefix("---\n").unwrap_or(&yaml).trim_end();
    if clean_yaml.is_empty() || clean_yaml == "{}" {
        Ok(format!("---\n---\n\n{body}"))
    } else {
        Ok(format!("---\n{clean_yaml}\n---\n\n{body}"))
    }
}
