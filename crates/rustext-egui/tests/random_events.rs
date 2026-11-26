//! Random egui input on random texts and configs: the widget must never panic, and the
//! selections must stay valid. Seeded, so failures reproduce.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Vec2};
use egui_kittest::Harness;
use rustext_core::EditorState;
use rustext_egui::code_editor::{WhitespaceRendering, WordWrap};
use rustext_egui::{CodeEditor, EditorTheme, EditorView, Keymap, ViewConfig};

struct Random(u64);

impl Random {
    fn next(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % bound.max(1) as u64) as usize
    }

    fn unit(&mut self) -> f32 {
        self.next(10_000) as f32 / 10_000.0
    }
}

struct Fixture {
    state: EditorState,
    view: EditorView,
    config: ViewConfig,
    theme: EditorTheme,
    keymap: Keymap,
}

const PIECES: &[&str] = &[
    "fn a() {\n",
    "    x();\n",
    "}\n",
    "\t",
    "é",
    "ß",
    "ä",
    "\r\n",
    "long line with words that wrap around ",
    "\n",
    "(",
    ")",
];
const KEYS: &[Key] = &[
    Key::ArrowLeft,
    Key::ArrowRight,
    Key::ArrowUp,
    Key::ArrowDown,
    Key::Home,
    Key::End,
    Key::PageUp,
    Key::PageDown,
    Key::Backspace,
    Key::Delete,
    Key::Enter,
    Key::Tab,
    Key::Escape,
    Key::Z,
    Key::D,
    Key::F,
    Key::K,
    Key::Num0,
    Key::J,
    Key::Space,
    Key::F2,
    Key::F3,
    Key::F7,
    Key::F8,
    Key::OpenBracket,
    Key::CloseBracket,
    Key::Slash,
];

fn random_config(random: &mut Random) -> ViewConfig {
    ViewConfig {
        cursor_blinking: false,
        word_wrap: [WordWrap::Off, WordWrap::Column(17), WordWrap::Viewport][random.next(3)],
        minimap: random.next(2) == 0,
        folding: random.next(4) != 0,
        sticky_scroll: random.next(2) == 0,
        render_whitespace: [WhitespaceRendering::None, WhitespaceRendering::All, WhitespaceRendering::Boundary]
            [random.next(3)],
        show_carriage_returns: random.next(2) == 0,
        ..ViewConfig::default()
    }
}

#[test]
fn random_input_never_panics_and_keeps_selections_valid() {
    let mut random = Random(0xfeed_face_dead_beef);
    for case in 0..60 {
        let content: String = (0..random.next(30)).map(|_| PIECES[random.next(PIECES.len())]).collect();
        let fixture = Fixture {
            state: EditorState::new(&content),
            view: EditorView::default(),
            config: random_config(&mut random),
            theme: EditorTheme::dark(),
            keymap: Keymap::vscode(),
        };
        let mut harness =
            Harness::builder().with_size(Vec2::new(500.0, 300.0)).with_step_dt(1.0 / 60.0).build_ui_state(
                |ui, fixture: &mut Fixture| {
                    CodeEditor::new(
                        &mut fixture.state,
                        &mut fixture.view,
                        &fixture.config,
                        &fixture.theme,
                        &fixture.keymap,
                    )
                    .show(ui);
                },
                fixture,
            );
        harness.step();
        for _ in 0..80 {
            let pos = Pos2::new(random.unit() * 500.0, random.unit() * 300.0);
            let modifiers = [
                Modifiers::NONE,
                Modifiers::SHIFT,
                Modifiers::COMMAND,
                Modifiers::ALT,
                Modifiers::COMMAND | Modifiers::SHIFT,
            ][random.next(5)];
            match random.next(6) {
                0 => {
                    harness.event(Event::PointerMoved(pos));
                    harness.event(Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers,
                    });
                    harness.step();
                    let to = Pos2::new(random.unit() * 500.0, random.unit() * 300.0);
                    harness.event(Event::PointerMoved(to));
                    harness.event(Event::PointerButton {
                        pos: to,
                        button: PointerButton::Primary,
                        pressed: false,
                        modifiers,
                    });
                }
                1 => harness.event(Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(0.0, (random.unit() - 0.5) * 400.0),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers: Modifiers::NONE,
                }),
                2 => harness.event(Event::Text(PIECES[random.next(PIECES.len())].replace('\n', ""))),
                3 => harness.event(Event::Paste(PIECES[random.next(PIECES.len())].to_string())),
                _ => harness.event(Event::Key {
                    key: KEYS[random.next(KEYS.len())],
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                }),
            }
            harness.event(Event::PointerMoved(pos));
            harness.step();

            let state = &harness.state().state;
            let text = state.text();
            for selection in state.selections().iter() {
                for offset in [selection.anchor, selection.head] {
                    assert!(offset <= text.len_chars(), "case {case}: selection past the end");
                    assert_eq!(text.clamp_offset(offset), offset, "case {case}: selection inside a line ending");
                }
            }
        }
    }
}
