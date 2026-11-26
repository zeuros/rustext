//! The find / replace widget (Ctrl+F, Ctrl+H) and the go-to-line box (Ctrl+G), drawn over the
//! editor's top right corner. Matches come from `rustext_core::find`.

use egui::{Align2, Area, Color32, Frame, Id, Key, Order, Ui, vec2};
use rustext_core::decoration::{Decoration, DecorationKind};
use rustext_core::find::{self, Match, SearchQuery};
use rustext_core::{Command, Position, Selections};

use super::layout::to_rgba;
use super::{CodeEditor, Metrics};
use crate::keymap::Action;

/// More matches than this are not counted nor highlighted (Monaco stops at 19 999).
const MAX_MATCHES: usize = 20_000;

/// What a list of matches was computed for.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MatchesKey {
    query: SearchQuery,
    scope: Option<(usize, usize)>,
    text_revision: u64,
}

/// Per-view state of the find widget.
#[derive(Clone, Debug, Default)]
pub struct FindState {
    pub is_open: bool,
    pub is_replace_shown: bool,
    pub query: SearchQuery,
    pub replacement: String,
    /// Searches only these char offsets (find in selection).
    pub scope: Option<(usize, usize)>,
    matches: Vec<Match>,
    matches_key: Option<MatchesKey>,
    current_index: Option<usize>,
    error: Option<String>,
    focus_requested: bool,
}

impl FindState {
    pub fn matches(&self) -> &[Match] {
        &self.matches
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

/// Per-view state of the go-to-line box.
#[derive(Clone, Debug, Default)]
pub struct GoToLineState {
    pub is_open: bool,
    pub input: String,
    focus_requested: bool,
}

/// A symbol in the monospace font, which has arrows and triangles (the default proportional
/// font has almost none).
fn symbol(text: &str) -> egui::RichText {
    egui::RichText::new(text).monospace()
}

/// `12` or `12:5` (1-based, as people write them) → a position.
pub fn parse_line_and_column(input: &str) -> Option<Position> {
    let mut parts = input.trim().splitn(2, [':', ',']);
    let line: usize = parts.next()?.trim().parse().ok()?;
    let column: usize = parts.next().map_or(Some(1), |column| column.trim().parse().ok())?;
    Some(Position::new(line.saturating_sub(1), column.saturating_sub(1)))
}

impl CodeEditor<'_> {
    /// Handles the find and go-to-line actions; returns the actions it doesn't handle.
    pub(super) fn run_find_action(&mut self, action: Action) -> Option<Action> {
        match action {
            Action::OpenFind | Action::OpenReplace => {
                self.open_find(action == Action::OpenReplace);
                None
            }
            Action::FindNext => {
                self.go_to_match(true);
                None
            }
            Action::FindPrevious => {
                self.go_to_match(false);
                None
            }
            Action::SelectAllMatches => {
                self.select_all_matches();
                None
            }
            Action::GoToLine => {
                self.view.go_to_line = GoToLineState { is_open: true, input: String::new(), focus_requested: true };
                None
            }
            other => Some(other),
        }
    }

    /// Opens the find widget, seeded with the selected text (one line) or the word at the cursor.
    fn open_find(&mut self, shows_replace: bool) {
        let text = self.state.text();
        let primary = self.state.selections().primary();
        let seed = if primary.is_empty() {
            rustext_core::cursors::word_range_at(text, &self.state.config, primary.head)
                .map(|(start, end)| text.slice(start, end))
        } else {
            Some(text.slice(primary.start(), primary.end())).filter(|selected| !selected.contains('\n'))
        };
        let find = &mut self.view.find;
        if let Some(seed) = seed {
            find.query.pattern = if find.query.is_regex { find::escape(&seed) } else { seed };
        }
        find.is_open = true;
        find.is_replace_shown = shows_replace;
        find.focus_requested = true;
    }

    /// Recomputes the matches when the query, the scope or the text changed.
    pub(super) fn refresh_find_matches(&mut self) {
        let find = &mut self.view.find;
        if !find.is_open {
            return;
        }
        let key = MatchesKey { query: find.query.clone(), scope: find.scope, text_revision: self.state.text_revision() };
        if find.matches_key.as_ref() == Some(&key) {
            return;
        }
        match find::find_matches(self.state.text(), &find.query, find.scope, MAX_MATCHES) {
            Ok(matches) => {
                find.matches = matches;
                find.error = None;
            }
            Err(error) => {
                find.matches.clear();
                find.error = Some(error);
            }
        }
        let cursor = self.state.selections().primary().start();
        find.current_index = find.matches.iter().position(|found| found.start <= cursor && cursor <= found.end);
        find.matches_key = Some(key);
    }

    fn go_to_match(&mut self, forward: bool) {
        if !self.view.find.is_open {
            self.open_find(false);
        }
        self.refresh_find_matches();
        let primary = self.state.selections().primary();
        let matches = &self.view.find.matches;
        let index = if forward {
            find::match_at_or_after(matches, primary.end().max(primary.start() + 1))
        } else {
            find::match_before(matches, primary.start())
        };
        self.select_match(index);
    }

    fn select_match(&mut self, index: Option<usize>) {
        let Some(index) = index else { return };
        let found = self.view.find.matches[index];
        self.view.find.current_index = Some(index);
        self.state
            .execute(Command::SetSelections(Selections::single(rustext_core::Selection::new(found.start, found.end))));
        self.view.reveal_cursor();
    }

    fn select_all_matches(&mut self) {
        self.refresh_find_matches();
        if let Some(selections) = find::select_matches(&self.view.find.matches, self.view.find.current_index) {
            self.state.execute(Command::SetSelections(selections));
        }
    }

    fn replace_current(&mut self) {
        self.refresh_find_matches();
        let find = &self.view.find;
        let primary = self.state.selections().primary();
        let current =
            find.matches.iter().copied().find(|found| found.start == primary.start() && found.end == primary.end());
        match current {
            Some(found) => {
                let transaction = find::replace_one(self.state.text(), found, &find.query, &find.replacement);
                self.state.execute(Command::Apply(transaction));
                self.refresh_find_matches();
                self.go_to_match(true);
            }
            None => self.go_to_match(true),
        }
    }

    fn replace_all(&mut self) {
        self.refresh_find_matches();
        let find = &self.view.find;
        if let Some(transaction) = find::replace_all(self.state.text(), &find.matches, &find.query, &find.replacement) {
            self.state.execute(Command::Apply(transaction));
        }
    }

    pub(super) fn close_find(&mut self) {
        self.view.find.is_open = false;
        self.view.find.matches.clear();
        self.view.find.matches_key = None;
    }

    /// Backgrounds of the matches touching the char offsets `visible_start..=visible_end`.
    pub(super) fn find_decorations(&self, visible_start: usize, visible_end: usize) -> Vec<Decoration> {
        let find = &self.view.find;
        if !find.is_open {
            return Vec::new();
        }
        let first_visible = find.matches.partition_point(|found| found.end < visible_start);
        find.matches[first_visible..]
            .iter()
            .take_while(|found| found.start <= visible_end)
            .enumerate()
            .map(|(index, found)| {
                let is_current = Some(first_visible + index) == find.current_index;
                let color = if is_current {
                    self.theme.current_find_match_background
                } else {
                    self.theme.find_match_background
                };
                Decoration::new(found.start, found.end, DecorationKind::Background(to_rgba(color)))
            })
            .collect()
    }

    /// Draws the find widget and the go-to-line box, and acts on what is done in them.
    pub(super) fn show_find_widgets(&mut self, ui: &mut Ui, metrics: &Metrics, editor_id: Id) -> bool {
        let mut wants_editor_focus = false;
        if self.view.find.is_open {
            wants_editor_focus |= self.show_find_widget(ui, metrics, editor_id);
        }
        if self.view.go_to_line.is_open {
            wants_editor_focus |= self.show_go_to_line(ui, metrics, editor_id);
        }
        wants_editor_focus
    }

    fn show_find_widget(&mut self, ui: &mut Ui, metrics: &Metrics, editor_id: Id) -> bool {
        enum Request {
            Next,
            Previous,
            SelectAll,
            Close,
            ReplaceOne,
            ReplaceAll,
            QueryChanged,
        }
        let mut requests = Vec::new();
        let theme = self.theme.clone();
        let anchor = metrics.text_rect.right_top() + vec2(-12.0, 4.0);
        let match_label = match (&self.view.find.error, self.view.find.matches.len(), self.view.find.current_index) {
            (Some(_), _, _) => "invalid regex".to_string(),
            (None, 0, _) => "No results".to_string(),
            (None, count, Some(index)) => {
                format!("{} of {}{}", index + 1, count, if count >= MAX_MATCHES { "+" } else { "" })
            }
            (None, count, None) => format!("? of {count}"),
        };
        let find = &mut self.view.find;

        Area::new(editor_id.with("find-widget"))
            .order(Order::Foreground)
            .fixed_pos(anchor)
            .pivot(Align2::RIGHT_TOP)
            .show(ui.ctx(), |ui| {
                Frame::popup(ui.style()).fill(theme.widget_background).stroke((1.0, theme.widget_border)).show(
                    ui,
                    |ui| {
                        ui.horizontal(|ui| {
                            let toggle = if find.is_replace_shown { "▾" } else { "▸" };
                            if ui.small_button(symbol(toggle)).on_hover_text("Toggle replace (Ctrl+H)").clicked() {
                                find.is_replace_shown = !find.is_replace_shown;
                            }
                            let search = ui.add(
                                egui::TextEdit::singleline(&mut find.query.pattern)
                                    .hint_text("Find")
                                    .desired_width(200.0)
                                    .id(editor_id.with("find-input")),
                            );
                            if find.focus_requested {
                                search.request_focus();
                                find.focus_requested = false;
                            }
                            if search.changed() {
                                requests.push(Request::QueryChanged);
                            }
                            let (enter, shift, alt, escape) = ui.input(|input| {
                                (
                                    input.key_pressed(Key::Enter),
                                    input.modifiers.shift,
                                    input.modifiers.alt,
                                    input.key_pressed(Key::Escape),
                                )
                            });
                            if search.lost_focus() && enter {
                                requests.push(if alt {
                                    Request::SelectAll
                                } else if shift {
                                    Request::Previous
                                } else {
                                    Request::Next
                                });
                                search.request_focus();
                            }
                            if (search.has_focus() || search.lost_focus()) && escape {
                                requests.push(Request::Close);
                            }
                            for (label, hover, flag) in [
                                ("Aa", "Match case", &mut find.query.match_case),
                                ("ab", "Match whole word", &mut find.query.whole_word),
                                (".*", "Use regular expression", &mut find.query.is_regex),
                            ] {
                                if ui.selectable_label(*flag, label).on_hover_text(hover).clicked() {
                                    *flag = !*flag;
                                    requests.push(Request::QueryChanged);
                                }
                            }
                            let color =
                                if find.error.is_some() || find.matches.is_empty() && !find.query.pattern.is_empty() {
                                    theme.error
                                } else {
                                    theme.foreground
                                };
                            ui.colored_label(color, &match_label);
                            if ui.small_button(symbol("↑")).on_hover_text("Previous match (Shift+Enter)").clicked() {
                                requests.push(Request::Previous);
                            }
                            if ui.small_button(symbol("↓")).on_hover_text("Next match (Enter)").clicked() {
                                requests.push(Request::Next);
                            }
                            if ui.small_button("×").on_hover_text("Close (Escape)").clicked() {
                                requests.push(Request::Close);
                            }
                        });
                        if find.is_replace_shown {
                            ui.horizontal(|ui| {
                                ui.add_space(22.0);
                                let replace = ui.add(
                                    egui::TextEdit::singleline(&mut find.replacement)
                                        .hint_text("Replace")
                                        .desired_width(200.0),
                                );
                                if replace.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                                    requests.push(Request::ReplaceOne);
                                    replace.request_focus();
                                }
                                if ui
                                    .selectable_label(find.query.preserve_case, "AB")
                                    .on_hover_text("Preserve case")
                                    .clicked()
                                {
                                    find.query.preserve_case = !find.query.preserve_case;
                                }
                                if ui.small_button("Replace").clicked() {
                                    requests.push(Request::ReplaceOne);
                                }
                                if ui.small_button("Replace all").clicked() {
                                    requests.push(Request::ReplaceAll);
                                }
                            });
                        }
                    },
                );
            });

        let mut wants_editor_focus = false;
        for request in requests {
            match request {
                Request::Next => self.go_to_match(true),
                Request::Previous => self.go_to_match(false),
                Request::SelectAll => {
                    self.select_all_matches();
                    self.close_find();
                    wants_editor_focus = true;
                }
                Request::Close => {
                    self.close_find();
                    wants_editor_focus = true;
                }
                Request::ReplaceOne => self.replace_current(),
                Request::ReplaceAll => self.replace_all(),
                Request::QueryChanged => {
                    self.refresh_find_matches();
                    let cursor = self.state.selections().primary().start();
                    let index = find::match_at_or_after(&self.view.find.matches, cursor);
                    self.select_match(index);
                }
            }
        }
        wants_editor_focus
    }

    fn show_go_to_line(&mut self, ui: &mut Ui, metrics: &Metrics, editor_id: Id) -> bool {
        let theme = self.theme.clone();
        let line_count = self.state.text().line_count();
        let anchor = metrics.text_rect.center_top() + vec2(0.0, 4.0);
        let mut target = None;
        let mut closes = false;
        let go_to_line = &mut self.view.go_to_line;
        Area::new(editor_id.with("go-to-line"))
            .order(Order::Foreground)
            .fixed_pos(anchor)
            .pivot(Align2::CENTER_TOP)
            .show(ui.ctx(), |ui| {
                Frame::popup(ui.style()).fill(theme.widget_background).stroke((1.0, theme.widget_border)).show(
                    ui,
                    |ui| {
                        let hint = format!("Line (1–{line_count}), or line:column");
                        let input = ui.add(
                            egui::TextEdit::singleline(&mut go_to_line.input).hint_text(hint).desired_width(260.0),
                        );
                        if go_to_line.focus_requested {
                            input.request_focus();
                            go_to_line.focus_requested = false;
                        }
                        let (enter, escape) =
                            ui.input(|input| (input.key_pressed(Key::Enter), input.key_pressed(Key::Escape)));
                        if input.lost_focus() && enter {
                            target = parse_line_and_column(&go_to_line.input);
                            closes = true;
                        }
                        if escape {
                            closes = true;
                        }
                        if !go_to_line.input.is_empty() && parse_line_and_column(&go_to_line.input).is_none() {
                            ui.colored_label(Color32::from_rgb(0xf1, 0x4c, 0x4c), "Type a line number");
                        }
                    },
                );
            });
        if let Some(position) = target {
            let offset = self.state.text().offset(position);
            self.state.execute(Command::SetSelections(Selections::cursor(offset)));
            self.view.reveal_cursor();
        }
        if closes {
            self.view.go_to_line.is_open = false;
        }
        closes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines_and_columns() {
        assert_eq!(parse_line_and_column("12"), Some(Position::new(11, 0)));
        assert_eq!(parse_line_and_column(" 3:7 "), Some(Position::new(2, 6)));
        assert_eq!(parse_line_and_column("3,7"), Some(Position::new(2, 6)));
        assert_eq!(parse_line_and_column("x"), None);
    }
}
