//! Keybindings as a value (DECISIONS D8): a `Keymap` maps key chords, or sequences of them
//! (Ctrl+K Ctrl+C), to actions. Applications extend or replace it.

use egui::{Key, Modifiers};
use rustext_core::cursors::VerticalDirection;
use rustext_core::lines::{CaseTransform, LinesRewrite};
use rustext_core::{Command, DeleteUnit, Motion};

/// What a key does: a core command, or something only the widget can do (it needs the
/// viewport, or opens a widget).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Edit(Command),
    /// Page Up / Down: the widget knows how many lines a page is.
    MovePage {
        direction: VerticalDirection,
        extend: bool,
    },
    /// Scrolls one line without moving the cursor (Ctrl+Up / Down).
    ScrollLine(VerticalDirection),
    OpenFind,
    OpenReplace,
    FindNext,
    FindPrevious,
    /// Selects every find match as cursors (Alt+Enter in the find widget).
    SelectAllMatches,
    GoToLine,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Fold,
    Unfold,
    FoldAll,
    UnfoldAll,
    JumpToBracket,
    /// Diff editors: the next / previous change (F7 / Shift+F7).
    NextChange,
    PreviousChange,
    /// The next / previous diagnostic squiggle (F8 / Shift+F8).
    NextProblem,
    PreviousProblem,
    /// Opens the completion list (Ctrl+Space).
    TriggerSuggest,
    /// Moves to the definition of the symbol at the cursor (F12).
    GoToDefinition,
    /// Lists every command to run one (F1, Ctrl+Shift+P).
    CommandPalette,
    /// Lists the document's symbols to go to one (Ctrl+Shift+O).
    GoToSymbol,
    /// Formats the document through the language services (Shift+Alt+F).
    FormatDocument,
    /// Formats the lines of the selection through the language services (Ctrl+K Ctrl+F).
    FormatSelection,
    /// Lists the quick fixes and refactorings at the selection (Ctrl+.).
    CodeActions,
    /// Diff editors: reads the change at the cursor line by line, for screen readers (Alt+F2).
    AccessibleDiffViewer,
    /// Renames the symbol at the cursor through the language services (F2).
    Rename,
    /// Lists the references of the symbol at the cursor (Shift+F12).
    FindReferences,
    /// Grows each selection to the syntax node holding it (Shift+Alt+Right).
    GrowSelection,
    /// Goes back to the selections the last growths started from (Shift+Alt+Left).
    ShrinkSelection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl KeyChord {
    pub const fn new(modifiers: Modifiers, key: Key) -> Self {
        Self { modifiers, key }
    }

    pub fn matches(&self, modifiers: Modifiers, key: Key) -> bool {
        self.key == key && modifiers.matches_exact(self.modifiers)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub chords: Vec<KeyChord>,
    pub action: Action,
}

/// Result of feeding a key to the keymap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyResolution {
    Action(Action),
    /// The key starts a chord sequence: wait for the next key.
    Pending,
    Unbound,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keymap {
    pub bindings: Vec<Binding>,
}

impl Keymap {
    /// Later bindings take precedence over earlier ones with the same chords.
    pub fn bind(mut self, chords: &[KeyChord], action: Action) -> Self {
        self.bindings.push(Binding { chords: chords.to_vec(), action });
        self
    }

    /// Resolves `pending` (keys of an unfinished sequence) followed by this key.
    pub fn resolve(&self, pending: &[KeyChord], modifiers: Modifiers, key: Key) -> KeyResolution {
        let matches_prefix = |binding: &Binding| {
            binding.chords.len() > pending.len()
                && binding.chords[..pending.len()] == *pending
                && binding.chords[pending.len()].matches(modifiers, key)
        };
        let mut is_pending = false;
        for binding in self.bindings.iter().rev().filter(|binding| matches_prefix(binding)) {
            if binding.chords.len() == pending.len() + 1 {
                return KeyResolution::Action(binding.action.clone());
            }
            is_pending = true;
        }
        if is_pending { KeyResolution::Pending } else { KeyResolution::Unbound }
    }

    /// The chord a key press makes, to remember while a sequence is pending.
    pub fn chord(modifiers: Modifiers, key: Key) -> KeyChord {
        KeyChord::new(
            Modifiers { alt: modifiers.alt, shift: modifiers.shift, command: modifiers.command, ..Modifiers::NONE },
            key,
        )
    }

    /// VS Code's default keybindings (Linux / Windows flavour, Cmd on macOS).
    #[expect(clippy::too_many_lines, reason = "predates the complexity lints (D28)")]
    pub fn vscode() -> Self {
        let none = Modifiers::NONE;
        let shift = Modifiers::SHIFT;
        let alt = Modifiers::ALT;
        let command = Modifiers::COMMAND;
        let command_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let command_alt = Modifiers::COMMAND | Modifiers::ALT;
        let shift_alt = Modifiers::SHIFT | Modifiers::ALT;
        let chord = KeyChord::new;
        let edit = Action::Edit;
        let mut keymap = Keymap::default();

        let moves = [
            (Key::ArrowLeft, none, Motion::Left),
            (Key::ArrowRight, none, Motion::Right),
            (Key::ArrowUp, none, Motion::Up),
            (Key::ArrowDown, none, Motion::Down),
            (Key::ArrowLeft, command, Motion::WordLeft),
            (Key::ArrowRight, command, Motion::WordRight),
            (Key::ArrowLeft, alt, Motion::WordPartLeft),
            (Key::ArrowRight, alt, Motion::WordPartRight),
            (Key::Home, none, Motion::LineStart),
            (Key::End, none, Motion::LineEnd),
            (Key::Home, command, Motion::DocumentStart),
            (Key::End, command, Motion::DocumentEnd),
        ];
        for (key, modifiers, motion) in moves {
            keymap = keymap
                .bind(&[chord(modifiers, key)], edit(Command::Move { motion, extend: false }))
                .bind(&[chord(modifiers | shift, key)], edit(Command::Move { motion, extend: true }));
        }
        for (key, direction) in [(Key::PageUp, VerticalDirection::Up), (Key::PageDown, VerticalDirection::Down)] {
            keymap = keymap
                .bind(&[chord(none, key)], Action::MovePage { direction, extend: false })
                .bind(&[chord(shift, key)], Action::MovePage { direction, extend: true });
        }
        for (key, direction) in [(Key::ArrowUp, VerticalDirection::Up), (Key::ArrowDown, VerticalDirection::Down)] {
            keymap = keymap
                .bind(&[chord(alt, key)], edit(Command::MoveLines(direction)))
                .bind(&[chord(shift_alt, key)], edit(Command::CopyLines(direction)))
                .bind(&[chord(command_alt, key)], edit(Command::AddCursor(direction)))
                .bind(&[chord(command_shift, key)], edit(Command::AddCursor(direction)))
                .bind(&[chord(command, key)], Action::ScrollLine(direction));
        }

        let deletes = [
            (Key::Backspace, none, Command::DeleteBackward(DeleteUnit::Char)),
            (Key::Backspace, shift, Command::DeleteBackward(DeleteUnit::Char)),
            (Key::Backspace, command, Command::DeleteBackward(DeleteUnit::Word)),
            (Key::Backspace, alt, Command::DeleteBackward(DeleteUnit::WordPart)),
            (Key::Backspace, command_shift, Command::DeleteBackward(DeleteUnit::Line)),
            (Key::Delete, none, Command::DeleteForward(DeleteUnit::Char)),
            (Key::Delete, command, Command::DeleteForward(DeleteUnit::Word)),
            (Key::Delete, alt, Command::DeleteForward(DeleteUnit::WordPart)),
            (Key::Delete, command_shift, Command::DeleteForward(DeleteUnit::Line)),
        ];
        for (key, modifiers, command) in deletes {
            keymap = keymap.bind(&[chord(modifiers, key)], edit(command));
        }

        let commands = [
            (chord(none, Key::Enter), Command::Newline),
            (chord(shift, Key::Enter), Command::Newline),
            (chord(command, Key::Enter), Command::InsertLine(VerticalDirection::Down)),
            (chord(command_shift, Key::Enter), Command::InsertLine(VerticalDirection::Up)),
            (chord(none, Key::Tab), Command::Tab),
            (chord(shift, Key::Tab), Command::Outdent),
            (chord(command, Key::CloseBracket), Command::Indent),
            (chord(command, Key::OpenBracket), Command::Outdent),
            (chord(command, Key::A), Command::SelectAll),
            (chord(command, Key::L), Command::ExpandToLines),
            (chord(none, Key::Escape), Command::CollapseSelections),
            (chord(command, Key::D), Command::AddNextOccurrence),
            (chord(command_shift, Key::L), Command::SelectAllOccurrences),
            (chord(shift_alt, Key::I), Command::CursorsAtLineEnds),
            (chord(command_shift, Key::K), Command::DeleteLines),
            (chord(command, Key::J), Command::JoinLines),
            (chord(command, Key::Slash), Command::ToggleLineComment),
            (chord(shift_alt, Key::A), Command::ToggleBlockComment),
            (chord(command, Key::Z), Command::Undo),
            (chord(command_shift, Key::Z), Command::Redo),
            (chord(command, Key::Y), Command::Redo),
        ];
        for (key_chord, command) in commands {
            keymap = keymap.bind(&[key_chord], edit(command));
        }

        let command_k = chord(command, Key::K);
        let sequences = [
            (Key::X, edit(Command::TrimTrailingWhitespace)),
            (Key::U, edit(Command::TransformCase(CaseTransform::Upper))),
            (Key::L, edit(Command::TransformCase(CaseTransform::Lower))),
            (Key::Num0, Action::FoldAll),
            (Key::J, Action::UnfoldAll),
            (Key::F, Action::FormatSelection),
        ];
        for (key, action) in sequences {
            keymap = keymap.bind(&[command_k, chord(command, key)], action);
        }
        keymap =
            keymap.bind(&[command_k, chord(none, Key::S)], edit(Command::RewriteLines(LinesRewrite::SortAscending)));

        let widget_actions = [
            (chord(command, Key::F), Action::OpenFind),
            (chord(command, Key::H), Action::OpenReplace),
            (chord(none, Key::F3), Action::FindNext),
            (chord(shift, Key::F3), Action::FindPrevious),
            (chord(command, Key::G), Action::GoToLine),
            (chord(command, Key::Equals), Action::ZoomIn),
            (chord(command, Key::Plus), Action::ZoomIn),
            (chord(command, Key::Minus), Action::ZoomOut),
            (chord(command, Key::Num0), Action::ZoomReset),
            (chord(command_shift, Key::OpenBracket), Action::Fold),
            (chord(command_shift, Key::CloseBracket), Action::Unfold),
            (chord(command_shift, Key::Backslash), Action::JumpToBracket),
            (chord(none, Key::F7), Action::NextChange),
            (chord(none, Key::F8), Action::NextProblem),
            (chord(command, Key::Space), Action::TriggerSuggest),
            (chord(none, Key::F12), Action::GoToDefinition),
            (chord(none, Key::F1), Action::CommandPalette),
            (chord(none, Key::F2), Action::Rename),
            (chord(shift, Key::F12), Action::FindReferences),
            (chord(shift_alt, Key::ArrowRight), Action::GrowSelection),
            (chord(shift_alt, Key::ArrowLeft), Action::ShrinkSelection),
            (chord(command_shift, Key::P), Action::CommandPalette),
            (chord(command_shift, Key::O), Action::GoToSymbol),
            (chord(shift_alt, Key::F), Action::FormatDocument),
            (chord(command, Key::Period), Action::CodeActions),
            (chord(alt, Key::F2), Action::AccessibleDiffViewer),
            (chord(shift, Key::F8), Action::PreviousProblem),
            (chord(shift, Key::F7), Action::PreviousChange),
        ];
        for (key_chord, action) in widget_actions {
            keymap = keymap.bind(&[key_chord], action);
        }
        keymap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_single_chords_and_sequences() {
        let keymap = Keymap::vscode();
        let command_k = KeyChord::new(Modifiers::COMMAND, Key::K);

        assert_eq!(
            keymap.resolve(&[], Modifiers::COMMAND, Key::D),
            KeyResolution::Action(Action::Edit(Command::AddNextOccurrence))
        );
        assert_eq!(keymap.resolve(&[], Modifiers::COMMAND, Key::K), KeyResolution::Pending);
        assert_eq!(keymap.resolve(&[command_k], Modifiers::COMMAND, Key::Num0), KeyResolution::Action(Action::FoldAll));
        assert_eq!(keymap.resolve(&[command_k], Modifiers::NONE, Key::Q), KeyResolution::Unbound);
    }

    #[test]
    fn shift_selects_while_moving() {
        let resolution = Keymap::vscode().resolve(&[], Modifiers::SHIFT, Key::End);

        assert_eq!(
            resolution,
            KeyResolution::Action(Action::Edit(Command::Move { motion: Motion::LineEnd, extend: true }))
        );
    }

    #[test]
    fn later_bindings_override_earlier_ones() {
        let keymap = Keymap::vscode().bind(&[KeyChord::new(Modifiers::COMMAND, Key::D)], Action::GoToLine);

        assert_eq!(keymap.resolve(&[], Modifiers::COMMAND, Key::D), KeyResolution::Action(Action::GoToLine));
    }
}
