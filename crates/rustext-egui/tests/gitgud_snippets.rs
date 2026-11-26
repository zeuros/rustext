//! The snippets of `docs/GITGUD_INTEGRATION.md`, compiled and run for a frame, so the guide
//! stays true to the API.

use egui::Vec2;
use egui_kittest::Harness;
use rustext_core::EditorState;
use rustext_core::decoration::{Decoration, DecorationKind};
use rustext_core::style::Rgba;
use rustext_diff::{CollapseOptions, LineSelection, PatchDirection, build_patch};
use rustext_egui::{
    CodeEditor, DiffDocument, DiffEditor, DiffViewConfig, DiffViewMode, EditorTheme, EditorView, Keymap, MergeDocument,
    MergeEditor, ViewConfig,
};
use rustext_syntax::{LanguageRegistry, SyntaxHighlighter, SyntaxTheme};

struct Gitgud {
    diff: DiffDocument,
    merge: MergeDocument,
    blame_state: EditorState,
    blame_view: EditorView,
    view_config: ViewConfig,
    theme: EditorTheme,
    keymap: Keymap,
    old_highlighter: SyntaxHighlighter,
    new_highlighter: SyntaxHighlighter,
    patches: Vec<String>,
}

#[test]
fn integration_guide_snippets_work() {
    let languages = LanguageRegistry::new();
    let language = languages.detect("src/main.rs", "");
    let highlighter = || SyntaxHighlighter::new(&languages, language.clone(), SyntaxTheme::dark_modern());
    let mut diff = DiffDocument::new("fn a() {}\nfn b() {}\n", "fn a() {}\nfn c() {}\n");
    diff.go_to_change(true);
    let gitgud = Gitgud {
        diff,
        merge: MergeDocument::new("x\n", "ours\n", "theirs\n"),
        blame_state: EditorState::new("line one\nline two\n"),
        blame_view: EditorView::default(),
        view_config: ViewConfig::default(),
        theme: EditorTheme::gitgud_dark(),
        keymap: Keymap::vscode(),
        old_highlighter: highlighter(),
        new_highlighter: highlighter(),
        patches: Vec::new(),
    };
    let mut harness = Harness::builder().with_size(Vec2::new(1200.0, 900.0)).build_ui_state(
        |ui, gitgud: &mut Gitgud| {
            let split =
                DiffViewConfig { mode: DiffViewMode::SideBySide, inline_below_width: 0.0, ..DiffViewConfig::default() };
            let hunk = DiffViewConfig { collapse_unchanged: Some(CollapseOptions::default()), ..split.clone() };
            ui.allocate_ui(Vec2::new(1200.0, 300.0), |ui| {
                let response =
                    DiffEditor::new(&mut gitgud.diff, &hunk, &gitgud.view_config, &gitgud.theme, &gitgud.keymap)
                        .stylers(&mut gitgud.old_highlighter, &mut gitgud.new_highlighter)
                        .show(ui);
                response.new.response.context_menu(|ui| {
                    if ui.button("Stage selected lines").clicked() {
                        ui.close();
                    }
                });
            });

            let selection = gitgud
                .diff
                .hunk_at_cursor()
                .map(|index| LineSelection::hunk(gitgud.diff.diff(), index))
                .unwrap_or_default();
            let document = &gitgud.diff;
            if let Some(patch) = build_patch(
                document.diff(),
                document.old.text(),
                document.new.text(),
                &selection,
                PatchDirection::Stage,
                "src/main.rs",
            ) {
                gitgud.patches.push(patch);
            }

            let text = gitgud.blame_state.text().clone();
            let blame = vec![
                Decoration::lines(&text, 0, 0, DecorationKind::WholeLineBackground(Rgba::hex(0x303030))),
                Decoration::lines(&text, 0, 0, DecorationKind::Glyph(Rgba::hex(0x3794ff)))
                    .with_hover_message("a1b2c3d Ada · today\nInitial"),
            ];
            ui.allocate_ui(Vec2::new(1200.0, 200.0), |ui| {
                let response = CodeEditor::new(
                    &mut gitgud.blame_state,
                    &mut gitgud.blame_view,
                    &gitgud.view_config,
                    &gitgud.theme,
                    &gitgud.keymap,
                )
                .decorations(&blame)
                .glyph_margin(true)
                .id_salt("blame")
                .show(ui);
                if let Some(rustext_egui::MouseTarget::LineNumber { line: _ }) = response.pressed {}
            });

            ui.allocate_ui(Vec2::new(1200.0, 350.0), |ui| {
                MergeEditor::new(&mut gitgud.merge, &gitgud.view_config, &gitgud.theme, &gitgud.keymap)
                    .labels("Ours: HEAD (main)", "Theirs: feature")
                    .show(ui);
            });
        },
        gitgud,
    );
    harness.run();

    let gitgud = harness.state();
    // The cursor went to the first change, which is the hunk staged.
    assert_eq!(
        gitgud.patches.last().map(String::as_str),
        Some("--- a/src/main.rs\n+++ b/src/main.rs\n@@ -2 +2 @@\n-fn b() {}\n+fn c() {}\n")
    );
    assert_eq!(gitgud.merge.unresolved_conflict_count(), 1);
}
