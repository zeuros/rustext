//! The rustext playground: the editor and the diff editor with their options on the side, like
//! Monaco's playground.
//!
//! `rustext-demo [file]` opens a file (a sample when none is given);
//! `rustext-demo --diff <old> <new>` opens a diff; `--page diff` starts on the diff page;
//! `--inline` shows the diff inline; `--find <pattern>` opens the find widget;
//! `--showcase` folds a block, shows whitespace and puts the cursor on a bracket; `--blame`
//! turns the blame demo on; `--wrap <columns>` wraps lines;
//! `--complete` opens the completion list.

mod diff_page;
mod editor_page;
mod merge_page;
mod theme_choice;

use eframe::egui;
use rustext_egui::Keymap;
use rustext_syntax::LanguageRegistry;

use crate::diff_page::DiffPage;
use crate::editor_page::EditorPage;
use crate::merge_page::MergePage;
use crate::theme_choice::ThemeChoice;

const SAMPLE: &str = include_str!("../samples/sample.rs");

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Editor,
    Diff,
    Merge,
}

struct Playground {
    page: Page,
    theme_choice: ThemeChoice,
    keymap: Keymap,
    editor_page: EditorPage,
    diff_page: DiffPage,
    merge_page: MergePage,
}

impl eframe::App for Playground {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let theme = self.theme_choice.theme();
        ui.ctx().set_visuals(if theme.is_dark { egui::Visuals::dark() } else { egui::Visuals::light() });

        egui::Panel::top("pages").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.page, Page::Editor, "Editor");
                ui.selectable_value(&mut self.page, Page::Diff, "Diff");
                ui.selectable_value(&mut self.page, Page::Merge, "Merge");
                ui.separator();
                self.theme_choice.combo_box(ui);
            });
        });
        match self.page {
            Page::Editor => self.editor_page.show(ui, &self.theme_choice, &self.keymap),
            Page::Diff => self.diff_page.show(ui, &self.theme_choice, &self.keymap),
            Page::Merge => self.merge_page.show(ui, &self.theme_choice, &self.keymap),
        }
    }
}

struct Arguments {
    file: Option<String>,
    diff_files: Option<(String, String)>,
    page: Page,
    inline: bool,
    find: Option<String>,
    showcase: bool,
    blame: bool,
    wrap_columns: Option<usize>,
    complete: bool,
}

fn parse_arguments() -> Arguments {
    let mut arguments = Arguments {
        file: None,
        diff_files: None,
        page: Page::Editor,
        inline: false,
        find: None,
        showcase: false,
        blame: false,
        wrap_columns: None,
        complete: false,
    };
    let mut raw = std::env::args().skip(1);
    while let Some(argument) = raw.next() {
        match argument.as_str() {
            "--diff" => {
                if let (Some(old), Some(new)) = (raw.next(), raw.next()) {
                    arguments.diff_files = Some((old, new));
                    arguments.page = Page::Diff;
                }
            }
            "--find" => arguments.find = raw.next(),
            "--showcase" => arguments.showcase = true,
            "--blame" => arguments.blame = true,
            "--complete" => arguments.complete = true,
            "--wrap" => arguments.wrap_columns = raw.next().and_then(|columns| columns.parse().ok()),
            "--inline" => {
                arguments.inline = true;
                arguments.page = Page::Diff;
            }
            "--page" => match raw.next().as_deref() {
                Some("diff") => arguments.page = Page::Diff,
                Some("merge") => arguments.page = Page::Merge,
                _ => {}
            },
            _ => arguments.file = Some(argument),
        }
    }
    arguments
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| format!("Cannot read {path}: {error}"))
}

fn main() -> eframe::Result {
    let arguments = parse_arguments();
    let (file_name, content) = match &arguments.file {
        Some(path) => (path.clone(), read(path)),
        None => ("sample.rs".to_string(), SAMPLE.to_string()),
    };
    let (diff_file_name, old_content, new_content) = match &arguments.diff_files {
        Some((old_path, new_path)) => (new_path.clone(), read(old_path), read(new_path)),
        None => ("sample.rs".to_string(), SAMPLE.to_string(), diff_page::edited_sample(SAMPLE)),
    };
    let renderer = if std::env::var("RUSTEXT_RENDERER").as_deref() == Ok("wgpu") {
        eframe::Renderer::Wgpu
    } else {
        eframe::Renderer::Glow
    };
    let options = eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default().with_inner_size([1300.0, 800.0]).with_title("rustext playground"),
        ..Default::default()
    };
    eframe::run_native(
        "rustext playground",
        options,
        Box::new(move |creation_context| {
            creation_context.egui_ctx.options_mut(|options| options.zoom_with_keyboard = false);
            let languages = LanguageRegistry::new();
            let theme_choice = ThemeChoice::Dark;
            Ok(Box::new(Playground {
                page: arguments.page,
                editor_page: {
                    let mut editor_page = EditorPage::new(
                        &languages,
                        &theme_choice,
                        file_name,
                        &content,
                        arguments.find,
                        arguments.showcase,
                    );
                    editor_page.shows_blame = arguments.blame;
                    if arguments.complete {
                        editor_page.open_completion_after(30, "shapes.it");
                    }
                    if let Some(columns) = arguments.wrap_columns {
                        editor_page.set_word_wrap(rustext_egui::code_editor::WordWrap::Column(columns));
                    }
                    editor_page
                },
                diff_page: DiffPage::new(
                    &languages,
                    &theme_choice,
                    diff_file_name,
                    &old_content,
                    &new_content,
                    arguments.inline,
                ),
                merge_page: MergePage::new(&languages, &theme_choice),
                theme_choice,
                keymap: Keymap::vscode(),
            }))
        }),
    )
}
