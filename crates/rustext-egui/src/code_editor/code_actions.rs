//! Code actions (Ctrl+.): the quick fixes and refactorings the language services offer at the
//! selection, listed under its line; picking one applies its edits as one undo step, the
//! cursors kept on their text.

use egui::{Id, Key, Rect, Ui, pos2, vec2};
use rustext_core::services::{CodeAction, Reply, transaction_keeping_selections};
use rustext_core::{Command, Range};

use super::list_at_cursor::{ListAtCursor, ListKey, ListPlace, Piece, show_list};
use super::{CodeEditor, Metrics};

pub type CodeActionsState = ListAtCursor<CodeAction>;

impl super::EditorView {
    /// Whether the lightbulb shows: the services offered actions where the cursor rests.
    pub fn shows_lightbulb(&self) -> bool {
        self.lightbulb.has_actions == Some(true)
    }

    /// Where the lightbulb was painted, if it was.
    pub fn lightbulb_rect(&self) -> Option<Rect> {
        self.lightbulb.rect
    }
}

/// How long the cursor rests before the services are asked whether it has code actions.
const LIGHTBULB_REST_SECONDS: f64 = 0.25;

/// The lightbulb: whether the services offer actions where the cursor rests.
#[derive(Clone, Debug, Default)]
pub struct LightbulbState {
    /// The (text revision, cursor offset) this is about, and since when the cursor rests there.
    key: Option<(u64, usize)>,
    rested_since: f64,
    /// `None` until the services answer.
    has_actions: Option<bool>,
    /// Where the bulb was painted (clicks open the actions).
    rect: Option<Rect>,
}

impl CodeEditor<'_> {
    /// Ctrl+.: asks for the actions at the primary selection.
    pub(super) fn open_code_actions(&mut self) {
        let head = self.state.selections().primary().head;
        self.view.code_actions = ListAtCursor::asked(self.state.text_revision(), head);
    }

    /// Takes the services' answer (none closes the list); an edit closes it. Returns whether an
    /// answer is awaited.
    pub(super) fn refresh_code_actions(&mut self) -> bool {
        let revision = self.state.text_revision();
        if self.view.code_actions.pending_request(revision).is_none() {
            return false;
        }
        let Some(services) = self.services.as_deref_mut() else {
            self.view.code_actions.close();
            return false;
        };
        let text = self.state.text();
        let primary = self.state.selections().primary();
        let range = Range::new(text.position(primary.start()), text.position(primary.end()));
        match services.code_actions(text, revision, range) {
            Reply::Ready(actions) => {
                self.view.code_actions.answer(actions, 0, revision);
                false
            }
            Reply::Pending => true,
        }
    }

    /// Asks the services whether the cursor has actions once it rested there (D19: not on
    /// every move). Returns whether the editor should paint again soon (waiting).
    pub(super) fn refresh_lightbulb(&mut self, now: f64) -> bool {
        let primary = self.state.selections().primary();
        let key = (self.state.text_revision(), primary.head);
        let lightbulb = &mut self.view.lightbulb;
        let Some(services) = self.services.as_deref_mut().filter(|_| self.config.lightbulb) else {
            *lightbulb = LightbulbState::default();
            return false;
        };
        if lightbulb.key != Some(key) {
            *lightbulb = LightbulbState { key: Some(key), rested_since: now, ..LightbulbState::default() };
            return true;
        }
        if lightbulb.has_actions.is_some() {
            return false;
        }
        if now - lightbulb.rested_since < LIGHTBULB_REST_SECONDS {
            return true;
        }
        let text = self.state.text();
        let cursor = text.position(primary.head);
        match services.code_actions(text, key.0, Range::new(cursor, cursor)) {
            Reply::Ready(actions) => {
                lightbulb.has_actions = Some(!actions.is_empty());
                false
            }
            Reply::Pending => true,
        }
    }

    /// A bulb at the left of the gutter on the cursor's row, in the warning color.
    pub(super) fn paint_lightbulb(&mut self, painter: &egui::Painter, metrics: &Metrics) {
        self.view.lightbulb.rect = None;
        if !self.view.shows_lightbulb() {
            return;
        }
        let text = self.state.text();
        let head = text.position(self.state.selections().primary().head);
        let top = self.row_top(metrics, self.row_of_position(head.line, head.column));
        let size = metrics.row_height * 0.8;
        let rect = Rect::from_min_size(
            pos2(metrics.gutter_rect().left() + 1.0, top + (metrics.row_height - size) / 2.0),
            vec2(size, size),
        );
        let bulb_center = rect.center() - vec2(0.0, size * 0.1);
        painter.circle_filled(bulb_center, size * 0.3, self.theme.warning);
        let base =
            Rect::from_center_size(pos2(rect.center().x, rect.bottom() - size * 0.15), vec2(size * 0.3, size * 0.15));
        painter.rect_filled(base, 1.0, self.theme.line_number);
        self.view.lightbulb.rect = Some(rect);
    }

    /// A press on the lightbulb opens the actions; returns whether it was one.
    pub(super) fn press_lightbulb(&mut self, pos: egui::Pos2) -> bool {
        let is_on_bulb = self.view.lightbulb.rect.is_some_and(|rect| rect.contains(pos));
        if is_on_bulb {
            self.open_code_actions();
        }
        is_on_bulb
    }

    /// Up/Down/Enter/Escape while the list is open; returns whether the key was taken.
    pub(super) fn code_actions_key(&mut self, key: Key) -> bool {
        match self.view.code_actions.key(key) {
            ListKey::Ignored => false,
            ListKey::Handled => true,
            ListKey::Picked(index) => {
                self.apply_code_action(index);
                true
            }
        }
    }

    fn apply_code_action(&mut self, index: usize) {
        let Some(action) = self.view.code_actions.take(index) else { return };
        let transaction = transaction_keeping_selections(self.state.text(), &action.edits, self.state.selections());
        if let Some(transaction) = transaction {
            self.execute_tracked(Command::Apply(transaction));
        }
    }

    /// The list under the selection's line: preferred actions in bold.
    pub(super) fn show_code_actions(&mut self, ui: &mut Ui, metrics: &Metrics, editor_id: Id) {
        let code_actions = &self.view.code_actions;
        if !code_actions.is_open() {
            return;
        }
        let rows: Vec<Vec<(String, Piece)>> = code_actions
            .entries()
            .iter()
            .map(|action| {
                let marker = if action.is_quick_fix { "fix  " } else { "     " };
                let title_piece = if action.is_preferred { Piece::Strong } else { Piece::Normal };
                vec![(marker.to_string(), Piece::Dim), (action.title.clone(), title_piece)]
            })
            .collect();
        let top_left = self.below_cursor_line(metrics, code_actions.anchor());
        let width = (metrics.text_rect.width() - 24.0).clamp(200.0, 480.0);
        let selected = code_actions.selected();
        let place = ListPlace { id: editor_id.with("code-actions"), top_left, width, title: "Code actions" };
        if let Some(index) = show_list(ui, place, self.theme, &rows, selected) {
            self.apply_code_action(index);
        }
    }
}
