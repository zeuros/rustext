//! Code lenses: a row of labels above their line; a click on one is reported, and the lines
//! under it are still where clicks land.

use egui::{Event, Modifiers, PointerButton, Pos2, Vec2};
use egui_kittest::Harness;
use rustext_core::services::CodeLens;
use rustext_core::{EditorState, Position};
use rustext_egui::{CodeEditor, EditorTheme, EditorView, Keymap, ViewConfig};

struct Fixture {
    state: EditorState,
    view: EditorView,
    config: ViewConfig,
    theme: EditorTheme,
    keymap: Keymap,
    code_lenses: Vec<CodeLens>,
    clicked: Vec<usize>,
}

fn harness(content: &str, code_lenses: Vec<CodeLens>) -> Harness<'static, Fixture> {
    let fixture = Fixture {
        state: EditorState::new(content),
        view: EditorView::default(),
        config: ViewConfig { cursor_blinking: false, ..ViewConfig::default() },
        theme: EditorTheme::dark(),
        keymap: Keymap::vscode(),
        code_lenses,
        clicked: Vec::new(),
    };
    let mut harness = Harness::builder().with_size(Vec2::new(800.0, 300.0)).build_ui_state(
        |ui, fixture: &mut Fixture| {
            let response = CodeEditor::new(
                &mut fixture.state,
                &mut fixture.view,
                &fixture.config,
                &fixture.theme,
                &fixture.keymap,
            )
            .code_lenses(&fixture.code_lenses)
            .show(ui);
            fixture.clicked.extend(response.clicked_code_lens);
        },
        fixture,
    );
    harness.run();
    harness
}

fn click_at(harness: &mut Harness<'_, Fixture>, pos: Pos2) {
    for pressed in [true, false] {
        harness.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.run();
    }
}

fn cell(harness: &Harness<'_, Fixture>, row: usize, visual_column: usize) -> Pos2 {
    harness.state().view.geometry().unwrap().cell_center(row, visual_column)
}

/// The center of the char at `column` of a code lens row (lenses use a font 0.85 times the
/// code's size).
fn lens_char(harness: &Harness<'_, Fixture>, row: usize, column: usize) -> Pos2 {
    let geometry = harness.state().view.geometry().unwrap();
    let row_start = geometry.cell_center(row, 0);
    let lens_char_width = geometry.char_width * 0.85;
    Pos2::new(row_start.x - geometry.char_width / 2.0 + (column as f32 + 0.5) * lens_char_width, row_start.y)
}

#[test]
fn clicking_a_code_lens_reports_its_index() {
    let lenses = vec![CodeLens::new(1, "Stage hunk"), CodeLens::new(1, "Discard"), CodeLens::new(0, "blame")];
    let mut harness = harness("first\nsecond\n", lenses);

    // Rows: lens of line 0, line 0, lens of line 1, line 1. "Discard" is after "Stage hunk | ".
    let discard = lens_char(&harness, 2, "Stage hunk | ".len() + 2);
    let blame = lens_char(&harness, 0, 1);
    click_at(&mut harness, discard);
    click_at(&mut harness, blame);

    assert_eq!(harness.state().clicked, vec![1, 2]);
}

#[test]
fn lines_below_code_lenses_still_take_clicks() {
    let mut harness = harness("first\nsecond\n", vec![CodeLens::new(1, "Run")]);

    // Rows: line 0, the lens of line 1, line 1.
    let char_width = harness.state().view.geometry().unwrap().char_width;
    let line_one = cell(&harness, 2, 3) - Vec2::new(char_width * 0.4, 0.0);
    click_at(&mut harness, line_one);

    let state = &harness.state().state;
    assert_eq!(state.text().position(state.selections().primary().head), Position::new(1, 3));
    assert!(harness.state().clicked.is_empty());
}

#[test]
fn a_click_between_labels_reports_nothing() {
    let mut harness = harness("first\n", vec![CodeLens::new(0, "Run")]);

    let past_the_label = cell(&harness, 0, 30);
    click_at(&mut harness, past_the_label);

    assert!(harness.state().clicked.is_empty());
}
