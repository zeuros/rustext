//! Random command sequences on random texts, checking invariants after every step: selections
//! inside the text, sorted, not overlapping, never inside a `\r\n`; undoing everything gives the
//! original text back, redoing as many steps the final one. Deterministic (seeded) so failures
//! reproduce.

use rustext_core::cursors::VerticalDirection;
use rustext_core::lines::{CaseTransform, LinesRewrite};
use rustext_core::{Command, DeleteUnit, EditorState, LanguageConfiguration, Motion, Selection, Selections, Text};

struct Random(u64);

impl Random {
    fn next(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % bound.max(1) as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.next(items.len())]
    }
}

const PIECES: &[&str] =
    &["fn", " ", "(", ")", "{", "}", "\n", "\r\n", "\t", "é", "👍🏽", "x", "abc", "    ", "\"", "//", "a\nb"];

fn random_text(random: &mut Random) -> String {
    (0..random.next(40)).map(|_| *random.pick(PIECES)).collect()
}

fn random_command(random: &mut Random, text_len: usize) -> Command {
    let motions = [
        Motion::Left,
        Motion::Right,
        Motion::Up,
        Motion::Down,
        Motion::WordLeft,
        Motion::WordRight,
        Motion::WordPartLeft,
        Motion::WordPartRight,
        Motion::LineStart,
        Motion::LineEnd,
        Motion::DocumentStart,
        Motion::DocumentEnd,
        Motion::PageDown { lines: 3 },
        Motion::PageUp { lines: 3 },
    ];
    let units = [DeleteUnit::Char, DeleteUnit::Word, DeleteUnit::WordPart, DeleteUnit::Line];
    let directions = [VerticalDirection::Up, VerticalDirection::Down];
    match random.next(30) {
        0..=3 => Command::Move { motion: *random.pick(&motions), extend: random.next(2) == 0 },
        4..=7 => Command::Type(random.pick(PIECES).to_string()),
        8 => Command::Newline,
        9 => Command::Tab,
        10 => Command::Outdent,
        11 => Command::DeleteBackward(*random.pick(&units)),
        12 => Command::DeleteForward(*random.pick(&units)),
        13 => Command::Undo,
        14 => Command::Redo,
        15 => Command::AddCursor(*random.pick(&directions)),
        16 => Command::AddNextOccurrence,
        17 => Command::MoveLines(*random.pick(&directions)),
        18 => Command::CopyLines(*random.pick(&directions)),
        19 => Command::DeleteLines,
        20 => Command::JoinLines,
        21 => Command::ToggleLineComment,
        22 => Command::ToggleBlockComment,
        23 => {
            Command::TransformCase(*random.pick(&[CaseTransform::Upper, CaseTransform::Snake, CaseTransform::Pascal]))
        }
        24 => Command::RewriteLines(*random.pick(&[
            LinesRewrite::SortAscending,
            LinesRewrite::Reverse,
            LinesRewrite::RemoveDuplicates,
        ])),
        25 => Command::Paste(random.pick(PIECES).repeat(1 + random.next(2))),
        26 => {
            let (anchor, head) = (random.next(text_len + 1), random.next(text_len + 1));
            Command::SetSelections(Selections::single(Selection::new(anchor, head)))
        }
        27 => Command::InsertSnippet { snippet: "f(${1:a}, ${2:b})$0".into(), replaced_before_cursor: 0 },
        28 => Command::Cut,
        _ => Command::SelectAllOccurrences,
    }
}

fn check_selections(state: &EditorState, context: &str) {
    let text = state.text();
    let selections: Vec<Selection> = state.selections().iter().copied().collect();
    assert!(!selections.is_empty(), "{context}: no selection");
    for selection in &selections {
        for offset in [selection.anchor, selection.head] {
            assert!(offset <= text.len_chars(), "{context}: offset {offset} past the end ({})", text.len_chars());
            assert_eq!(text.clamp_offset(offset), offset, "{context}: offset {offset} inside a line ending");
        }
    }
    for pair in selections.windows(2) {
        assert!(pair[0].end() <= pair[1].start(), "{context}: selections overlap or are unsorted: {pair:?}");
    }
}

#[test]
fn random_command_sequences_keep_the_invariants() {
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    for case in 0..400 {
        let original = random_text(&mut random);
        let mut state = EditorState::new(&original).with_language(LanguageConfiguration::c_like());
        let mut commands = Vec::new();
        for step in 0..60 {
            let command = random_command(&mut random, state.text().len_chars());
            commands.push(command.clone());
            state.execute(command);
            check_selections(&state, &format!("case {case}, step {step}, after {:?}", commands.last().unwrap()));
        }

        // Redoing as many steps as undoing took comes back to the final text (redoing more would
        // also redo what the sequence's own Undo commands undid).
        let final_text = state.text().clone();
        let mut undone_steps = 0;
        while state.history().can_undo() {
            state.execute(Command::Undo);
            undone_steps += 1;
        }
        assert_eq!(
            state.text(),
            &Text::new(&original),
            "case {case}: undo all didn't restore the original; commands: {commands:?}"
        );
        for _ in 0..undone_steps {
            state.execute(Command::Redo);
        }
        assert_eq!(
            state.text(),
            &final_text,
            "case {case}: redo all didn't restore the final text; commands: {commands:?}"
        );
    }
}
