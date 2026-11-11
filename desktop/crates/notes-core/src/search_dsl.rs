use std::collections::{HashMap, HashSet};

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::models::{Color, Note, Notebook, Tag};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InNotebookTarget {
    Bool(bool),
    Notebook(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct DateBound {
    pub timestamp: DateTime<Utc>,
    pub date_only: bool,
}

impl DateBound {
    pub fn matches_before(&self, ts: DateTime<Utc>) -> bool {
        ts.timestamp_millis() < self.timestamp.timestamp_millis()
    }

    pub fn matches_after(&self, ts: DateTime<Utc>) -> bool {
        if self.date_only {
            ts.timestamp_millis() >= self.timestamp.timestamp_millis()
        } else {
            ts.timestamp_millis() > self.timestamp.timestamp_millis()
        }
    }

    pub fn sql_before_op_and_ms(&self) -> (&'static str, i64) {
        ("<", self.timestamp.timestamp_millis())
    }

    pub fn sql_after_op_and_ms(&self) -> (&'static str, i64) {
        let op = if self.date_only { ">=" } else { ">" };
        (op, self.timestamp.timestamp_millis())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SearchFilter {
    Text(String),
    Phrase(String),
    Title { value: String, is_phrase: bool },
    Content { value: String, is_phrase: bool },
    Tag(String),
    Color(String),
    Pinned(bool),
    Locked(bool),
    Readonly(bool),
    Favorite(bool),
    Archived(bool),
    Tagged(bool),
    Colored(bool),
    InNotebook(InNotebookTarget),
    CreatedBefore(DateBound),
    CreatedAfter(DateBound),
    EditedBefore(DateBound),
    EditedAfter(DateBound),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SearchExpr {
    Filter(SearchFilter),
    Not(Box<SearchExpr>),
    And(Box<SearchExpr>, Box<SearchExpr>),
    Or(Box<SearchExpr>, Box<SearchExpr>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SearchQuery {
    pub raw: String,
    pub root: Option<SearchExpr>,
}

/// Unicode NFD diacritic-stripping + lowercase folding shared identically
/// between `notes-core` in-memory search and `notes-store` SQLite `norm_fold`.
pub fn normalize_search_text(s: &str) -> String {
    s.nfd()
        .filter(|&c| !is_combining_mark(c))
        .flat_map(char::to_lowercase)
        .collect()
}

pub fn ast_mentions_archived(expr: &SearchExpr) -> bool {
    match expr {
        SearchExpr::Filter(SearchFilter::Archived(_)) => true,
        SearchExpr::Filter(_) => false,
        SearchExpr::Not(inner) => ast_mentions_archived(inner),
        SearchExpr::And(a, b) | SearchExpr::Or(a, b) => {
            ast_mentions_archived(a) || ast_mentions_archived(b)
        }
    }
}

pub fn collect_positive_highlight_terms(expr: &SearchExpr, out: &mut Vec<String>) {
    match expr {
        SearchExpr::Filter(filter) => match filter {
            SearchFilter::Text(t) => {
                for w in t.split_whitespace() {
                    if !w.is_empty() {
                        out.push(w.to_owned());
                    }
                }
            }
            SearchFilter::Phrase(p) => {
                if !p.is_empty() {
                    out.push(p.clone());
                }
            }
            SearchFilter::Title { value, is_phrase }
            | SearchFilter::Content { value, is_phrase } => {
                if *is_phrase {
                    if !value.is_empty() {
                        out.push(value.clone());
                    }
                } else {
                    for w in value.split_whitespace() {
                        if !w.is_empty() {
                            out.push(w.to_owned());
                        }
                    }
                }
            }
            _ => {}
        },
        SearchExpr::Not(_) => {}
        SearchExpr::And(a, b) | SearchExpr::Or(a, b) => {
            collect_positive_highlight_terms(a, out);
            collect_positive_highlight_terms(b, out);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    LParen,
    RParen,
    And,
    Or,
    Not,
    Term {
        negated: bool,
        raw: String,
        was_quoted: bool,
        key_val: Option<(String, String, bool)>,
    },
}

fn tokenize_dsl(input: &str) -> Vec<Token> {
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0usize;
    let mut tokens = Vec::new();
    let mut open_parens = 0usize;

    while i < len {
        while i < len && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= len {
            break;
        }

        if chars[i] == '(' {
            open_parens += 1;
            tokens.push(Token::LParen);
            i += 1;
            continue;
        }

        if chars[i] == ')' && open_parens > 0 {
            open_parens -= 1;
            tokens.push(Token::RParen);
            i += 1;
            continue;
        }

        // Handle prefix `-` negation before `(` or `"..."` or word
        if chars[i] == '-' && i + 1 < len && !chars[i + 1].is_whitespace() {
            if chars[i + 1] == '(' {
                tokens.push(Token::Not);
                i += 1;
                continue;
            }
            if chars[i + 1] == '"' {
                i += 2;
                let phrase = read_quoted(&chars, &mut i);
                tokens.push(Token::Term {
                    negated: true,
                    raw: phrase,
                    was_quoted: true,
                    key_val: None,
                });
                continue;
            }
            i += 1;
            let token = read_word_or_keyval(&chars, &mut i, open_parens, true);
            tokens.push(token);
            continue;
        }

        if chars[i] == '"' {
            i += 1;
            let phrase = read_quoted(&chars, &mut i);
            tokens.push(Token::Term {
                negated: false,
                raw: phrase,
                was_quoted: true,
                key_val: None,
            });
            continue;
        }

        let token = read_word_or_keyval(&chars, &mut i, open_parens, false);
        tokens.push(token);
    }

    tokens
}

fn read_quoted(chars: &[char], i: &mut usize) -> String {
    let mut buf = String::new();
    let len = chars.len();
    while *i < len {
        let ch = chars[*i];
        if ch == '\\' && *i + 1 < len && (chars[*i + 1] == '"' || chars[*i + 1] == '\\') {
            buf.push(chars[*i + 1]);
            *i += 2;
            continue;
        }
        if ch == '"' {
            if *i + 1 < len && chars[*i + 1] == '"' {
                buf.push('"');
                *i += 2;
                continue;
            }
            *i += 1;
            break;
        }
        buf.push(ch);
        *i += 1;
    }
    buf
}

fn read_word_or_keyval(
    chars: &[char],
    i: &mut usize,
    open_parens: usize,
    negated: bool,
) -> Token {
    let len = chars.len();
    let mut word = String::new();

    while *i < len {
        let ch = chars[*i];
        if ch.is_whitespace() {
            break;
        }
        if ch == ')' && open_parens > 0 {
            break;
        }
        if ch == ':'
            && !word.is_empty()
            && word.chars().all(|c| c.is_ascii_alphabetic() || c == '_')
            && *i + 1 < len
            && chars[*i + 1] == '"'
        {
            let key = word.clone();
            *i += 2; // skip `:"`
            let val = read_quoted(chars, i);
            let raw = format!("{key}:\"{val}\"");
            return Token::Term {
                negated,
                raw,
                was_quoted: false,
                key_val: Some((key, val, true)),
            };
        }
        word.push(ch);
        *i += 1;
    }

    if !negated {
        match word.as_str() {
            "AND" => return Token::And,
            "OR" => return Token::Or,
            "NOT" => return Token::Not,
            _ => {}
        }
    }

    let key_val = if let Some((k, v)) = word.split_once(':') {
        if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphabetic() || c == '_') {
            Some((k.to_owned(), v.to_owned(), false))
        } else {
            None
        }
    } else {
        None
    };

    Token::Term {
        negated,
        raw: word,
        was_quoted: false,
        key_val,
    }
}

fn parse_dsl_bool(val: &str) -> Option<bool> {
    if val.eq_ignore_ascii_case("true") {
        Some(true)
    } else if val.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

fn parse_dsl_date(val: &str) -> Option<DateBound> {
    let trimmed = val.trim();
    if let Ok(nd) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        let dt = nd.and_hms_opt(0, 0, 0)?.and_utc();
        return Some(DateBound {
            timestamp: dt,
            date_only: true,
        });
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(DateBound {
            timestamp: dt.with_timezone(&Utc),
            date_only: false,
        });
    }
    None
}

fn resolve_term_to_expr(
    negated: bool,
    raw: String,
    was_quoted: bool,
    key_val: Option<(String, String, bool)>,
) -> Option<SearchExpr> {
    if was_quoted {
        if raw.is_empty() {
            return None;
        }
        let expr = SearchExpr::Filter(SearchFilter::Phrase(raw));
        return Some(if negated {
            SearchExpr::Not(Box::new(expr))
        } else {
            expr
        });
    }

    let filter = if let Some((key, val, val_quoted)) = key_val {
        match key.to_ascii_lowercase().as_str() {
            "title" if !val.is_empty() => SearchFilter::Title {
                value: val,
                is_phrase: val_quoted,
            },
            "content" if !val.is_empty() => SearchFilter::Content {
                value: val,
                is_phrase: val_quoted,
            },
            "tag" if !val.is_empty() => SearchFilter::Tag(val),
            "color" if !val.is_empty() => SearchFilter::Color(val),
            "pinned" => match parse_dsl_bool(&val) {
                Some(b) => SearchFilter::Pinned(b),
                None => SearchFilter::Text(raw),
            },
            "locked" => match parse_dsl_bool(&val) {
                Some(b) => SearchFilter::Locked(b),
                None => SearchFilter::Text(raw),
            },
            "readonly" => match parse_dsl_bool(&val) {
                Some(b) => SearchFilter::Readonly(b),
                None => SearchFilter::Text(raw),
            },
            "favorite" => match parse_dsl_bool(&val) {
                Some(b) => SearchFilter::Favorite(b),
                None => SearchFilter::Text(raw),
            },
            "archived" => match parse_dsl_bool(&val) {
                Some(b) => SearchFilter::Archived(b),
                None => SearchFilter::Text(raw),
            },
            "tagged" => match parse_dsl_bool(&val) {
                Some(b) => SearchFilter::Tagged(b),
                None => SearchFilter::Text(raw),
            },
            "colored" => match parse_dsl_bool(&val) {
                Some(b) => SearchFilter::Colored(b),
                None => SearchFilter::Text(raw),
            },
            "in_notebook" => {
                if let Some(b) = parse_dsl_bool(&val) {
                    SearchFilter::InNotebook(InNotebookTarget::Bool(b))
                } else if !val.is_empty() {
                    SearchFilter::InNotebook(InNotebookTarget::Notebook(val))
                } else {
                    SearchFilter::Text(raw)
                }
            }
            "created_before" => match parse_dsl_date(&val) {
                Some(bound) => SearchFilter::CreatedBefore(bound),
                None => SearchFilter::Text(raw),
            },
            "created_after" => match parse_dsl_date(&val) {
                Some(bound) => SearchFilter::CreatedAfter(bound),
                None => SearchFilter::Text(raw),
            },
            "edited_before" => match parse_dsl_date(&val) {
                Some(bound) => SearchFilter::EditedBefore(bound),
                None => SearchFilter::Text(raw),
            },
            "edited_after" => match parse_dsl_date(&val) {
                Some(bound) => SearchFilter::EditedAfter(bound),
                None => SearchFilter::Text(raw),
            },
            _ => SearchFilter::Text(raw),
        }
    } else {
        if raw.is_empty() {
            return None;
        }
        SearchFilter::Text(raw)
    };

    let expr = SearchExpr::Filter(filter);
    Some(if negated {
        SearchExpr::Not(Box::new(expr))
    } else {
        expr
    })
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        if self.pos < self.tokens.len() {
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(tok)
        } else {
            None
        }
    }

    fn parse_or(&mut self) -> Option<SearchExpr> {
        let mut lhs = self.parse_and()?;
        while matches!(self.peek(), Some(Token::Or)) {
            self.next();
            if let Some(rhs) = self.parse_and() {
                lhs = SearchExpr::Or(Box::new(lhs), Box::new(rhs));
            } else {
                lhs = SearchExpr::And(
                    Box::new(lhs),
                    Box::new(SearchExpr::Filter(SearchFilter::Text("OR".to_owned()))),
                );
            }
        }
        Some(lhs)
    }

    fn parse_and(&mut self) -> Option<SearchExpr> {
        let mut lhs = self.parse_unary()?;
        loop {
            match self.peek() {
                None | Some(Token::Or | Token::RParen) => break,
                Some(Token::And) => {
                    self.next();
                    if let Some(rhs) = self.parse_unary() {
                        lhs = SearchExpr::And(Box::new(lhs), Box::new(rhs));
                    } else {
                        lhs = SearchExpr::And(
                            Box::new(lhs),
                            Box::new(SearchExpr::Filter(SearchFilter::Text("AND".to_owned()))),
                        );
                    }
                }
                Some(_) => {
                    if let Some(rhs) = self.parse_unary() {
                        lhs = SearchExpr::And(Box::new(lhs), Box::new(rhs));
                    } else {
                        break;
                    }
                }
            }
        }
        Some(lhs)
    }

    fn parse_unary(&mut self) -> Option<SearchExpr> {
        if matches!(self.peek(), Some(Token::Not)) {
            self.next();
            if let Some(operand) = self.parse_unary() {
                return Some(SearchExpr::Not(Box::new(operand)));
            }
            return Some(SearchExpr::Filter(SearchFilter::Text("NOT".to_owned())));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Option<SearchExpr> {
        match self.peek()? {
            Token::LParen => {
                self.next();
                if matches!(self.peek(), Some(Token::RParen)) {
                    self.next();
                    return Some(SearchExpr::Filter(SearchFilter::Text("()".to_owned())));
                }
                let inner = self.parse_or();
                if matches!(self.peek(), Some(Token::RParen)) {
                    self.next();
                }
                inner.or_else(|| Some(SearchExpr::Filter(SearchFilter::Text("(".to_owned()))))
            }
            Token::RParen => {
                self.next();
                Some(SearchExpr::Filter(SearchFilter::Text(")".to_owned())))
            }
            Token::And => {
                self.next();
                Some(SearchExpr::Filter(SearchFilter::Text("AND".to_owned())))
            }
            Token::Or => {
                self.next();
                Some(SearchExpr::Filter(SearchFilter::Text("OR".to_owned())))
            }
            Token::Not => {
                self.next();
                Some(SearchExpr::Filter(SearchFilter::Text("NOT".to_owned())))
            }
            Token::Term { .. } => {
                let Some(Token::Term {
                    negated,
                    raw,
                    was_quoted,
                    key_val,
                }) = self.next()
                else {
                    unreachable!()
                };
                resolve_term_to_expr(negated, raw, was_quoted, key_val)
            }
        }
    }
}

pub fn parse_search_query(input: &str) -> SearchQuery {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return SearchQuery {
            raw: input.to_owned(),
            root: None,
        };
    }

    let tokens = tokenize_dsl(trimmed);
    let mut parser = Parser::new(tokens);
    let mut expr = parser.parse_or();

    while parser.peek().is_some() {
        if let Some(extra) = parser.parse_or() {
            expr = Some(match expr {
                Some(prev) => SearchExpr::And(Box::new(prev), Box::new(extra)),
                None => extra,
            });
        } else {
            break;
        }
    }

    SearchQuery {
        raw: input.to_owned(),
        root: expr,
    }
}

pub struct SearchEvalContext<'a> {
    pub note: &'a Note,
    pub content_data: Option<&'a str>,
    pub tags: &'a [Tag],
    pub color: Option<&'a Color>,
    pub assigned_notebook_ids: &'a [String],
    pub all_notebooks: &'a [Notebook],
}

pub fn matches_note(query: &SearchQuery, ctx: &SearchEvalContext<'_>) -> bool {
    let Some(root) = &query.root else {
        return true;
    };
    eval_expr(root, ctx)
}

fn eval_expr(expr: &SearchExpr, ctx: &SearchEvalContext<'_>) -> bool {
    match expr {
        SearchExpr::Filter(f) => eval_filter(f, ctx),
        SearchExpr::Not(inner) => !eval_expr(inner, ctx),
        SearchExpr::And(a, b) => eval_expr(a, ctx) && eval_expr(b, ctx),
        SearchExpr::Or(a, b) => eval_expr(a, ctx) || eval_expr(b, ctx),
    }
}

fn text_matches_words_or_phrase(haystack_folded: &str, query: &str, is_phrase: bool) -> bool {
    let q_folded = normalize_search_text(query);
    if q_folded.is_empty() {
        return true;
    }
    if is_phrase {
        haystack_folded.contains(&q_folded)
    } else {
        q_folded
            .split_whitespace()
            .all(|word| haystack_folded.contains(word))
    }
}

fn eval_filter(filter: &SearchFilter, ctx: &SearchEvalContext<'_>) -> bool {
    let note = ctx.note;
    let unlocked_body = if note.locked {
        None
    } else {
        ctx.content_data
    };

    match filter {
        SearchFilter::Text(term) => {
            let title_folded = normalize_search_text(&note.title);
            if text_matches_words_or_phrase(&title_folded, term, false) {
                return true;
            }
            if let Some(body) = unlocked_body {
                let body_folded = normalize_search_text(body);
                return text_matches_words_or_phrase(&body_folded, term, false);
            }
            false
        }
        SearchFilter::Phrase(phrase) => {
            let title_folded = normalize_search_text(&note.title);
            if text_matches_words_or_phrase(&title_folded, phrase, true) {
                return true;
            }
            if let Some(body) = unlocked_body {
                let body_folded = normalize_search_text(body);
                return text_matches_words_or_phrase(&body_folded, phrase, true);
            }
            false
        }
        SearchFilter::Title { value, is_phrase } => {
            let title_folded = normalize_search_text(&note.title);
            text_matches_words_or_phrase(&title_folded, value, *is_phrase)
        }
        SearchFilter::Content { value, is_phrase } => {
            let Some(body) = unlocked_body else {
                return false;
            };
            let body_folded = normalize_search_text(body);
            text_matches_words_or_phrase(&body_folded, value, *is_phrase)
        }
        SearchFilter::Tag(target) => {
            let stripped = target.strip_prefix('#').unwrap_or(target);
            ctx.tags
                .iter()
                .filter(|t| !t.deleted)
                .any(|t| t.title == *target || t.title == stripped)
        }
        SearchFilter::Color(target) => ctx.color.filter(|c| !c.deleted).is_some_and(|c| {
            c.title.eq_ignore_ascii_case(target)
                || c.color_code.eq_ignore_ascii_case(target)
                || c.color_code
                    .strip_prefix('#')
                    .is_some_and(|hex| hex.eq_ignore_ascii_case(target))
        }),
        SearchFilter::Pinned(b) => note.pinned == *b,
        SearchFilter::Locked(b) => note.locked == *b,
        SearchFilter::Readonly(b) => note.readonly == *b,
        SearchFilter::Favorite(b) => note.favorite == *b,
        SearchFilter::Archived(b) => note.archived == *b,
        SearchFilter::Tagged(b) => {
            let has_tags = ctx.tags.iter().any(|t| !t.deleted);
            has_tags == *b
        }
        SearchFilter::Colored(b) => {
            let has_color = ctx
                .color
                .filter(|c| !c.deleted)
                .is_some()
                || (ctx.color.is_none() && note.color_id.is_some());
            has_color == *b
        }
        SearchFilter::InNotebook(target) => {
            let active_notebooks: HashMap<&str, &Notebook> = ctx
                .all_notebooks
                .iter()
                .filter(|nb| !nb.deleted && nb.trashed_at.is_none())
                .map(|nb| (nb.id.as_str(), nb))
                .collect();

            let active_assigned: Vec<&str> = ctx
                .assigned_notebook_ids
                .iter()
                .map(String::as_str)
                .filter(|id| active_notebooks.contains_key(id))
                .collect();

            match target {
                InNotebookTarget::Bool(b) => (!active_assigned.is_empty()) == *b,
                InNotebookTarget::Notebook(nb_query) => {
                    for &nb_id in &active_assigned {
                        let mut cur_opt = Some(nb_id);
                        let mut visited = HashSet::new();
                        while let Some(cur_id) = cur_opt {
                            if !visited.insert(cur_id) {
                                break;
                            }
                            let Some(nb) = active_notebooks.get(cur_id) else {
                                break;
                            };
                            if nb.id == *nb_query || nb.title.eq_ignore_ascii_case(nb_query) {
                                return true;
                            }
                            cur_opt = nb.parent_id.as_deref();
                        }
                    }
                    false
                }
            }
        }
        SearchFilter::CreatedBefore(b) => b.matches_before(note.date_created),
        SearchFilter::CreatedAfter(b) => b.matches_after(note.date_created),
        SearchFilter::EditedBefore(b) => b.matches_before(note.date_edited),
        SearchFilter::EditedAfter(b) => b.matches_after(note.date_edited),
    }
}
