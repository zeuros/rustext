//! The completion list (Monaco's suggest widget): opened by Ctrl+Space, by typing a word or a
//! trigger char; filtered as the prefix grows; accepted with Enter or Tab. Items come from the
//! application's `LanguageServices` (DECISIONS D9).

use egui::{Align2, Area, Color32, Frame, Id, Key, Order, RichText, Ui, pos2, vec2};
use rustext_core::edit::{Replacement, replace_each};
use rustext_core::history::EditKind;
use rustext_core::services::{CompletionItem, CompletionKind, Reply, filter_completions};
use rustext_core::words::CharClass;
use rustext_core::{Command, Outcome};

use super::{CodeEditor, Metrics};

/// Items the list shows at once.
const VISIBLE_ITEMS: usize = 12;

#[derive(Clone, Debug, Default)]
pub struct CompletionState {
    session: Option<CompletionSession>,
    /// Ctrl+Space was pressed: open even without a typed word.
    is_requested: bool,
}

#[derive(Clone, Debug)]
struct CompletionSession {
    /// Char offset where the word being completed starts.
    prefix_start: usize,
    /// Everything the service answered; `filtered` is what matches the prefix.
    items: Vec<CompletionItem>,
    filtered: Vec<CompletionItem>,
    selected: usize,
    /// The service hasn't answered yet: ask again next frame.
    is_pending: bool,
    first_visible: usize,
    /// The prefix `filtered` was computed for: a new prefix selects the best match again.
    filtered_prefix: Option<String>,
}

impl CompletionState {
    pub fn is_open(&self) -> bool {
        self.session.as_ref().is_some_and(|session| !session.filtered.is_empty())
    }

    /// Labels of the items shown, best first.
    pub fn shown_labels(&self) -> Vec<String> {
        self.session
            .as_ref()
            .map(|session| session.filtered.iter().map(|item| item.label.clone()).collect())
            .unwrap_or_default()
    }

    /// The service hasn't answered yet: the editor repaints to ask again.
    pub fn is_waiting(&self) -> bool {
        self.session.as_ref().is_some_and(|session| session.is_pending)
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.session.as_ref().filter(|session| !session.filtered.is_empty()).map(|session| session.selected)
    }

    /// Opens the list on the next frame, as Ctrl+Space does.
    pub fn request(&mut self) {
        self.is_requested = true;
    }

    pub(super) fn close(&mut self) {
        self.session = None;
        self.is_requested = false;
    }
}

fn kind_label(kind: CompletionKind) -> &'static str {
    match kind {
        CompletionKind::Text => "abc",
        CompletionKind::Keyword => "key",
        CompletionKind::Function | CompletionKind::Method => "fn",
        CompletionKind::Variable => "var",
        CompletionKind::Field => "fld",
        CompletionKind::Type => "type",
        CompletionKind::Module => "mod",
        CompletionKind::Constant => "const",
        CompletionKind::Snippet => "snip",
    }
}

impl CodeEditor<'_> {
    /// Where the word ending at the primary cursor starts.
    fn word_start_before_cursor(&self) -> usize {
        let text = self.state.text();
        let head = self.state.selections().primary().head;
        let word_len = text
            .chars_before(head)
            .take_while(|&character| self.state.config.words.class_of(character) == CharClass::Word)
            .count();
        head - word_len
    }

    /// Opens, refreshes or closes the list after this frame's input. `typed` is what was typed.
    pub(super) fn refresh_completion(&mut self, typed: &str) {
        if self.services.is_none() {
            return;
        }
        let word_start = self.word_start_before_cursor();
        let Some(services) = self.services.as_deref_mut() else { return };
        let text = self.state.text().clone();
        let primary = self.state.selections().primary();
        let head = primary.head;
        let words = &self.state.config.words;

        let last_typed = typed.chars().last();
        let typed_word_char = last_typed.is_some_and(|character| words.class_of(character) == CharClass::Word);
        let typed_trigger =
            last_typed.is_some_and(|character| services.completion_trigger_chars().contains(&character));
        let completion = &mut self.view.completion;

        if let Some(session) = &completion.session {
            let prefix_is_word = text
                .slice(session.prefix_start, head)
                .chars()
                .all(|character| words.class_of(character) == CharClass::Word);
            let stays_on_line =
                head >= session.prefix_start && text.line_of_offset(head) == text.line_of_offset(session.prefix_start);
            if !primary.is_empty() || !stays_on_line || !prefix_is_word || typed_trigger {
                completion.session = None;
            }
        }
        let starts = completion.session.is_none()
            && primary.is_empty()
            && (completion.is_requested || typed_word_char || typed_trigger);
        if starts {
            let prefix_start = if typed_trigger { head } else { word_start };
            let completion = &mut self.view.completion;
            completion.session = Some(CompletionSession {
                prefix_start,
                items: Vec::new(),
                filtered: Vec::new(),
                selected: 0,
                is_pending: true,
                first_visible: 0,
                filtered_prefix: None,
            });
        }
        let completion = &mut self.view.completion;
        let is_requested = std::mem::take(&mut completion.is_requested);
        let Some(session) = completion.session.as_mut() else { return };

        if session.is_pending {
            match services.completions(&text, self.state.text_revision(), text.position(head)) {
                Reply::Ready(items) => {
                    session.items = items;
                    session.is_pending = false;
                }
                Reply::Pending => return,
            }
        }
        let prefix = text.slice(session.prefix_start, head);
        if session.filtered_prefix.as_deref() == Some(prefix.as_str()) {
            return;
        }
        session.filtered = filter_completions(&session.items, &prefix);
        session.filtered.retain(|item| item.label != prefix || is_requested);
        session.selected = 0;
        session.first_visible = 0;
        session.filtered_prefix = Some(prefix);
        if session.filtered.is_empty() && !session.is_pending {
            completion.session = None;
        }
    }

    /// Keys the open list takes before the keymap; returns whether it took this one.
    pub(super) fn completion_key(&mut self, key: Key, outcome: &mut Outcome) -> bool {
        let Some(session) = self.view.completion.session.as_mut().filter(|session| !session.filtered.is_empty()) else {
            return false;
        };
        let last_index = session.filtered.len() - 1;
        match key {
            Key::ArrowDown => session.selected = if session.selected == last_index { 0 } else { session.selected + 1 },
            Key::ArrowUp => session.selected = if session.selected == 0 { last_index } else { session.selected - 1 },
            Key::PageDown => session.selected = (session.selected + VISIBLE_ITEMS).min(last_index),
            Key::PageUp => session.selected = session.selected.saturating_sub(VISIBLE_ITEMS),
            Key::Enter | Key::Tab => {
                let item = session.filtered[session.selected].clone();
                self.accept_completion(&item, outcome);
                return true;
            }
            Key::Escape => {
                self.view.completion.close();
                return true;
            }
            _ => return false,
        }
        let session = self.view.completion.session.as_mut().expect("checked above");
        if session.selected < session.first_visible {
            session.first_visible = session.selected;
        } else if session.selected >= session.first_visible + VISIBLE_ITEMS {
            session.first_visible = session.selected + 1 - VISIBLE_ITEMS;
        }
        true
    }

    /// Replaces the prefix with the item's text at every cursor (each cursor's prefix has the
    /// primary's length, as in VS Code).
    fn accept_completion(&mut self, item: &CompletionItem, outcome: &mut Outcome) {
        let Some(session) = self.view.completion.session.take() else { return };
        let prefix_len = self.state.selections().primary().head - session.prefix_start;
        if item.is_snippet {
            let command = Command::InsertSnippet {
                snippet: item.text_to_insert().to_string(),
                replaced_before_cursor: prefix_len,
            };
            let result = self.state.execute(command);
            *outcome = std::mem::take(outcome).merged(result);
            return;
        }
        let insert = item.text_to_insert().to_string();
        let context = self.state.context();
        let transaction = replace_each(&context, EditKind::Other, |selection| {
            Some(Replacement::new(selection.head.saturating_sub(prefix_len), selection.head, insert.clone()))
        });
        if let Some(transaction) = transaction {
            let result = self.state.execute(Command::Apply(transaction));
            *outcome = std::mem::take(outcome).merged(result);
        }
    }

    /// Paints the list under the primary cursor; a click on an item accepts it.
    pub(super) fn show_completion(&mut self, ui: &mut Ui, metrics: &Metrics, editor_id: Id, outcome: &mut Outcome) {
        let Some(session) = self.view.completion.session.clone().filter(|session| !session.filtered.is_empty()) else {
            return;
        };
        let text = self.state.text();
        let anchor = text.position(session.prefix_start);
        let row = self.row_of_position(anchor.line, anchor.column);
        let x = metrics.text_rect.left() - self.view.scroll_offset.x + anchor.column as f32 * metrics.char_width;
        let top = self.row_top(metrics, row) + metrics.row_height;
        let theme = self.theme.clone();
        let mut clicked = None;
        Area::new(editor_id.with("completion"))
            .order(Order::Foreground)
            .fade_in(false)
            .fixed_pos(pos2(x - 4.0, top))
            .pivot(Align2::LEFT_TOP)
            .show(ui.ctx(), |ui| {
                Frame::popup(ui.style())
                    .fill(theme.widget_background)
                    .stroke((1.0, theme.widget_border))
                    .inner_margin(2.0)
                    .show(ui, |ui| {
                        ui.set_min_width(320.0);
                        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                        let visible =
                            session.filtered.iter().enumerate().skip(session.first_visible).take(VISIBLE_ITEMS);
                        for (index, item) in visible {
                            let is_selected = index == session.selected;
                            let row_response = ui
                                .horizontal(|ui| {
                                    ui.set_min_width(316.0);
                                    let kind_color = if is_selected { theme.foreground } else { theme.line_number };
                                    ui.label(
                                        RichText::new(format!("{:<5}", kind_label(item.kind)))
                                            .monospace()
                                            .size(11.0)
                                            .color(kind_color),
                                    );
                                    ui.label(RichText::new(&item.label).monospace().color(theme.foreground));
                                    if let Some(detail) = &item.detail {
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(RichText::new(detail).size(11.0).color(theme.line_number));
                                        });
                                    }
                                })
                                .response;
                            if is_selected {
                                ui.painter().rect_filled(
                                    row_response.rect,
                                    2.0,
                                    theme.selection_background.gamma_multiply(0.5),
                                );
                            }
                            if ui
                                .interact(
                                    row_response.rect,
                                    editor_id.with(("completion-item", index)),
                                    egui::Sense::click(),
                                )
                                .clicked()
                            {
                                clicked = Some(item.clone());
                            }
                        }
                        if session.filtered.len() > VISIBLE_ITEMS {
                            let shown = format!(
                                "{}–{} of {}",
                                session.first_visible + 1,
                                (session.first_visible + VISIBLE_ITEMS).min(session.filtered.len()),
                                session.filtered.len()
                            );
                            ui.label(RichText::new(shown).size(10.0).color(Color32::GRAY));
                        }
                    });
            });
        if let Some(item) = clicked {
            self.accept_completion(&item, outcome);
        }
    }
}
