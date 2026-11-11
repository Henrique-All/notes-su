use std::sync::LazyLock;

use csv::{ReaderBuilder, WriterBuilder};
use regex::Regex;

static BR_TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").expect("valid br tag regex"));
static SEPARATOR_CELL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^:?-{1,}:?$").expect("valid GFM separator cell regex"));

pub fn csv_to_gfm_table(csv_input: &str) -> String {
    let stripped = csv_input.strip_prefix('\u{feff}').unwrap_or(csv_input);
    if stripped.trim().is_empty() {
        return String::new();
    }
    if has_unclosed_csv_quotes(stripped) {
        return csv_input.to_owned();
    }

    let mut reader = ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(stripped.as_bytes());

    let mut rows: Vec<Vec<String>> = Vec::new();
    for result in reader.records() {
        let Ok(record) = result else {
            return csv_input.to_owned();
        };
        let row: Vec<String> = record.iter().map(str::to_owned).collect();
        if !row.is_empty() && !(row.len() == 1 && row[0].is_empty() && rows.is_empty()) {
            rows.push(row);
        }
    }

    let max_cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if rows.is_empty() || max_cols == 0 {
        return csv_input.to_owned();
    }

    let mut out_lines = Vec::with_capacity(rows.len() + 1);

    let header = format_gfm_row(&rows[0], max_cols);
    out_lines.push(header);

    let sep_cells: Vec<&str> = vec!["---"; max_cols];
    out_lines.push(format!("| {} |", sep_cells.join(" | ")));

    for row in rows.iter().skip(1) {
        out_lines.push(format_gfm_row(row, max_cols));
    }

    out_lines.join("\n")
}

fn format_gfm_row(row: &[String], max_cols: usize) -> String {
    let mut cells = Vec::with_capacity(max_cols);
    for col_idx in 0..max_cols {
        let raw = row.get(col_idx).map(String::as_str).unwrap_or("");
        let encoded = raw
            .replace("\r\n", "<br>")
            .replace(['\r', '\n'], "<br>")
            .replace('|', "\\|");
        cells.push(encoded);
    }
    format!("| {} |", cells.join(" | "))
}

pub fn gfm_table_to_csv(markdown_table: &str) -> String {
    let stripped = markdown_table
        .strip_prefix('\u{feff}')
        .unwrap_or(markdown_table);
    let non_empty_lines: Vec<&str> = stripped
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    if non_empty_lines.is_empty() {
        return String::new();
    }

    let mut parsed_rows: Vec<Vec<String>> = Vec::with_capacity(non_empty_lines.len());
    for line in &non_empty_lines {
        let Some(cells) = split_gfm_table_row(line) else {
            return markdown_table.to_owned();
        };
        parsed_rows.push(cells);
    }

    let mut writer = WriterBuilder::new().from_writer(Vec::new());
    for (idx, row) in parsed_rows.iter().enumerate() {
        if idx == 1 && is_separator_row(row) {
            continue;
        }
        if writer.write_record(row).is_err() {
            return markdown_table.to_owned();
        }
    }

    let Ok(bytes) = writer.into_inner() else {
        return markdown_table.to_owned();
    };
    String::from_utf8(bytes).unwrap_or_else(|_| markdown_table.to_owned())
}

fn is_separator_row(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells
            .iter()
            .all(|c| SEPARATOR_CELL_RE.is_match(c.trim()))
}

fn split_gfm_table_row(line: &str) -> Option<Vec<String>> {
    if !has_unescaped_pipe(line) {
        return None;
    }

    let mut working = line.trim();
    if let Some(after_lead) = working.strip_prefix('|') {
        working = after_lead;
    }
    if ends_with_unescaped_pipe(working) {
        working = &working[..working.len() - 1];
    }

    let mut cells = Vec::new();
    let mut current = String::new();
    let mut chars = working.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(&next_ch) = chars.peek()
                && (next_ch == '|' || next_ch == '\\')
            {
                current.push(chars.next().unwrap());
                continue;
            }
            current.push('\\');
        } else if ch == '|' {
            cells.push(decode_gfm_cell(&current));
            current.clear();
        } else {
            current.push(ch);
        }
    }
    cells.push(decode_gfm_cell(&current));

    if cells.is_empty() {
        None
    } else {
        Some(cells)
    }
}

fn decode_gfm_cell(raw: &str) -> String {
    let trimmed = raw.trim();
    BR_TAG_RE.replace_all(trimmed, "\n").into_owned()
}

fn has_unescaped_pipe(s: &str) -> bool {
    let mut backslashes = 0usize;
    for ch in s.chars() {
        if ch == '\\' {
            backslashes += 1;
        } else {
            if ch == '|' && backslashes.is_multiple_of(2) {
                return true;
            }
            backslashes = 0;
        }
    }
    false
}

fn ends_with_unescaped_pipe(s: &str) -> bool {
    if !s.ends_with('|') {
        return false;
    }
    let without_pipe = &s[..s.len() - 1];
    let trailing_bs = without_pipe
        .chars()
        .rev()
        .take_while(|&c| c == '\\')
        .count();
    trailing_bs.is_multiple_of(2)
}

fn has_unclosed_csv_quotes(s: &str) -> bool {
    let mut in_quotes = false;
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    let _ = chars.next();
                } else {
                    in_quotes = false;
                }
            }
        } else if ch == '"' {
            in_quotes = true;
        }
    }
    in_quotes
}
