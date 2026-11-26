//! The rustext playground: an editor with its options on the side, like Monaco's playground.
//!
//! `rustext-demo [file]` opens a file (a sample when none is given).

use eframe::egui;
use rustext_core::{EditorState, Text};
use rustext_egui::code_editor::{CursorStyle, LineNumbers};
use rustext_egui::{CodeEditor, EditorTheme, EditorView, Keymap, ViewConfig};
use rustext_syntax::{LanguageRegistry, SyntaxHighlighter, SyntaxTheme};

const SAMPLE: &str = include_str!("../samples/sample.rs");

struct Playground {
    state: EditorState,
    view: EditorView,
    view_config: ViewConfig,
    theme_name: ThemeName,
    keymap: Keymap,
    file_name: String,
    saved_version: u64,
    last_action: String,
    languages: LanguageRegistry,
    highlighter: SyntaxHighlighter,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ThemeName {
    Dark,
    Light,
    GitgudDark,
    GitgudLight,
}

impl ThemeName {
    const ALL: [ThemeName; 4] = [ThemeName::Dark, ThemeName::Light, ThemeName::GitgudDark, ThemeName::GitgudLight];

    fn label(self) -> &'static str {
        match self {
            ThemeName::Dark => "Dark",
            ThemeName::Light => "Light",
            ThemeName::GitgudDark => "Gitgud dark",
            ThemeName::GitgudLight => "Gitgud light",
        }
    }

    fn syntax_theme(self) -> SyntaxTheme {
        match self {
            ThemeName::Dark | ThemeName::GitgudDark => SyntaxTheme::dark_modern(),
            ThemeName::Light | ThemeName::GitgudLight => SyntaxTheme::light_modern(),
        }
    }

    fn theme(self) -> EditorTheme {
        match self {
            ThemeName::Dark => EditorTheme::dark(),
            ThemeName::Light => EditorTheme::light(),
            ThemeName::GitgudDark => EditorTheme::gitgud_dark(),
            ThemeName::GitgudLight => EditorTheme::gitgud_light(),
        }
    }
}

impl Playground {
    fn new(file_name: String, content: &str) -> Self {
        let text = Text::new(content);
        let mut config = rustext_core::EditingConfig::default();
        config.indentation = rustext_core::indentation::Indentation::detect(&text, config.indentation);
        let languages = LanguageRegistry::new();
        let language = languages.detect(&file_name, &text.line_content(0));
        let state = EditorState::with_text(text).with_config(config).with_language(language.configuration.clone());
        let highlighter = SyntaxHighlighter::new(&languages, language, ThemeName::Dark.syntax_theme());
        Self {
            languages,
            highlighter,
            saved_version: state.history().version(),
            state,
            view: EditorView::default(),
            view_config: ViewConfig { rulers: vec![100], ..ViewConfig::default() },
            theme_name: ThemeName::Dark,
            keymap: Keymap::vscode(),
            file_name,
            last_action: String::new(),
        }
    }

    fn options_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Options");
        let previous_theme_name = self.theme_name;
        egui::ComboBox::from_label("Theme").selected_text(self.theme_name.label()).show_ui(ui, |ui| {
            for theme_name in ThemeName::ALL {
                ui.selectable_value(&mut self.theme_name, theme_name, theme_name.label());
            }
        });
        if self.theme_name != previous_theme_name {
            self.highlighter.set_theme(self.theme_name.syntax_theme());
        }
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

impl eframe::App for Playground {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let theme = self.theme_name.theme();
        ui.ctx().set_visuals(if theme.is_dark { egui::Visuals::dark() } else { egui::Visuals::light() });

        egui::Panel::left("options").resizable(true).default_size(220.0).show(ui, |ui| self.options_panel(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let response = CodeEditor::new(&mut self.state, &mut self.view, &self.view_config, &theme, &self.keymap)
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
}

fn main() -> eframe::Result {
    let path = std::env::args().nth(1);
    let (file_name, content) = match &path {
        Some(path) => {
            (path.clone(), std::fs::read_to_string(path).unwrap_or_else(|error| format!("Cannot read {path}: {error}")))
        }
        None => ("sample.rs".to_string(), SAMPLE.to_string()),
    };
    let renderer = if std::env::var("RUSTEXT_RENDERER").as_deref() == Ok("wgpu") {
        eframe::Renderer::Wgpu
    } else {
        eframe::Renderer::Glow
    };
    let options = eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default().with_inner_size([1200.0, 760.0]).with_title("rustext playground"),
        ..Default::default()
    };
    eframe::run_native(
        "rustext playground",
        options,
        Box::new(move |creation_context| {
            creation_context.egui_ctx.options_mut(|options| options.zoom_with_keyboard = false);
            Ok(Box::new(Playground::new(file_name, &content)))
        }),
    )
}
