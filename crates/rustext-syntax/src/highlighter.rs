//! The editor's syntax highlighter: tree-sitter for the languages with a bundled grammar
//! (DECISIONS D22), TextMate grammars for every other one (D13), behind one type.

use rustext_core::structure::FoldRange;
use rustext_core::style::{LineStyler, StyledSpan};
use rustext_core::{ChangeSet, EditedLines, Text};

use crate::language::{Language, LanguageRegistry};
use crate::textmate::TextMateHighlighter;
use crate::theme::SyntaxTheme;
use crate::tree::TreeHighlighter;

enum Engine {
    Tree(Box<TreeHighlighter>),
    TextMate(Box<TextMateHighlighter>),
}

/// Highlights one text in one language with one theme. Implements the editor's `LineStyler`.
pub struct SyntaxHighlighter {
    registry: LanguageRegistry,
    language: Language,
    theme: SyntaxTheme,
    engine: Engine,
}

fn engine(registry: &LanguageRegistry, language: &Language, theme: &SyntaxTheme) -> Engine {
    match registry.grammar(&language.name) {
        Some(grammar) => Engine::Tree(Box::new(TreeHighlighter::new(grammar, theme))),
        None => Engine::TextMate(Box::new(TextMateHighlighter::new(registry, language.clone(), theme.clone()))),
    }
}

impl SyntaxHighlighter {
    pub fn new(registry: &LanguageRegistry, language: Language, theme: SyntaxTheme) -> Self {
        let engine = engine(registry, &language, &theme);
        Self { registry: registry.clone(), language, theme, engine }
    }

    pub fn language(&self) -> &Language {
        &self.language
    }

    pub fn set_language(&mut self, language: Language) {
        self.language = language;
        self.engine = engine(&self.registry, &self.language, &self.theme);
    }

    pub fn set_theme(&mut self, theme: SyntaxTheme) {
        self.theme = theme;
        self.engine = engine(&self.registry, &self.language, &self.theme);
    }

    /// Whether the text is parsed with tree-sitter (else tokenized with a TextMate grammar).
    pub fn uses_tree_sitter(&self) -> bool {
        matches!(self.engine, Engine::Tree(_))
    }

    fn styler(&mut self) -> &mut dyn LineStyler {
        match &mut self.engine {
            Engine::Tree(highlighter) => highlighter.as_mut(),
            Engine::TextMate(highlighter) => highlighter.as_mut(),
        }
    }
}

impl LineStyler for SyntaxHighlighter {
    fn style_line(&mut self, text: &Text, line: usize) -> Vec<StyledSpan> {
        self.styler().style_line(text, line)
    }

    fn invalidate_from(&mut self, first_changed_line: usize) {
        self.styler().invalidate_from(first_changed_line);
    }

    fn lines_edited(&mut self, edited: &EditedLines) {
        self.styler().lines_edited(edited);
    }

    fn text_edited(&mut self, text_before: &Text, changes: &ChangeSet, edited: &EditedLines) {
        self.styler().text_edited(text_before, changes, edited);
    }

    fn enclosing_syntax_node(&mut self, text: &Text, start: usize, end: usize) -> Option<(usize, usize)> {
        self.styler().enclosing_syntax_node(text, start, end)
    }

    fn folds_by_syntax(&self) -> bool {
        self.uses_tree_sitter()
    }

    fn syntax_fold_range(&mut self, text: &Text, line: usize) -> Option<FoldRange> {
        self.styler().syntax_fold_range(text, line)
    }

    fn is_catching_up(&self) -> bool {
        match &self.engine {
            Engine::Tree(highlighter) => highlighter.is_catching_up(),
            Engine::TextMate(highlighter) => highlighter.is_catching_up(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn languages_with_a_bundled_grammar_use_tree_sitter() {
        let registry = LanguageRegistry::new();
        let highlighter = |file_name: &str| {
            SyntaxHighlighter::new(&registry, registry.detect(file_name, ""), SyntaxTheme::dark_modern())
        };

        assert!(highlighter("main.rs").uses_tree_sitter());
        assert!(highlighter("app.tsx").uses_tree_sitter());
        assert!(!highlighter("Dockerfile").uses_tree_sitter());
        assert!(!highlighter("notes.unknown-extension").uses_tree_sitter());
    }

    #[test]
    fn grammars_are_compiled_once_per_registry() {
        let registry = LanguageRegistry::new();

        assert!(Arc::ptr_eq(&registry.grammar("Rust").unwrap(), &registry.clone().grammar("Rust").unwrap()));
    }
}
