//! Highlighting from a tree-sitter syntax tree (DECISIONS D22): the text is parsed once, edits
//! are applied to the tree and parsed again incrementally, and each painted line asks the
//! grammar's highlight query for the captures crossing it. Capture names (`keyword`,
//! `function.method`) are mapped to TextMate scopes, so every theme keeps working.

use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use rustext_core::services::{DocumentSymbol, SymbolKind};
use rustext_core::structure::{FoldRange, SyntaxBrackets};
use rustext_core::style::{LineStyler, StyledSpan, TextStyle};
use rustext_core::{Change, ChangeSet, EditedLines, Range, Text};
use syntect::highlighting::Highlighter;
use syntect::parsing::Scope;
use tree_sitter::{InputEdit, Node, Parser, Point, Query, QueryCursor, StreamingIterator, TextProvider, Tree};
use tree_sitter_language::LanguageFn;

use crate::textmate::text_style;
use crate::theme::SyntaxTheme;

/// A compiled grammar: the language, its highlight query, its tags query and a query of its
/// bracket tokens.
pub struct Grammar {
    language: tree_sitter::Language,
    highlights: Query,
    tags: Option<Query>,
    brackets: Option<Query>,
}

impl std::fmt::Debug for Grammar {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Grammar").field("captures", &self.highlights.capture_names().len()).finish()
    }
}

/// The bundled tree-sitter grammar of a language (by its syntect name): the language, its
/// highlight queries (the first winning where two capture the same node) and its tags query
/// (definitions, for Go to Symbol), when the grammar crate has one.
fn grammar_source(language_name: &str) -> Option<(LanguageFn, Vec<&'static str>, Option<&'static str>)> {
    let javascript = tree_sitter_javascript::HIGHLIGHT_QUERY;
    let jsx = tree_sitter_javascript::JSX_HIGHLIGHT_QUERY;
    let typescript = tree_sitter_typescript::HIGHLIGHTS_QUERY;
    Some(match language_name {
        "Rust" => {
            (tree_sitter_rust::LANGUAGE, vec![tree_sitter_rust::HIGHLIGHTS_QUERY], Some(tree_sitter_rust::TAGS_QUERY))
        }
        "Python" => (
            tree_sitter_python::LANGUAGE,
            vec![tree_sitter_python::HIGHLIGHTS_QUERY],
            Some(tree_sitter_python::TAGS_QUERY),
        ),
        "JavaScript" | "JavaScript (Babel)" => {
            (tree_sitter_javascript::LANGUAGE, vec![jsx, javascript], Some(tree_sitter_javascript::TAGS_QUERY))
        }
        "TypeScript" => (
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
            vec![typescript, javascript],
            Some(tree_sitter_typescript::TAGS_QUERY),
        ),
        "TypeScriptReact" => (
            tree_sitter_typescript::LANGUAGE_TSX,
            vec![typescript, jsx, javascript],
            Some(tree_sitter_typescript::TAGS_QUERY),
        ),
        "JSON" => (tree_sitter_json::LANGUAGE, vec![tree_sitter_json::HIGHLIGHTS_QUERY], None),
        "Go" => (tree_sitter_go::LANGUAGE, vec![tree_sitter_go::HIGHLIGHTS_QUERY], Some(tree_sitter_go::TAGS_QUERY)),
        "C" => (tree_sitter_c::LANGUAGE, vec![tree_sitter_c::HIGHLIGHT_QUERY], Some(tree_sitter_c::TAGS_QUERY)),
        "C++" => (
            tree_sitter_cpp::LANGUAGE,
            vec![tree_sitter_cpp::HIGHLIGHT_QUERY, tree_sitter_c::HIGHLIGHT_QUERY],
            Some(tree_sitter_cpp::TAGS_QUERY),
        ),
        "Bourne Again Shell (bash)" | "Shell-Unix-Generic" => {
            (tree_sitter_bash::LANGUAGE, vec![tree_sitter_bash::HIGHLIGHT_QUERY], None)
        }
        "HTML" => (tree_sitter_html::LANGUAGE, vec![tree_sitter_html::HIGHLIGHTS_QUERY], None),
        "CSS" => (tree_sitter_css::LANGUAGE, vec![tree_sitter_css::HIGHLIGHTS_QUERY], None),
        "TOML" => (tree_sitter_toml_ng::LANGUAGE, vec![tree_sitter_toml_ng::HIGHLIGHTS_QUERY], None),
        "YAML" => (tree_sitter_yaml::LANGUAGE, vec![tree_sitter_yaml::HIGHLIGHTS_QUERY], None),
        "Java" => {
            (tree_sitter_java::LANGUAGE, vec![tree_sitter_java::HIGHLIGHTS_QUERY], Some(tree_sitter_java::TAGS_QUERY))
        }
        "C#" => (
            tree_sitter_c_sharp::LANGUAGE,
            vec![tree_sitter_c_sharp::HIGHLIGHTS_QUERY],
            Some(tree_sitter_c_sharp::TAGS_QUERY),
        ),
        "Ruby" => {
            (tree_sitter_ruby::LANGUAGE, vec![tree_sitter_ruby::HIGHLIGHTS_QUERY], Some(tree_sitter_ruby::TAGS_QUERY))
        }
        "Lua" => {
            (tree_sitter_lua::LANGUAGE, vec![tree_sitter_lua::HIGHLIGHTS_QUERY], Some(tree_sitter_lua::TAGS_QUERY))
        }
        _ => return None,
    })
}

/// Names of the languages with a bundled tree-sitter grammar.
pub const TREE_SITTER_LANGUAGES: &[&str] = &[
    "Rust",
    "Python",
    "JavaScript",
    "TypeScript",
    "TypeScriptReact",
    "JSON",
    "Go",
    "C",
    "C++",
    "Bourne Again Shell (bash)",
    "HTML",
    "CSS",
    "TOML",
    "YAML",
    "Java",
    "C#",
    "Ruby",
    "Lua",
];

impl Grammar {
    /// Compiles the bundled grammar of `language_name`; `None` without one, or when its query
    /// doesn't compile (the TextMate grammar is used instead).
    pub(crate) fn compile(language_name: &str) -> Result<Option<Grammar>, String> {
        let Some((language_fn, queries, tags)) = grammar_source(language_name) else { return Ok(None) };
        let language = tree_sitter::Language::new(language_fn);
        let highlights = Query::new(&language, &queries.join("\n")).map_err(|error| error.to_string())?;
        let tags = tags.map(|tags| Query::new(&language, tags)).transpose().map_err(|error| error.to_string())?;
        let brackets = bracket_query(&language);
        Ok(Some(Grammar { language, highlights, tags, brackets }))
    }
}

/// The chars that are brackets in some language; the language configuration picks its own.
const BRACKET_CHARS: &str = "()[]{}";

/// A query capturing the bracket tokens of `language`: its anonymous node kinds of at most
/// three punctuation chars holding a bracket (`(`, `${`, `$((`, `[[`). Brackets in strings and
/// comments are inside other tokens, so they are not captured. `None` when it has none.
fn bracket_query(language: &tree_sitter::Language) -> Option<Query> {
    let is_bracket_token = |kind: &str| {
        kind.len() <= 3
            && kind.chars().all(|character| character.is_ascii_punctuation() && !"\"\\".contains(character))
            && kind.chars().any(|character| BRACKET_CHARS.contains(character))
    };
    let kinds: BTreeSet<&str> = (0..language.node_kind_count() as u16)
        .filter(|&id| !language.node_kind_is_named(id) && language.node_kind_is_visible(id))
        .filter_map(|id| language.node_kind_for_id(id))
        .filter(|kind| is_bracket_token(kind))
        .collect();
    if kinds.is_empty() {
        return None;
    }
    let alternatives: Vec<String> = kinds.iter().map(|kind| format!("\"{kind}\"")).collect();
    Query::new(language, &format!("[{}] @bracket", alternatives.join(" "))).ok()
}

/// The columns of the bracket chars of the tokens `query` captures on each of `lines`.
fn bracket_columns_of_lines(
    query: &Query,
    cursor: &mut QueryCursor,
    tree: &Tree,
    text: &Text,
    lines: std::ops::Range<usize>,
) -> Vec<Vec<usize>> {
    let last_line = lines.end - 1;
    let block_end = text.line_start_byte(last_line) + text.line_content(last_line).len();
    cursor.set_byte_range(text.line_start_byte(lines.start)..block_end);
    let mut columns_of_lines = vec![Vec::new(); lines.len()];
    let mut matches = cursor.matches(query, tree.root_node(), RopeText(text));
    while let Some(found) = matches.next() {
        for capture in found.captures() {
            let token = capture.node;
            let line = token.start_position().row;
            // An alias may name a token after other text: its kind must be its text.
            if token.is_missing() || !lines.contains(&line) || token.byte_range().len() != token.kind().len() {
                continue;
            }
            let first_column = text.char_of_byte(token.start_byte()) - text.line_start(line);
            let bracket_columns = token
                .kind()
                .chars()
                .enumerate()
                .filter(|(_, character)| BRACKET_CHARS.contains(*character))
                .map(|(index, _)| first_column + index);
            columns_of_lines[line - lines.start].extend(bracket_columns);
        }
    }
    for columns in &mut columns_of_lines {
        columns.sort_unstable();
        columns.dedup();
    }
    columns_of_lines
}

/// TextMate scopes for tree-sitter capture names; a capture uses its longest listed prefix
/// (`function.method.call` → `function.method`).
const CAPTURE_SCOPES: &[(&str, &str)] = &[
    ("comment", "comment"),
    ("string", "string"),
    ("string.special.regex", "string.regexp"),
    ("string.escape", "constant.character.escape"),
    ("escape", "constant.character.escape"),
    ("character", "constant.character"),
    ("number", "constant.numeric"),
    ("float", "constant.numeric"),
    ("boolean", "constant.language"),
    ("constant.builtin", "constant.language"),
    ("constant", "variable.other.constant"),
    ("function.macro", "support.macro"),
    ("function.builtin", "support.function"),
    ("function", "entity.name.function"),
    ("method", "entity.name.function"),
    ("constructor", "entity.name.type"),
    ("type.builtin", "storage.type.primitive"),
    ("type", "entity.name.type"),
    ("keyword.function", "storage.type.function"),
    ("keyword.operator", "keyword.operator"),
    ("keyword.return", "keyword.control"),
    ("keyword.control", "keyword.control"),
    ("keyword", "keyword"),
    ("conditional", "keyword.control"),
    ("repeat", "keyword.control"),
    ("import", "keyword.control.import"),
    ("preproc", "keyword.control.directive"),
    ("charset", "keyword.control.at-rule"),
    ("media", "keyword.control.at-rule"),
    ("keyframes", "keyword.control.at-rule"),
    ("supports", "keyword.control.at-rule"),
    ("operator", "keyword.operator"),
    ("variable.parameter", "variable.parameter"),
    ("parameter", "variable.parameter"),
    ("variable.builtin", "variable.language"),
    ("variable", "variable"),
    ("property", "variable.other.property"),
    ("field", "variable.other.property"),
    ("attribute", "entity.other.attribute-name"),
    ("tag", "entity.name.tag"),
    ("label", "entity.name.label"),
    ("namespace", "entity.name.namespace"),
    ("module", "entity.name.namespace"),
    ("punctuation.delimiter", "punctuation.separator"),
    ("delimiter", "punctuation.separator"),
    ("punctuation", "punctuation"),
];

/// The TextMate scope of a capture name, if any prefix of it is listed.
fn textmate_scope(capture_name: &str) -> Option<&'static str> {
    let mut name = capture_name;
    loop {
        if let Some(&(_, scope)) = CAPTURE_SCOPES.iter().find(|(capture, _)| *capture == name) {
            return Some(scope);
        }
        name = name.rsplit_once('.')?.0;
    }
}

/// The style of each capture of `query` in `theme`; `None` for the default style.
fn capture_styles(query: &Query, theme: &SyntaxTheme) -> Vec<Option<TextStyle>> {
    let highlighter = Highlighter::new(&theme.theme);
    let default_style = highlighter.get_default();
    query
        .capture_names()
        .iter()
        .map(|name| {
            let scope = Scope::new(textmate_scope(name)?).ok()?;
            let style = highlighter.style_for_stack(&[scope]);
            (style != default_style).then(|| text_style(style, default_style))
        })
        .collect()
}

/// The text of syntax nodes, read from the rope chunk by chunk (for `#match?` predicates).
struct RopeText<'text>(&'text Text);

impl<'text> TextProvider<&'text [u8]> for RopeText<'text> {
    type I = Box<dyn Iterator<Item = &'text [u8]> + 'text>;

    fn text(&mut self, node: Node) -> Self::I {
        Box::new(self.0.byte_chunks(node.start_byte(), node.end_byte()))
    }
}

/// The tree-sitter point (row, byte column) of byte `byte_offset`.
fn point_of_byte(text: &Text, byte_offset: usize) -> Point {
    let line = text.line_of_offset(text.char_of_byte(byte_offset));
    Point::new(line, byte_offset - text.line_start_byte(line))
}

/// The point after `inserted`, inserted at `start`.
fn point_after(start: Point, inserted: &str) -> Point {
    match inserted.rsplit_once('\n') {
        Some((before_last_line, last_line)) => {
            Point::new(start.row + before_last_line.matches('\n').count() + 1, last_line.len())
        }
        None => Point::new(start.row, start.column + inserted.len()),
    }
}

/// `change` on `text_before`, as tree-sitter takes it.
fn input_edit(text_before: &Text, change: &Change) -> InputEdit {
    let start_byte = text_before.byte_of_char(change.start);
    let old_end_byte = text_before.byte_of_char(change.end);
    let start_position = point_of_byte(text_before, start_byte);
    InputEdit {
        start_byte,
        old_end_byte,
        new_end_byte: start_byte + change.insert.len(),
        start_position,
        old_end_position: point_of_byte(text_before, old_end_byte),
        new_end_position: point_after(start_position, &change.insert),
    }
}

/// A text to parse, from the tree of an earlier version edited to match it.
struct ParseJob {
    generation: u64,
    text: Text,
    old_tree: Option<Tree>,
}

struct ParsedTree {
    generation: u64,
    tree: Tree,
    parse_duration: Duration,
}

/// A thread parsing jobs one after the other; it ends when its highlighter is dropped.
struct BackgroundParser {
    jobs: Sender<ParseJob>,
    parsed: Receiver<ParsedTree>,
}

impl BackgroundParser {
    fn spawn(language: tree_sitter::Language) -> Self {
        let (jobs, job_receiver) = channel::<ParseJob>();
        let (parsed_sender, parsed) = channel();
        std::thread::spawn(move || {
            let mut parser = Parser::new();
            parser.set_language(&language).expect("bundled grammars match the tree-sitter version");
            for job in job_receiver {
                let started = Instant::now();
                let read_chunk = &mut |byte_offset: usize, _: Point| job.text.chunk_from_byte(byte_offset);
                let Some(tree) = parser.parse_with_options(read_chunk, job.old_tree.as_ref(), None) else { continue };
                let parsed = ParsedTree { generation: job.generation, tree, parse_duration: started.elapsed() };
                if parsed_sender.send(parsed).is_err() {
                    break;
                }
            }
        });
        Self { jobs, parsed }
    }
}

/// The symbol kind of a tags query's `definition.<kind>` capture.
fn symbol_kind(tag_kind: &str) -> SymbolKind {
    match tag_kind {
        "function" | "macro" => SymbolKind::Function,
        "method" => SymbolKind::Method,
        "class" | "interface" | "type" | "struct" | "enum" | "union" | "trait" => SymbolKind::Type,
        "module" | "namespace" => SymbolKind::Module,
        "constant" => SymbolKind::Constant,
        "field" | "property" => SymbolKind::Field,
        "variable" => SymbolKind::Variable,
        _ => SymbolKind::Other,
    }
}

/// Words that end a block on a line of their own, kept visible by its fold.
const CLOSING_KEYWORDS: &[&str] = &["end", "fi", "done", "esac", "endif", "endfor", "endwhile"];

fn is_closing_bracket(node: Node) -> bool {
    matches!(node.kind(), "}" | "]" | ")")
}

/// Whether a multi-line node folds from the line it starts on: a bracketed node (`{…}`,
/// `[…]`, `(…)`) does; a node ending with a bracket doesn't (`if a {…} else {…}` folds
/// through its blocks, not as a whole); another node (a Python `def`, a Ruby `def…end`, a
/// comment, an HTML element) does unless it is the tail of a construct started on an
/// earlier line (a Python `block`, a YAML value).
fn folds_from_its_first_line(node: Node) -> bool {
    let last_index = node.child_count().saturating_sub(1);
    let is_bracketed = node.child_count() >= 2
        && node.child(0).is_some_and(|child| matches!(child.kind(), "{" | "[" | "("))
        && node.child(last_index).is_some_and(is_closing_bracket);
    if is_bracketed {
        return true;
    }
    let mut last_leaf = node;
    while let Some(child) = last_leaf.child(last_leaf.child_count().saturating_sub(1)) {
        last_leaf = child;
    }
    let is_tail = !node.kind().contains("comment")
        && node.parent().is_some_and(|parent| {
            let is_last_named_child =
                parent.named_child(parent.named_child_count().saturating_sub(1) as u32) == Some(node);
            parent.start_position().row < node.start_position().row && is_last_named_child
        });
    !is_closing_bracket(last_leaf) && !is_tail
}

/// Highlights one text with a tree-sitter grammar and one theme. Parsing runs on a thread of
/// its own (an incremental parse of a 3 MB file takes ~11 ms): until a new tree arrives, lines
/// are highlighted from the previous tree, edited to match the text.
pub struct TreeHighlighter {
    grammar: Arc<Grammar>,
    capture_styles: Vec<Option<TextStyle>>,
    parser: BackgroundParser,
    /// The latest tree, edited to match the text (its nodes shifted by the edits).
    tree: Option<Tree>,
    /// The text changed since `tree` was parsed (or there is no tree yet).
    is_tree_stale: bool,
    /// The generation of the job being parsed; a result of an older generation is dropped.
    parsing: Option<u64>,
    /// Bumped when the text changed in a way no edit describes: earlier trees are useless.
    generation: u64,
    /// How long the last parse took: a frame doesn't wait for a parse that won't fit its wait.
    last_parse_duration: Duration,
    /// Edits made while the job was parsed, to apply to its tree when it arrives.
    edits_while_parsing: Vec<InputEdit>,
    /// Spans of lines highlighted from an up-to-date tree.
    line_spans: HashMap<usize, Vec<StyledSpan>>,
    /// Columns of the bracket tokens of lines, from the up-to-date tree.
    line_brackets: HashMap<usize, Vec<usize>>,
    query_cursor: QueryCursor,
    /// How long a frame waits for a parse it starts: small files are highlighted in the frame
    /// that changed them, large ones a frame or more later (D19).
    pub parse_wait: Duration,
    /// Lines longer than this are not highlighted (minified files).
    pub max_line_chars: usize,
}

impl TreeHighlighter {
    pub(crate) fn new(grammar: Arc<Grammar>, theme: &SyntaxTheme) -> Self {
        Self {
            capture_styles: capture_styles(&grammar.highlights, theme),
            parser: BackgroundParser::spawn(grammar.language.clone()),
            grammar,
            tree: None,
            is_tree_stale: true,
            parsing: None,
            generation: 0,
            edits_while_parsing: Vec::new(),
            last_parse_duration: Duration::ZERO,
            line_spans: HashMap::new(),
            line_brackets: HashMap::new(),
            query_cursor: QueryCursor::new(),
            parse_wait: Duration::from_millis(3),
            max_line_chars: 20_000,
        }
    }

    /// Takes a parsed tree if one arrived within `wait`, then starts parsing the text if the
    /// tree is stale and nothing is being parsed.
    fn poll_parser(&mut self, text: &Text, wait: Duration) {
        self.take_parsed_tree(Duration::ZERO);
        if self.is_tree_stale && self.parsing.is_none() {
            let job = ParseJob { generation: self.generation, text: text.clone(), old_tree: self.tree.clone() };
            if self.parser.jobs.send(job).is_ok() {
                self.parsing = Some(self.generation);
                self.edits_while_parsing.clear();
            }
            let is_worth_waiting = self.last_parse_duration <= wait;
            self.take_parsed_tree(if is_worth_waiting { wait } else { Duration::ZERO });
        }
    }

    fn take_parsed_tree(&mut self, wait: Duration) {
        let Some(job_generation) = self.parsing else { return };
        let Ok(ParsedTree { generation, mut tree, parse_duration }) = self.parser.parsed.recv_timeout(wait) else {
            return;
        };
        debug_assert_eq!(generation, job_generation);
        self.parsing = None;
        self.last_parse_duration = parse_duration;
        if generation != self.generation {
            return;
        }
        // Edited while it was parsed: right but for the edited parts, parsed again next.
        self.is_tree_stale = !self.edits_while_parsing.is_empty();
        for edit in self.edits_while_parsing.drain(..) {
            tree.edit(&edit);
        }
        self.tree = Some(tree);
        self.forget_lines();
    }

    /// Drops what was read from the tree: it changed.
    fn forget_lines(&mut self) {
        self.line_spans.clear();
        self.line_brackets.clear();
    }

    /// Waits until the tree matches `text` (tests, and actions that need the exact tree).
    pub fn wait_until_parsed(&mut self, text: &Text) {
        while self.is_tree_stale || self.parsing.is_some() {
            self.poll_parser(text, Duration::from_secs(60));
        }
    }

    /// Whether the tree matches the text.
    pub fn is_parsed(&self) -> bool {
        !self.is_tree_stale && self.parsing.is_none()
    }

    /// The tree no longer matches the text and can't be edited to: parse from scratch.
    fn forget_tree(&mut self) {
        self.tree = None;
        self.is_tree_stale = true;
        self.generation += 1;
        self.edits_while_parsing.clear();
        self.forget_lines();
    }
}

/// Lines highlighted by one query: the visible lines take a few queries, not one each (a
/// query seeking far into a large file is the costly part).
const HIGHLIGHT_BLOCK_LINES: usize = 32;

/// Lines whose brackets one query reads: bracket states are computed line after line, and
/// matching scans far past the visible lines.
const BRACKET_BLOCK_LINES: usize = 256;

/// A line being highlighted: where it is, and the captures crossing it as
/// `(node length, pattern index, start column, end column, style)`.
struct LineCaptures {
    start_byte: usize,
    char_starts: Vec<usize>,
    captured: Vec<(usize, usize, usize, usize, Option<TextStyle>)>,
}

impl LineCaptures {
    fn end_byte(&self, text: &Text, line: usize) -> usize {
        self.start_byte + text.line_content(line).len()
    }

    fn column_of(&self, byte_offset: usize) -> usize {
        self.char_starts.partition_point(|&start| start < byte_offset)
    }

    /// The spans: a smaller node wins over the node holding it, and of two patterns capturing
    /// the same node the first wins.
    fn into_spans(mut self) -> Vec<StyledSpan> {
        self.captured.sort_by_key(|&(node_len, pattern_index, ..)| (Reverse(node_len), Reverse(pattern_index)));
        let mut column_styles: Vec<Option<TextStyle>> = vec![None; self.char_starts.len()];
        for (_, _, start_column, end_column, style) in self.captured {
            column_styles[start_column..end_column].fill(style);
        }
        let mut spans: Vec<StyledSpan> = Vec::new();
        for (column, style) in column_styles.into_iter().enumerate() {
            let Some(style) = style else { continue };
            match spans.last_mut() {
                Some(last) if last.end_column == column && last.style == style => last.end_column = column + 1,
                _ => spans.push(StyledSpan { start_column: column, end_column: column + 1, style }),
            }
        }
        spans
    }
}

/// The spans of `lines` from the captures of `query` crossing them, in one query; lines
/// longer than `max_line_chars` get none.
fn highlight_lines(
    highlighter: (&Query, &[Option<TextStyle>]),
    cursor: &mut QueryCursor,
    tree: &Tree,
    text: &Text,
    lines: std::ops::Range<usize>,
    max_line_chars: usize,
) -> Vec<Vec<StyledSpan>> {
    let (query, capture_styles) = highlighter;
    let mut block: Vec<LineCaptures> = lines
        .clone()
        .map(|line| {
            let start_byte = text.line_start_byte(line);
            let content = text.line_content(line);
            let char_starts = if content.chars().count() > max_line_chars {
                Vec::new()
            } else {
                content.char_indices().map(|(byte, _)| start_byte + byte).collect()
            };
            LineCaptures { start_byte, char_starts, captured: Vec::new() }
        })
        .collect();
    let block_start = block[0].start_byte;
    let block_end = block[block.len() - 1].end_byte(text, lines.end - 1);

    cursor.set_byte_range(block_start..block_end);
    let mut captures = cursor.captures(query, tree.root_node(), RopeText(text));
    while let Some((query_match, capture_index)) = captures.next() {
        let capture = query_match.captures()[*capture_index];
        let node_bytes = capture.node.byte_range();
        let first = block.partition_point(|line| line.start_byte <= node_bytes.start).saturating_sub(1);
        for line in block[first..].iter_mut().take_while(|line| line.start_byte < node_bytes.end) {
            let (start_column, end_column) = (line.column_of(node_bytes.start), line.column_of(node_bytes.end));
            if start_column < end_column {
                let style = capture_styles[capture.index as usize];
                line.captured.push((node_bytes.len(), query_match.pattern_index, start_column, end_column, style));
            }
        }
    }
    block.into_iter().map(LineCaptures::into_spans).collect()
}

impl LineStyler for TreeHighlighter {
    fn style_line(&mut self, text: &Text, line: usize) -> Vec<StyledSpan> {
        if line >= text.line_count() {
            return Vec::new();
        }
        self.poll_parser(text, self.parse_wait);
        if let Some(spans) = self.line_spans.get(&line) {
            return spans.clone();
        }
        let Some(tree) = &self.tree else { return Vec::new() };
        // From an edited tree not parsed again yet, spans are about right; they are kept until
        // the tree changes.
        let lines = line..(line + HIGHLIGHT_BLOCK_LINES).min(text.line_count());
        let highlighter = (&self.grammar.highlights, self.capture_styles.as_slice());
        let block = highlight_lines(highlighter, &mut self.query_cursor, tree, text, lines, self.max_line_chars);
        for (block_line, spans) in (line..).zip(block) {
            self.line_spans.insert(block_line, spans);
        }
        self.line_spans[&line].clone()
    }

    fn invalidate_from(&mut self, _first_changed_line: usize) {
        self.forget_tree();
    }

    fn lines_edited(&mut self, _edited: &EditedLines) {
        self.forget_tree();
    }

    /// Edits the tree, last change first so each one's offsets are still those of the text
    /// it applies to; the next call parses again from it.
    fn text_edited(&mut self, text_before: &Text, changes: &ChangeSet, _edited: &EditedLines) {
        self.forget_lines();
        self.is_tree_stale = true;
        let is_parsing_this_text = self.parsing == Some(self.generation);
        for change in changes.changes().iter().rev() {
            let edit = input_edit(text_before, change);
            if let Some(tree) = &mut self.tree {
                tree.edit(&edit);
            }
            if is_parsing_this_text {
                self.edits_while_parsing.push(edit);
            }
        }
    }

    /// From the latest tree, edited to match the text if a newer parse is still running.
    fn enclosing_syntax_node(&mut self, text: &Text, start: usize, end: usize) -> Option<(usize, usize)> {
        if self.tree.is_none() {
            self.wait_until_parsed(text);
        }
        let tree = self.tree.as_ref()?;
        let (start_byte, end_byte) = (text.byte_of_char(start), text.byte_of_char(end));
        let mut node = tree.root_node().descendant_for_byte_range(start_byte, end_byte)?;
        while node.start_byte() > start_byte
            || node.end_byte() < end_byte
            || (node.start_byte() == start_byte && node.end_byte() == end_byte)
        {
            node = node.parent()?;
        }
        Some((text.char_of_byte(node.start_byte()), text.char_of_byte(node.end_byte())))
    }

    fn is_catching_up(&self) -> bool {
        !self.is_parsed()
    }

    /// From the parsed tree only: an edited tree knows nothing of the brackets just typed.
    fn code_brackets(&mut self, text: &Text, line: usize) -> SyntaxBrackets {
        if self.grammar.brackets.is_none() || line >= text.line_count() {
            return SyntaxBrackets::NoParser;
        }
        self.poll_parser(text, self.parse_wait);
        let (Some(query), Some(tree)) = (&self.grammar.brackets, &self.tree) else { return SyntaxBrackets::Parsing };
        if !self.is_parsed() {
            return SyntaxBrackets::Parsing;
        }
        if let Some(columns) = self.line_brackets.get(&line) {
            return SyntaxBrackets::Columns(columns.clone());
        }
        // Aligned blocks: scanning up, line after line, reads one block per block, not per line.
        let block_start = line / BRACKET_BLOCK_LINES * BRACKET_BLOCK_LINES;
        let lines = block_start..(block_start + BRACKET_BLOCK_LINES).min(text.line_count());
        let block = bracket_columns_of_lines(query, &mut self.query_cursor, tree, text, lines);
        self.line_brackets.extend((block_start..).zip(block));
        SyntaxBrackets::Columns(self.line_brackets[&line].clone())
    }

    fn folds_by_syntax(&self) -> bool {
        true
    }

    /// The `@definition.*` matches of the grammar's tags query, named by their `@name`.
    fn document_symbols(&mut self, text: &Text) -> Vec<DocumentSymbol> {
        if self.grammar.tags.is_none() {
            return Vec::new();
        }
        if self.tree.is_none() {
            self.wait_until_parsed(text);
        }
        let Some(tags) = &self.grammar.tags else { return Vec::new() };
        let Some(tree) = &self.tree else { return Vec::new() };
        let capture_names = tags.capture_names();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(tags, tree.root_node(), RopeText(text));
        let mut symbols = Vec::new();
        while let Some(found) = matches.next() {
            let captures = found.captures();
            let kind =
                captures.iter().find_map(|capture| capture_names[capture.index as usize].strip_prefix("definition."));
            let name = captures.iter().find(|capture| capture_names[capture.index as usize] == "name");
            let (Some(kind), Some(name)) = (kind, name) else { continue };
            let (start, end) = (text.char_of_byte(name.node.start_byte()), text.char_of_byte(name.node.end_byte()));
            symbols.push(DocumentSymbol {
                name: text.slice(start, end),
                kind: symbol_kind(kind),
                range: Range::new(text.position(start), text.position(end)),
                container: None,
            });
        }
        symbols.sort_by_key(|symbol| symbol.range.start);
        symbols.dedup_by(|second, first| first.range == second.range);
        symbols
    }

    /// Looks at the nodes starting on `line`, token by token (so `} else {` starts the `else`);
    /// of those that fold (`folds_from_its_first_line`), the one ending last gives the range,
    /// its last line kept visible when it only closes (`}`, `end`, `</div>`).
    fn syntax_fold_range(&mut self, text: &Text, line: usize) -> Option<FoldRange> {
        let tree = self.tree.as_ref()?;
        let content = text.line_content(line);
        let mut byte_column = content.len() - content.trim_start().len();
        let mut fold_end = Point::new(line, 0);
        while byte_column < content.len() {
            let point = Point::new(line, byte_column);
            let token = tree.root_node().descendant_for_point_range(point, point)?;
            let mut node = token;
            while let Some(parent) = node.parent().filter(|_| node.start_position().row == line) {
                if node.end_position().row > line && folds_from_its_first_line(node) {
                    fold_end = fold_end.max(node.end_position());
                }
                node = parent;
            }
            let token_end = token.end_position();
            byte_column = if token_end.row == line && token_end.column > byte_column {
                token_end.column
            } else {
                content[byte_column..].char_indices().nth(1).map_or(content.len(), |(next, _)| byte_column + next)
            };
        }
        let last_line_content = text.line_content(fold_end.row.min(text.last_line()));
        let before_end = last_line_content.get(..fold_end.column).unwrap_or(&last_line_content);
        let closing = before_end.trim();
        let only_closes = closing.chars().all(|character| ")]};,".contains(character))
            || CLOSING_KEYWORDS.contains(&closing)
            || (closing.starts_with("</") && closing.ends_with('>') && !closing[2..].contains('<'));
        let last_hidden_line = if only_closes { fold_end.row.saturating_sub(1) } else { fold_end.row };
        (last_hidden_line > line).then_some(FoldRange { header_line: line, last_hidden_line })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::LanguageRegistry;

    fn rust_highlighter() -> TreeHighlighter {
        let registry = LanguageRegistry::new();
        TreeHighlighter::new(registry.grammar("Rust").unwrap(), &SyntaxTheme::dark_modern())
    }

    /// `(word, red channel of its color)` for each styled run of `line`.
    fn colored_runs(highlighter: &mut TreeHighlighter, text: &Text, line: usize) -> Vec<(String, u8)> {
        let content: Vec<char> = text.line_content(line).chars().collect();
        highlighter
            .style_line(text, line)
            .iter()
            .map(|span| {
                let run: String = content[span.start_column..span.end_column].iter().collect();
                (run, span.style.foreground.map_or(0, |color| color.red))
            })
            .collect()
    }

    #[test]
    fn every_bundled_grammar_compiles_its_queries() {
        for name in TREE_SITTER_LANGUAGES {
            assert!(matches!(Grammar::compile(name), Ok(Some(_))), "{name}: {:?}", Grammar::compile(name).err());
        }
    }

    #[test]
    fn every_bundled_grammar_but_html_has_bracket_tokens() {
        for name in TREE_SITTER_LANGUAGES {
            let has_brackets = Grammar::compile(name).unwrap().unwrap().brackets.is_some();
            assert_eq!(has_brackets, *name != "HTML", "{name}");
        }
    }

    /// The bracket columns of each line of `content` in `language_name`, from the parsed tree.
    fn code_brackets(language_name: &str, content: &str) -> Vec<Vec<usize>> {
        let registry = LanguageRegistry::new();
        let mut highlighter =
            TreeHighlighter::new(registry.grammar(language_name).unwrap(), &SyntaxTheme::dark_modern());
        let text = Text::new(content);
        highlighter.wait_until_parsed(&text);
        (0..text.line_count())
            .map(|line| match highlighter.code_brackets(&text, line) {
                SyntaxBrackets::Columns(columns) => columns,
                other => panic!("line {line}: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn brackets_in_raw_strings_chars_and_comments_are_not_tokens() {
        let rust = "f(r#\"(\"#, '(', /* ( /* ) */ [ */ x)";

        assert_eq!(code_brackets("Rust", rust), vec![vec![1, 34]]);
    }

    #[test]
    fn tokens_of_several_chars_hold_their_brackets() {
        let bash = "echo $(ls ${dir})";

        assert_eq!(code_brackets("Bourne Again Shell (bash)", bash), vec![vec![6, 11, 15, 16]]);
    }

    #[test]
    fn brackets_wait_for_the_tree_to_match_the_text() {
        let mut highlighter = rust_highlighter();
        highlighter.parse_wait = Duration::ZERO;
        let text = Text::new(&"fn f() {}\n".repeat(20_000));

        assert_eq!(highlighter.code_brackets(&text, 0), SyntaxBrackets::Parsing);
        highlighter.wait_until_parsed(&text);
        assert_eq!(highlighter.code_brackets(&text, 0), SyntaxBrackets::Columns(vec![4, 5, 7, 8]));
    }

    #[test]
    fn captures_take_the_theme_colors_of_their_scopes() {
        let mut highlighter = rust_highlighter();
        let text = Text::new("fn main() { let s = \"é\\n\"; } // done\n");

        let runs = colored_runs(&mut highlighter, &text, 0);

        assert!(runs.contains(&("fn".into(), 0x56)), "{runs:?}");
        assert!(runs.contains(&("main".into(), 0xdc)), "{runs:?}");
        assert!(runs.contains(&("\\n".into(), 0xd7)), "{runs:?}");
        assert!(runs.contains(&("// done".into(), 0x6a)), "{runs:?}");
    }

    #[test]
    fn a_block_comment_colors_every_line_it_spans() {
        let mut highlighter = rust_highlighter();
        let text = Text::new("/* one\ntwo */ fn f() {}\n");

        assert_eq!(colored_runs(&mut highlighter, &text, 1)[0], ("two */".into(), 0x6a));
    }

    #[test]
    fn an_edit_is_parsed_incrementally_from_the_edited_tree() {
        let mut highlighter = rust_highlighter();
        let before = Text::new("fn f() {}\nlet x = 1;\n");
        highlighter.style_line(&before, 1);
        let changes = ChangeSet::single(Change::insert(0, "/* "));
        let after = changes.apply(&before);

        highlighter.text_edited(&before, &changes, &EditedLines { first_line: 0, old_last_line: 0, new_last_line: 0 });
        highlighter.wait_until_parsed(&after);

        assert_eq!(colored_runs(&mut highlighter, &after, 1)[0].1, 0x6a);
    }

    #[test]
    fn a_long_parse_goes_on_while_lines_are_painted() {
        let mut highlighter = rust_highlighter();
        highlighter.parse_wait = Duration::ZERO;
        let text = Text::new(&"fn f(argument: usize) -> usize { argument + 1 }\n".repeat(20_000));

        highlighter.style_line(&text, 0);
        assert!(highlighter.is_catching_up());
        highlighter.wait_until_parsed(&text);
        assert!(!highlighter.is_catching_up());
        assert!(!highlighter.style_line(&text, 0).is_empty());
    }

    #[test]
    fn edits_made_while_parsing_reach_the_parsed_tree() {
        let mut highlighter = rust_highlighter();
        highlighter.parse_wait = Duration::ZERO;
        let before = Text::new(&"fn f() {}\n".repeat(20_000));
        highlighter.style_line(&before, 0);
        let changes = ChangeSet::single(Change::insert(0, "/* "));
        let after = changes.apply(&before);

        highlighter.text_edited(&before, &changes, &EditedLines { first_line: 0, old_last_line: 0, new_last_line: 0 });
        highlighter.wait_until_parsed(&after);

        assert_eq!(colored_runs(&mut highlighter, &after, 5)[0].1, 0x6a);
    }

    fn fold_ranges(language_name: &str, content: &str) -> Vec<(usize, usize)> {
        let registry = LanguageRegistry::new();
        let mut highlighter =
            TreeHighlighter::new(registry.grammar(language_name).unwrap(), &SyntaxTheme::dark_modern());
        let text = Text::new(content);
        highlighter.wait_until_parsed(&text);
        (0..text.line_count())
            .filter_map(|line| highlighter.syntax_fold_range(&text, line))
            .map(|range| (range.header_line, range.last_hidden_line))
            .collect()
    }

    #[test]
    fn tags_queries_give_the_definitions_in_text_order() {
        let registry = LanguageRegistry::new();
        let mut highlighter = TreeHighlighter::new(registry.grammar("Rust").unwrap(), &SyntaxTheme::dark_modern());
        let text = Text::new("struct Point;\nimpl Point {\n    fn length(&self) {}\n}\nfn main() {}\n");

        let symbols: Vec<(String, SymbolKind, usize)> = highlighter
            .document_symbols(&text)
            .into_iter()
            .map(|symbol| (symbol.name, symbol.kind, symbol.range.start.line))
            .collect();

        assert_eq!(
            symbols,
            vec![
                ("Point".into(), SymbolKind::Type, 0),
                ("length".into(), SymbolKind::Method, 2),
                ("main".into(), SymbolKind::Function, 4)
            ]
        );
    }

    #[test]
    fn syntax_folds_keep_the_closing_bracket_line_visible() {
        let rust = "fn f() {\n    if a {\n        b();\n    } else {\n        c();\n    }\n}\n";

        assert_eq!(fold_ranges("Rust", rust), vec![(0, 5), (1, 2), (3, 4)]);
    }

    #[test]
    fn syntax_folds_without_closing_brackets_hide_to_the_last_line() {
        let python = "def f():\n    a = 1\n    return a\n\nprint(f())\n";

        assert_eq!(fold_ranges("Python", python), vec![(0, 2)]);
    }

    #[test]
    fn syntax_folds_keep_a_closing_keyword_line_visible() {
        let ruby = "def f\n  a = 1\n  a\nend\n";

        assert_eq!(fold_ranges("Ruby", ruby), vec![(0, 2)]);
    }

    #[test]
    fn block_comments_fold() {
        let c = "/*\n * one\n * two\n */\nint a;\n";

        assert_eq!(fold_ranges("C", c), vec![(0, 3)]);
    }

    #[test]
    fn syntax_folds_follow_structure_not_indentation() {
        let json = "{\n\"a\": [\n1,\n2\n]\n}\n";

        assert_eq!(fold_ranges("JSON", json), vec![(0, 4), (1, 3)]);
    }

    #[test]
    fn enclosing_nodes_grow_from_the_cursor_to_the_whole_text() {
        let mut highlighter = rust_highlighter();
        let text = Text::new("fn f() { g(1 + 2); }");
        let cursor = 11; // On the `1`.

        let literal = highlighter.enclosing_syntax_node(&text, cursor, cursor).unwrap();
        let sum = highlighter.enclosing_syntax_node(&text, literal.0, literal.1).unwrap();

        assert_eq!(literal, (11, 12));
        assert_eq!(sum, (11, 16));
        let mut range = sum;
        while let Some(larger) = highlighter.enclosing_syntax_node(&text, range.0, range.1) {
            assert!(larger.0 <= range.0 && range.1 <= larger.1 && larger != range);
            range = larger;
        }
        assert_eq!(range, (0, text.len_chars()));
    }
}
