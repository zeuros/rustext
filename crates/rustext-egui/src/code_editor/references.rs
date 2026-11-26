//! Find references (Shift+F12): the language services' references of the symbol at the cursor,
//! listed under its line with each line's text; Up/Down choose, Enter or a click jumps.

use egui::{Area, Frame, Id, Key, Order, RichText, Ui, pos2};
use rustext_core::services::Reply;
use rustext_core::{Command, Outcome, Selection, Selections};

use super::{CodeEditor, Metrics};

const VISIBLE_ITEMS: usize = 10;

#[derive(Clone, Debug, Default)]
pub struct ReferencesState {
    /// Char offset ranges of the references, in text order; empty when the list is closed.
    entries: Vec<(usize, usize)>,
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

    pub fn entries(&self) -> &[(usize, usize)] {
        &self.entries
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    fn close(&mut self) {
        *self = ReferencesState::default();
    }
}

impl CodeEditor<'_> {
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
            Reply::Ready(ranges) => {
                let mut entries: Vec<(usize, usize)> =
                    ranges.into_iter().map(|range| text.range_offsets(range)).collect();
                entries.sort_unstable();
                entries.dedup();
                let selected = entries.iter().position(|&(start, end)| start <= offset && offset <= end).unwrap_or(0);
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
        let Some(&(start, end)) = self.view.references.entries.get(index) else { return };
        self.view.references.close();
        let result = self.state.execute(Command::SetSelections(Selections::single(Selection::new(start, end))));
        *outcome = std::mem::take(outcome).merged(result);
    }

    /// The list under the line it was asked at: `line  text` per reference, the reference in bold.
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
            .map(|(index, &(start, end))| {
                let position = text.position(start);
                let content: Vec<char> = text.line_content(position.line).chars().collect();
                let end_column = (position.column + end - start).min(content.len());
                let piece = |range: std::ops::Range<usize>| content[range].iter().collect::<String>();
                let before = piece(0..position.column).trim_start().to_string();
                (
                    index,
                    format!("{:>5}  ", position.line + 1),
                    before,
                    piece(position.column..end_column),
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
                        for (index, line_number, before, reference, after) in rows {
                            let row = ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                ui.label(RichText::new(line_number).monospace().color(theme.line_number));
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
