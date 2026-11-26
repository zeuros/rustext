//! The playground's merge page: a three-way merge with conflicts to resolve.

use eframe::egui;
use rustext_egui::{Keymap, MergeDocument, MergeEditor, ViewConfig};
use rustext_syntax::{LanguageRegistry, SyntaxHighlighter};

use crate::theme_choice::ThemeChoice;

const BASE: &str = "fn greet(name: &str) -> String {
    format!(\"Hello, {name}\")
}

fn main() {
    let names = [\"Ada\", \"Grace\"];
    for name in names {
        println!(\"{}\", greet(name));
    }
}
";

const OURS: &str = "fn greet(name: &str) -> String {
    format!(\"Hello, {name}!\")
}

fn main() {
    let names = [\"Ada\", \"Grace\", \"Linus\"];
    for name in names {
        println!(\"{}\", greet(name));
    }
}
";

const THEIRS: &str = "/// Greets someone politely.
fn greet(name: &str) -> String {
    format!(\"Good morning, {name}\")
}

fn main() {
    let names = [\"Ada\", \"Grace\"];
    for name in names {
        println!(\"{}\", greet(name));
    }
    println!(\"done\");
}
";

pub struct MergePage {
    document: MergeDocument,
    view_config: ViewConfig,
    highlighters: [SyntaxHighlighter; 3],
    highlighted_theme: ThemeChoice,
}

impl MergePage {
    pub fn new(languages: &LanguageRegistry, theme_choice: &ThemeChoice) -> Self {
        let language = languages.detect("main.rs", "");
        let highlighter = || SyntaxHighlighter::new(languages, language.clone(), theme_choice.syntax_theme());
        Self {
            document: MergeDocument::new(BASE, OURS, THEIRS),
            view_config: ViewConfig::default(),
            highlighters: [highlighter(), highlighter(), highlighter()],
            highlighted_theme: *theme_choice,
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, theme_choice: &ThemeChoice, keymap: &Keymap) {
        if self.highlighted_theme != *theme_choice {
            for highlighter in &mut self.highlighters {
                highlighter.set_theme(theme_choice.syntax_theme());
            }
            self.highlighted_theme = *theme_choice;
        }
        let theme = theme_choice.theme();
        egui::CentralPanel::default().show(ui, |ui| {
            let [ours, theirs, result] = &mut self.highlighters;
            MergeEditor::new(&mut self.document, &self.view_config, &theme, keymap)
                .labels("Ours: HEAD (main)", "Theirs: feature/polite-greeting")
                .stylers([ours, theirs, result])
                .show(ui);
        });
    }
}
