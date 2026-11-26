//! Going to definitions and references, from the language services' answers, which may arrive
//! frames after the asking: F12 / Ctrl+click jump to the definition; Shift+F12 lists the
//! references under the cursor's line (Up/Down choose, Enter or a click jumps). A target in
//! another document becomes `EditorResponse::open_request`, for the application to open.

use egui::{Area, Frame, Id, Key, Order, RichText, Ui, pos2};
use rustext_core::services::{Location, Reply};
use rustext_core::{Command, Outcome, Position, Selection, Selections};

use super::{CodeEditor, Metrics};

const VISIBLE_ITEMS: usize = 10;

#[derive(Clone, Debug, Default)]
pub struct ReferencesState {
    /// The references, this document's first in text order, then the others'; empty when the
    /// list is closed.
    entries: Vec<Location>,
    selected: usize,
    /// The text revision the entries are for (an edit closes the list).
    revision: u64,
    /// The cursor offset the list was asked at, where it shows.
    anchor: usize,
    /// Waiting for the service's answer: (text revision, cursor offset).
    request: Option<(u64, usize)>,
}

impl ReferencesState {
    pub fn is_open(&self) -> bool {
        !self.entries.is_empty()
    }

    pub fn entries(&self) -> &[Location] {
        &self.entries
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    fn close(&mut self) {
        *self = ReferencesState::default();
    }
}

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
        self.view.references =
            ReferencesState { anchor: head, request: Some((self.state.text_revision(), head)), ..Default::default() };
    }

    /// Takes the service's answer; closes the list after an edit. Returns whether an answer is
    /// awaited.
    pub(super) fn refresh_references(&mut self) -> bool {
        let revision = self.state.text_revision();
        let references = &mut self.view.references;
        if references.is_open() && references.revision != revision {
            references.close();
        }
        let Some((request_revision, offset)) = references.request else { return false };
        let Some(services) = self.services.as_deref_mut().filter(|_| request_revision == revision) else {
            references.close();
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
                *references = ReferencesState { entries, selected, revision, anchor: offset, request: None };
                false
            }
            Reply::Pending => true,
        }
    }

    /// Up/Down/Enter/Escape while the list is open; returns whether the key was taken.
    pub(super) fn references_key(&mut self, key: Key, outcome: &mut Outcome) -> bool {
        let references = &mut self.view.references;
        if !references.is_open() {
            return false;
        }
        let last_index = references.entries.len() - 1;
        match key {
            Key::ArrowDown => {
                references.selected = if references.selected == last_index { 0 } else { references.selected + 1 }
            }
            Key::ArrowUp => {
                references.selected = if references.selected == 0 { last_index } else { references.selected - 1 }
            }
            Key::Enter => {
                let selected = references.selected;
                self.jump_to_reference(selected, outcome);
            }
            Key::Escape => references.close(),
            _ => return false,
        }
        true
    }

    fn jump_to_reference(&mut self, index: usize, outcome: &mut Outcome) {
        let Some(location) = self.view.references.entries.get(index).cloned() else { return };
        self.view.references.close();
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
        if !self.view.references.is_open() {
            return;
        }
        let text = self.state.text();
        let anchor = text.position(self.view.references.anchor);
        let top = self.row_top(metrics, self.row_of_position(anchor.line, anchor.column)) + metrics.row_height;
        let theme = self.theme.clone();
        let references = &self.view.references;
        let first_visible = references.selected.saturating_sub(VISIBLE_ITEMS - 1);
        let rows: Vec<(usize, String, String, String, String)> = references
            .entries
            .iter()
            .enumerate()
            .skip(first_visible)
            .take(VISIBLE_ITEMS)
            .map(|(index, location)| {
                let start = location.range.start;
                if let Some(document) = &location.document {
                    let place = format!("{}:{}", short_document_name(document), start.line + 1);
                    return (index, format!("{place}  "), String::new(), String::new(), String::new());
                }
                let content: Vec<char> = text.line_content(start.line).chars().collect();
                let start_column = start.column.min(content.len());
                let end_column = if location.range.end.line == start.line {
                    location.range.end.column.clamp(start_column, content.len())
                } else {
                    content.len()
                };
                let piece = |range: std::ops::Range<usize>| content[range].iter().collect::<String>();
                let before = piece(0..start_column).trim_start().to_string();
                (
                    index,
                    format!("{:>5}  ", start.line + 1),
                    before,
                    piece(start_column..end_column),
                    piece(end_column..content.len()),
                )
            })
            .collect();
        let (selected, count) = (references.selected, references.entries.len());
        let mut clicked = None;
        Area::new(editor_id.with("references"))
            .order(Order::Foreground)
            .fade_in(false)
            .fixed_pos(pos2(metrics.text_rect.left() + 8.0, top))
            .show(ui.ctx(), |ui| {
                Frame::popup(ui.style()).fill(theme.widget_background).stroke((1.0, theme.widget_border)).show(
                    ui,
                    |ui| {
                        ui.set_width((metrics.text_rect.width() - 24.0).clamp(200.0, 720.0));
                        ui.label(RichText::new(format!("{count} references")).size(11.0).color(theme.line_number));
                        for (index, place, before, reference, after) in rows {
                            let row = ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.label(RichText::new(place).monospace().color(theme.line_number));
                                ui.label(RichText::new(before).monospace().color(theme.foreground));
                                ui.label(RichText::new(reference).monospace().strong().color(theme.foreground));
                                ui.label(RichText::new(after).monospace().color(theme.foreground));
                            });
                            let rect = row.response.rect.with_max_x(ui.max_rect().right());
                            if index == selected {
                                ui.painter().rect_filled(rect, 2.0, theme.selection_background.gamma_multiply(0.5));
                            }
                            if ui.interact(rect, editor_id.with(("reference", index)), egui::Sense::click()).clicked() {
                                clicked = Some(index);
                            }
                        }
                    },
                );
            });
        if let Some(index) = clicked {
            self.jump_to_reference(index, outcome);
        }
    }
}
