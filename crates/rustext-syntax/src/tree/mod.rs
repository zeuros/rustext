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

use crate::language::LanguageRegistry;
use crate::textmate::text_style;
use crate::theme::SyntaxTheme;

mod grammar;
mod nodes;
mod parser;

use grammar::*;
pub use grammar::{Grammar, TREE_SITTER_LANGUAGES};
use nodes::*;
use parser::*;

/// A syntax tree of the text: the one of the text's own language (the first layer), or of a
/// language embedded in it (DECISIONS D28), parsed over the parts the first layer's injection
/// query captures (the scripts and styles of a page).
struct Layer {
    grammar: Arc<Grammar>,
    capture_styles: Vec<Option<TextStyle>>,
    /// The latest tree, edited to match the text (its nodes shifted by the edits); `None`
    /// before the first parse, and for an embedded language the text holds none of.
    tree: Option<Tree>,
}

/// Highlights one text with a tree-sitter grammar and one theme. Parsing runs on a thread of
/// its own (an incremental parse of a 3 MB file takes ~11 ms): until new trees arrive, lines
/// are highlighted from the previous trees, edited to match the text.
pub struct TreeHighlighter {
    /// The text's own language first, then the languages embedded in it.
    layers: Vec<Layer>,
    parser: BackgroundParser,
    /// The text changed since the trees were parsed (or there are no trees yet).
    is_tree_stale: bool,
    /// The generation of the job being parsed; a result of an older generation is dropped.
    parsing: Option<u64>,
    /// Bumped when the text changed in a way no edit describes: earlier trees are useless.
    generation: u64,
    /// How long the last parse took: a frame doesn't wait for a parse that won't fit its wait.
    last_parse_duration: Duration,
    /// Edits made while the job was parsed, to apply to its trees when they arrive.
    edits_while_parsing: Vec<InputEdit>,
    /// Spans of lines highlighted from up-to-date trees.
    line_spans: HashMap<usize, Vec<StyledSpan>>,
    /// Columns of the bracket tokens of lines, from the up-to-date trees.
    line_brackets: HashMap<usize, Vec<usize>>,
    query_cursor: QueryCursor,
    /// How long a frame waits for a parse it starts: small files are highlighted in the frame
    /// that changed them, large ones a frame or more later (D19).
    pub parse_wait: Duration,
    /// Lines longer than this are not highlighted (minified files).
    pub max_line_chars: usize,
}

impl TreeHighlighter {
    /// Highlights with `grammar`, and the grammars `registry` has of the languages embedded in it.
    pub(crate) fn new(grammar: Arc<Grammar>, theme: &SyntaxTheme, registry: &LanguageRegistry) -> Self {
        let (embedded, layer_of_pattern) = embedded_grammars(&grammar, registry);
        let grammars: Vec<Arc<Grammar>> = std::iter::once(grammar).chain(embedded).collect();
        let layers = grammars
            .iter()
            .map(|grammar| Layer {
                grammar: grammar.clone(),
                capture_styles: capture_styles(&grammar.highlights, theme),
                tree: None,
            })
            .collect();
        Self {
            layers,
            parser: BackgroundParser::spawn(grammars, layer_of_pattern),
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

    /// The tree of the text's own language.
    fn tree(&self) -> Option<&Tree> {
        self.layers[0].tree.as_ref()
    }

    /// Takes parsed trees if they arrived within `wait`, then starts parsing the text if the
    /// trees are stale and nothing is being parsed.
    fn poll_parser(&mut self, text: &Text, wait: Duration) {
        self.take_parsed_trees(Duration::ZERO);
        if self.is_tree_stale && self.parsing.is_none() {
            let old_trees = self.layers.iter().map(|layer| layer.tree.clone()).collect();
            let job = ParseJob { generation: self.generation, text: text.clone(), old_trees };
            if self.parser.jobs.send(job).is_ok() {
                self.parsing = Some(self.generation);
                self.edits_while_parsing.clear();
            }
            let is_worth_waiting = self.last_parse_duration <= wait;
            self.take_parsed_trees(if is_worth_waiting { wait } else { Duration::ZERO });
        }
    }

    fn take_parsed_trees(&mut self, wait: Duration) {
        let Some(job_generation) = self.parsing else { return };
        let Ok(ParsedTrees { generation, trees, parse_duration }) = self.parser.parsed.recv_timeout(wait) else {
            return;
        };
        debug_assert_eq!(generation, job_generation);
        self.parsing = None;
        self.last_parse_duration = parse_duration;
        if generation != self.generation {
            return;
        }
        // Edited while they were parsed: right but for the edited parts, parsed again next.
        self.is_tree_stale = !self.edits_while_parsing.is_empty();
        for (layer, mut tree) in self.layers.iter_mut().zip(trees) {
            for edit in &self.edits_while_parsing {
                if let Some(tree) = &mut tree {
                    tree.edit(edit);
                }
            }
            layer.tree = tree;
        }
        self.edits_while_parsing.clear();
        self.forget_lines();
    }

    /// Drops what was read from the trees: they changed.
    fn forget_lines(&mut self) {
        self.line_spans.clear();
        self.line_brackets.clear();
    }

    /// Waits until the trees match `text` (tests, and actions that need the exact trees).
    pub fn wait_until_parsed(&mut self, text: &Text) {
        while self.is_tree_stale || self.parsing.is_some() {
            self.poll_parser(text, Duration::from_secs(60));
        }
    }

    /// Whether the trees match the text.
    pub fn is_parsed(&self) -> bool {
        !self.is_tree_stale && self.parsing.is_none()
    }

    /// The trees no longer match the text and can't be edited to: parse from scratch.
    fn forget_tree(&mut self) {
        for layer in &mut self.layers {
            layer.tree = None;
        }
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

impl LineStyler for TreeHighlighter {
    fn style_line(&mut self, text: &Text, line: usize) -> Vec<StyledSpan> {
        if line >= text.line_count() {
            return Vec::new();
        }
        self.poll_parser(text, self.parse_wait);
        if let Some(spans) = self.line_spans.get(&line) {
            return spans.clone();
        }
        if self.tree().is_none() {
            return Vec::new();
        }
        // From edited trees not parsed again yet, spans are about right; they are kept until
        // the trees change.
        let lines = line..(line + HIGHLIGHT_BLOCK_LINES).min(text.line_count());
        let block = highlight_lines(&self.layers, &mut self.query_cursor, text, lines, self.max_line_chars);
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

    /// Edits the trees, last change first so each one's offsets are still those of the text
    /// it applies to; the next call parses again from them.
    fn text_edited(&mut self, text_before: &Text, changes: &ChangeSet, _edited: &EditedLines) {
        self.forget_lines();
        self.is_tree_stale = true;
        let is_parsing_this_text = self.parsing == Some(self.generation);
        for change in changes.changes().iter().rev() {
            let edit = input_edit(text_before, change);
            for tree in self.layers.iter_mut().filter_map(|layer| layer.tree.as_mut()) {
                tree.edit(&edit);
            }
            if is_parsing_this_text {
                self.edits_while_parsing.push(edit);
            }
        }
    }

    /// The smallest node of any layer holding the range, a node of an embedded language only
    /// within one of its parts; from the latest trees, edited to match the text if a newer
    /// parse is still running.
    fn enclosing_syntax_node(&mut self, text: &Text, start: usize, end: usize) -> Option<(usize, usize)> {
        if self.tree().is_none() {
            self.wait_until_parsed(text);
        }
        let (start_byte, end_byte) = (text.byte_of_char(start), text.byte_of_char(end));
        let node = self
            .layers
            .iter()
            .filter_map(|layer| {
                let tree = layer.tree.as_ref()?;
                let node = enclosing_node(tree, start_byte, end_byte)?;
                let is_in_a_part =
                    parsed_byte_ranges(tree).iter().any(|parsed| parsed.start <= node.start && node.end <= parsed.end);
                is_in_a_part.then_some(node)
            })
            .min_by_key(|node| node.len())?;
        Some((text.char_of_byte(node.start), text.char_of_byte(node.end)))
    }

    fn is_catching_up(&self) -> bool {
        !self.is_parsed()
    }

    /// From the parsed trees only: an edited tree knows nothing of the brackets just typed.
    /// The text's own grammar decides whether there are bracket tokens at all.
    fn code_brackets(&mut self, text: &Text, line: usize) -> SyntaxBrackets {
        if self.layers[0].grammar.brackets.is_none() || line >= text.line_count() {
            return SyntaxBrackets::NoParser;
        }
        self.poll_parser(text, self.parse_wait);
        if self.tree().is_none() || !self.is_parsed() {
            return SyntaxBrackets::Parsing;
        }
        if let Some(columns) = self.line_brackets.get(&line) {
            return SyntaxBrackets::Columns(columns.clone());
        }

        // Aligned blocks: scanning up, line after line, reads one block per block, not per line.
        let block_start = line / BRACKET_BLOCK_LINES * BRACKET_BLOCK_LINES;
        let lines = block_start..(block_start + BRACKET_BLOCK_LINES).min(text.line_count());
        let mut block = vec![Vec::new(); lines.len()];
        for layer in &self.layers {
            let (Some(query), Some(tree)) = (&layer.grammar.brackets, &layer.tree) else { continue };
            let layer_block = bracket_columns_of_lines(query, &mut self.query_cursor, tree, text, lines.clone());
            for (columns, layer_columns) in block.iter_mut().zip(layer_block) {
                columns.extend(layer_columns);
            }
        }
        for columns in &mut block {
            columns.sort_unstable();
            columns.dedup();
        }
        self.line_brackets.extend((block_start..).zip(block));
        SyntaxBrackets::Columns(self.line_brackets[&line].clone())
    }

    fn folds_by_syntax(&self) -> bool {
        true
    }

    /// The definitions the tags queries of the layers find.
    fn document_symbols(&mut self, text: &Text) -> Vec<DocumentSymbol> {
        if self.layers.iter().all(|layer| layer.grammar.tags.is_none()) {
            return Vec::new();
        }
        if self.tree().is_none() {
            self.wait_until_parsed(text);
        }
        let mut symbols: Vec<DocumentSymbol> = self
            .layers
            .iter()
            .filter_map(|layer| Some(tagged_symbols(layer.grammar.tags.as_ref()?, layer.tree.as_ref()?, text)))
            .flatten()
            .collect();
        symbols.sort_by_key(|symbol| symbol.range.start);
        symbols.dedup_by(|second, first| first.range == second.range);
        symbols
    }

    /// The fold of `line` reaching furthest in the layers whose parts cross it.
    fn syntax_fold_range(&mut self, text: &Text, line: usize) -> Option<FoldRange> {
        let line_start_byte = text.line_start_byte(line);
        let line_bytes = line_start_byte..line_start_byte + text.line_content(line).len();
        self.layers
            .iter()
            .filter_map(|layer| layer.tree.as_ref())
            .filter(|tree| {
                parsed_byte_ranges(tree)
                    .iter()
                    .any(|parsed| parsed.start <= line_bytes.end && line_bytes.start < parsed.end)
            })
            .filter_map(|tree| syntax_fold_range_in(tree, text, line))
            .max_by_key(|fold| fold.last_hidden_line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::LanguageRegistry;

    fn rust_highlighter() -> TreeHighlighter {
        let registry = LanguageRegistry::new();
        TreeHighlighter::new(registry.grammar("Rust").unwrap(), &SyntaxTheme::dark_modern(), &registry)
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
    fn every_bundled_grammar_has_a_bracket_query() {
        for name in TREE_SITTER_LANGUAGES {
            assert!(Grammar::compile(name).unwrap().unwrap().brackets.is_some(), "{name}");
        }
    }

    /// The bracket columns of each line of `content` in `language_name`, from the parsed tree.
    fn code_brackets(language_name: &str, content: &str) -> Vec<Vec<usize>> {
        let registry = LanguageRegistry::new();
        let mut highlighter =
            TreeHighlighter::new(registry.grammar(language_name).unwrap(), &SyntaxTheme::dark_modern(), &registry);
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
            TreeHighlighter::new(registry.grammar(language_name).unwrap(), &SyntaxTheme::dark_modern(), &registry);
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
        let mut highlighter =
            TreeHighlighter::new(registry.grammar("Rust").unwrap(), &SyntaxTheme::dark_modern(), &registry);
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

    fn parsed_highlighter(language_name: &str, text: &Text) -> TreeHighlighter {
        let registry = LanguageRegistry::new();
        let mut highlighter =
            TreeHighlighter::new(registry.grammar(language_name).unwrap(), &SyntaxTheme::dark_modern(), &registry);
        highlighter.wait_until_parsed(text);
        highlighter
    }

    const PAGE: &str =
        "<p>(see)</p>\n<script>\nfunction f() { return \")\"; }\n</script>\n<style>\nbody { color: red; }\n</style>\n";

    #[test]
    fn scripts_of_a_page_are_colored_as_javascript() {
        let page = Text::new(PAGE);
        let script = Text::new("function f() { return \")\"; }");

        let in_page = colored_runs(&mut parsed_highlighter("HTML", &page), &page, 2);

        assert_eq!(in_page, colored_runs(&mut parsed_highlighter("JavaScript", &script), &script, 0));
    }

    #[test]
    fn styles_of_a_page_are_colored_as_css() {
        let page = Text::new(PAGE);
        let style = Text::new("body { color: red; }");

        let in_page = colored_runs(&mut parsed_highlighter("HTML", &page), &page, 5);

        assert_eq!(in_page, colored_runs(&mut parsed_highlighter("CSS", &style), &style, 0));
    }

    #[test]
    fn brackets_of_a_page_are_those_of_its_prose_and_its_scripts() {
        let brackets = code_brackets("HTML", PAGE);

        assert_eq!(brackets[0], vec![3, 7]);
        assert_eq!(brackets[2], vec![10, 11, 13, 27]);
        assert_eq!(brackets[5], vec![5, 19]);
    }

    #[test]
    fn a_script_edited_is_colored_from_its_new_tree() {
        let page = Text::new(PAGE);
        let mut highlighter = parsed_highlighter("HTML", &page);
        let changes = ChangeSet::new([Change::insert(page.offset(rustext_core::Position::new(2, 0)), "const a = 1; ")]);
        let edited = changes.apply(&page);

        highlighter.text_edited(&page, &changes, &EditedLines { first_line: 2, old_last_line: 2, new_last_line: 2 });
        highlighter.wait_until_parsed(&edited);

        let script = Text::new("const a = 1; function f() { return \")\"; }");
        let expected = colored_runs(&mut parsed_highlighter("JavaScript", &script), &script, 0);
        assert_eq!(colored_runs(&mut highlighter, &edited, 2), expected);
    }

    #[test]
    fn functions_of_scripts_are_symbols_of_the_page() {
        let page = Text::new(PAGE);

        let symbols = parsed_highlighter("HTML", &page).document_symbols(&page);

        let names: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, vec!["f"]);
    }

    #[test]
    fn functions_of_scripts_fold() {
        let folds = fold_ranges("HTML", "<script>\nfunction f() {\n  a();\n}\n</script>\n");

        assert!(folds.contains(&(1, 2)), "{folds:?}");
    }

    #[test]
    fn smart_select_grows_through_a_script_to_its_element() {
        let page = Text::new("<script>f(a)</script>");
        let mut highlighter = parsed_highlighter("HTML", &page);

        let arguments = highlighter.enclosing_syntax_node(&page, 10, 11);
        let call = highlighter.enclosing_syntax_node(&page, 9, 12);

        assert_eq!(arguments, Some((9, 12)));
        assert_eq!(call, Some((8, 12)));
    }
}
