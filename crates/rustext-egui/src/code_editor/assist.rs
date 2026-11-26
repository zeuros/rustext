//! Rename (F2), signature help and inlay hints, from the language services (DECISIONS D9).

use egui::{Align2, Area, Frame, Id, Key, Order, RichText, Ui, pos2};
use rustext_core::decoration::Decoration;
use rustext_core::services::{Reply, SignatureHelp, transaction_of_edits};
use rustext_core::{ChangeSet, Command, Outcome};

use super::layout::to_rgba;
use super::{CodeEditor, Metrics};

/// Inlay hints are asked for whole blocks of this many lines around the visible ones, so
/// scrolling a little reuses the answer.
const INLAY_HINT_BLOCK_LINES: usize = 100;

/// The shown inlay hints (they follow edits until the new answer) and the lines asked for.
#[derive(Clone, Debug, Default)]
pub struct InlayHintsState {
    decorations: Vec<Decoration>,
    /// (text revision, lines) of the shown hints, and of the request waiting for an answer.
    answered: Option<(u64, std::ops::Range<usize>)>,
    request: Option<(u64, std::ops::Range<usize>)>,
}

impl InlayHintsState {
    pub fn decorations(&self) -> &[Decoration] {
        &self.decorations
    }

    pub fn labels(&self) -> Vec<&str> {
        self.decorations
            .iter()
            .filter_map(|decoration| match &decoration.kind {
                rustext_core::decoration::DecorationKind::InjectedText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    pub(super) fn map(&mut self, changes: &ChangeSet) {
        self.decorations = self.decorations.iter().filter_map(|decoration| decoration.map(changes)).collect();
    }
}

#[derive(Clone, Debug, Default)]
pub struct RenameState {
    pub is_open: bool,
    pub new_name: String,
    /// Char offset of the symbol being renamed.
    offset: usize,
    /// Waiting for the service's edits: (text revision, offset, new name).
    request: Option<(u64, usize, String)>,
    focus_requested: bool,
}

/// The signature tooltip and the request waiting for one.
#[derive(Clone, Debug, Default)]
pub struct SignatureState {
    shown: Option<(usize, SignatureHelp)>,
    request: Option<(u64, usize)>,
}

impl super::EditorView {
    pub(super) fn signature_is_waiting(&self) -> bool {
        self.signature.request.is_some()
    }
}

impl SignatureState {
    pub fn shown_label(&self) -> Option<&str> {
        self.shown.as_ref().map(|(_, help)| help.label.as_str())
    }
}

impl CodeEditor<'_> {
    /// F2: opens the rename box on the word at the cursor.
    pub(super) fn open_rename(&mut self) {
        let text = self.state.text();
        let head = self.state.selections().primary().head;
        let Some((start, end)) = rustext_core::cursors::word_range_at(text, &self.state.config, head) else { return };
        self.view.rename = RenameState {
            is_open: true,
            new_name: text.slice(start, end),
            offset: start,
            request: None,
            focus_requested: true,
        };
    }

    /// Applies the service's edits once they arrive.
    fn refresh_rename(&mut self, outcome: &mut Outcome) {
        let Some((revision, offset, new_name)) = self.view.rename.request.clone() else { return };
        if revision != self.state.text_revision() {
            self.view.rename.request = None;
            return;
        }
        let Some(services) = self.services.as_deref_mut() else { return };
        let text = self.state.text().clone();
        match services.rename(&text, revision, text.position(offset), &new_name) {
            Reply::Ready(edits) => {
                self.view.rename.request = None;
                if let Some(transaction) = transaction_of_edits(&text, &edits) {
                    let result = self.state.execute(Command::Apply(transaction));
                    *outcome = std::mem::take(outcome).merged(result);
                }
            }
            Reply::Pending => {}
        }
    }

    /// Draws the rename box under the symbol; returns whether the editor gets the focus back.
    pub(super) fn show_rename(&mut self, ui: &mut Ui, metrics: &Metrics, editor_id: Id, outcome: &mut Outcome) -> bool {
        self.refresh_rename(outcome);
        if self.view.rename.request.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
        }
        if !self.view.rename.is_open {
            return false;
        }
        let text = self.state.text();
        let position = text.position(self.view.rename.offset);
        let row = self.row_of_position(position.line, position.column);
        let x = metrics.text_rect.left() - self.view.scroll_offset.x + position.column as f32 * metrics.char_width;
        let top = self.row_top(metrics, row) + metrics.row_height;
        let theme = self.theme.clone();
        let (enter, escape) = ui.input(|input| (input.key_pressed(Key::Enter), input.key_pressed(Key::Escape)));
        let rename = &mut self.view.rename;
        let mut gives_focus_back = false;
        Area::new(editor_id.with("rename")).order(Order::Foreground).fade_in(false).fixed_pos(pos2(x, top)).show(
            ui.ctx(),
            |ui| {
                Frame::popup(ui.style()).fill(theme.widget_background).stroke((1.0, theme.widget_border)).show(
                    ui,
                    |ui| {
                        let input = ui.add(
                            egui::TextEdit::singleline(&mut rename.new_name).hint_text("New name").desired_width(220.0),
                        );
                        if rename.focus_requested {
                            input.request_focus();
                            rename.focus_requested = false;
                        }
                        ui.label(
                            RichText::new("Enter to rename, Escape to cancel").size(10.0).color(theme.line_number),
                        );
                        if input.lost_focus() && enter && !rename.new_name.is_empty() {
                            rename.request = Some((self.state.text_revision(), rename.offset, rename.new_name.clone()));
                            rename.is_open = false;
                            gives_focus_back = true;
                        }
                        if escape {
                            rename.is_open = false;
                            gives_focus_back = true;
                        }
                    },
                );
            },
        );
        gives_focus_back
    }

    /// Asks for the inlay hints of the blocks around the visible lines when the text or those
    /// blocks changed; returns whether an answer is awaited.
    pub(super) fn refresh_inlay_hints(&mut self, metrics: &Metrics) -> bool {
        let visible = self.visible_lines_for_hints(metrics);
        let (Some(services), Some(visible)) = (self.services.as_deref_mut(), visible) else {
            self.view.inlay_hints = InlayHintsState::default();
            return false;
        };
        let wanted = (self.state.text_revision(), visible);
        let hints = &mut self.view.inlay_hints;
        if hints.answered.as_ref() != Some(&wanted) {
            hints.request = Some(wanted);
        }
        let Some((revision, lines)) = hints.request.clone() else { return false };
        let text = self.state.text();
        match services.inlay_hints(text, revision, lines.clone()) {
            Reply::Ready(answer) => {
                let color = to_rgba(self.theme.hint);
                hints.decorations = answer
                    .into_iter()
                    .filter(|hint| hint.position.line < text.line_count())
                    .map(|hint| Decoration::injected_text(text.offset(hint.position), hint.label, color))
                    .collect();
                hints.answered = hints.request.take();
                false
            }
            Reply::Pending => true,
        }
    }

    /// The visible lines widened to whole `INLAY_HINT_BLOCK_LINES` blocks, `None` when hints
    /// are off.
    fn visible_lines_for_hints(&self, metrics: &Metrics) -> Option<std::ops::Range<usize>> {
        if !self.config.inlay_hints {
            return None;
        }
        let visible = self.visible_lines(metrics)?;
        let first = visible.start() / INLAY_HINT_BLOCK_LINES * INLAY_HINT_BLOCK_LINES;
        let end = (visible.end() / INLAY_HINT_BLOCK_LINES + 1) * INLAY_HINT_BLOCK_LINES;
        Some(first..end.min(self.state.text().line_count()))
    }

    /// Asks for the signature after `(` or `,`; closes it after `)`, Escape or a line change.
    pub(super) fn refresh_signature_help(&mut self, typed: &str) {
        let head = self.state.selections().primary().head;
        let revision = self.state.text_revision();
        let text = self.state.text().clone();
        let signature = &mut self.view.signature;
        if typed.ends_with(')') {
            *signature = SignatureState::default();
        }
        if let Some((anchor, _)) = signature.shown.as_ref()
            && text.line_of_offset(*anchor) != text.line_of_offset(head)
        {
            signature.shown = None;
        }
        if typed.ends_with('(') || typed.ends_with(',') {
            signature.request = Some((revision, head));
        }
        let Some((request_revision, request_offset)) = signature.request else { return };
        let Some(services) = self.services.as_deref_mut() else { return };
        match services.signature_help(&text, request_revision, text.position(request_offset)) {
            Reply::Ready(help) => {
                signature.request = None;
                signature.shown = help.map(|help| (request_offset, help));
            }
            Reply::Pending => {}
        }
    }

    pub(super) fn close_signature_help(&mut self) -> bool {
        let was_shown = self.view.signature.shown.is_some();
        self.view.signature = SignatureState::default();
        was_shown
    }

    /// The signature above the cursor's row, the active parameter in bold.
    pub(super) fn show_signature_help(&self, ui: &mut Ui, metrics: &Metrics, editor_id: Id) {
        let Some((anchor, help)) = &self.view.signature.shown else { return };
        let text = self.state.text();
        let position = text.position(*anchor);
        let row = self.row_of_position(position.line, position.column);
        let x = metrics.text_rect.left() - self.view.scroll_offset.x + position.column as f32 * metrics.char_width;
        let top = self.row_top(metrics, row);
        let theme = self.theme.clone();
        Area::new(editor_id.with("signature"))
            .order(Order::Foreground)
            .fade_in(false)
            .interactable(false)
            .fixed_pos(pos2(x, top))
            .pivot(Align2::LEFT_BOTTOM)
            .show(ui.ctx(), |ui| {
                Frame::popup(ui.style()).fill(theme.widget_background).stroke((1.0, theme.widget_border)).show(
                    ui,
                    |ui| {
                        let chars: Vec<char> = help.label.chars().collect();
                        let (start, end) = help.active_parameter.unwrap_or((chars.len(), chars.len()));
                        let piece = |range: std::ops::Range<usize>| {
                            chars[range.start.min(chars.len())..range.end.min(chars.len())].iter().collect::<String>()
                        };
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label(RichText::new(piece(0..start)).monospace().color(theme.foreground));
                            ui.label(
                                RichText::new(piece(start..end))
                                    .monospace()
                                    .strong()
                                    .underline()
                                    .color(theme.foreground),
                            );
                            ui.label(RichText::new(piece(end..chars.len())).monospace().color(theme.foreground));
                        });
                        if let Some(documentation) = &help.documentation {
                            ui.label(RichText::new(documentation).size(11.0).color(theme.line_number));
                        }
                    },
                );
            });
    }
}
