//! The accessible diff viewer (Alt+F2): the change at the cursor, line by line, for a screen
//! reader.

use egui::{Rect, Ui, pos2};

use super::DiffEditor;

impl DiffEditor<'_> {
    /// The change at the cursor as lines a screen reader reads (`removed line 9: …`), in a
    /// panel along the bottom; F7 / Shift+F7 move to the next / previous change, Escape closes.
    pub(super) fn show_accessible_viewer(&mut self, ui: &Ui, rect: Rect) {
        const CONTEXT_LINES: usize = 3;
        let document = &mut *self.document;
        if !document.shows_accessible_viewer {
            return;
        }
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            document.shows_accessible_viewer = false;
            return;
        }
        let hunk_index = document.hunk_at_cursor();
        let lines = hunk_index
            .map(|index| {
                rustext_diff::hunk_read_lines(
                    &document.diff,
                    document.old.text(),
                    document.new.text(),
                    index,
                    CONTEXT_LINES,
                )
            })
            .unwrap_or_default();
        let title = match hunk_index {
            Some(index) => format!("Change {} of {}", index + 1, document.diff.hunks.len()),
            None => "No change at the cursor: F7 goes to the next one".to_string(),
        };
        let theme = self.theme;
        egui::Area::new(ui.make_persistent_id("rustext-accessible-diff-viewer"))
            .order(egui::Order::Foreground)
            .fade_in(false)
            .fixed_pos(pos2(rect.left() + 8.0, rect.bottom() - 8.0))
            .pivot(egui::Align2::LEFT_BOTTOM)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).fill(theme.widget_background).stroke((1.0, theme.widget_border)).show(
                    ui,
                    |ui| {
                        ui.set_width((rect.width() - 32.0).max(200.0));
                        ui.label(egui::RichText::new(title).strong().color(theme.foreground));
                        for line in &lines {
                            let color = match line.kind {
                                rustext_diff::ReadLineKind::Unchanged => theme.line_number,
                                rustext_diff::ReadLineKind::Removed => theme.error,
                                rustext_diff::ReadLineKind::Added => theme.info,
                            };
                            ui.label(egui::RichText::new(line.spoken()).monospace().color(color));
                        }
                    },
                );
            });
    }
}
