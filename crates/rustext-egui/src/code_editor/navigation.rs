//! Going to definitions and references, from the language services' answers, which may arrive
//! frames after the asking: F12 / Ctrl+click jump to the definition; Shift+F12 lists the
//! references under the cursor's line (Up/Down choose, Enter or a click jumps). A target in
//! another document becomes `EditorResponse::open_request`, for the application to open.

use egui::{Id, Key, Ui, pos2};
use rustext_core::services::{Location, Reply};
use rustext_core::{Command, Outcome, Position, Selection, Selections};

use super::list_at_cursor::{ListAtCursor, ListKey, ListPlace, Piece, show_list};
use super::{CodeEditor, Metrics};

/// The references list (Shift+F12): this document's first in text order, then the others'.
pub type ReferencesState = ListAtCursor<Location>;

/// The last part of a document's name: `main.rs` for `file:///src/main.rs`.
fn short_document_name(document: &str) -> &str {
    document.rsplit('/').next().unwrap_or(document)
}

impl CodeEditor<'_> {
    /// F12 / Ctrl+click: asks for the definition of the symbol at `position`; the answer
    /// moves the cursor (or asks to open another document) when it comes.
    pub(super) fn request_definition(&mut self, position: Position) {
        self.view.definition_request = Some((self.state.text_revision(), position));
    }

    /// Goes to the definition once the services answer; an edit meanwhile drops the request.
    /// Returns whether an answer is awaited.
    pub(super) fn refresh_definition(&mut self) -> bool {
        let Some((revision, position)) = self.view.definition_request else { return false };
        let text = self.state.text();
        let services = self.services.as_deref_mut().filter(|_| revision == self.state.text_revision());
        let reply = services.map(|services| services.definition(text, revision, position));
        match reply {
            Some(Reply::Pending) => true,
            Some(Reply::Ready(definition)) => {
                self.view.definition_request = None;
                if let Some(definition) = definition {
                    self.go_to(definition, false);
                }
                false
            }
            None => {
                self.view.definition_request = None;
                false
            }
        }
    }

    /// Moves to `location`: selects its range (`selects`) or puts the cursor at its start in
    /// this document; asks the application to open another one.
    fn go_to(&mut self, location: Location, selects: bool) {
        if location.document.is_some() {
            self.open_request = Some(location);
            return;
        }
        let (start, end) = self.state.text().range_offsets(location.range);
        let selection = if selects { Selection::new(start, end) } else { Selection::cursor(start) };
        self.execute_tracked(Command::SetSelections(Selections::single(selection)));
    }

    /// Shift+F12: asks for the references of the symbol at the cursor.
    pub(super) fn open_references(&mut self) {
        let head = self.state.selections().primary().head;
        self.view.references = ListAtCursor::asked(self.state.text_revision(), head);
    }

    /// Takes the service's answer; closes the list after an edit. Returns whether an answer is
    /// awaited.
    pub(super) fn refresh_references(&mut self) -> bool {
        let revision = self.state.text_revision();
        let Some(offset) = self.view.references.pending_request(revision) else { return false };
        let Some(services) = self.services.as_deref_mut() else {
            self.view.references.close();
            return false;
        };
        let text = self.state.text();
        match services.references(text, revision, text.position(offset)) {
            Reply::Ready(mut entries) => {
                entries.sort_by(|first, second| {
                    (&first.document, first.range.start).cmp(&(&second.document, second.range.start))
                });
                entries.dedup();
                let holds_cursor = |location: &Location| {
                    let (start, end) = text.range_offsets(location.range);
                    location.document.is_none() && start <= offset && offset <= end
                };
                let selected = entries.iter().position(holds_cursor).unwrap_or(0);
                self.view.references.answer(entries, selected, revision);
                false
            }
            Reply::Pending => true,
        }
    }

    /// Up/Down/Enter/Escape while the list is open; returns whether the key was taken.
    pub(super) fn references_key(&mut self, key: Key, outcome: &mut Outcome) -> bool {
        match self.view.references.key(key) {
            ListKey::Ignored => false,
            ListKey::Handled => true,
            ListKey::Picked(index) => {
                self.jump_to_reference(index, outcome);
                true
            }
        }
    }

    fn jump_to_reference(&mut self, index: usize, outcome: &mut Outcome) {
        let Some(location) = self.view.references.take(index) else { return };
        if location.document.is_some() {
            self.open_request = Some(location);
            return;
        }
        let (start, end) = self.state.text().range_offsets(location.range);
        let result = self.state.execute(Command::SetSelections(Selections::single(Selection::new(start, end))));
        *outcome = std::mem::take(outcome).merged(result);
    }

    /// The list under the line it was asked at, one row per reference: `line  text` with the
    /// reference in bold for this document, `name:line` for another.
    pub(super) fn show_references(&mut self, ui: &mut Ui, metrics: &Metrics, editor_id: Id, outcome: &mut Outcome) {
        let references = &self.view.references;
        if !references.is_open() {
            return;
        }
        let text = self.state.text();
        let rows: Vec<Vec<(String, Piece)>> = references
            .entries()
            .iter()
            .map(|location| {
                let start = location.range.start;
                if let Some(document) = &location.document {
                    return vec![(format!("{}:{}", short_document_name(document), start.line + 1), Piece::Dim)];
                }
                let content: Vec<char> = text.line_content(start.line).chars().collect();
                let start_column = start.column.min(content.len());
                let end_column = if location.range.end.line == start.line {
                    location.range.end.column.clamp(start_column, content.len())
                } else {
                    content.len()
                };
                let piece = |range: std::ops::Range<usize>| content[range].iter().collect::<String>();
                vec![
                    (format!("{:>5}  ", start.line + 1), Piece::Dim),
                    (piece(0..start_column).trim_start().to_string(), Piece::Normal),
                    (piece(start_column..end_column), Piece::Strong),
                    (piece(end_column..content.len()), Piece::Normal),
                ]
            })
            .collect();
        let title = format!("{} references", rows.len());
        let top_left = self.below_cursor_line(metrics, references.anchor());
        let width = (metrics.text_rect.width() - 24.0).clamp(200.0, 720.0);
        let selected = references.selected();
        let place = ListPlace { id: editor_id.with("references"), top_left, width, title: &title };
        let clicked = show_list(ui, place, self.theme, &rows, selected);
        if let Some(index) = clicked {
            self.jump_to_reference(index, outcome);
        }
    }

    /// Where a list asked at `offset` shows: under its line, a little right of the text's left.
    pub(super) fn below_cursor_line(&self, metrics: &Metrics, offset: usize) -> egui::Pos2 {
        let position = self.state.text().position(offset);
        let top = self.row_top(metrics, self.row_of_position(position.line, position.column)) + metrics.row_height;
        pos2(metrics.text_rect.left() + 8.0, top)
    }
}
