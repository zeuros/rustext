//! The playground's editor page: one file in a `CodeEditor`, its options on the side.

use eframe::egui;
use rustext_core::indentation::Indentation;
use rustext_core::{EditingConfig, EditorState, Text};
use rustext_egui::code_editor::{CursorStyle, LineNumbers};
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
}

impl EditorPage {
    pub fn new(
        languages: &LanguageRegistry,
        theme_choice: &ThemeChoice,
        file_name: String,
        content: &str,
        find: Option<String>,
    ) -> Self {
        let text = Text::new(content);
        let config = EditingConfig {
            indentation: Indentation::detect(&text, Indentation::default()),
            ..EditingConfig::default()
        };
        let language = languages.detect(&file_name, &text.line_content(0));
        let state = EditorState::with_text(text).with_config(config).with_language(language.configuration.clone());
        let mut view = EditorView::default();
        if let Some(pattern) = find {
            view.find.is_open = true;
            view.find.is_replace_shown = true;
            view.find.query.pattern = pattern;
        }
        Self {
            saved_version: state.history().version(),
            state,
            view,
            view_config: ViewConfig { rulers: vec![100], ..ViewConfig::default() },
            file_name,
            last_action: String::new(),
            highlighter: SyntaxHighlighter::new(languages, language, theme_choice.syntax_theme()),
            languages: languages.clone(),
            highlighted_theme: *theme_choice,
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
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let response = CodeEditor::new(&mut self.state, &mut self.view, &self.view_config, &theme, keymap)
                .styler(&mut self.highlighter)
                .show(ui);
            if let Some(action) = response.unhandled_actions.last() {
                self.last_action = format!("{action:?}");
            }
            if response.outcome.rejected_read_only {
                self.last_action = "Cannot edit in read-only editor".into();
            }
        });
    }

    fn options_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Editor");
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
