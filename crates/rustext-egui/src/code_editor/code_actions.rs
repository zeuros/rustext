//! Code actions (Ctrl+.): the quick fixes and refactorings the language services offer at the
//! selection, listed under its line; picking one applies its edits as one undo step, the
//! cursors kept on their text.

use egui::{Id, Key, Ui};
use rustext_core::services::{CodeAction, Reply, transaction_keeping_selections};
use rustext_core::{Command, Range};

use super::list_at_cursor::{ListAtCursor, ListKey, ListPlace, Piece, show_list};
use super::{CodeEditor, Metrics};

pub type CodeActionsState = ListAtCursor<CodeAction>;

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
