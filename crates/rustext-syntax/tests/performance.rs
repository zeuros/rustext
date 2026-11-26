//! Tree-sitter costs on a large file. Ignored by default; run with
//! `cargo test --release -p rustext-syntax --test performance -- --ignored --nocapture`.

use std::time::Instant;

use rustext_core::style::LineStyler;
use rustext_core::{Change, ChangeSet, EditedLines, Text};
use rustext_syntax::{LanguageRegistry, SyntaxHighlighter, SyntaxTheme};

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

fn milliseconds(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

#[test]
#[ignore = "slow; run in release"]
fn tree_sitter_costs_on_a_large_file() {
    let languages = LanguageRegistry::new();
    let started = Instant::now();
    let mut highlighter =
        SyntaxHighlighter::new(&languages, languages.detect("large.rs", ""), SyntaxTheme::dark_modern());
    println!("highlighter created: {:.1} ms", milliseconds(started));
    let text = Text::new(&large_source(100_000));

    let started = Instant::now();
    let mut calls = 0;
    loop {
        highlighter.style_line(&text, 0);
        calls += 1;
        if !highlighter.is_catching_up() {
            break;
        }
    }
    println!("first parse: {:.1} ms over {calls} calls", milliseconds(started));

    let started = Instant::now();
    for line in 0..50 {
        highlighter.style_line(&text, line);
    }
    println!("50 lines highlighted: {:.2} ms", milliseconds(started));

    let started = Instant::now();
    for line in 99_900..99_950 {
        highlighter.style_line(&text, line);
    }
    println!("50 lines at the end highlighted: {:.2} ms", milliseconds(started));

    let changes = ChangeSet::single(Change::insert(text.line_start(10) + 4, "x"));
    let edited = changes.apply(&text);
    let started = Instant::now();
    highlighter.text_edited(&text, &changes, &EditedLines { first_line: 10, old_last_line: 10, new_last_line: 10 });
    highlighter.style_line(&edited, 0);
    println!("edit and parse again: {:.2} ms", milliseconds(started));
    let started = Instant::now();
    for line in 1..50 {
        highlighter.style_line(&edited, line);
    }
    println!("49 lines highlighted after it: {:.2} ms", milliseconds(started));
}

#[test]
#[ignore = "slow; run in release"]
fn raw_tree_sitter_incremental_parse() {
    let source = large_source(100_000);
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
    let started = Instant::now();
    let mut tree = parser.parse(&source, None).unwrap();
    println!("raw: full parse {:.1} ms", milliseconds(started));

    let line_start = source.match_indices('\n').nth(9).unwrap().0 + 1;
    let at = line_start + 4;
    let mut edited = source.clone();
    edited.insert(at, 'x');
    let point = tree_sitter::Point::new(10, 4);
    tree.edit(&tree_sitter::InputEdit {
        start_byte: at,
        old_end_byte: at,
        new_end_byte: at + 1,
        start_position: point,
        old_end_position: point,
        new_end_position: tree_sitter::Point::new(10, 5),
    });
    let started = Instant::now();
    let mut tree = parser.parse(&edited, Some(&tree)).unwrap();
    println!("raw: incremental parse {:.2} ms", milliseconds(started));
    for round in 0..5 {
        let at = at + 1 + round;
        edited.insert(at, 'y');
        let point = tree_sitter::Point::new(10, 5 + round);
        tree.edit(&tree_sitter::InputEdit {
            start_byte: at,
            old_end_byte: at,
            new_end_byte: at + 1,
            start_position: point,
            old_end_position: point,
            new_end_position: tree_sitter::Point::new(10, 6 + round),
        });
        let started = Instant::now();
        tree = parser.parse(&edited, Some(&tree)).unwrap();
        println!("raw: incremental parse again {:.2} ms", milliseconds(started));
    }

    let text = Text::new(&edited);
    let started = Instant::now();
    let _ = parser
        .parse_with_options(&mut |byte: usize, _: tree_sitter::Point| text.chunk_from_byte(byte), Some(&tree), None)
        .unwrap();
    println!("raw: incremental parse from rope chunks {:.2} ms", milliseconds(started));
}
