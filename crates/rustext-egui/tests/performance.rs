//! Frame times on a large file. Ignored by default (slow in debug builds); run with
//! `cargo test --release -p rustext-egui --test performance -- --ignored --nocapture`.

use std::time::Instant;

use egui::{Event, Key, Modifiers, PointerButton, Vec2};
use egui_kittest::Harness;
use rustext_core::EditorState;
use rustext_egui::{CodeEditor, EditorTheme, EditorView, Keymap, ViewConfig};

struct Fixture {
    state: EditorState,
    view: EditorView,
    config: ViewConfig,
    theme: EditorTheme,
    keymap: Keymap,
}

fn large_source(line_count: usize) -> String {
    (0..line_count)
        .map(|line| match line % 5 {
            0 => format!("fn function_{line}(argument: usize) -> usize {{\n"),
            1 | 2 => format!("    let value_{line} = argument * {line} + (argument >> 2);\n"),
            3 => "    value\n".to_string(),
            _ => "}\n".to_string(),
        })
        .collect()
}

fn milliseconds_per_frame(
    harness: &mut Harness<'_, Fixture>,
    frames: usize,
    mut before_frame: impl FnMut(&mut Harness<'_, Fixture>),
) -> f64 {
    let started = Instant::now();
    for _ in 0..frames {
        before_frame(harness);
        harness.step();
    }
    started.elapsed().as_secs_f64() * 1000.0 / frames as f64
}

#[test]
#[ignore = "slow; run in release"]
fn large_file_frame_times() {
    let fixture = Fixture {
        state: EditorState::new(&large_source(100_000)),
        view: EditorView::default(),
        config: ViewConfig {
            cursor_blinking: false,
            folding: std::env::var("NO_FOLDING").is_err(),
            indent_guides: std::env::var("NO_GUIDES").is_err(),
            ..ViewConfig::default()
        },
        theme: EditorTheme::dark(),
        keymap: Keymap::vscode(),
    };
    let started = Instant::now();
    let mut harness = Harness::builder().with_size(Vec2::new(1200.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, fixture: &mut Fixture| {
            CodeEditor::new(&mut fixture.state, &mut fixture.view, &fixture.config, &fixture.theme, &fixture.keymap)
                .show(ui);
        },
        fixture,
    );
    harness.step();
    println!("first frame: {:.1} ms", started.elapsed().as_secs_f64() * 1000.0);

    let pos = harness.state().view.geometry().unwrap().cell_center(3, 4);
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
    harness.step();

    let idle = milliseconds_per_frame(&mut harness, 30, |_| {});
    println!("idle frame: {idle:.2} ms");
    let typing = milliseconds_per_frame(&mut harness, 30, |harness| harness.event(Event::Text("x".into())));
    println!("typing frame: {typing:.2} ms");
    let scrolling = milliseconds_per_frame(&mut harness, 30, |harness| {
        harness.event(Event::Key {
            key: Key::PageDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
    });
    println!("page-down frame: {scrolling:.2} ms");
    let to_end = milliseconds_per_frame(&mut harness, 2, |harness| {
        harness.event(Event::Key {
            key: Key::End,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        });
    });
    println!("ctrl+end frame: {to_end:.2} ms");
    let typing_at_end = milliseconds_per_frame(&mut harness, 30, |harness| harness.event(Event::Text("w".into())));
    println!("typing frame at the end: {typing_at_end:.2} ms");

    let key = |key: Key, modifiers: Modifiers| Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    let fold_all = milliseconds_per_frame(&mut harness, 1, |harness| {
        harness.event(key(Key::K, Modifiers::COMMAND));
        harness.event(key(Key::Num0, Modifiers::COMMAND));
    });
    println!("fold all frame: {fold_all:.2} ms ({} folds)", harness.state().view.folds.folded_header_count());
    let typing_folded = milliseconds_per_frame(&mut harness, 30, |harness| harness.event(Event::Text("y".into())));
    println!("typing frame, all folded: {typing_folded:.2} ms");

    harness.state_mut().view.find.is_open = true;
    harness.state_mut().view.find.query.pattern = "value".into();
    harness.step();
    let typing_with_find = milliseconds_per_frame(&mut harness, 30, |harness| harness.event(Event::Text("z".into())));
    println!(
        "typing frame, find open ({} matches): {typing_with_find:.2} ms",
        harness.state().view.find.matches().len()
    );
}

struct HighlightedFixture {
    editor: Fixture,
    highlighter: rustext_syntax::SyntaxHighlighter,
}

#[test]
#[ignore = "slow; run in release"]
fn large_file_frame_times_highlighted() {
    let languages = rustext_syntax::LanguageRegistry::new();
    let highlighter = rustext_syntax::SyntaxHighlighter::new(
        &languages,
        languages.detect("large.rs", ""),
        rustext_syntax::SyntaxTheme::dark_modern(),
    );
    assert!(highlighter.uses_tree_sitter());
    let fixture = HighlightedFixture {
        editor: Fixture {
            state: EditorState::new(&large_source(100_000)),
            view: EditorView::default(),
            config: ViewConfig { cursor_blinking: false, ..ViewConfig::default() },
            theme: EditorTheme::dark(),
            keymap: Keymap::vscode(),
        },
        highlighter,
    };
    let started = Instant::now();
    let mut harness = Harness::builder().with_size(Vec2::new(1200.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, fixture: &mut HighlightedFixture| {
            let editor = &mut fixture.editor;
            CodeEditor::new(&mut editor.state, &mut editor.view, &editor.config, &editor.theme, &editor.keymap)
                .styler(&mut fixture.highlighter)
                .show(ui);
        },
        fixture,
    );
    let mut frames_until_parsed = 0;
    loop {
        harness.step();
        frames_until_parsed += 1;
        if !rustext_core::style::LineStyler::is_catching_up(&harness.state().highlighter) {
            break;
        }
    }
    let parse_time = started.elapsed().as_secs_f64() * 1000.0;
    println!("highlighted: parsed in {frames_until_parsed} frames, {parse_time:.1} ms in all");
    let pos = harness.state().editor.view.geometry().unwrap().cell_center(3, 4);
    for pressed in [true, false] {
        harness.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.step();
    }

    let typing = milliseconds_per_frame_highlighted(&mut harness, 30, |harness| {
        harness.event(Event::Text("x".into()));
    });
    println!("highlighted: typing frame: {typing:.2} ms");
    let to_end = milliseconds_per_frame_highlighted(&mut harness, 2, |harness| {
        harness.event(Event::Key {
            key: Key::End,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        });
    });
    println!("highlighted: ctrl+end frame: {to_end:.2} ms");
    let typing_at_end = milliseconds_per_frame_highlighted(&mut harness, 30, |harness| {
        harness.event(Event::Text("y".into()));
    });
    println!("highlighted: typing frame at the end: {typing_at_end:.2} ms");
    assert!(harness.state().editor.state.text().to_string().ends_with("y".repeat(30).as_str()));
}

fn milliseconds_per_frame_highlighted(
    harness: &mut Harness<'_, HighlightedFixture>,
    frames: usize,
    mut before_frame: impl FnMut(&mut Harness<'_, HighlightedFixture>),
) -> f64 {
    let started = Instant::now();
    for _ in 0..frames {
        before_frame(harness);
        harness.step();
    }
    started.elapsed().as_secs_f64() * 1000.0 / frames as f64
}
