use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::markdown::title::detect_fence_marker;

static TASK_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([ \t]*)([-*+])([ \t]+\[)([ xX])(\](?:[ \t]+(.*))?)$")
        .expect("valid task line regex")
});

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskItem {
    pub index: usize,
    pub line: usize,
    pub indent: usize,
    pub depth: usize,
    pub bullet: char,
    pub checked: bool,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct TaskCounts {
    pub total: usize,
    pub completed: usize,
}

#[derive(Debug, Clone)]
struct ParsedTaskLine {
    indent: usize,
    bullet: char,
    checked: bool,
    text: String,
}

fn compute_indent(ws: &str) -> usize {
    ws.chars().map(|c| if c == '\t' { 2 } else { 1 }).sum()
}

fn parse_task_line(line: &str) -> Option<ParsedTaskLine> {
    let caps = TASK_LINE_RE.captures(line)?;
    let indent_str = caps.get(1)?.as_str();
    let bullet = caps.get(2)?.as_str().chars().next()?;
    let state = caps.get(4)?.as_str();
    let text = caps.get(6).map(|m| m.as_str()).unwrap_or("").to_owned();
    let indent = compute_indent(indent_str);
    Some(ParsedTaskLine {
        indent,
        bullet,
        checked: state.eq_ignore_ascii_case("x"),
        text,
    })
}

fn toggle_single_task_line(line: &str) -> Option<String> {
    let caps = TASK_LINE_RE.captures(line)?;
    let indent = &caps[1];
    let bullet = &caps[2];
    let mid = &caps[3];
    let state = &caps[4];
    let tail = &caps[5];
    let next_state = if state.eq_ignore_ascii_case("x") {
        " "
    } else {
        "x"
    };
    Some(format!("{indent}{bullet}{mid}{next_state}{tail}"))
}

fn split_lines_preserving_endings(markdown: &str) -> (Vec<String>, bool) {
    let trailing_newline = markdown.ends_with('\n');
    let lines: Vec<String> = markdown
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_owned())
        .collect();
    // If trailing_newline is true, split('\n') produces a trailing "" element.
    let mut clean_lines = lines;
    if trailing_newline && clean_lines.last().is_some_and(String::is_empty) {
        clean_lines.pop();
    }
    (clean_lines, trailing_newline)
}

fn join_lines_preserving_endings(lines: &[String], trailing_newline: bool) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut joined = lines.join("\n");
    if trailing_newline {
        joined.push('\n');
    }
    joined
}

fn compute_non_fenced_mask(lines: &[String]) -> Vec<bool> {
    let mut mask = Vec::with_capacity(lines.len());
    let mut in_fence: Option<(char, usize)> = None;

    for line in lines {
        let trimmed_start = line.trim_start();
        if let Some((fence_ch, fence_len)) = detect_fence_marker(trimmed_start) {
            match in_fence {
                None => {
                    in_fence = Some((fence_ch, fence_len));
                    mask.push(false);
                    continue;
                }
                Some((open_ch, open_len))
                    if open_ch == fence_ch
                        && fence_len >= open_len
                        && trimmed_start[fence_len..].trim().is_empty() =>
                {
                    in_fence = None;
                    mask.push(false);
                    continue;
                }
                _ => {}
            }
        }
        mask.push(in_fence.is_none());
    }
    mask
}

pub fn parse_tasks(markdown: &str) -> Vec<TaskItem> {
    let (lines, _) = split_lines_preserving_endings(markdown);
    let non_fenced = compute_non_fenced_mask(&lines);
    let mut items = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        if !non_fenced[i] {
            continue;
        }
        if let Some(parsed) = parse_task_line(line) {
            items.push(TaskItem {
                index: items.len(),
                line: i + 1,
                indent: parsed.indent,
                depth: parsed.indent / 2,
                bullet: parsed.bullet,
                checked: parsed.checked,
                text: parsed.text,
            });
        }
    }

    items
}

pub fn task_counts(markdown: &str) -> TaskCounts {
    let tasks = parse_tasks(markdown);
    let completed = tasks.iter().filter(|t| t.checked).count();
    TaskCounts {
        total: tasks.len(),
        completed,
    }
}

pub fn toggle_task_at_line(markdown: &str, line_1_based: usize) -> Result<String, CoreError> {
    if line_1_based == 0 {
        return Err(CoreError::TaskLineOutOfRange(0));
    }
    let (mut lines, trailing_nl) = split_lines_preserving_endings(markdown);
    let idx = line_1_based - 1;
    if idx >= lines.len() {
        return Err(CoreError::TaskLineOutOfRange(line_1_based));
    }
    let non_fenced = compute_non_fenced_mask(&lines);
    if !non_fenced[idx] {
        return Err(CoreError::TaskLineOutOfRange(line_1_based));
    }
    let Some(updated) = toggle_single_task_line(&lines[idx]) else {
        return Err(CoreError::TaskLineOutOfRange(line_1_based));
    };
    lines[idx] = updated;
    Ok(join_lines_preserving_endings(&lines, trailing_nl))
}

pub fn toggle_task_by_index(markdown: &str, task_index: usize) -> Result<String, CoreError> {
    let tasks = parse_tasks(markdown);
    let Some(target) = tasks.get(task_index) else {
        return Err(CoreError::TaskIndexOutOfRange(task_index));
    };
    toggle_task_at_line(markdown, target.line)
}

#[derive(Debug, Clone)]
struct TaskTreeNode {
    indent: usize,
    checked: bool,
    raw_line: String,
    children: Vec<TaskTreeNode>,
}

fn build_task_forest(task_lines: &[(ParsedTaskLine, String)]) -> Vec<TaskTreeNode> {
    let mut roots: Vec<TaskTreeNode> = Vec::new();
    for (parsed, raw_line) in task_lines {
        let node = TaskTreeNode {
            indent: parsed.indent,
            checked: parsed.checked,
            raw_line: raw_line.clone(),
            children: Vec::new(),
        };
        insert_task_node(&mut roots, node);
    }
    roots
}

fn insert_task_node(siblings: &mut Vec<TaskTreeNode>, node: TaskTreeNode) {
    if let Some(last) = siblings.last_mut()
        && node.indent > last.indent
    {
        insert_task_node(&mut last.children, node);
        return;
    }
    siblings.push(node);
}

fn filter_completed_nodes(nodes: Vec<TaskTreeNode>) -> Vec<TaskTreeNode> {
    nodes
        .into_iter()
        .filter(|n| !n.checked)
        .map(|mut n| {
            n.children = filter_completed_nodes(n.children);
            n
        })
        .collect()
}

fn sort_completed_nodes(nodes: Vec<TaskTreeNode>) -> Vec<TaskTreeNode> {
    let mut unchecked = Vec::new();
    let mut checked = Vec::new();

    for mut node in nodes {
        node.children = sort_completed_nodes(node.children);
        if node.checked {
            checked.push(node);
        } else {
            unchecked.push(node);
        }
    }

    unchecked.extend(checked);
    unchecked
}

fn flatten_task_forest(nodes: &[TaskTreeNode], out: &mut Vec<String>) {
    for node in nodes {
        out.push(node.raw_line.clone());
        flatten_task_forest(&node.children, out);
    }
}

fn transform_task_blocks<F>(markdown: &str, mut transform: F) -> String
where
    F: FnMut(Vec<TaskTreeNode>) -> Vec<TaskTreeNode>,
{
    if markdown.is_empty() {
        return String::new();
    }
    let (lines, trailing_nl) = split_lines_preserving_endings(markdown);
    let non_fenced = compute_non_fenced_mask(&lines);

    let mut out_lines: Vec<String> = Vec::with_capacity(lines.len());
    let mut current_block: Vec<(ParsedTaskLine, String)> = Vec::new();

    for (idx, line) in lines.iter().enumerate() {
        if non_fenced[idx]
            && let Some(parsed) = parse_task_line(line)
        {
            current_block.push((parsed, line.clone()));
            continue;
        }

        if !current_block.is_empty() {
            let forest = build_task_forest(&current_block);
            let transformed = transform(forest);
            flatten_task_forest(&transformed, &mut out_lines);
            current_block.clear();
        }
        out_lines.push(line.clone());
    }

    if !current_block.is_empty() {
        let forest = build_task_forest(&current_block);
        let transformed = transform(forest);
        flatten_task_forest(&transformed, &mut out_lines);
    }

    join_lines_preserving_endings(&out_lines, trailing_nl)
}

pub fn clear_completed_tasks(markdown: &str) -> String {
    transform_task_blocks(markdown, filter_completed_nodes)
}

pub fn sort_completed_tasks_to_bottom(markdown: &str) -> String {
    transform_task_blocks(markdown, sort_completed_nodes)
}
