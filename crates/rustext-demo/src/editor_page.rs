//! The playground's editor page: one file in a `CodeEditor`, its options on the side.

use eframe::egui;
use rustext_core::indentation::Indentation;
use rustext_core::{EditingConfig, EditorState, Text};
use rustext_egui::code_editor::{CursorStyle, LineNumbers, WhitespaceRendering, WordWrap};
use rustext_egui::{CodeEditor, EditorView, Keymap, ViewConfig};
use rustext_syntax::{LanguageRegistry, SyntaxHighlighter};

use crate::theme_choice::ThemeChoice;

pub struct EditorPage {
    state: EditorState,
    view: EditorView,
    view_config: ViewConfig,
    file_name: String,
    saved_version: u64,
    last_action: String,
    languages: LanguageRegistry,
    highlighter: SyntaxHighlighter,
    highlighted_theme: ThemeChoice,
    shows_diagnostics: bool,
    pub shows_blame: bool,
}

impl EditorPage {
    pub fn set_word_wrap(&mut self, word_wrap: WordWrap) {
        self.view_config.word_wrap = word_wrap;
    }
}

/// A made-up commit, for the blame demo.
struct BlameCommit {
    short_sha: &'static str,
    author: &'static str,
    age: &'static str,
    summary: &'static str,
}

const BLAME_COMMITS: [BlameCommit; 4] = [
    BlameCommit { short_sha: "a1b2c3d", author: "Ada", age: "2 years ago", summary: "Initial sample" },
    BlameCommit { short_sha: "e4f5a6b", author: "Linus", age: "8 months ago", summary: "Count words" },
    BlameCommit { short_sha: "c7d8e9f", author: "Grace", age: "3 weeks ago", summary: "Add shapes" },
    BlameCommit { short_sha: "0a1b2c3", author: "Ken", age: "yesterday", summary: "Print totals" },
];

/// Which made-up commit last touched `line`: runs of a few lines each.
fn blame_commit_index(line: usize) -> usize {
    [0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 2, 2, 2, 2, 2, 0, 2, 2, 2, 2, 2, 2, 2, 2, 0, 3, 3, 3, 3, 3, 3, 3]
        .get(line)
        .copied()
        .unwrap_or(3)
}

impl EditorPage {
    pub fn new(
        languages: &LanguageRegistry,
        theme_choice: &ThemeChoice,
        file_name: String,
        content: &str,
        find: Option<String>,
        showcase: bool,
    ) -> Self {
        let text = Text::new(content);
        let config = EditingConfig {
            indentation: Indentation::detect(&text, Indentation::default()),
            ..EditingConfig::default()
        };
        let language = languages.detect(&file_name, &text.line_content(0));
        let mut state = EditorState::with_text(text).with_config(config).with_language(language.configuration.clone());
        let mut view = EditorView::default();
        let mut view_config = ViewConfig { rulers: vec![100], ..ViewConfig::default() };
        if showcase {
            let text = state.text().clone();
            let bracket_offset = text.offset(rustext_core::Position::new(5, 57));
            state.execute(rustext_core::Command::SetSelections(rustext_core::Selections::cursor(bracket_offset)));
            view.folds.fold_line(19);
            view_config.render_whitespace = WhitespaceRendering::All;
        }
        if let Some(pattern) = find {
            view.find.is_open = true;
            view.find.is_replace_shown = true;
            view.find.query.pattern = pattern;
        }
        Self {
            saved_version: state.history().version(),
            state,
            view,
            view_config,
            file_name,
            last_action: String::new(),
            highlighter: SyntaxHighlighter::new(languages, language, theme_choice.syntax_theme()),
            languages: languages.clone(),
            highlighted_theme: *theme_choice,
            shows_diagnostics: showcase,
            shows_blame: false,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme_choice: &ThemeChoice, keymap: &Keymap) {
        if self.highlighted_theme != *theme_choice {
            self.highlighter.set_theme(theme_choice.syntax_theme());
            self.highlighted_theme = *theme_choice;
        }
        let theme = theme_choice.theme();
        egui::Panel::left("editor-options").resizable(true).default_size(220.0).show(ui, |ui| self.options_panel(ui));
        egui::Panel::bottom("editor-status").show(ui, |ui| self.status_bar(ui));
        let decorations = self.demo_decorations(&theme);
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let response = CodeEditor::new(&mut self.state, &mut self.view, &self.view_config, &theme, keymap)
                .decorations(&decorations)
                .glyph_margin(self.shows_blame)
                .styler(&mut self.highlighter)
                .show(ui);
            if let Some(action) = response.unhandled_actions.last() {
                self.last_action = format!("{action:?}");
            }
            if let (true, Some(rustext_egui::MouseTarget::LineNumber { line })) = (self.shows_blame, response.pressed) {
                let commit = &BLAME_COMMITS[blame_commit_index(line)];
                self.last_action =
                    format!("blame: line {} is from {} ({})", line + 1, commit.short_sha, commit.summary);
            }
            if response.outcome.rejected_read_only {
                self.last_action = "Cannot edit in read-only editor".into();
            }
        });
    }

    /// Decorations of the demos: diagnostics on a few words, blame shading and glyphs.
    fn demo_decorations(&self, theme: &rustext_egui::EditorTheme) -> Vec<rustext_core::decoration::Decoration> {
        use rustext_core::decoration::{Decoration, DecorationKind};
        use rustext_core::find::{SearchQuery, find_matches};
        use rustext_egui::{Severity, diagnostic};

        let text = self.state.text();
        let mut decorations = Vec::new();
        if self.shows_diagnostics {
            let examples = [
                ("HashMap::new()", Severity::Warning, "consider `HashMap::with_capacity` when the size is known"),
                ("PI", Severity::Info, "`std::f64::consts::PI` could be imported"),
                ("word_counts(\"the", Severity::Error, "mismatched types: expected `&str`, found `String` (made up)"),
                ("counts", Severity::Hint, "variable `counts` is shadowed later"),
            ];
            for (pattern, severity, message) in examples {
                let query = SearchQuery { match_case: true, ..SearchQuery::new(pattern) };
                if let Some(found) =
                    find_matches(text, &query, None, 1).ok().and_then(|matches| matches.first().copied())
                {
                    decorations.push(diagnostic(found.start, found.end, severity, message, theme));
                }
            }
        }
        if self.shows_blame {
            let shade = rustext_egui::code_editor::to_rgba(theme.current_line_background).with_alpha(150);
            let age_colors =
                [theme.line_number.gamma_multiply(0.6), theme.line_number, theme.info, theme.active_line_number];
            for line in 0..text.line_count() {
                let commit_index = blame_commit_index(line);
                let commit = &BLAME_COMMITS[commit_index];
                let message = format!("{} {} · {}\n{}", commit.short_sha, commit.author, commit.age, commit.summary);
                let age_color = rustext_egui::code_editor::to_rgba(age_colors[commit_index]);
                if commit_index % 2 == 1 {
                    decorations.push(Decoration::lines(text, line, line, DecorationKind::WholeLineBackground(shade)));
                }
                decorations.push(
                    Decoration::lines(text, line, line, DecorationKind::LineNumberColor(age_color))
                        .with_hover_message(message.clone()),
                );
                let starts_group = line == 0 || blame_commit_index(line - 1) != commit_index;
                if starts_group {
                    decorations.push(
                        Decoration::lines(text, line, line, DecorationKind::Glyph(age_color))
                            .with_hover_message(message),
                    );
                }
            }
        }
        decorations
    }

    fn options_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Editor");
        ui.checkbox(&mut self.shows_diagnostics, "Diagnostics demo (F8)");
        ui.checkbox(&mut self.shows_blame, "Blame demo");
        let mut language_name = self.highlighter.language().name.clone();
        egui::ComboBox::from_label("Language").selected_text(&language_name).height(400.0).show_ui(ui, |ui| {
            for name in self.languages.names() {
                ui.selectable_value(&mut language_name, name.clone(), name);
            }
        });
        if language_name != self.highlighter.language().name
            && let Some(language) = self.languages.by_name(&language_name)
        {
            self.state.language = language.configuration.clone();
            self.highlighter.set_language(language);
        }
        egui::ComboBox::from_label("Line numbers")
            .selected_text(format!("{:?}", self.view_config.line_numbers))
            .show_ui(ui, |ui| {
                for line_numbers in [LineNumbers::On, LineNumbers::Relative, LineNumbers::Off] {
                    ui.selectable_value(&mut self.view_config.line_numbers, line_numbers, format!("{line_numbers:?}"));
                }
            });
        egui::ComboBox::from_label("Cursor").selected_text(format!("{:?}", self.view_config.cursor_style)).show_ui(
            ui,
            |ui| {
                for cursor_style in [CursorStyle::Line, CursorStyle::Block, CursorStyle::Underline] {
                    ui.selectable_value(&mut self.view_config.cursor_style, cursor_style, format!("{cursor_style:?}"));
                }
            },
        );
        egui::ComboBox::from_label("Whitespace")
            .selected_text(format!("{:?}", self.view_config.render_whitespace))
            .show_ui(ui, |ui| {
                for mode in [
                    WhitespaceRendering::None,
                    WhitespaceRendering::Selection,
                    WhitespaceRendering::Boundary,
                    WhitespaceRendering::Trailing,
                    WhitespaceRendering::All,
                ] {
                    ui.selectable_value(&mut self.view_config.render_whitespace, mode, format!("{mode:?}"));
                }
            });
        egui::ComboBox::from_label("Word wrap").selected_text(format!("{:?}", self.view_config.word_wrap)).show_ui(
            ui,
            |ui| {
                for word_wrap in [WordWrap::Off, WordWrap::Viewport, WordWrap::Column(80), WordWrap::Bounded(100)] {
                    ui.selectable_value(&mut self.view_config.word_wrap, word_wrap, format!("{word_wrap:?}"));
                }
            },
        );
        ui.checkbox(&mut self.view_config.show_carriage_returns, "Show CR of CRLF lines");
        ui.checkbox(&mut self.view_config.folding, "Folding");
        ui.checkbox(&mut self.view_config.indent_guides, "Indent guides");
        ui.checkbox(&mut self.view_config.match_brackets, "Match brackets");
        ui.checkbox(&mut self.view_config.occurrences_highlight, "Highlight occurrences");
        ui.checkbox(&mut self.view_config.cursor_blinking, "Cursor blinking");
        ui.checkbox(&mut self.view_config.scroll_beyond_last_line, "Scroll beyond last line");
        ui.checkbox(&mut self.state.config.read_only, "Read-only");
        ui.add(egui::Slider::new(&mut self.view_config.font_size, 8.0..=32.0).text("Font size"));
        ui.add(egui::Slider::new(&mut self.state.config.indentation.tab_size, 1..=8).text("Tab size"));
        ui.checkbox(&mut self.state.config.indentation.insert_spaces, "Insert spaces");
        ui.separator();
        ui.label(format!("Last: {}", self.last_action));
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        let text = self.state.text();
        let primary = self.state.selections().primary();
        let head = text.position(primary.head);
        let selection_count = self.state.selections().len();
        let is_dirty = self.state.history().version() != self.saved_version;
        let indentation = self.state.config.indentation;
        ui.horizontal(|ui| {
            ui.label(format!("{}{}", self.file_name, if is_dirty { " ●" } else { "" }));
            ui.separator();
            ui.label(format!("Ln {}, Col {}", head.line + 1, head.column + 1));
            if !primary.is_empty() {
                ui.label(format!("({} selected)", primary.end() - primary.start()));
            }
            if selection_count > 1 {
                ui.label(format!("{selection_count} selections"));
            }
            ui.separator();
            let indent = if indentation.insert_spaces {
                format!("Spaces: {}", indentation.tab_size)
            } else {
                format!("Tab size: {}", indentation.tab_size)
            };
            ui.label(indent);
            ui.label(format!("{:?}", text.line_ending()).to_uppercase());
            if !self.view.pending_chords().is_empty() {
                ui.label("waiting for the second key of a chord…");
            }
        });
    }
}
