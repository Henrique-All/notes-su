use notes_core::{
    InNotebookTarget, NoteSummary, SearchExpr, SearchFilter, ast_mentions_archived,
    collect_positive_highlight_terms, escape_html, normalize_search_text, parse_search_query,
};
use rusqlite::{params_from_iter, types::Value as SqlValue};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::error::StoreResult;
use crate::notes::load_note_summary_conn;
use crate::schema::NotesStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchScope {
    #[default]
    Default,
    Archive,
    Trash,
    Favorites,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub note: NoteSummary,
    pub score: f64,
    pub highlighted_title: String,
    pub highlighted_snippet: String,
}

impl NotesStore {
    pub fn search_notes(&self, raw_query: &str, scope: SearchScope) -> StoreResult<Vec<SearchHit>> {
        let parsed = parse_search_query(raw_query);
        let conn = self.lock_conn();

        let mut where_clauses: Vec<String> = vec!["n.deleted = 0".to_owned()];
        let mut sql_params: Vec<SqlValue> = Vec::new();
        let mut positive_fts_exprs: Vec<String> = Vec::new();
        let mut highlight_terms: Vec<String> = Vec::new();

        match scope {
            SearchScope::Trash => {
                where_clauses.push("n.trashed_at IS NOT NULL".to_owned());
            }
            SearchScope::Archive => {
                where_clauses.push("n.trashed_at IS NULL".to_owned());
                where_clauses.push("n.archived = 1".to_owned());
            }
            SearchScope::Favorites => {
                where_clauses.push("n.trashed_at IS NULL".to_owned());
                where_clauses.push("n.favorite = 1".to_owned());
                if !parsed.root.as_ref().is_some_and(ast_mentions_archived) {
                    where_clauses.push("n.archived = 0".to_owned());
                }
            }
            SearchScope::Default => {
                where_clauses.push("n.trashed_at IS NULL".to_owned());
                if !parsed.root.as_ref().is_some_and(ast_mentions_archived) {
                    where_clauses.push("n.archived = 0".to_owned());
                }
            }
        }

        if let Some(ref root) = parsed.root {
            collect_positive_highlight_terms(root, &mut highlight_terms);
            let (pred_sql, pred_params) = compile_ast_sql(root, &mut positive_fts_exprs, false);
            where_clauses.push(pred_sql);
            sql_params.extend(pred_params);
        }

        let where_sql = where_clauses.join(" AND ");

        let (rank_join_sql, final_params) = if positive_fts_exprs.is_empty() {
            (
                "LEFT JOIN (SELECT NULL AS note_id, 0.0 AS fts_score WHERE 0) r ON r.note_id = n.id"
                    .to_owned(),
                sql_params,
            )
        } else {
            let combined_rank = positive_fts_exprs
                .iter()
                .map(|e| format!("({e})"))
                .collect::<Vec<_>>()
                .join(" OR ");
            let mut all_params = vec![SqlValue::Text(combined_rank)];
            all_params.extend(sql_params);
            (
                "LEFT JOIN (
                    SELECT note_id, -bm25(notes_fts, 0.0, 5.0, 1.0) AS fts_score
                    FROM notes_fts
                    WHERE notes_fts MATCH ?
                 ) r ON r.note_id = n.id"
                    .to_owned(),
                all_params,
            )
        };

        let full_sql = format!(
            "SELECT n.id, n.title, n.locked, COALESCE(nc.data, ''), COALESCE(r.fts_score, 0.0) AS score
             FROM notes n
             LEFT JOIN note_contents nc ON nc.note_id = n.id AND nc.deleted = 0
             {rank_join_sql}
             WHERE {where_sql}
             ORDER BY score DESC, n.pinned DESC, n.date_edited DESC"
        );

        let mut stmt = conn.prepare(&full_sql)?;
        let rows = stmt.query_map(params_from_iter(final_params), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i32>(2)? != 0,
                r.get::<_, String>(3)?,
                r.get::<_, f64>(4)?,
            ))
        })?;

        let mut raw_hits = Vec::new();
        for r in rows {
            raw_hits.push(r?);
        }

        let mut results = Vec::with_capacity(raw_hits.len());
        for (note_id, title, locked, body_data, score) in raw_hits {
            let highlighted_title = highlight_text_xss_safe(&title, &highlight_terms, false);
            let highlighted_snippet = if locked {
                String::new()
            } else {
                highlight_text_xss_safe(&body_data, &highlight_terms, true)
            };
            let mut summary = load_note_summary_conn(&conn, &note_id)?;
            if !highlighted_snippet.is_empty() {
                summary.snippet = Some(highlighted_snippet.clone());
            }
            results.push(SearchHit {
                note: summary,
                score,
                highlighted_title,
                highlighted_snippet,
            });
        }

        Ok(results)
    }
}

#[derive(Debug, Clone, Copy)]
enum TextTarget {
    All,
    Title,
    Content,
}

fn fts_quote_token(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn compile_text_predicate(
    raw_term: &str,
    is_phrase: bool,
    target: TextTarget,
    positive_fts: &mut Vec<String>,
    in_negation: bool,
) -> (String, Vec<SqlValue>) {
    let folded = normalize_search_text(raw_term);
    let words: Vec<&str> = raw_term.split_whitespace().filter(|w| !w.is_empty()).collect();

    if folded.is_empty() || words.is_empty() {
        return ("1 = 1".to_owned(), Vec::new());
    }

    let can_use_fts = if is_phrase {
        folded.chars().count() >= 3 && folded.chars().any(char::is_alphanumeric)
    } else {
        words.iter().all(|w| {
            let wf = normalize_search_text(w);
            wf.chars().count() >= 3 && wf.chars().any(char::is_alphanumeric)
        })
    };

    if !can_use_fts {
        return compile_substring_fallback(raw_term, is_phrase, target);
    }

    let base_fts_expr = if is_phrase {
        fts_quote_token(raw_term)
    } else {
        words
            .iter()
            .map(|w| format!("{}*", fts_quote_token(w)))
            .collect::<Vec<_>>()
            .join(" AND ")
    };

    match target {
        TextTarget::Title => {
            let col_fts = format!("title : ({base_fts_expr})");
            if !in_negation {
                positive_fts.push(col_fts.clone());
            }
            if is_phrase {
                (
                    "(n.id IN (SELECT note_id FROM notes_fts WHERE notes_fts MATCH ?) OR norm_contains(n.title, ?))"
                        .to_owned(),
                    vec![
                        SqlValue::Text(col_fts),
                        SqlValue::Text(raw_term.to_owned()),
                    ],
                )
            } else {
                let (sub_sql, mut sub_params) =
                    compile_substring_fallback(raw_term, false, TextTarget::Title);
                let mut params = vec![SqlValue::Text(col_fts)];
                params.append(&mut sub_params);
                (
                    format!("(n.id IN (SELECT note_id FROM notes_fts WHERE notes_fts MATCH ?) OR {sub_sql})"),
                    params,
                )
            }
        }
        TextTarget::Content => {
            let col_fts = format!("body : ({base_fts_expr})");
            if !in_negation {
                positive_fts.push(col_fts.clone());
            }
            if is_phrase {
                (
                    "(n.locked = 0 AND COALESCE(nc.locked, 0) = 0 AND (n.id IN (SELECT note_id FROM notes_fts WHERE notes_fts MATCH ?) OR norm_contains(COALESCE(nc.data, ''), ?)))"
                        .to_owned(),
                    vec![
                        SqlValue::Text(col_fts),
                        SqlValue::Text(raw_term.to_owned()),
                    ],
                )
            } else {
                let (sub_sql, mut sub_params) =
                    compile_substring_fallback(raw_term, false, TextTarget::Content);
                let mut params = vec![SqlValue::Text(col_fts)];
                params.append(&mut sub_params);
                (
                    format!("((n.locked = 0 AND COALESCE(nc.locked, 0) = 0 AND n.id IN (SELECT note_id FROM notes_fts WHERE notes_fts MATCH ?)) OR {sub_sql})"),
                    params,
                )
            }
        }
        TextTarget::All => {
            if !in_negation {
                positive_fts.push(base_fts_expr.clone());
            }
            let (sub_sql, mut sub_params) =
                compile_substring_fallback(raw_term, is_phrase, TextTarget::All);
            let mut params = vec![SqlValue::Text(base_fts_expr)];
            params.append(&mut sub_params);
            (
                format!("(n.id IN (SELECT note_id FROM notes_fts WHERE notes_fts MATCH ?) OR {sub_sql})"),
                params,
            )
        }
    }
}

fn compile_substring_fallback(
    raw_term: &str,
    is_phrase: bool,
    target: TextTarget,
) -> (String, Vec<SqlValue>) {
    let pieces: Vec<&str> = if is_phrase {
        vec![raw_term]
    } else {
        raw_term.split_whitespace().filter(|w| !w.is_empty()).collect()
    };

    match target {
        TextTarget::Title => {
            let mut clauses = Vec::with_capacity(pieces.len());
            let mut params = Vec::with_capacity(pieces.len());
            for p in pieces {
                clauses.push("norm_contains(n.title, ?)".to_owned());
                params.push(SqlValue::Text(p.to_owned()));
            }
            (format!("({})", clauses.join(" AND ")), params)
        }
        TextTarget::Content => {
            let mut clauses = Vec::with_capacity(pieces.len());
            let mut params = Vec::with_capacity(pieces.len());
            for p in pieces {
                clauses.push("norm_contains(COALESCE(nc.data, ''), ?)".to_owned());
                params.push(SqlValue::Text(p.to_owned()));
            }
            (
                format!(
                    "(n.locked = 0 AND COALESCE(nc.locked, 0) = 0 AND ({}))",
                    clauses.join(" AND ")
                ),
                params,
            )
        }
        TextTarget::All => {
            let (t_sql, mut t_params) = compile_substring_fallback(raw_term, is_phrase, TextTarget::Title);
            let (c_sql, mut c_params) = compile_substring_fallback(raw_term, is_phrase, TextTarget::Content);
            t_params.append(&mut c_params);
            (format!("({t_sql} OR {c_sql})"), t_params)
        }
    }
}

fn compile_ast_sql(
    expr: &SearchExpr,
    positive_fts: &mut Vec<String>,
    in_negation: bool,
) -> (String, Vec<SqlValue>) {
    match expr {
        SearchExpr::And(a, b) => {
            let (sa, mut pa) = compile_ast_sql(a, positive_fts, in_negation);
            let (sb, mut pb) = compile_ast_sql(b, positive_fts, in_negation);
            pa.append(&mut pb);
            (format!("({sa} AND {sb})"), pa)
        }
        SearchExpr::Or(a, b) => {
            let (sa, mut pa) = compile_ast_sql(a, positive_fts, in_negation);
            let (sb, mut pb) = compile_ast_sql(b, positive_fts, in_negation);
            pa.append(&mut pb);
            (format!("({sa} OR {sb})"), pa)
        }
        SearchExpr::Not(inner) => {
            let (si, pi) = compile_ast_sql(inner, positive_fts, true);
            (format!("(NOT ({si}))"), pi)
        }
        SearchExpr::Filter(filter) => compile_filter_sql(filter, positive_fts, in_negation),
    }
}

fn compile_filter_sql(
    filter: &SearchFilter,
    positive_fts: &mut Vec<String>,
    in_negation: bool,
) -> (String, Vec<SqlValue>) {
    match filter {
        SearchFilter::Text(t) => {
            compile_text_predicate(t, false, TextTarget::All, positive_fts, in_negation)
        }
        SearchFilter::Phrase(p) => {
            compile_text_predicate(p, true, TextTarget::All, positive_fts, in_negation)
        }
        SearchFilter::Title { value, is_phrase } => {
            compile_text_predicate(value, *is_phrase, TextTarget::Title, positive_fts, in_negation)
        }
        SearchFilter::Content { value, is_phrase } => compile_text_predicate(
            value,
            *is_phrase,
            TextTarget::Content,
            positive_fts,
            in_negation,
        ),
        SearchFilter::Tag(val) => {
            let stripped = val.strip_prefix('#').unwrap_or(val);
            (
                "EXISTS (
                    SELECT 1 FROM note_tags nt
                    JOIN tags t ON t.id = nt.tag_id
                    WHERE nt.note_id = n.id AND nt.deleted = 0 AND t.deleted = 0
                      AND (t.title = ? OR t.title = ?)
                 )"
                .to_owned(),
                vec![SqlValue::Text(val.clone()), SqlValue::Text(stripped.to_owned())],
            )
        }
        SearchFilter::Color(val) => {
            let with_hash = if val.starts_with('#') {
                val.clone()
            } else {
                format!("#{val}")
            };
            (
                "EXISTS (
                    SELECT 1 FROM colors c
                    WHERE c.id = n.color_id AND c.deleted = 0
                      AND (LOWER(c.title) = LOWER(?) OR LOWER(c.color_code) = LOWER(?) OR LOWER(c.color_code) = LOWER(?))
                 )"
                .to_owned(),
                vec![
                    SqlValue::Text(val.clone()),
                    SqlValue::Text(val.clone()),
                    SqlValue::Text(with_hash),
                ],
            )
        }
        SearchFilter::Pinned(b) => ("n.pinned = ?".to_owned(), vec![SqlValue::Integer(i64::from(*b))]),
        SearchFilter::Locked(b) => ("n.locked = ?".to_owned(), vec![SqlValue::Integer(i64::from(*b))]),
        SearchFilter::Readonly(b) => (
            "n.readonly = ?".to_owned(),
            vec![SqlValue::Integer(i64::from(*b))],
        ),
        SearchFilter::Favorite(b) => (
            "n.favorite = ?".to_owned(),
            vec![SqlValue::Integer(i64::from(*b))],
        ),
        SearchFilter::Archived(b) => (
            "n.archived = ?".to_owned(),
            vec![SqlValue::Integer(i64::from(*b))],
        ),
        SearchFilter::Tagged(b) => {
            let prefix = if *b { "EXISTS" } else { "NOT EXISTS" };
            (
                format!(
                    "{prefix} (
                        SELECT 1 FROM note_tags nt
                        JOIN tags t ON t.id = nt.tag_id
                        WHERE nt.note_id = n.id AND nt.deleted = 0 AND t.deleted = 0
                    )"
                ),
                Vec::new(),
            )
        }
        SearchFilter::Colored(b) => {
            if *b {
                (
                    "(n.color_id IS NOT NULL AND EXISTS (SELECT 1 FROM colors c WHERE c.id = n.color_id AND c.deleted = 0))"
                        .to_owned(),
                    Vec::new(),
                )
            } else {
                (
                    "(n.color_id IS NULL OR NOT EXISTS (SELECT 1 FROM colors c WHERE c.id = n.color_id AND c.deleted = 0))"
                        .to_owned(),
                    Vec::new(),
                )
            }
        }
        SearchFilter::InNotebook(target) => match target {
            InNotebookTarget::Bool(b) => {
                let prefix = if *b { "EXISTS" } else { "NOT EXISTS" };
                (
                    format!(
                        "{prefix} (
                            SELECT 1 FROM note_notebooks nn
                            JOIN notebooks nb ON nb.id = nn.notebook_id
                            WHERE nn.note_id = n.id AND nn.deleted = 0 AND nb.deleted = 0 AND nb.trashed_at IS NULL
                        )"
                    ),
                    Vec::new(),
                )
            }
            InNotebookTarget::Notebook(nb_query) => (
                "EXISTS (
                    WITH RECURSIVE nb_sub(id, depth) AS (
                        SELECT id, 0 FROM notebooks
                        WHERE deleted = 0 AND trashed_at IS NULL AND (id = ? OR LOWER(title) = LOWER(?))
                        UNION ALL
                        SELECT child.id, s.depth + 1
                        FROM notebooks child JOIN nb_sub s ON child.parent_id = s.id
                        WHERE child.deleted = 0 AND child.trashed_at IS NULL AND s.depth < 256
                    )
                    SELECT 1 FROM note_notebooks nn
                    JOIN nb_sub s ON s.id = nn.notebook_id
                    WHERE nn.note_id = n.id AND nn.deleted = 0
                 )"
                .to_owned(),
                vec![SqlValue::Text(nb_query.clone()), SqlValue::Text(nb_query.clone())],
            ),
        },
        SearchFilter::CreatedBefore(bound) => {
            let (op, ms) = bound.sql_before_op_and_ms();
            (format!("n.date_created {op} ?"), vec![SqlValue::Integer(ms)])
        }
        SearchFilter::CreatedAfter(bound) => {
            let (op, ms) = bound.sql_after_op_and_ms();
            (format!("n.date_created {op} ?"), vec![SqlValue::Integer(ms)])
        }
        SearchFilter::EditedBefore(bound) => {
            let (op, ms) = bound.sql_before_op_and_ms();
            (format!("n.date_edited {op} ?"), vec![SqlValue::Integer(ms)])
        }
        SearchFilter::EditedAfter(bound) => {
            let (op, ms) = bound.sql_after_op_and_ms();
            (format!("n.date_edited {op} ?"), vec![SqlValue::Integer(ms)])
        }
    }
}

/// Builds XSS-safe `<mark>` highlights over `raw_text`, preserving original UTF-8 accents/casing
/// while matching diacritic-insensitively and HTML-escaping all raw text!
pub fn highlight_text_xss_safe(raw_text: &str, terms: &[String], crop_snippet: bool) -> String {
    if raw_text.is_empty() {
        return String::new();
    }

    let clean_text = if crop_snippet {
        raw_text.replace(['\r', '\n', '\t'], " ")
    } else {
        raw_text.to_owned()
    };

    let folded_needles: Vec<Vec<char>> = terms
        .iter()
        .map(|t| normalize_search_text(t).chars().collect::<Vec<char>>())
        .filter(|v| !v.is_empty())
        .collect();

    if folded_needles.is_empty() {
        let slice: String = if crop_snippet {
            clean_text.chars().take(180).collect()
        } else {
            clean_text
        };
        return escape_html(&slice);
    }

    let mut folded_chars: Vec<char> = Vec::with_capacity(clean_text.len());
    let mut folded_to_orig_bytes: Vec<(usize, usize)> = Vec::with_capacity(clean_text.len());

    for (byte_start, orig_ch) in clean_text.char_indices() {
        let byte_end = byte_start + orig_ch.len_utf8();
        for nfd_ch in orig_ch.nfd() {
            if !is_combining_mark(nfd_ch) {
                for lc in nfd_ch.to_lowercase() {
                    folded_chars.push(lc);
                    folded_to_orig_bytes.push((byte_start, byte_end));
                }
            }
        }
    }

    let mut byte_intervals: Vec<(usize, usize)> = Vec::new();
    for needle in &folded_needles {
        let n_len = needle.len();
        if n_len == 0 || n_len > folded_chars.len() {
            continue;
        }
        let mut idx = 0usize;
        while idx + n_len <= folded_chars.len() {
            if &folded_chars[idx..idx + n_len] == needle.as_slice() {
                let start_b = folded_to_orig_bytes[idx].0;
                let end_b = folded_to_orig_bytes[idx + n_len - 1].1;
                byte_intervals.push((start_b, end_b));
                idx += n_len;
            } else {
                idx += 1;
            }
        }
    }

    if byte_intervals.is_empty() {
        let slice: String = if crop_snippet {
            clean_text.chars().take(180).collect()
        } else {
            clean_text
        };
        return escape_html(&slice);
    }

    byte_intervals.sort_by_key(|&(s, e)| (s, e));
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(byte_intervals.len());
    for (s, e) in byte_intervals {
        if let Some(last) = merged.last_mut()
            && s <= last.1
        {
            last.1 = last.1.max(e);
        } else {
            merged.push((s, e));
        }
    }

    let (window_start, window_end, prefix_ellipsis, suffix_ellipsis) = if crop_snippet {
        let first_match = merged[0].0;
        let start_b = prev_char_boundary(&clean_text, first_match, 60);
        let end_b = next_char_boundary(&clean_text, merged[0].1, 140);
        (start_b, end_b, start_b > 0, end_b < clean_text.len())
    } else {
        (0, clean_text.len(), false, false)
    };

    let mut out = String::with_capacity((window_end - window_start) + 64);
    if prefix_ellipsis {
        out.push('…');
    }

    let mut cursor = window_start;
    for (ms, me) in merged {
        if me <= window_start || ms >= window_end {
            continue;
        }
        let clamped_s = ms.max(window_start);
        let clamped_e = me.min(window_end);
        if clamped_s > cursor {
            out.push_str(&escape_html(&clean_text[cursor..clamped_s]));
        }
        out.push_str("<mark>");
        out.push_str(&escape_html(&clean_text[clamped_s..clamped_e]));
        out.push_str("</mark>");
        cursor = clamped_e;
    }
    if cursor < window_end {
        out.push_str(&escape_html(&clean_text[cursor..window_end]));
    }
    if suffix_ellipsis {
        out.push('…');
    }

    out
}

fn prev_char_boundary(s: &str, from_byte: usize, max_chars_back: usize) -> usize {
    let mut pos = from_byte.min(s.len());
    let mut count = 0usize;
    while pos > 0 && count < max_chars_back {
        pos -= 1;
        while pos > 0 && !s.is_char_boundary(pos) {
            pos -= 1;
        }
        count += 1;
    }
    pos
}

fn next_char_boundary(s: &str, from_byte: usize, max_chars_forward: usize) -> usize {
    let mut pos = from_byte.min(s.len());
    let mut count = 0usize;
    while pos < s.len() && count < max_chars_forward {
        pos += 1;
        while pos < s.len() && !s.is_char_boundary(pos) {
            pos += 1;
        }
        count += 1;
    }
    pos
}
