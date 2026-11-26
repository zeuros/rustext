//! Links: Ctrl+click on a URL opens it; a plain click places the cursor.

use egui::{Event, Modifiers, OutputCommand, PointerButton, Pos2, Vec2};
use egui_kittest::Harness;
use rustext_core::EditorState;
use rustext_egui::{CodeEditor, EditorTheme, EditorView, Keymap, ViewConfig};

struct Fixture {
    state: EditorState,
    view: EditorView,
    config: ViewConfig,
    theme: EditorTheme,
    keymap: Keymap,
    opened_urls: Vec<String>,
}

fn harness(content: &str) -> Harness<'static, Fixture> {
    let fixture = Fixture {
        state: EditorState::new(content),
        view: EditorView::default(),
        config: ViewConfig { cursor_blinking: false, ..ViewConfig::default() },
        theme: EditorTheme::dark(),
        keymap: Keymap::vscode(),
        opened_urls: Vec::new(),
    };
    let mut harness = Harness::builder().with_size(Vec2::new(800.0, 200.0)).build_ui_state(
        |ui, fixture: &mut Fixture| {
            CodeEditor::new(&mut fixture.state, &mut fixture.view, &fixture.config, &fixture.theme, &fixture.keymap)
                .show(ui);
            let opened = ui.ctx().output(|output| {
                output
                    .commands
                    .iter()
                    .filter_map(|command| match command {
                        OutputCommand::OpenUrl(open_url) => Some(open_url.url.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            });
            fixture.opened_urls.extend(opened);
        },
        fixture,
    );
    harness.run();
    harness
}

fn click(harness: &mut Harness<'_, Fixture>, pos: Pos2, modifiers: Modifiers) {
    harness.event(Event::ModifiersChanged(modifiers));
    harness.event(Event::PointerMoved(pos));
    for pressed in [true, false] {
        harness.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers });
        harness.step();
    }
    harness.event(Event::ModifiersChanged(Modifiers::NONE));
    harness.run();
}

#[test]
fn ctrl_click_on_a_url_opens_it() {
    let mut harness = harness("docs: https://example.com/guide.\n");
    let on_url = harness.state().view.geometry().unwrap().cell_center(0, 12);

    click(&mut harness, on_url, Modifiers::COMMAND);

    assert_eq!(harness.state().opened_urls, vec!["https://example.com/guide"]);
}

#[test]
fn a_plain_click_on_a_url_places_the_cursor() {
    let mut harness = harness("docs: https://example.com/guide.\n");
    let on_url = harness.state().view.geometry().unwrap().cell_center(0, 12);

    click(&mut harness, on_url, Modifiers::NONE);

    assert!(harness.state().opened_urls.is_empty());
    assert!(harness.state().state.selections().primary().head > 6);
}
