//! The playground's diff page: a `DiffEditor` with its options, and the patch gitgud would
//! apply to stage the selected lines.

use eframe::egui;
use rustext_diff::{CollapseOptions, PatchDirection, build_patch};
use rustext_egui::{DiffDocument, DiffEditor, DiffViewConfig, DiffViewMode, Keymap, ViewConfig};
use rustext_syntax::{LanguageRegistry, SyntaxHighlighter};

use crate::theme_choice::ThemeChoice;

/// The sample with a few edits, to diff against itself.
pub fn edited_sample(sample: &str) -> String {
    sample
        .replace("/// Counts how often each word appears in `text`.", "/// Counts how many times each word appears in `text`,\n/// case-sensitively.")
        .replace("        *counts.entry(word).or_insert(0) += 1;\n", "        let count = counts.entry(word).or_insert(0);\n        *count += 1;\n")
        .replace("#[derive(Debug, Clone, PartialEq)]\n", "#[derive(Debug, Clone, Copy, PartialEq)]\n")
        .replace("    Rectangle { width: f64, height: f64 },\n", "    Rectangle { width: f64, height: f64 },\n    Square { side: f64 },\n")
        .replace("            Shape::Rectangle { width, height } => width * height,\n", "            Shape::Rectangle { width, height } => width * height,\n            Shape::Square { side } => side * side,\n")
        .replace("    let counts = word_counts(\"the quick brown fox jumps over the lazy dog the end\");\n    println!(\"{counts:?}\");\n", "")
}

pub struct DiffPage {
    document: DiffDocument,
    config: DiffViewConfig,
    view_config: ViewConfig,
    file_name: String,
    old_highlighter: SyntaxHighlighter,
    new_highlighter: SyntaxHighlighter,
    highlighted_theme: ThemeChoice,
    patch_direction: PatchDirection,
}

impl DiffPage {
    #[expect(clippy::too_many_arguments, reason = "predates the complexity lints (D28)")]
    pub fn new(
        languages: &LanguageRegistry,
        theme_choice: &ThemeChoice,
        file_name: String,
        old: &str,
        new: &str,
        inline: bool,
    ) -> Self {
        let language = languages.detect(&file_name, new.lines().next().unwrap_or_default());
        let mut document = DiffDocument::new(old, new);
        document.new.language = language.configuration.clone();
        Self {
            document,
            config: DiffViewConfig {
                mode: if inline { DiffViewMode::Inline } else { DiffViewMode::SideBySide },
                collapse_unchanged: Some(CollapseOptions::default()),
                ..DiffViewConfig::default()
            },
            view_config: ViewConfig {
                current_line_highlight: rustext_egui::code_editor::CurrentLineHighlight::Gutter,
                ..ViewConfig::default()
            },
            old_highlighter: SyntaxHighlighter::new(languages, language.clone(), theme_choice.syntax_theme()),
            new_highlighter: SyntaxHighlighter::new(languages, language, theme_choice.syntax_theme()),
            highlighted_theme: *theme_choice,
            file_name,
            patch_direction: PatchDirection::Stage,
        }
    }

    pub fn set_word_wrap(&mut self, word_wrap: rustext_egui::code_editor::WordWrap) {
        self.view_config.word_wrap = word_wrap;
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme_choice: &ThemeChoice, keymap: &Keymap) {
        if self.highlighted_theme != *theme_choice {
            self.old_highlighter.set_theme(theme_choice.syntax_theme());
            self.new_highlighter.set_theme(theme_choice.syntax_theme());
            self.highlighted_theme = *theme_choice;
        }
        let theme = theme_choice.theme();
        egui::Panel::left("diff-options").resizable(true).default_size(240.0).show(ui, |ui| self.options_panel(ui));
        egui::Panel::bottom("diff-patch").resizable(true).default_size(170.0).show(ui, |ui| self.patch_panel(ui));
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            DiffEditor::new(&mut self.document, &self.config, &self.view_config, &theme, keymap)
                .stylers(&mut self.old_highlighter, &mut self.new_highlighter)
                .show(ui);
        });
    }

    fn options_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Diff");
        let diff = self.document.diff();
        ui.label(format!(
            "{}: +{} −{} in {} hunks",
            self.file_name,
            diff.added_line_count(),
            diff.removed_line_count(),
            diff.hunks.len()
        ));
        ui.separator();
        ui.radio_value(&mut self.config.mode, DiffViewMode::SideBySide, "Side by side");
        ui.radio_value(&mut self.config.mode, DiffViewMode::Inline, "Inline");
        let mut collapses = self.config.collapse_unchanged.is_some();
        if ui.checkbox(&mut collapses, "Collapse unchanged regions").changed() {
            self.config.collapse_unchanged = collapses.then(CollapseOptions::default);
        }
        ui.checkbox(&mut self.config.diff_options.ignore_trim_whitespace, "Ignore trim whitespace");
        ui.checkbox(&mut self.config.old_editable, "Old side editable");
        ui.add(egui::Slider::new(&mut self.config.inline_below_width, 0.0..=1500.0).text("Inline below width"));
        ui.separator();
        if ui.button("Next change (F7)").clicked() {
            self.document.go_to_change(true);
        }
        if ui.button("Previous change (Shift+F7)").clicked() {
            self.document.go_to_change(false);
        }
    }

    fn patch_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Patch of the selected lines:");
            ui.radio_value(
                &mut self.patch_direction,
                PatchDirection::Stage,
                "stage (git apply --cached --unidiff-zero)",
            );
            ui.radio_value(
                &mut self.patch_direction,
                PatchDirection::Revert,
                "discard / unstage (git apply -R --unidiff-zero)",
            );
        });
        let selection = self.document.selected_lines();
        let patch = build_patch(
            self.document.diff(),
            self.document.old.text(),
            self.document.new.text(),
            &selection,
            self.patch_direction,
            &self.file_name,
        );
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut shown = patch.unwrap_or_else(|| "(select changed lines: click or drag in either side)".into());
            ui.add(egui::TextEdit::multiline(&mut shown).code_editor().desired_width(f32::INFINITY).interactive(false));
        });
    }
}
