//! Drives the `DiffEditor` headlessly: selecting lines for a patch, navigating changes,
//! expanding collapsed regions, editing the new side.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Vec2};
use egui_kittest::Harness;
use rustext_diff::{CollapseOptions, LineSelection, PatchDirection, build_patch};
use rustext_egui::diff_editor::select_lines;
use rustext_egui::{DiffDocument, DiffEditor, DiffSide, DiffViewConfig, DiffViewMode, EditorTheme, Keymap, ViewConfig};

struct Fixture {
    document: DiffDocument,
    config: DiffViewConfig,
    view_config: ViewConfig,
    theme: EditorTheme,
    keymap: Keymap,
}

fn harness(old: &str, new: &str, config: DiffViewConfig) -> Harness<'static, Fixture> {
    let fixture = Fixture {
        document: DiffDocument::new(old, new),
        config,
        view_config: ViewConfig { cursor_blinking: false, ..ViewConfig::default() },
        theme: EditorTheme::dark(),
        keymap: Keymap::vscode(),
    };
    let mut harness = Harness::builder().with_size(Vec2::new(1000.0, 500.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, fixture: &mut Fixture| {
            DiffEditor::new(
                &mut fixture.document,
                &fixture.config,
                &fixture.view_config,
                &fixture.theme,
                &fixture.keymap,
            )
            .show(ui);
        },
        fixture,
    );
    harness.run();
    harness
}

fn click(harness: &mut Harness<'_, Fixture>, pos: Pos2) {
    harness.event(Event::PointerMoved(pos));
    harness.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    harness.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();
}

fn click_new_row(harness: &mut Harness<'_, Fixture>, row: usize) {
    let pos = harness.state().document.new_view.geometry().expect("painted").cell_center(row, 2);
    click(harness, pos);
}

#[test]
fn clicking_an_added_line_selects_it_for_staging() {
    let mut harness =
        harness("a\nb\n", "a\nx\nb\n", DiffViewConfig { inline_below_width: 0.0, ..DiffViewConfig::default() });

    click_new_row(&mut harness, 1);

    let document = &harness.state().document;
    assert_eq!(document.focused_side(), DiffSide::New);
    let selection = document.selected_lines();
    assert_eq!(selection, LineSelection { new_lines: [1].into(), ..LineSelection::default() });
    let patch =
        build_patch(document.diff(), document.old.text(), document.new.text(), &selection, PatchDirection::Stage, "f");
    assert_eq!(patch.unwrap(), "--- a/f\n+++ b/f\n@@ -1,0 +2 @@\n+x\n");
}

#[test]
fn f7_goes_to_the_next_change() {
    let old: String = (0..30).map(|line| format!("line {line}\n")).collect();
    let new = old.replace("line 5\n", "five\n").replace("line 20\n", "twenty\n");
    let mut harness = harness(&old, &new, DiffViewConfig { inline_below_width: 0.0, ..DiffViewConfig::default() });
    click_new_row(&mut harness, 0);

    harness.key_press(Key::F7);
    harness.run();
    let line_of_cursor = |harness: &Harness<'_, Fixture>| {
        let new = &harness.state().document.new;
        new.text().line_of_offset(new.selections().primary().head)
    };
    assert_eq!(line_of_cursor(&harness), 5);

    harness.key_press(Key::F7);
    harness.run();
    assert_eq!(line_of_cursor(&harness), 20);

    harness.key_press_modifiers(Modifiers::SHIFT, Key::F7);
    harness.run();
    assert_eq!(line_of_cursor(&harness), 5);
}

#[test]
fn clicking_a_collapsed_region_expands_it() {
    let old: String = (0..30).map(|line| format!("line {line}\n")).collect();
    let new = old.replace("line 20\n", "twenty\n");
    let config = DiffViewConfig {
        collapse_unchanged: Some(CollapseOptions::default()),
        inline_below_width: 0.0,
        ..DiffViewConfig::default()
    };
    let mut harness = harness(&old, &new, config);

    click_new_row(&mut harness, 0);

    // The first 17 unchanged lines were one collapsed row: now every line has its own row.
    click_new_row(&mut harness, 3);
    let new = &harness.state().document.new;
    assert_eq!(new.text().line_of_offset(new.selections().primary().head), 3);
}

#[test]
fn typing_in_the_new_side_recomputes_the_diff() {
    let mut harness =
        harness("a\nb\n", "a\nb\n", DiffViewConfig { inline_below_width: 0.0, ..DiffViewConfig::default() });
    assert!(harness.state().document.diff().is_identical());

    click_new_row(&mut harness, 1);
    harness.event(Event::Text("!".into()));
    harness.run();

    assert_eq!(harness.state().document.diff().hunks.len(), 1);
}

#[test]
fn inline_view_lets_removed_lines_be_selected() {
    let config = DiffViewConfig { mode: DiffViewMode::Inline, ..DiffViewConfig::default() };
    let mut harness = harness("a\nold\nb\n", "a\nnew\nb\n", config);

    // Row 1 shows the removed line "old" (a foreign row), row 2 the added "new".
    click_new_row(&mut harness, 1);

    let selection = harness.state().document.selected_lines();
    assert!(selection.old_lines.contains(&1));
}

#[test]
fn old_side_is_read_only_by_default() {
    let mut harness = harness("a\n", "a\n", DiffViewConfig { inline_below_width: 0.0, ..DiffViewConfig::default() });
    let pos = harness.state().document.old_view.geometry().expect("painted").cell_center(0, 0);
    click(&mut harness, pos);

    harness.event(Event::Text("x".into()));
    harness.run();

    assert_eq!(harness.state().document.old.text().to_string(), "a\n");
}

#[test]
fn programmatic_line_selection() {
    let mut harness =
        harness("a\nb\nc\n", "a\nB\nC\n", DiffViewConfig { inline_below_width: 0.0, ..DiffViewConfig::default() });

    select_lines(&mut harness.state_mut().document.new, 1, 2);

    assert_eq!(harness.state().document.selected_lines().new_lines, [1, 2].into());
}

#[test]
fn dragging_the_separator_resizes_the_sides() {
    let mut harness = harness("a\n", "b\n", DiffViewConfig { inline_below_width: 0.0, ..DiffViewConfig::default() });
    let middle = Pos2::new(500.0, 250.0);
    let target = Pos2::new(300.0, 250.0);

    harness.event(Event::PointerMoved(middle));
    harness.event(Event::PointerButton {
        pos: middle,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    harness.event(Event::PointerMoved(target));
    harness.step();
    harness.event(Event::PointerButton {
        pos: target,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();

    let ratio = harness.state().document.dragged_split_ratio.expect("dragged");
    assert!((ratio - 0.3).abs() < 0.02, "ratio {ratio}");
}
