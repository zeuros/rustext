//! A list under the cursor's line answering a request to the services (Find References, code
//! actions): the request waits for its answer frame after frame, Up/Down choose, Enter or a
//! click picks, Escape closes, and an edit of the text closes it.

use egui::{Area, Frame, Id, Key, Order, Pos2, RichText, Ui};

use crate::EditorTheme;

/// What the list is waiting for, what it shows, and which entry is chosen.
#[derive(Clone, Debug)]
pub struct ListAtCursor<T> {
    entries: Vec<T>,
    selected: usize,
    /// The text revision the entries are for.
    revision: u64,
    /// The cursor offset the list was asked at, where it shows.
    anchor: usize,
    /// Waiting for the services' answer: (text revision, cursor offset).
    request: Option<(u64, usize)>,
}

impl<T> Default for ListAtCursor<T> {
    fn default() -> Self {
        Self { entries: Vec::new(), selected: 0, revision: 0, anchor: 0, request: None }
    }
}

/// What a key did to an open list.
pub(super) enum ListKey {
    Ignored,
    Handled,
    Picked(usize),
}

impl<T> ListAtCursor<T> {
    pub fn is_open(&self) -> bool {
        !self.entries.is_empty()
    }

    pub fn entries(&self) -> &[T] {
        &self.entries
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub(super) fn anchor(&self) -> usize {
        self.anchor
    }

    /// A list asked for at `anchor`, waiting for its answer.
    pub(super) fn asked(revision: u64, anchor: usize) -> Self {
        Self { anchor, request: Some((revision, anchor)), ..Self::default() }
    }

    /// The request still waiting, for the text at `revision`; closes the list after an edit.
    pub(super) fn pending_request(&mut self, revision: u64) -> Option<usize> {
        let is_stale =
            self.request.map_or(self.is_open() && self.revision != revision, |request| request.0 != revision);
        if is_stale {
            self.close();
        }
        self.request.map(|(_, anchor)| anchor)
    }

    /// Shows the answer: `entries` (an empty answer closes the list), `selected` chosen.
    pub(super) fn answer(&mut self, entries: Vec<T>, selected: usize, revision: u64) {
        *self = Self {
            selected: selected.min(entries.len().saturating_sub(1)),
            entries,
            revision,
            anchor: self.anchor,
            request: None,
        };
    }

    pub(super) fn close(&mut self) {
        *self = Self::default();
    }

    /// Up/Down move the choice (wrapping), Enter picks it, Escape closes.
    pub(super) fn key(&mut self, key: Key) -> ListKey {
        if !self.is_open() {
            return ListKey::Ignored;
        }
        let last_index = self.entries.len() - 1;
        match key {
            Key::ArrowDown => self.selected = if self.selected == last_index { 0 } else { self.selected + 1 },
            Key::ArrowUp => self.selected = if self.selected == 0 { last_index } else { self.selected - 1 },
            Key::Enter => return ListKey::Picked(self.selected),
            Key::Escape => self.close(),
            _ => return ListKey::Ignored,
        }
        ListKey::Handled
    }

    /// Takes the entry at `index`, closing the list.
    pub(super) fn take(&mut self, index: usize) -> Option<T> {
        let entry = (index < self.entries.len()).then(|| self.entries.swap_remove(index));
        self.close();
        entry
    }
}

/// How a piece of a row looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Piece {
    Dim,
    Normal,
    Strong,
}

/// Rows of a list, at most this many shown, scrolled to keep the chosen one in view.
const VISIBLE_ROWS: usize = 10;

/// Where a list shows and what it is called.
pub(super) struct ListPlace<'a> {
    pub id: Id,
    pub top_left: Pos2,
    pub width: f32,
    pub title: &'a str,
}

/// Draws a list: a dim title, then the rows (pieces of monospace text), the chosen one
/// highlighted. Returns the index of a clicked row.
pub(super) fn show_list(
    ui: &Ui,
    place: ListPlace<'_>,
    theme: &EditorTheme,
    rows: &[Vec<(String, Piece)>],
    selected: usize,
) -> Option<usize> {
    let ListPlace { id, top_left, width, title } = place;
    let first_visible = selected.saturating_sub(VISIBLE_ROWS - 1);
    let mut clicked = None;
    Area::new(id).order(Order::Foreground).fade_in(false).fixed_pos(top_left).show(ui.ctx(), |ui| {
        Frame::popup(ui.style()).fill(theme.widget_background).stroke((1.0, theme.widget_border)).show(ui, |ui| {
            ui.set_width(width);
            ui.label(RichText::new(title).size(11.0).color(theme.line_number));
            for (index, pieces) in rows.iter().enumerate().skip(first_visible).take(VISIBLE_ROWS) {
                let row = ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (text, piece) in pieces {
                        let text = RichText::new(text).monospace();
                        ui.label(match piece {
                            Piece::Dim => text.color(theme.line_number),
                            Piece::Normal => text.color(theme.foreground),
                            Piece::Strong => text.strong().color(theme.foreground),
                        });
                    }
                });
                let rect = row.response.rect.with_max_x(ui.max_rect().right());
                if index == selected {
                    ui.painter().rect_filled(rect, 2.0, theme.selection_background.gamma_multiply(0.5));
                }
                if ui.interact(rect, id.with(index), egui::Sense::click()).clicked() {
                    clicked = Some(index);
                }
            }
        });
    });
    clicked
}
