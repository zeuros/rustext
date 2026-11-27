//! Structure read from the text: matching brackets, indentation guides, indentation folding
//! ranges. Brackets come from the styler's syntax tree when it parses the text (D27), else from
//! a lexer reading the language configuration (D21).

use crate::config::LanguageConfiguration;
use crate::indentation::Indentation;
use crate::lexical::{LexicalContext, LineScan, scan_line_chars};
use crate::style::LineStyler;
use crate::text::Text;

/// Brackets further apart than this many chars are not matched (keeps the search bounded).
const MAX_BRACKET_DISTANCE: usize = 200_000;

/// The code brackets of a line as a styler's syntax tree tells them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyntaxBrackets {
    /// The styler doesn't parse the text: the lexer tells them (D21).
    NoParser,
    /// The tree doesn't match the text yet: the lexer tells them meanwhile.
    Parsing,
    /// The columns of the line's bracket tokens, in order (brackets in strings and comments
    /// are not tokens).
    Columns(Vec<usize>),
}

/// Where the code brackets of a line come from: the syntax tree of the styler when it has one
/// matching the text, else the lexer.
pub struct BracketScanner<'a> {
    language: &'a LanguageConfiguration,
    styler: Option<&'a mut dyn LineStyler>,
}

impl<'a> BracketScanner<'a> {
    pub fn new<'styler: 'a>(
        language: &'a LanguageConfiguration,
        styler: Option<&'a mut (dyn LineStyler + 'styler)>,
    ) -> Self {
        Self { language, styler: styler.map(|styler| styler as &mut dyn LineStyler) }
    }

    /// Brackets from the lexer alone.
    pub fn lexer(language: &'a LanguageConfiguration) -> Self {
        Self { language, styler: None }
    }

    pub fn language(&self) -> &LanguageConfiguration {
        self.language
    }

    /// The code brackets of `line`, starting in `context` (which only the lexer reads).
    pub fn scan(&mut self, text: &Text, line: usize, context: LexicalContext) -> LineScan {
        self.scan_telling_if_provisional(text, line, context).0
    }

    /// `scan`, and whether the lexer answered for a tree still being parsed.
    fn scan_telling_if_provisional(&mut self, text: &Text, line: usize, context: LexicalContext) -> (LineScan, bool) {
        let syntax_brackets =
            self.styler.as_deref_mut().map_or(SyntaxBrackets::NoParser, |styler| styler.code_brackets(text, line));
        let columns = match syntax_brackets {
            SyntaxBrackets::Columns(columns) => columns,
            other => {
                let scan = scan_line_chars(line_chars(text, line), context, self.language);
                return (scan, other == SyntaxBrackets::Parsing);
            }
        };
        let line_start = text.line_start(line);
        let brackets = columns
            .into_iter()
            .filter_map(|column| Some((column, text.char_at(line_start + column)?)))
            .filter(|&(_, character)| self.language.is_bracket(character))
            .collect();
        (LineScan { brackets, context_at_end: LexicalContext::Code }, false)
    }

    /// Whether the tree answers for `line` again after the lexer answered while it was parsed.
    fn is_parsed(&mut self, text: &Text, line: usize) -> bool {
        let styler = self.styler.as_deref_mut();
        styler.is_none_or(|styler| styler.code_brackets(text, line) != SyntaxBrackets::Parsing)
    }
}

/// The code bracket touching `offset` (right after it, else right before it) and its match, as
/// two char offsets in text order. Brackets in strings and comments neither match nor count.
/// `None` also when the line states before `offset` would take more than `budget_lines`.
pub fn matching_bracket_pair(
    text: &Text,
    offset: usize,
    scanner: &mut BracketScanner,
    line_starts: &mut BracketLineStarts,
    budget_lines: &mut usize,
) -> Option<(usize, usize)> {
    let candidates = [Some(offset), offset.checked_sub(1)];
    candidates.into_iter().flatten().find_map(|position| {
        let line = text.line_of_offset(position);
        let column = position - text.line_start(line);
        let start = line_starts.state_at(text, scanner, line, budget_lines)?;
        let scan = scanner.scan(text, line, start.context);
        let &(_, character) = scan.brackets.iter().find(|(bracket_column, _)| *bracket_column == column)?;
        let &(open, close) =
            scanner.language.brackets.iter().find(|(open, close)| *open == character || *close == character)?;
        let matched = if character == open {
            find_closing(text, scanner, line, column, scan, (open, close))
        } else {
            find_opening(text, scanner, line_starts, line, column, scan.brackets, (open, close))
        }?;
        Some((position.min(matched), position.max(matched)))
    })
}

#[expect(clippy::too_many_arguments, reason = "predates the complexity lints (D28)")]
fn find_closing(
    text: &Text,
    scanner: &mut BracketScanner,
    open_line: usize,
    open_column: usize,
    open_line_scan: LineScan,
    (open, close): (char, char),
) -> Option<usize> {
    let mut depth = 0usize;
    let mut scanned_chars = 0;
    let mut scan = open_line_scan;
    let mut line = open_line;
    loop {
        let after_open = |&&(column, _): &&(usize, char)| line != open_line || column > open_column;
        for &(column, character) in scan.brackets.iter().filter(after_open) {
            if character == open {
                depth += 1;
            } else if character == close {
                if depth == 0 {
                    return Some(text.line_start(line) + column);
                }
                depth -= 1;
            }
        }
        scanned_chars += text.line_len(line) + 1;
        if line == text.last_line() || scanned_chars > MAX_BRACKET_DISTANCE {
            return None;
        }
        line += 1;
        scan = scanner.scan(text, line, scan.context_at_end);
    }
}

#[expect(clippy::too_many_arguments, reason = "predates the complexity lints (D28)")]
fn find_opening(
    text: &Text,
    scanner: &mut BracketScanner,
    line_starts: &mut BracketLineStarts,
    close_line: usize,
    close_column: usize,
    close_line_brackets: Vec<(usize, char)>,
    (open, close): (char, char),
) -> Option<usize> {
    let mut depth = 0usize;
    let is_the_opening = |character: char| {
        if character == close {
            depth += 1;
        } else if character == open {
            if depth == 0 {
                return true;
            }
            depth -= 1;
        }
        false
    };
    find_bracket_before(text, scanner, line_starts, (close_line, close_column), close_line_brackets, is_the_opening)
}

/// The code bracket of the innermost pair holding `offset` (after its opening bracket, not
/// after its closing one): its offsets and nesting depth (for its color). `None` outside any
/// pair, or when the line states before `offset` would take more than `budget_lines`.
pub fn enclosing_bracket_pair(
    text: &Text,
    offset: usize,
    scanner: &mut BracketScanner,
    line_starts: &mut BracketLineStarts,
    budget_lines: &mut usize,
) -> Option<EnclosingBracketPair> {
    let line = text.line_of_offset(offset);
    let column = offset - text.line_start(line);
    let start = line_starts.state_at(text, scanner, line, budget_lines)?;
    let brackets = scanner.scan(text, line, start.context).brackets;
    let mut unmatched_closers: Vec<char> = Vec::new();
    let language = scanner.language;
    let is_unclosed_opening = |character: char| {
        if language.is_closing_bracket(character) {
            unmatched_closers.push(character);
            return false;
        }
        unmatched_closers.pop().is_none()
    };
    let open = find_bracket_before(text, scanner, line_starts, (line, column), brackets, is_unclosed_opening)?;

    let open_line = text.line_of_offset(open);
    let open_column = open - text.line_start(open_line);
    // The scan went through the opening bracket's line: its state is known, no budget needed.
    let mut unlimited = usize::MAX;
    let open_start = line_starts.state_at(text, scanner, open_line, &mut unlimited)?;
    let open_char = text.char_at(open)?;
    let pair = *language.brackets.iter().find(|(opening, _)| *opening == open_char)?;
    let depth = bracket_columns(text, open_line, open_start, scanner)
        .into_iter()
        .find_map(|(column, depth)| (column == open_column).then_some(depth))?;
    let open_scan = scanner.scan(text, open_line, open_start.context);
    let close = find_closing(text, scanner, open_line, open_column, open_scan, pair)?;
    Some(EnclosingBracketPair { open, close, depth })
}

/// A bracket pair holding a position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EnclosingBracketPair {
    pub open: usize,
    pub close: usize,
    /// Nesting depth of the pair, 0 for the outermost.
    pub depth: usize,
}

/// Visits the code brackets before `(line, column)`, nearest first, starting with
/// `line_brackets` (those of `line`), until `is_target` says yes; returns that bracket's
/// offset. Bounded by `MAX_BRACKET_DISTANCE`.
#[expect(clippy::too_many_arguments, reason = "predates the complexity lints (D28)")]
fn find_bracket_before(
    text: &Text,
    scanner: &mut BracketScanner,
    line_starts: &mut BracketLineStarts,
    (start_line, start_column): (usize, usize),
    line_brackets: Vec<(usize, char)>,
    mut is_target: impl FnMut(char) -> bool,
) -> Option<usize> {
    let mut scanned_chars = 0;
    let mut brackets = line_brackets;
    let mut line = start_line;
    loop {
        let before_start = |&&(column, _): &&(usize, char)| line != start_line || column < start_column;
        for &(column, character) in brackets.iter().filter(before_start).rev() {
            if is_target(character) {
                return Some(text.line_start(line) + column);
            }
        }
        scanned_chars += text.line_len(line) + 1;
        if line == 0 || scanned_chars > MAX_BRACKET_DISTANCE {
            return None;
        }
        line -= 1;
        // Lines before one whose state is known are known too: no budget needed.
        let mut unlimited = usize::MAX;
        let start = line_starts.state_at(text, scanner, line, &mut unlimited)?;
        brackets = scanner.scan(text, line, start.context).brackets;
    }
}

/// The chars of `line` without its line ending, read without copying the line.
fn line_chars(text: &Text, line: usize) -> impl Iterator<Item = char> + '_ {
    text.chars_from(text.line_start(line)).take(text.line_len(line))
}

/// What is open where a line starts: code brackets, and a string or block comment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BracketLineStart {
    /// Open code brackets before the line (never negative: extra closers are ignored).
    pub depth: usize,
    pub context: LexicalContext,
}

/// The `BracketLineStart` of each line, computed lazily and kept across frames: an edit drops
/// the states after its first line, painting extends them to the painted lines within a budget
/// of lines shared by a frame's calls (like the highlighter's).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BracketLineStarts {
    states: Vec<BracketLineStart>,
    /// The first line the lexer scanned while the syntax tree was being parsed: the states
    /// after it are computed again from the tree once it is parsed.
    first_provisional_line: Option<usize>,
}

impl BracketLineStarts {
    /// The text changed from `first_changed_line` on.
    pub fn invalidate_from(&mut self, first_changed_line: usize) {
        self.states.truncate(first_changed_line + 1);
        if self.first_provisional_line.is_some_and(|line| line >= first_changed_line) {
            self.first_provisional_line = None;
        }
    }

    /// The state at the start of `line`, computing at most `budget_lines` more lines (the
    /// budget is decreased by the lines computed); `None` when the budget ran out first.
    pub fn state_at(
        &mut self,
        text: &Text,
        scanner: &mut BracketScanner,
        line: usize,
        budget_lines: &mut usize,
    ) -> Option<BracketLineStart> {
        if let Some(provisional_line) = self.first_provisional_line
            && scanner.is_parsed(text, provisional_line)
        {
            self.invalidate_from(provisional_line);
        }
        if self.states.is_empty() {
            self.states.push(BracketLineStart::default());
        }
        let target = line.min(text.last_line());
        while self.states.len() <= target {
            if *budget_lines == 0 {
                return None;
            }
            self.extend_one(text, scanner);
            *budget_lines -= 1;
        }
        Some(self.states[target])
    }

    fn extend_one(&mut self, text: &Text, scanner: &mut BracketScanner) {
        let line = self.states.len() - 1;
        let start = self.states[line];
        let (scan, is_provisional) = scanner.scan_telling_if_provisional(text, line, start.context);
        if is_provisional {
            self.first_provisional_line.get_or_insert(line);
        }
        let depth = scan.brackets.iter().fold(start.depth, |depth, &(_, character)| {
            if scanner.language.is_closing_bracket(character) { depth.saturating_sub(1) } else { depth + 1 }
        });
        self.states.push(BracketLineStart { depth, context: scan.context_at_end });
    }
}

/// Columns of the code brackets of `line` with their nesting depth (the depth inside an
/// opening bracket's pair, which its closing bracket shares), from its `start` state.
pub fn bracket_columns(
    text: &Text,
    line: usize,
    start: BracketLineStart,
    scanner: &mut BracketScanner,
) -> Vec<(usize, usize)> {
    let mut depth = start.depth;
    scanner
        .scan(text, line, start.context)
        .brackets
        .into_iter()
        .map(|(column, character)| {
            if scanner.language.is_closing_bracket(character) {
                depth = depth.saturating_sub(1);
                (column, depth)
            } else {
                depth += 1;
                (column, depth - 1)
            }
        })
        .collect()
}

/// Visual width of the indentation of `line`, `None` for a blank line. Reads only the
/// line's leading whitespace, without allocating.
pub fn line_indent(text: &Text, indentation: &Indentation, line: usize) -> Option<usize> {
    let tab_size = indentation.tab_size.max(1);
    let mut width = 0;
    for character in text.chars_from(text.line_start(line)) {
        match character {
            ' ' => width += 1,
            '\t' => width = (width / tab_size + 1) * tab_size,
            '\n' | '\r' => return None,
            _ => return Some(width),
        }
    }
    None
}

/// Lines searched at most for the nearest non-blank line (keeps blank runs cheap).
const MAX_BLANK_SEARCH: usize = 2_000;

fn nearest_indent(text: &Text, indentation: &Indentation, lines: impl Iterator<Item = usize>) -> usize {
    lines.take(MAX_BLANK_SEARCH).find_map(|line| line_indent(text, indentation, line)).unwrap_or(0)
}

/// Indentation level of a line, in indent units; a blank line takes the smaller level of the
/// nearest non-blank lines around it (so guides run through blank lines inside blocks).
pub fn indent_level(text: &Text, indentation: &Indentation, line: usize) -> usize {
    let tab_size = indentation.tab_size.max(1);
    let width = line_indent(text, indentation, line).unwrap_or_else(|| {
        let above = nearest_indent(text, indentation, (0..line).rev());
        let below = nearest_indent(text, indentation, line + 1..text.line_count());
        above.min(below)
    });
    width / tab_size
}

/// Indentation levels of `lines` (the visible ones).
pub fn indent_levels(text: &Text, indentation: &Indentation, lines: std::ops::Range<usize>) -> Vec<usize> {
    lines.map(|line| indent_level(text, indentation, line)).collect()
}

/// The guide to highlight for a cursor on `cursor_line`, as (level, first line, last line),
/// searching at most `max_lines` away from the cursor. `None` at the top level.
pub fn active_indent_guide(
    text: &Text,
    indentation: &Indentation,
    cursor_line: usize,
    max_lines: usize,
) -> Option<(usize, usize, usize)> {
    let block_level = indent_level(text, indentation, cursor_line);
    if block_level == 0 {
        return None;
    }
    let is_in_block = |line: usize| indent_level(text, indentation, line) >= block_level;
    let first_line = (cursor_line.saturating_sub(max_lines)..cursor_line)
        .rev()
        .take_while(|&line| is_in_block(line))
        .last()
        .unwrap_or(cursor_line);
    let last_line = (cursor_line + 1..text.line_count().min(cursor_line + max_lines))
        .take_while(|&line| is_in_block(line))
        .last()
        .unwrap_or(cursor_line);
    Some((block_level - 1, first_line, last_line))
}

/// A foldable region: `header_line` stays visible, `first_hidden_line..=last_hidden_line` hide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FoldRange {
    pub header_line: usize,
    pub last_hidden_line: usize,
}

impl FoldRange {
    pub fn first_hidden_line(&self) -> usize {
        self.header_line + 1
    }

    pub fn hides(&self, line: usize) -> bool {
        (self.first_hidden_line()..=self.last_hidden_line).contains(&line)
    }
}

/// The next non-blank line after `line` with its indentation.
fn next_non_blank(text: &Text, indentation: &Indentation, line: usize) -> Option<(usize, usize)> {
    (line + 1..text.line_count())
        .take(MAX_BLANK_SEARCH)
        .find_map(|next| line_indent(text, indentation, next).map(|indent| (next, indent)))
}

/// Whether `line` starts an indentation fold: the next non-blank line is more indented. Local:
/// cheap enough for every visible line.
pub fn is_fold_header(text: &Text, indentation: &Indentation, line: usize) -> bool {
    let Some(indent) = line_indent(text, indentation, line) else { return false };
    next_non_blank(text, indentation, line).is_some_and(|(_, next_indent)| next_indent > indent)
}

/// The indentation fold starting at `header_line` (Monaco's `foldingStrategy: 'indentation'`):
/// the more indented lines after it, up to the last of them that is not blank. Reads the
/// whole block.
pub fn fold_range_at(text: &Text, indentation: &Indentation, header_line: usize) -> Option<FoldRange> {
    if !is_fold_header(text, indentation, header_line) {
        return None;
    }
    let header_indent = line_indent(text, indentation, header_line)?;
    let mut last_hidden_line = header_line;
    for line in header_line + 1..text.line_count() {
        match line_indent(text, indentation, line) {
            Some(indent) if indent <= header_indent => break,
            Some(_) => last_hidden_line = line,
            None => {}
        }
    }
    Some(FoldRange { header_line, last_hidden_line })
}

/// The innermost fold whose header is `line` or that hides `line`: looks up for headers less
/// indented than the line.
pub fn enclosing_fold_range(text: &Text, indentation: &Indentation, line: usize) -> Option<FoldRange> {
    if let Some(range) = fold_range_at(text, indentation, line) {
        return Some(range);
    }
    let mut indent =
        line_indent(text, indentation, line).unwrap_or_else(|| nearest_indent(text, indentation, (0..line).rev()));
    for candidate in (0..line).rev() {
        let Some(candidate_indent) = line_indent(text, indentation, candidate) else { continue };
        if candidate_indent >= indent {
            continue;
        }
        if let Some(range) = fold_range_at(text, indentation, candidate).filter(|range| range.hides(line)) {
            return Some(range);
        }
        indent = candidate_indent;
        if indent == 0 {
            break;
        }
    }
    None
}

/// Header lines of the folds enclosing `line` (its header above it, that header's header…),
/// outermost first, at most `max_headers`, looking at most `max_lines` up (sticky scroll).
pub fn enclosing_headers(
    text: &Text,
    indentation: &Indentation,
    line: usize,
    max_headers: usize,
    max_lines: usize,
) -> Vec<usize> {
    let mut indent =
        line_indent(text, indentation, line).unwrap_or_else(|| nearest_indent(text, indentation, (0..line).rev()));
    let mut headers = Vec::new();
    for candidate in (line.saturating_sub(max_lines)..line).rev() {
        if indent == 0 {
            break;
        }
        let Some(candidate_indent) = line_indent(text, indentation, candidate) else { continue };
        if candidate_indent < indent {
            indent = candidate_indent;
            if is_fold_header(text, indentation, candidate) {
                headers.push(candidate);
            }
        }
    }
    headers.reverse();
    let skipped = headers.len().saturating_sub(max_headers);
    headers.drain(..skipped);
    headers
}

/// Every indentation fold of the text, sorted by header line (for "fold all").
pub fn indentation_fold_ranges(text: &Text, indentation: &Indentation) -> Vec<FoldRange> {
    let mut ranges = Vec::new();
    // Headers whose block is still open, with their indentation width.
    let mut open_headers: Vec<(usize, usize)> = Vec::new();
    let mut last_non_blank_line = 0;
    for line in 0..text.line_count() {
        let Some(width) = line_indent(text, indentation, line) else { continue };
        while let Some(&(header_line, header_width)) = open_headers.last() {
            if width > header_width {
                break;
            }
            open_headers.pop();
            if last_non_blank_line > header_line {
                ranges.push(FoldRange { header_line, last_hidden_line: last_non_blank_line });
            }
        }
        open_headers.push((line, width));
        last_non_blank_line = line;
    }
    while let Some((header_line, _)) = open_headers.pop() {
        if last_non_blank_line > header_line {
            ranges.push(FoldRange { header_line, last_hidden_line: last_non_blank_line });
        }
    }
    ranges.sort_by_key(|range| range.header_line);
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(text: &str, offset: usize) -> Option<(usize, usize)> {
        let text = Text::new(text);
        let language = LanguageConfiguration::c_like();
        matching_bracket_pair(
            &text,
            offset,
            &mut BracketScanner::lexer(&language),
            &mut BracketLineStarts::default(),
            &mut 100,
        )
    }

    /// `(open, close, depth)` of the pair holding `offset`.
    fn enclosing(text: &str, offset: usize) -> Option<(usize, usize, usize)> {
        let text = Text::new(text);
        let language = LanguageConfiguration::c_like();
        let mut scanner = BracketScanner::lexer(&language);
        enclosing_bracket_pair(&text, offset, &mut scanner, &mut BracketLineStarts::default(), &mut 100)
            .map(|pair| (pair.open, pair.close, pair.depth))
    }

    #[test]
    fn the_enclosing_pair_is_the_innermost_holding_the_cursor() {
        let source = "fn f() {\n    g(a,\n      b);\n}";

        assert_eq!(enclosing(source, 15), Some((14, 25, 1)));
        assert_eq!(enclosing(source, 10), Some((7, 28, 0)));
        assert_eq!(enclosing(source, 29), None);
    }

    #[test]
    fn the_enclosing_pair_skips_closed_pairs_and_brackets_in_strings() {
        let source = "{ f(x) \"{\" [1] | }";

        assert_eq!(enclosing(source, 16), Some((0, 17, 0)));
    }

    fn depth(starts: &mut BracketLineStarts, text: &Text, line: usize, budget: &mut usize) -> Option<usize> {
        let language = LanguageConfiguration::c_like();
        starts.state_at(text, &mut BracketScanner::lexer(&language), line, budget).map(|start| start.depth)
    }

    fn at_depth(depth: usize) -> BracketLineStart {
        BracketLineStart { depth, context: LexicalContext::Code }
    }

    #[test]
    fn matches_brackets_after_then_before_the_cursor() {
        assert_eq!(pair("f(a[1], (b))", 1), Some((1, 11)));
        assert_eq!(pair("f(a[1], (b))", 12), Some((1, 11)));
        assert_eq!(pair("f(a[1], (b))", 4), Some((3, 5)));
        assert_eq!(pair("f(a[1], (b))", 2), Some((1, 11)));
        assert_eq!(pair("ab", 1), None);
    }

    #[test]
    fn matching_skips_brackets_in_strings_and_comments() {
        let text = "f(\")\", // )\n  /* ( */ x)";

        assert_eq!(pair(text, 1), Some((1, 23)));
        assert_eq!(pair(text, 24), Some((1, 23)));
        assert_eq!(pair(text, 3), None);
    }

    #[test]
    fn bracket_depths_follow_nesting_across_lines() {
        let text = Text::new("f(a, {\n  [b]\n})");
        let language = LanguageConfiguration::c_like();
        let mut starts = BracketLineStarts::default();

        assert_eq!(depth(&mut starts, &text, 1, &mut 100), Some(2));
        let mut scanner = BracketScanner::lexer(&language);
        assert_eq!(bracket_columns(&text, 1, at_depth(2), &mut scanner), vec![(2, 2), (4, 2)]);
        assert_eq!(depth(&mut starts, &text, 2, &mut 100), Some(2));
        assert_eq!(bracket_columns(&text, 2, at_depth(2), &mut scanner), vec![(0, 1), (1, 0)]);
    }

    #[test]
    fn bracket_depths_ignore_strings_and_comments_across_lines() {
        let text = Text::new("a(\"{\n}\" /* [\n] */ (\n");
        let mut starts = BracketLineStarts::default();

        assert_eq!(depth(&mut starts, &text, 1, &mut 100), Some(1));
        assert_eq!(depth(&mut starts, &text, 3, &mut 100), Some(2));
    }

    #[test]
    fn bracket_depths_have_a_budget_and_recompute_after_edits() {
        let text = Text::new(&"(\n".repeat(50));
        let mut starts = BracketLineStarts::default();

        let mut budget = 10;
        assert_eq!(depth(&mut starts, &text, 40, &mut budget), None);
        assert_eq!(budget, 0);
        assert_eq!(depth(&mut starts, &text, 40, &mut 100), Some(40));
        starts.invalidate_from(5);
        let edited = Text::new(&")\n".repeat(50));
        assert_eq!(depth(&mut starts, &edited, 7, &mut 100), Some(3));
    }

    /// A styler whose tree puts the brackets of each line at fixed columns, or is parsing.
    struct FakeTree {
        columns_of_line: Vec<Vec<usize>>,
        is_parsing: bool,
    }

    impl LineStyler for FakeTree {
        fn style_line(&mut self, _text: &Text, _line: usize) -> Vec<crate::style::StyledSpan> {
            Vec::new()
        }

        fn invalidate_from(&mut self, _first_changed_line: usize) {}

        fn code_brackets(&mut self, _text: &Text, line: usize) -> SyntaxBrackets {
            if self.is_parsing {
                return SyntaxBrackets::Parsing;
            }
            SyntaxBrackets::Columns(self.columns_of_line[line].clone())
        }
    }

    #[test]
    fn the_syntax_tree_tells_which_brackets_are_code() {
        // As in `r"(" (x)`, where the lexer can't tell the raw string's bracket.
        let text = Text::new("a(b(c)");
        let language = LanguageConfiguration::c_like();
        let mut tree = FakeTree { columns_of_line: vec![vec![3, 5]], is_parsing: false };
        let mut scanner = BracketScanner::new(&language, Some(&mut tree));
        let mut pair_at =
            |offset| matching_bracket_pair(&text, offset, &mut scanner, &mut BracketLineStarts::default(), &mut 100);

        assert_eq!(pair_at(1), None);
        assert_eq!(pair_at(3), Some((3, 5)));
    }

    #[test]
    fn states_lexed_while_parsing_are_computed_again_from_the_tree() {
        let text = Text::new("(\n(\n(");
        let language = LanguageConfiguration::c_like();
        let mut tree = FakeTree { columns_of_line: vec![vec![], vec![0], vec![0]], is_parsing: true };
        let mut starts = BracketLineStarts::default();

        let lexed = starts.state_at(&text, &mut BracketScanner::new(&language, Some(&mut tree)), 2, &mut 100);
        tree.is_parsing = false;
        let parsed = starts.state_at(&text, &mut BracketScanner::new(&language, Some(&mut tree)), 2, &mut 100);

        assert_eq!(lexed.map(|start| start.depth), Some(2));
        assert_eq!(parsed.map(|start| start.depth), Some(1));
    }

    #[test]
    fn unmatched_bracket_has_no_pair() {
        assert_eq!(pair("((", 0), None);
    }

    #[test]
    fn blank_lines_take_the_smaller_neighbour_level() {
        let text = Text::new("a\n    b\n\n    c\n\nd");

        assert_eq!(indent_levels(&text, &Indentation::default(), 0..6), vec![0, 1, 1, 1, 0, 0]);
    }

    #[test]
    fn active_guide_spans_the_cursor_block() {
        let text = Text::new("a\n    b\n        c\n    d\n    e\nf");
        let indentation = Indentation::default();

        assert_eq!(active_indent_guide(&text, &indentation, 3, 100), Some((0, 1, 4)));
        assert_eq!(active_indent_guide(&text, &indentation, 2, 100), Some((1, 2, 2)));
        assert_eq!(active_indent_guide(&text, &indentation, 0, 100), None);
    }

    #[test]
    fn local_fold_queries_agree_with_the_whole_text_ranges() {
        let text = Text::new("fn a() {\n    if x {\n        y();\n    }\n\n    z();\n}\nfn b() {}");
        let indentation = Indentation::default();

        for range in indentation_fold_ranges(&text, &indentation) {
            assert!(is_fold_header(&text, &indentation, range.header_line));
            assert_eq!(fold_range_at(&text, &indentation, range.header_line), Some(range));
        }
        assert_eq!(
            enclosing_fold_range(&text, &indentation, 2),
            Some(FoldRange { header_line: 1, last_hidden_line: 2 })
        );
        assert_eq!(
            enclosing_fold_range(&text, &indentation, 5),
            Some(FoldRange { header_line: 0, last_hidden_line: 5 })
        );
        assert_eq!(
            enclosing_fold_range(&text, &indentation, 4),
            Some(FoldRange { header_line: 0, last_hidden_line: 5 })
        );
        assert_eq!(enclosing_fold_range(&text, &indentation, 7), None);
    }

    #[test]
    fn enclosing_headers_are_outermost_first() {
        let text = Text::new(
            "impl A {\n    fn b() {\n        if c {\n            d();\n\n            e();\n        }\n    }\n}",
        );
        let indentation = Indentation::default();

        assert_eq!(enclosing_headers(&text, &indentation, 5, 10, 100), vec![0, 1, 2]);
        assert_eq!(enclosing_headers(&text, &indentation, 4, 10, 100), vec![0, 1, 2]);
        assert_eq!(enclosing_headers(&text, &indentation, 5, 2, 100), vec![1, 2]);
        assert_eq!(enclosing_headers(&text, &indentation, 0, 10, 100), Vec::<usize>::new());
    }

    #[test]
    fn line_indent_expands_tabs_and_skips_blank_lines() {
        let text = Text::new("\t  x\n   \ny");
        let indentation = Indentation { tab_size: 4, insert_spaces: false };

        assert_eq!(line_indent(&text, &indentation, 0), Some(6));
        assert_eq!(line_indent(&text, &indentation, 1), None);
        assert_eq!(line_indent(&text, &indentation, 2), Some(0));
    }

    #[test]
    fn folds_indented_blocks_keeping_the_closing_line() {
        let text = Text::new("fn a() {\n    if x {\n        y();\n    }\n\n    z();\n}\nfn b() {}");

        let ranges = indentation_fold_ranges(&text, &Indentation::default());

        assert_eq!(
            ranges,
            vec![FoldRange { header_line: 0, last_hidden_line: 5 }, FoldRange { header_line: 1, last_hidden_line: 2 }]
        );
    }

    #[test]
    fn trailing_blank_lines_are_not_folded() {
        let text = Text::new("a\n  b\n\n\nc");

        assert_eq!(
            indentation_fold_ranges(&text, &Indentation::default()),
            vec![FoldRange { header_line: 0, last_hidden_line: 1 }]
        );
    }
}
