//! Highlighting with TextMate grammars (syntect, DECISIONS D13), for languages without a
//! tree-sitter grammar: lines are tokenized in order, keeping the parser state at the start of
//! each line; an edit drops what follows the edited line, and only lines that get painted are
//! tokenized again.

use rustext_core::Text;
use rustext_core::style::{LineStyler, StyledSpan, TextStyle};
use syntect::highlighting::{FontStyle, HighlightIterator, HighlightState, Highlighter, Style};
use syntect::parsing::{ParseState, ScopeStack};

use crate::language::{Language, LanguageRegistry};
use crate::theme::{SyntaxTheme, to_rgba};

/// Highlights one text in one language with one theme, from its TextMate grammar.
pub struct TextMateHighlighter {
    registry: LanguageRegistry,
    language: Language,
    theme: SyntaxTheme,
    /// `line_states[line]`: the parser state at the start of `line`.
    line_states: Vec<(ParseState, HighlightState)>,
    /// Spans of the lines tokenized so far (`line_spans.len() == line_states.len() - 1`).
    line_spans: Vec<Vec<StyledSpan>>,
    /// Lines tokenized per call at most; lines further down are left unstyled until later calls.
    pub max_lines_per_call: usize,
    /// Lines longer than this are not tokenized (minified files).
    pub max_line_chars: usize,
    is_catching_up: bool,
    /// Lines before this one have correct spans and states; lines after it keep spans and
    /// states from before an edit (shifted), right until proven otherwise.
    verified_lines: usize,
    /// No early stop before this line: the edited lines hold placeholder states.
    edited_until: usize,
    /// Lines tokenized so far, to check that edits don't re-tokenize what they needn't.
    tokenized_line_count: usize,
}

impl TextMateHighlighter {
    pub fn new(registry: &LanguageRegistry, language: Language, theme: SyntaxTheme) -> Self {
        let mut highlighter = Self {
            registry: registry.clone(),
            language,
            theme,
            line_states: Vec::new(),
            line_spans: Vec::new(),
            max_lines_per_call: 5_000,
            max_line_chars: 20_000,
            is_catching_up: false,
            verified_lines: 0,
            edited_until: 0,
            tokenized_line_count: 0,
        };
        highlighter.reset();
        highlighter
    }

    pub fn language(&self) -> &Language {
        &self.language
    }

    pub fn set_language(&mut self, language: Language) {
        self.language = language;
        self.reset();
    }

    pub fn set_theme(&mut self, theme: SyntaxTheme) {
        self.theme = theme;
        self.reset();
    }

    fn reset(&mut self) {
        let highlighter = Highlighter::new(&self.theme.theme);
        let initial = (ParseState::new(&self.language.syntax), HighlightState::new(&highlighter, ScopeStack::new()));
        self.line_states = vec![initial];
        self.line_spans.clear();
        self.verified_lines = 0;
        self.edited_until = 0;
    }

    /// Lines tokenized since the highlighter was created.
    pub fn tokenized_line_count(&self) -> usize {
        self.tokenized_line_count
    }

    fn tokenize_through(&mut self, text: &Text, last_line: usize) {
        let highlighter = Highlighter::new(&self.theme.theme);
        let syntaxes = self.registry.syntaxes.clone();
        let budget_end = self.verified_lines + self.max_lines_per_call;
        self.is_catching_up = last_line >= budget_end;
        let last_line = last_line.min(budget_end.saturating_sub(1)).min(text.last_line());

        while self.verified_lines <= last_line {
            let line = self.verified_lines;
            let (mut parse_state, mut highlight_state) = self.line_states[line].clone();
            let content = text.line_content(line);
            let spans = if content.chars().count() > self.max_line_chars {
                Vec::new()
            } else {
                let line_with_newline = format!("{content}\n");
                match parse_state.parse_line(&line_with_newline, &syntaxes) {
                    Ok(operations) => {
                        let pieces =
                            HighlightIterator::new(&mut highlight_state, &operations, &line_with_newline, &highlighter);
                        spans_of(pieces, highlighter.get_default())
                    }
                    Err(_) => Vec::new(),
                }
            };
            self.tokenized_line_count += 1;
            let end_state = (parse_state, highlight_state);
            if line < self.line_spans.len() {
                self.line_spans[line] = spans;
                // Past the edit, a line ending in the state cached before the edit means every
                // following line is still right: stop there.
                if line + 1 >= self.edited_until && self.line_states[line + 1] == end_state {
                    self.verified_lines = self.line_spans.len();
                    return;
                }
                self.line_states[line + 1] = end_state;
            } else {
                self.line_spans.push(spans);
                self.line_states.push(end_state);
            }
            self.verified_lines = line + 1;
        }
    }
}

/// A syntect style as the editor's, its background left out when it is the theme's.
pub(crate) fn text_style(style: Style, default_style: Style) -> TextStyle {
    TextStyle {
        foreground: Some(to_rgba(style.foreground)),
        background: (style.background != default_style.background).then(|| to_rgba(style.background)),
        bold: style.font_style.contains(FontStyle::BOLD),
        italic: style.font_style.contains(FontStyle::ITALIC),
        underline: style.font_style.contains(FontStyle::UNDERLINE),
        strikethrough: false,
    }
}

/// Turns syntect's `(style, piece)` runs into spans in char columns, merging equal neighbours
/// and leaving default-styled runs out.
fn spans_of<'a>(pieces: impl Iterator<Item = (Style, &'a str)>, default_style: Style) -> Vec<StyledSpan> {
    let mut spans: Vec<StyledSpan> = Vec::new();
    let mut column = 0;
    for (style, piece) in pieces {
        let piece = piece.trim_end_matches('\n');
        let piece_len = piece.chars().count();
        if piece_len == 0 {
            continue;
        }
        let text_style = text_style(style, default_style);
        let (start_column, end_column) = (column, column + piece_len);
        column = end_column;
        if style == default_style {
            continue;
        }
        match spans.last_mut() {
            Some(last) if last.end_column == start_column && last.style == text_style => last.end_column = end_column,
            _ => spans.push(StyledSpan { start_column, end_column, style: text_style }),
        }
    }
    spans
}

impl LineStyler for TextMateHighlighter {
    fn style_line(&mut self, text: &Text, line: usize) -> Vec<StyledSpan> {
        if line >= self.verified_lines {
            self.tokenize_through(text, line);
        }
        if line >= self.verified_lines {
            return Vec::new();
        }
        self.line_spans.get(line).cloned().unwrap_or_default()
    }

    fn invalidate_from(&mut self, first_changed_line: usize) {
        self.line_spans.truncate(first_changed_line);
        self.line_states.truncate(first_changed_line + 1);
        self.verified_lines = self.verified_lines.min(first_changed_line);
        self.edited_until = 0;
    }

    /// Replaces the edited lines' cache entries by placeholders and keeps the lines after them
    /// (shifted), to be confirmed by the early stop in `tokenize_through`.
    fn lines_edited(&mut self, edited: &rustext_core::EditedLines) {
        let first_line = edited.first_line;
        if edited.old_last_line >= self.line_spans.len() {
            self.invalidate_from(first_line);
            return;
        }
        let new_line_count = edited.new_last_line - first_line + 1;
        let placeholder_state = self.line_states[first_line].clone();
        self.line_spans.splice(first_line..=edited.old_last_line, std::iter::repeat_n(Vec::new(), new_line_count));
        self.line_states
            .splice(first_line + 1..=edited.old_last_line + 1, std::iter::repeat_n(placeholder_state, new_line_count));
        let shifted_edited_until = if self.edited_until > edited.old_last_line {
            self.edited_until.saturating_add_signed(edited.line_delta())
        } else {
            self.edited_until
        };
        self.edited_until = shifted_edited_until.max(edited.new_last_line + 1);
        self.verified_lines = self.verified_lines.min(first_line);
    }

    fn is_catching_up(&self) -> bool {
        self.is_catching_up
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn highlighter(file_name: &str) -> TextMateHighlighter {
        let registry = LanguageRegistry::new();
        let language = registry.detect(file_name, "");
        TextMateHighlighter::new(&registry, language, SyntaxTheme::dark_modern())
    }

    fn colored_words(highlighter: &mut TextMateHighlighter, text: &Text, line: usize) -> Vec<(String, u8)> {
        let content: Vec<char> = text.line_content(line).chars().collect();
        highlighter
            .style_line(text, line)
            .iter()
            .map(|span| {
                (content[span.start_column..span.end_column].iter().collect(), span.style.foreground.unwrap().red)
            })
            .collect()
    }

    #[test]
    fn colors_rust_keywords_strings_and_comments() {
        let mut highlighter = highlighter("main.rs");
        let text = Text::new("fn main() { let s = \"hi\"; } // done");

        let words = colored_words(&mut highlighter, &text, 0);

        assert!(words.contains(&("fn".to_string(), 0x56)), "{words:?}");
        assert!(words.iter().any(|(word, red)| word.contains("hi") && *red == 0xce), "{words:?}");
        assert!(words.iter().any(|(word, red)| word.contains("done") && *red == 0x6a), "{words:?}");
    }

    #[test]
    fn state_carries_across_lines() {
        let mut highlighter = highlighter("main.rs");
        let text = Text::new("/* start\nstill comment\n*/ fn");

        let words = colored_words(&mut highlighter, &text, 1);

        assert_eq!(words, vec![("still comment".to_string(), 0x6a)]);
    }

    #[test]
    fn invalidation_re_tokenizes_after_an_edit() {
        let mut highlighter = highlighter("main.rs");
        let before = Text::new("let a = 1;\nlet b = 2;");
        highlighter.style_line(&before, 1);
        let after = Text::new("/*\nlet b = 2;");

        highlighter.invalidate_from(0);

        assert_eq!(colored_words(&mut highlighter, &after, 1), vec![("let b = 2;".to_string(), 0x6a)]);
    }

    #[test]
    fn an_edit_re_tokenizes_only_until_the_states_agree_again() {
        let mut highlighter = highlighter("main.rs");
        let content: String = (0..1_000).map(|line| format!("let value_{line} = {line};\n")).collect();
        let text = Text::new(&content);
        highlighter.style_line(&text, 999);
        let before_edit = highlighter.tokenized_line_count();

        // Line 10 changes without changing the parser state at its end.
        let edited_text = Text::new(&content.replacen("let value_10 = 10;", "let renamed = 10;", 1));
        highlighter.lines_edited(&rustext_core::EditedLines { first_line: 10, old_last_line: 10, new_last_line: 10 });
        let spans_at_end = highlighter.style_line(&edited_text, 999);

        assert!(
            highlighter.tokenized_line_count() - before_edit < 5,
            "re-tokenized {}",
            highlighter.tokenized_line_count() - before_edit
        );
        assert!(!spans_at_end.is_empty());
    }

    #[test]
    fn an_edit_that_changes_the_state_re_tokenizes_what_follows() {
        let mut highlighter = highlighter("main.rs");
        let before = Text::new("let a = 1;\nlet b = 2;\nlet c = 3;\n");
        highlighter.style_line(&before, 2);
        let after = Text::new("/* let a = 1;\nlet b = 2;\nlet c = 3;\n");

        highlighter.lines_edited(&rustext_core::EditedLines { first_line: 0, old_last_line: 0, new_last_line: 0 });

        assert_eq!(colored_words(&mut highlighter, &after, 2), vec![("let c = 3;".to_string(), 0x6a)]);
    }

    #[test]
    fn inserted_lines_shift_the_cache() {
        let mut highlighter = highlighter("main.rs");
        let content: String = (0..100).map(|line| format!("let v{line} = {line};\n")).collect();
        highlighter.style_line(&Text::new(&content), 99);
        let before_edit = highlighter.tokenized_line_count();
        let edited = Text::new(&content.replacen("let v5 = 5;\n", "let v5 = 5;\nlet x = 0;\nlet y = 1;\n", 1));

        highlighter.lines_edited(&rustext_core::EditedLines { first_line: 5, old_last_line: 5, new_last_line: 7 });
        let last = colored_words(&mut highlighter, &edited, 101);

        assert!(highlighter.tokenized_line_count() - before_edit < 6);
        assert_eq!(last[0].0, "let");
    }

    #[test]
    fn budget_leaves_far_lines_unstyled_until_later_calls() {
        let mut highlighter = highlighter("main.rs");
        highlighter.max_lines_per_call = 10;
        let text = Text::new(&"let x = 1;\n".repeat(100));

        assert!(highlighter.style_line(&text, 50).is_empty());
        assert!(highlighter.is_catching_up());
        for _ in 0..5 {
            highlighter.style_line(&text, 50);
        }
        assert!(!highlighter.style_line(&text, 50).is_empty());
    }
}
