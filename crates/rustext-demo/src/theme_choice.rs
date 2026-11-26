//! The playground's themes: editor colors and matching token colors.

use eframe::egui;
use rustext_egui::EditorTheme;
use rustext_syntax::SyntaxTheme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThemeChoice {
    Dark,
    Light,
    GitgudDark,
    GitgudLight,
}

impl ThemeChoice {
    const ALL: [ThemeChoice; 4] =
        [ThemeChoice::Dark, ThemeChoice::Light, ThemeChoice::GitgudDark, ThemeChoice::GitgudLight];

    fn label(self) -> &'static str {
        match self {
            ThemeChoice::Dark => "Dark",
            ThemeChoice::Light => "Light",
            ThemeChoice::GitgudDark => "Gitgud dark",
            ThemeChoice::GitgudLight => "Gitgud light",
        }
    }

    pub fn syntax_theme(self) -> SyntaxTheme {
        match self {
            ThemeChoice::Dark | ThemeChoice::GitgudDark => SyntaxTheme::dark_modern(),
            ThemeChoice::Light | ThemeChoice::GitgudLight => SyntaxTheme::light_modern(),
        }
    }

    pub fn theme(self) -> EditorTheme {
        match self {
            ThemeChoice::Dark => EditorTheme::dark(),
            ThemeChoice::Light => EditorTheme::light(),
            ThemeChoice::GitgudDark => EditorTheme::gitgud_dark(),
            ThemeChoice::GitgudLight => EditorTheme::gitgud_light(),
        }
    }

    pub fn combo_box(&mut self, ui: &mut egui::Ui) {
        egui::ComboBox::from_label("Theme").selected_text(self.label()).show_ui(ui, |ui| {
            for choice in Self::ALL {
                ui.selectable_value(self, choice, choice.label());
            }
        });
    }
}
