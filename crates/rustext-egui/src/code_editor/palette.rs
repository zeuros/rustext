//! The command palette (F1, Ctrl+Shift+P): every keymap binding by name, fuzzy-filtered, run
//! with Enter. Names come from `Command::name()` and the widget actions (DECISIONS D8).

use egui::{Align2, Area, Frame, Id, Key, Order, RichText, Ui, vec2};
use rustext_core::services::FuzzyQuery;

use super::{CodeEditor, Metrics};
use crate::keymap::{Action, KeyChord};

const VISIBLE_ITEMS: usize = 14;

#[derive(Clone, Debug, Default)]
pub struct PaletteState {
    pub is_open: bool,
    pub query: String,
    selected: usize,
    focus_requested: bool,
}

impl PaletteState {
    pub(super) fn open(&mut self) {
        *self = PaletteState { is_open: true, focus_requested: true, ..PaletteState::default() };
    }
}

/// `lines.moveUp` → `Lines: Move Up`; `ZoomIn` → `Zoom In`.
pub fn humanized(name: &str) -> String {
    let mut words = String::new();
    for (index, character) in name.chars().enumerate() {
        if character == '.' {
            words.push(':');
            words.push(' ');
            continue;
        }
        if character.is_uppercase() && index > 0 && !words.ends_with(' ') {
            words.push(' ');
        }
        if words.is_empty() || words.ends_with(' ') {
            words.extend(character.to_uppercase());
        } else {
            words.push(character);
        }
    }
    words
}

/// What the palette shows for an action.
pub fn action_label(action: &Action) -> String {
    match action {
        Action::Edit(command) => humanized(command.name()),
        other => humanized(format!("{other:?}").split([' ', '{', '(']).next().unwrap_or_default()),
    }
}

/// `Ctrl+Shift+K`, `Ctrl+K Ctrl+0`.
pub fn chords_label(chords: &[KeyChord]) -> String {
    chords
        .iter()
        .map(|chord| {
            let modifiers = chord.modifiers;
            let mut parts = Vec::new();
            if modifiers.command || modifiers.ctrl {
                parts.push("Ctrl".to_string());
            }
            if modifiers.shift {
                parts.push("Shift".to_string());
            }
            if modifiers.alt {
                parts.push("Alt".to_string());
            }
            parts.push(chord.key.name().to_string());
            parts.join("+")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl CodeEditor<'_> {
    /// The entries matching the query: (label, key hint, action), best first, one per label.
    fn palette_entries(&self) -> Vec<(String, String, Action)> {
        let mut fuzzy_query = FuzzyQuery::new(&self.view.palette.query);
        let mut entries: Vec<(u16, String, String, Action)> = Vec::new();
        for binding in self.keymap.bindings.iter().rev() {
            let label = action_label(&binding.action);
            if entries.iter().any(|(_, existing, _, _)| *existing == label) {
                continue;
            }
            if let Some(score) = fuzzy_query.score(&label) {
                entries.push((score, label, chords_label(&binding.chords), binding.action.clone()));
            }
        }
        entries.sort_by(|first, second| second.0.cmp(&first.0).then_with(|| first.1.cmp(&second.1)));
        entries.into_iter().map(|(_, label, keys, action)| (label, keys, action)).collect()
    }

    /// Draws the palette; returns the action picked, if any, and whether the editor gets the
    /// focus back.
    pub(super) fn show_palette(&mut self, ui: &mut Ui, metrics: &Metrics, editor_id: Id) -> (Option<Action>, bool) {
        if !self.view.palette.is_open {
            return (None, false);
        }
        let entries = self.palette_entries();
        let theme = self.theme.clone();
        let palette = &mut self.view.palette;
        palette.selected = palette.selected.min(entries.len().saturating_sub(1));
        let (enter, escape, down, up) = ui.input(|input| {
            (
                input.key_pressed(Key::Enter),
                input.key_pressed(Key::Escape),
                input.key_pressed(Key::ArrowDown),
                input.key_pressed(Key::ArrowUp),
            )
        });
        let mut picked = None;
        let mut closes = false;
        let anchor = metrics.text_rect.center_top() + vec2(0.0, 4.0);
        Area::new(editor_id.with("command-palette"))
            .order(Order::Foreground)
            .fade_in(false)
            .fixed_pos(anchor)
            .pivot(Align2::CENTER_TOP)
            .show(ui.ctx(), |ui| {
                Frame::popup(ui.style()).fill(theme.widget_background).stroke((1.0, theme.widget_border)).show(
                    ui,
                    |ui| {
                        ui.set_width(480.0);
                        let input = ui.add(
                            egui::TextEdit::singleline(&mut palette.query)
                                .hint_text("Type a command")
                                .desired_width(f32::INFINITY),
                        );
                        if palette.focus_requested {
                            input.request_focus();
                            palette.focus_requested = false;
                        }
                        if input.changed() {
                            palette.selected = 0;
                        }
                        if down && !entries.is_empty() {
                            palette.selected = (palette.selected + 1) % entries.len();
                        }
                        if up && !entries.is_empty() {
                            palette.selected = (palette.selected + entries.len() - 1) % entries.len();
                        }
                        let first_visible = palette.selected.saturating_sub(VISIBLE_ITEMS - 1);
                        for (index, (label, keys, action)) in
                            entries.iter().enumerate().skip(first_visible).take(VISIBLE_ITEMS)
                        {
                            let is_selected = index == palette.selected;
                            let row = ui.horizontal(|ui| {
                                ui.set_width(470.0);
                                ui.label(RichText::new(label).color(theme.foreground));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(RichText::new(keys).monospace().size(11.0).color(theme.line_number));
                                });
                            });
                            if is_selected {
                                ui.painter().rect_filled(
                                    row.response.rect,
                                    2.0,
                                    theme.selection_background.gamma_multiply(0.5),
                                );
                            }
                            if ui
                                .interact(
                                    row.response.rect,
                                    editor_id.with(("palette-item", index)),
                                    egui::Sense::click(),
                                )
                                .clicked()
                            {
                                picked = Some(action.clone());
                            }
                        }
                        if entries.is_empty() {
                            ui.label(RichText::new("No matching commands").color(theme.line_number));
                        }
                        if (input.lost_focus() || input.has_focus()) && enter {
                            picked = entries.get(palette.selected).map(|(_, _, action)| action.clone());
                        }
                        closes = escape;
                    },
                );
            });
        if picked.is_some() || closes {
            self.view.palette.is_open = false;
        }
        let gives_focus_back = picked.is_some() || closes;
        (picked, gives_focus_back)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Modifiers;

    #[test]
    fn humanizes_command_names() {
        assert_eq!(humanized("lines.moveUp"), "Lines: Move Up");
        assert_eq!(humanized("ZoomIn"), "Zoom In");
        assert_eq!(humanized("selection.addNextOccurrence"), "Selection: Add Next Occurrence");
    }

    #[test]
    fn labels_chords() {
        let chords = [
            KeyChord::new(Modifiers::COMMAND, Key::K),
            KeyChord::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Num0),
        ];

        assert_eq!(chords_label(&chords), "Ctrl+K Ctrl+Shift+0");
    }
}
