//! The commands of the editor state, and their names.

use crate::cursors::VerticalDirection;
use crate::edit::{DeleteUnit, Transaction};
use crate::lines::{CaseTransform, LinesRewrite};
use crate::motion::Motion;
use crate::position::Position;
use crate::selection::Selections;

/// Everything the editor can do to its state. Commands that need the viewport (page moves)
/// carry what they need.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Move {
        motion: Motion,
        extend: bool,
    },
    SetSelections(Selections),
    SelectAll,
    /// Ctrl+L.
    ExpandToLines,
    /// Escape.
    CollapseSelections,
    AddCursor(VerticalDirection),
    /// Ctrl+D.
    AddNextOccurrence,
    /// Ctrl+Shift+L.
    SelectAllOccurrences,
    /// Shift+Alt+I.
    CursorsAtLineEnds,
    /// Shift+Alt+drag.
    ColumnSelect {
        anchor: Position,
        head_line: usize,
        head_visual_column: usize,
    },
    /// Ctrl+U: back to the selections before the last cursor move.
    CursorUndo,
    CursorRedo,

    Type(String),
    Newline,
    Tab,
    Indent,
    Outdent,
    DeleteBackward(DeleteUnit),
    DeleteForward(DeleteUnit),
    Cut,
    Copy,
    Paste(String),
    Undo,
    Redo,

    MoveLines(VerticalDirection),
    CopyLines(VerticalDirection),
    DeleteLines,
    DuplicateSelection,
    JoinLines,
    RewriteLines(LinesRewrite),
    TrimTrailingWhitespace,
    InsertLine(VerticalDirection),
    TransformCase(CaseTransform),
    ToggleLineComment,
    ToggleBlockComment,
    /// Swaps the chars around each cursor (VS Code's `transposeLetters`).
    TransposeLetters,
    /// Applies a transaction computed outside (find / replace, formatting, an application edit).
    Apply(Transaction),
    /// Inserts a snippet (`${1:name}`…) in place of the primary selection and the
    /// `replaced_before_cursor` chars before it (a completion's prefix), then selects its first
    /// tab stop; Tab and Shift+Tab go through the others.
    InsertSnippet {
        snippet: String,
        replaced_before_cursor: usize,
    },
}

impl Command {
    /// Whether the command changes the text (rejected when read-only).
    pub fn edits_text(&self) -> bool {
        !matches!(
            self,
            Command::Move { .. }
                | Command::SetSelections(_)
                | Command::SelectAll
                | Command::ExpandToLines
                | Command::CollapseSelections
                | Command::AddCursor(_)
                | Command::AddNextOccurrence
                | Command::SelectAllOccurrences
                | Command::CursorsAtLineEnds
                | Command::ColumnSelect { .. }
                | Command::CursorUndo
                | Command::CursorRedo
                | Command::Copy
        )
    }

    /// A stable name for keymaps in settings files and the command palette.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Move { .. } => "cursor.move",
            Command::SetSelections(_) => "cursor.setSelections",
            Command::SelectAll => "selection.selectAll",
            Command::ExpandToLines => "selection.expandToLines",
            Command::CollapseSelections => "selection.collapse",
            Command::AddCursor(VerticalDirection::Up) => "cursor.addAbove",
            Command::AddCursor(VerticalDirection::Down) => "cursor.addBelow",
            Command::AddNextOccurrence => "selection.addNextOccurrence",
            Command::SelectAllOccurrences => "selection.selectAllOccurrences",
            Command::CursorsAtLineEnds => "cursor.atLineEnds",
            Command::ColumnSelect { .. } => "selection.column",
            Command::CursorUndo => "cursor.undo",
            Command::CursorRedo => "cursor.redo",
            Command::Type(_) => "edit.type",
            Command::Newline => "edit.newline",
            Command::Tab => "edit.tab",
            Command::Indent => "edit.indent",
            Command::Outdent => "edit.outdent",
            Command::DeleteBackward(_) => "edit.deleteBackward",
            Command::DeleteForward(_) => "edit.deleteForward",
            Command::Cut => "clipboard.cut",
            Command::Copy => "clipboard.copy",
            Command::Paste(_) => "clipboard.paste",
            Command::Undo => "history.undo",
            Command::Redo => "history.redo",
            Command::MoveLines(VerticalDirection::Up) => "lines.moveUp",
            Command::MoveLines(VerticalDirection::Down) => "lines.moveDown",
            Command::CopyLines(VerticalDirection::Up) => "lines.copyUp",
            Command::CopyLines(VerticalDirection::Down) => "lines.copyDown",
            Command::DeleteLines => "lines.delete",
            Command::DuplicateSelection => "selection.duplicate",
            Command::JoinLines => "lines.join",
            Command::RewriteLines(LinesRewrite::SortAscending) => "lines.sortAscending",
            Command::RewriteLines(LinesRewrite::SortDescending) => "lines.sortDescending",
            Command::RewriteLines(LinesRewrite::Reverse) => "lines.reverse",
            Command::RewriteLines(LinesRewrite::RemoveDuplicates) => "lines.removeDuplicates",
            Command::TrimTrailingWhitespace => "lines.trimTrailingWhitespace",
            Command::InsertLine(VerticalDirection::Up) => "lines.insertAbove",
            Command::InsertLine(VerticalDirection::Down) => "lines.insertBelow",
            Command::TransformCase(CaseTransform::Upper) => "case.upper",
            Command::TransformCase(CaseTransform::Lower) => "case.lower",
            Command::TransformCase(CaseTransform::Title) => "case.title",
            Command::TransformCase(CaseTransform::Snake) => "case.snake",
            Command::TransformCase(CaseTransform::Camel) => "case.camel",
            Command::TransformCase(CaseTransform::Pascal) => "case.pascal",
            Command::TransformCase(CaseTransform::Kebab) => "case.kebab",
            Command::ToggleLineComment => "comment.line",
            Command::ToggleBlockComment => "comment.block",
            Command::TransposeLetters => "edit.transposeLetters",
            Command::Apply(_) => "edit.apply",
            Command::InsertSnippet { .. } => "edit.insertSnippet",
        }
    }
}
