//! Highlighting from a tree-sitter syntax tree (DECISIONS D22): the text is parsed once, edits
//! are applied to the tree and parsed again incrementally, and each painted line asks the
//! grammar's highlight query for the captures crossing it. Capture names (`keyword`,
//! `function.method`) are mapped to TextMate scopes, so every theme keeps working.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use rustext_core::style::{LineStyler, StyledSpan, TextStyle};
use rustext_core::{Change, ChangeSet, EditedLines, Text};
use syntect::highlighting::Highlighter;
use syntect::parsing::Scope;
use tree_sitter::{InputEdit, Node, Parser, Point, Query, QueryCursor, StreamingIterator, TextProvider, Tree};
use tree_sitter_language::LanguageFn;

use crate::textmate::text_style;
use crate::theme::SyntaxTheme;

/// A compiled grammar: the language and its highlight query.
pub struct Grammar {
    language: tree_sitter::Language,
    highlights: Query,
}

impl std::fmt::Debug for Grammar {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Grammar").field("captures", &self.highlights.capture_names().len()).finish()
    }
}

/// The bundled tree-sitter grammar of a language (by its syntect name) and its highlight
/// queries, the first winning where two capture the same node.
fn grammar_source(language_name: &str) -> Option<(LanguageFn, Vec<&'static str>)> {
    let javascript = tree_sitter_javascript::HIGHLIGHT_QUERY;
    let jsx = tree_sitter_javascript::JSX_HIGHLIGHT_QUERY;
    let typescript = tree_sitter_typescript::HIGHLIGHTS_QUERY;
    Some(match language_name {
        "Rust" => (tree_sitter_rust::LANGUAGE, vec![tree_sitter_rust::HIGHLIGHTS_QUERY]),
        "Python" => (tree_sitter_python::LANGUAGE, vec![tree_sitter_python::HIGHLIGHTS_QUERY]),
        "JavaScript" | "JavaScript (Babel)" => (tree_sitter_javascript::LANGUAGE, vec![jsx, javascript]),
        "TypeScript" => (tree_sitter_typescript::LANGUAGE_TYPESCRIPT, vec![typescript, javascript]),
        "TypeScriptReact" => (tree_sitter_typescript::LANGUAGE_TSX, vec![typescript, jsx, javascript]),
        "JSON" => (tree_sitter_json::LANGUAGE, vec![tree_sitter_json::HIGHLIGHTS_QUERY]),
        "Go" => (tree_sitter_go::LANGUAGE, vec![tree_sitter_go::HIGHLIGHTS_QUERY]),
        "C" => (tree_sitter_c::LANGUAGE, vec![tree_sitter_c::HIGHLIGHT_QUERY]),
        "C++" => (tree_sitter_cpp::LANGUAGE, vec![tree_sitter_cpp::HIGHLIGHT_QUERY, tree_sitter_c::HIGHLIGHT_QUERY]),
        "Bourne Again Shell (bash)" | "Shell-Unix-Generic" => {
            (tree_sitter_bash::LANGUAGE, vec![tree_sitter_bash::HIGHLIGHT_QUERY])
        }
        "HTML" => (tree_sitter_html::LANGUAGE, vec![tree_sitter_html::HIGHLIGHTS_QUERY]),
        "CSS" => (tree_sitter_css::LANGUAGE, vec![tree_sitter_css::HIGHLIGHTS_QUERY]),
        "TOML" => (tree_sitter_toml_ng::LANGUAGE, vec![tree_sitter_toml_ng::HIGHLIGHTS_QUERY]),
        "YAML" => (tree_sitter_yaml::LANGUAGE, vec![tree_sitter_yaml::HIGHLIGHTS_QUERY]),
        "Java" => (tree_sitter_java::LANGUAGE, vec![tree_sitter_java::HIGHLIGHTS_QUERY]),
        "C#" => (tree_sitter_c_sharp::LANGUAGE, vec![tree_sitter_c_sharp::HIGHLIGHTS_QUERY]),
        "Ruby" => (tree_sitter_ruby::LANGUAGE, vec![tree_sitter_ruby::HIGHLIGHTS_QUERY]),
        "Lua" => (tree_sitter_lua::LANGUAGE, vec![tree_sitter_lua::HIGHLIGHTS_QUERY]),
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
        let Some((language_fn, queries)) = grammar_source(language_name) else { return Ok(None) };
        let language = tree_sitter::Language::new(language_fn);
        let highlights = Query::new(&language, &queries.join("\n")).map_err(|error| error.to_string())?;
        Ok(Some(Grammar { language, highlights }))
    }
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
        self.line_spans.clear();
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
        self.line_spans.clear();
    }
}

/// Lines highlighted by one query: the visible lines take a few queries, not one each (a
/// query seeking far into a large file is the costly part).
const HIGHLIGHT_BLOCK_LINES: usize = 32;

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
        self.line_spans.clear();
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

        assert_eq!(colored_runs(&mut highlighter, &after, 1)[0].1, 0x6a);
        assert!(highlighter.is_parsed());
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
