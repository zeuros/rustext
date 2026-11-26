//! Drives the `MergeEditor` headlessly through its toolbar.

use egui::Vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use rustext_egui::{EditorTheme, Keymap, MergeDocument, MergeEditor, ViewConfig};

struct Fixture {
    document: MergeDocument,
    view_config: ViewConfig,
    theme: EditorTheme,
    keymap: Keymap,
}

fn harness(base: &str, ours: &str, theirs: &str) -> Harness<'static, Fixture> {
    let fixture = Fixture {
        document: MergeDocument::new(base, ours, theirs),
        view_config: ViewConfig { cursor_blinking: false, ..ViewConfig::default() },
        theme: EditorTheme::dark(),
        keymap: Keymap::vscode(),
    };
    let mut harness = Harness::builder().with_size(Vec2::new(1100.0, 600.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, fixture: &mut Fixture| {
            MergeEditor::new(&mut fixture.document, &fixture.view_config, &fixture.theme, &fixture.keymap).show(ui);
        },
        fixture,
    );
    harness.run();
    harness
}

#[test]
fn toolbar_buttons_resolve_conflicts() {
    let mut harness = harness("a\nx\nb\ny\n", "a\nX1\nb\nY1\n", "a\nX2\nb\nY2\n");
    assert_eq!(harness.state().document.unresolved_conflict_count(), 2);

    harness.get_by_label("Accept theirs").click();
    harness.run();
    assert_eq!(harness.state().document.result_text(), "a\nX2\nb\ny\n");

    harness.get_by_label("Next").click();
    harness.run();
    harness.get_by_label("Ours then theirs").click();
    harness.run();
    assert_eq!(harness.state().document.result_text(), "a\nX2\nb\nY1\nY2\n");
    assert_eq!(harness.state().document.unresolved_conflict_count(), 0);

    harness.get_by_label("All ours").click();
    harness.run();
    assert_eq!(harness.state().document.result_text(), "a\nX1\nb\nY1\n");
}

#[test]
fn the_result_scrolls_with_the_inputs_and_settles() {
    // 60 unchanged lines, then a conflict: the inputs and the result have the same lines up to it.
    let base: String = (0..60).map(|line| format!("line {line}\n")).collect::<String>() + "x\n";
    let ours = base.replace("x\n", "ours\n");
    let theirs = base.replace("x\n", "theirs\n");
    let mut harness = harness(&base, &ours, &theirs);
    let row_height = harness.state().document.result_view.geometry().unwrap().row_height;

    harness.state_mut().document.ours_view.scroll_offset.y = 20.0 * row_height;
    harness.run();

    let result_scroll = harness.state().document.result_view.scroll_offset.y;
    assert!((result_scroll - 20.0 * row_height).abs() < 1.0, "result scrolled to {result_scroll}");

    harness.state_mut().document.result_view.scroll_offset.y = 30.0 * row_height;
    harness.run();
    let inputs_scroll = harness.state().document.theirs_view.scroll_offset.y;
    assert!((inputs_scroll - 30.0 * row_height).abs() < 1.0, "inputs scrolled to {inputs_scroll}");
}
