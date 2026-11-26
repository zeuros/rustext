//! Incremental syntax highlighting (DECISIONS D13): lines are tokenized in order, keeping the
//! parser state at the start of each line; an edit drops what follows the edited line, and
//! only lines that get painted are tokenized again.

use rustext_core::Text;
use rustext_core::style::{LineStyler, StyledSpan, TextStyle};
use syntect::highlighting::{FontStyle, HighlightIterator, HighlightState, Highlighter, Style};
use syntect::parsing::{ParseState, ScopeStack};

use crate::language::{Language, LanguageRegistry};
use crate::theme::{SyntaxTheme, to_rgba};

/// Highlights one text in one language with one theme. Implements the editor's `LineStyler`.
pub struct SyntaxHighlighter {
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
}

impl SyntaxHighlighter {
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
    }

    fn tokenize_through(&mut self, text: &Text, last_line: usize) {
        let highlighter = Highlighter::new(&self.theme.theme);
        let syntaxes = self.registry.syntaxes.clone();
        let budget_end = self.line_spans.len() + self.max_lines_per_call;
        self.is_catching_up = last_line >= budget_end;
        let last_line = last_line.min(budget_end.saturating_sub(1)).min(text.last_line());

        while self.line_spans.len() <= last_line {
            let line = self.line_spans.len();
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
            self.line_spans.push(spans);
            self.line_states.push((parse_state, highlight_state));
        }
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
        let text_style = TextStyle {
            foreground: Some(to_rgba(style.foreground)),
            background: (style.background != default_style.background).then(|| to_rgba(style.background)),
            bold: style.font_style.contains(FontStyle::BOLD),
            italic: style.font_style.contains(FontStyle::ITALIC),
            underline: style.font_style.contains(FontStyle::UNDERLINE),
            strikethrough: false,
        };
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

impl LineStyler for SyntaxHighlighter {
    fn style_line(&mut self, text: &Text, line: usize) -> Vec<StyledSpan> {
        if line >= self.line_spans.len() {
            self.tokenize_through(text, line);
        }
        self.line_spans.get(line).cloned().unwrap_or_default()
    }

    fn invalidate_from(&mut self, first_changed_line: usize) {
        self.line_spans.truncate(first_changed_line);
        self.line_states.truncate(first_changed_line + 1);
    }

    fn is_catching_up(&self) -> bool {
        self.is_catching_up
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn highlighter(file_name: &str) -> SyntaxHighlighter {
        let registry = LanguageRegistry::new();
        let language = registry.detect(file_name, "");
        SyntaxHighlighter::new(&registry, language, SyntaxTheme::dark_modern())
    }

    fn colored_words(highlighter: &mut SyntaxHighlighter, text: &Text, line: usize) -> Vec<(String, u8)> {
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
