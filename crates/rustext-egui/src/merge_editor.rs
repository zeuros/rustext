//! The `MergeEditor` widget: "ours" and "theirs" side by side with aligned rows, the editable
//! result below, and accept actions per conflict (VS Code's merge editor, gitgud's conflict
//! view). The model is `rustext_diff::merge`.

use egui::{Rect, Stroke, Ui, UiBuilder, pos2};
use rustext_core::change::{Bias, Change, ChangeSet};
use rustext_core::decoration::{Decoration, DecorationKind};
use rustext_core::display::{DisplayLayout, DisplayRow, RowContent};
use rustext_core::history::EditKind;
use rustext_core::style::LineStyler;
use rustext_core::{Command, EditingConfig, EditorState, Selections, Text, Transaction};
use rustext_diff::merge::{
    MergeInput, MergeRegion, RegionKind, Resolution, merge_regions, merged_text, resolved_region,
};

use crate::code_editor::{CodeEditor, EditorView, ViewConfig, to_rgba};
use crate::keymap::Keymap;
use crate::theme::EditorTheme;

/// A merge being resolved: the three inputs, the regions, and the result being written.
pub struct MergeDocument {
    input: MergeInput,
    regions: Vec<MergeRegion>,
    /// What the user chose per region; `None` keeps the default (non-conflicting changes taken,
    /// conflicts showing the base).
    chosen_resolutions: Vec<Option<Resolution>>,
    /// Char offsets of each region in the result; they follow edits of the result.
    result_ranges: Vec<(usize, usize)>,
    pub ours: EditorState,
    pub theirs: EditorState,
    pub result: EditorState,
    pub ours_view: EditorView,
    pub theirs_view: EditorView,
    pub result_view: EditorView,
    current_conflict: Option<usize>,
    /// The inputs' and the result's vertical scroll after the last sync, to see which one the
    /// user scrolled since.
    synced_scroll: (f32, f32),
}

impl MergeDocument {
    pub fn new(base: &str, ours: &str, theirs: &str) -> Self {
        let input = MergeInput { base: Text::new(base), ours: Text::new(ours), theirs: Text::new(theirs) };
        let regions = merge_regions(&input);
        let resolutions: Vec<Resolution> = regions.iter().map(|region| region.kind.default_resolution()).collect();

        let mut result_ranges = Vec::with_capacity(regions.len());
        let mut offset = 0;
        for (region, &resolution) in regions.iter().zip(&resolutions) {
            let region_len = resolved_region(&input, region, resolution).chars().count();
            result_ranges.push((offset, offset + region_len));
            offset += region_len;
        }
        let read_only = EditingConfig { read_only: true, ..EditingConfig::default() };
        let mut document = Self {
            result: EditorState::new(&merged_text(&input, &regions, &resolutions)),
            ours: EditorState::with_text(input.ours.clone()).with_config(read_only.clone()),
            theirs: EditorState::with_text(input.theirs.clone()).with_config(read_only),
            chosen_resolutions: vec![None; regions.len()],
            result_ranges,
            regions,
            input,
            ours_view: EditorView::default(),
            theirs_view: EditorView::default(),
            result_view: EditorView::default(),
            current_conflict: None,
            synced_scroll: (0.0, 0.0),
        };
        document.current_conflict = document.conflict_indexes().first().copied();
        document
    }

    pub fn regions(&self) -> &[MergeRegion] {
        &self.regions
    }

    /// Indexes (in `regions`) of the conflicts.
    pub fn conflict_indexes(&self) -> Vec<usize> {
        self.regions
            .iter()
            .enumerate()
            .filter(|(_, region)| region.kind == RegionKind::Conflict)
            .map(|(index, _)| index)
            .collect()
    }

    pub fn unresolved_conflict_count(&self) -> usize {
        self.conflict_indexes().into_iter().filter(|&index| self.chosen_resolutions[index].is_none()).count()
    }

    pub fn current_conflict(&self) -> Option<usize> {
        self.current_conflict
    }

    pub fn resolution(&self, region_index: usize) -> Option<Resolution> {
        self.chosen_resolutions[region_index]
    }

    pub fn result_text(&self) -> String {
        self.result.text().to_string()
    }

    /// Replaces a region's text in the result with `resolution`.
    pub fn accept(&mut self, region_index: usize, resolution: Resolution) {
        let (start, end) = self.result_ranges[region_index];
        let replacement = resolved_region(&self.input, &self.regions[region_index], resolution);
        let replacement_len = replacement.chars().count();
        let changes = ChangeSet::single(Change::new(start, end, replacement));
        let transaction = Transaction {
            changes,
            selections: Selections::cursor(start),
            kind: EditKind::Other,
            auto_closed_offsets: Vec::new(),
        };
        let outcome = self.result.execute(Command::Apply(transaction));
        for changes in &outcome.applied_changes {
            self.map_result_ranges(changes);
        }
        self.result_ranges[region_index] = (start, start + replacement_len);
        self.chosen_resolutions[region_index] = Some(resolution);
    }

    /// Accepts one side for every conflict.
    pub fn accept_all(&mut self, resolution: Resolution) {
        for index in self.conflict_indexes() {
            self.accept(index, resolution);
        }
    }

    /// Moves to the next (or previous) conflict, wrapping, and shows it in the three editors.
    pub fn go_to_conflict(&mut self, forward: bool) {
        let conflicts = self.conflict_indexes();
        if conflicts.is_empty() {
            return;
        }
        let next = match self.current_conflict.and_then(|current| conflicts.iter().position(|&index| index == current))
        {
            Some(position) if forward => conflicts[(position + 1) % conflicts.len()],
            Some(position) => conflicts[(position + conflicts.len() - 1) % conflicts.len()],
            None => conflicts[0],
        };
        self.current_conflict = Some(next);
        self.reveal_region(next);
    }

    fn reveal_region(&mut self, region_index: usize) {
        let region = self.regions[region_index];
        let (result_start, _) = self.result_ranges[region_index];
        let ours_offset = self.ours.text().line_start(region.ours.start);
        let theirs_offset = self.theirs.text().line_start(region.theirs.start);
        self.ours.execute(Command::SetSelections(Selections::cursor(ours_offset)));
        self.theirs.execute(Command::SetSelections(Selections::cursor(theirs_offset)));
        self.result.execute(Command::SetSelections(Selections::cursor(result_start)));
        self.ours_view.reveal_cursor();
        self.theirs_view.reveal_cursor();
        self.result_view.reveal_cursor();
    }

    fn map_result_ranges(&mut self, changes: &ChangeSet) {
        for range in &mut self.result_ranges {
            let start = changes.map_offset(range.0, Bias::Before);
            *range = (start, changes.map_offset(range.1, Bias::Before).max(start));
        }
    }

    /// First input row of each region (both inputs have one row per region line, padded).
    fn region_first_rows(&self) -> Vec<usize> {
        let mut first_rows = Vec::with_capacity(self.regions.len());
        let mut row = 0;
        for region in &self.regions {
            first_rows.push(row);
            row += region.ours.len().max(region.theirs.len());
        }
        first_rows
    }

    /// Keeps the result scrolled to the region the inputs show, or the other way round,
    /// following whichever the user scrolled.
    fn sync_result_scroll(&mut self, row_height: f32) -> bool {
        let inputs_y = self.ours_view.scroll_offset.y;
        let result_y = self.result_view.scroll_offset.y;
        let (synced_inputs_y, synced_result_y) = self.synced_scroll;
        let first_rows = self.region_first_rows();
        let text = self.result.text().clone();
        let result_line_of_region = |index: usize| text.line_of_offset(self.result_ranges[index].0);
        if inputs_y != synced_inputs_y {
            let top_row = (inputs_y / row_height).floor() as usize;
            let region = first_rows.partition_point(|&first_row| first_row <= top_row).saturating_sub(1);
            let (start, end) = self.result_ranges[region];
            let region_lines = text.line_of_offset(end.max(start)) - text.line_of_offset(start) + 1;
            let line =
                result_line_of_region(region) + (top_row - first_rows[region]).min(region_lines.saturating_sub(1));
            self.result_view.scroll_offset.y = line as f32 * row_height + inputs_y.rem_euclid(row_height);
        } else if result_y != synced_result_y {
            let top_line = (result_y / row_height).floor() as usize;
            let region =
                (0..self.regions.len()).rev().find(|&index| result_line_of_region(index) <= top_line).unwrap_or(0);
            let row = first_rows[region] + (top_line - result_line_of_region(region));
            let input_y = row as f32 * row_height + result_y.rem_euclid(row_height);
            self.ours_view.scroll_offset.y = input_y;
            self.theirs_view.scroll_offset.y = input_y;
        } else {
            return false;
        }
        self.synced_scroll = (self.ours_view.scroll_offset.y, self.result_view.scroll_offset.y);
        true
    }

    /// The region holding a result offset.
    fn region_at_result_offset(&self, offset: usize) -> Option<usize> {
        self.result_ranges.iter().position(|&(start, end)| start <= offset && offset < end.max(start + 1))
    }
}

/// Aligned rows of the two inputs: each region takes as many rows as its longer side.
fn input_layouts(document: &MergeDocument, theme: &EditorTheme) -> (DisplayLayout, DisplayLayout) {
    let change = to_rgba(theme.merge_change_background);
    let conflict = to_rgba(theme.merge_conflict_background);
    let mut ours_rows = Vec::new();
    let mut theirs_rows = Vec::new();
    for region in &document.regions {
        let height = region.ours.len().max(region.theirs.len());
        let (ours_background, theirs_background, marker) = match region.kind {
            RegionKind::Unchanged => (None, None, None),
            RegionKind::Ours => (Some(change), None, Some('+')),
            RegionKind::Theirs => (None, Some(change), Some('+')),
            RegionKind::Both => (Some(change), Some(change), Some('=')),
            RegionKind::Conflict => (Some(conflict), Some(conflict), Some('!')),
        };
        let side_row = |line: Option<usize>, background: Option<rustext_core::style::Rgba>| match line {
            None => DisplayRow::new(RowContent::Padding),
            Some(line) => {
                let row = DisplayRow::new(RowContent::Line(line));
                match (background, marker) {
                    (Some(background), Some(marker)) => row.with_background(background).with_marker(marker),
                    _ => row,
                }
            }
        };
        for offset in 0..height {
            ours_rows
                .push(side_row((offset < region.ours.len()).then_some(region.ours.start + offset), ours_background));
            theirs_rows.push(side_row(
                (offset < region.theirs.len()).then_some(region.theirs.start + offset),
                theirs_background,
            ));
        }
    }
    for (rows, text) in [(&mut ours_rows, document.ours.text()), (&mut theirs_rows, document.theirs.text())] {
        if text.line_len(text.last_line()) == 0 {
            rows.push(DisplayRow::new(RowContent::Line(text.last_line())));
        }
    }
    let ours_layout = DisplayLayout::new(ours_rows, document.ours.text().line_count());
    let theirs_layout = DisplayLayout::new(theirs_rows, document.theirs.text().line_count());
    (ours_layout, theirs_layout)
}

/// Backgrounds of the regions in the result: changes taken, conflicts open or resolved.
fn result_decorations(document: &MergeDocument, theme: &EditorTheme) -> Vec<Decoration> {
    let text = document.result.text();
    document
        .regions
        .iter()
        .zip(&document.result_ranges)
        .zip(&document.chosen_resolutions)
        .filter_map(|((region, &(start, end)), resolution)| {
            let color = match (region.kind, resolution) {
                (RegionKind::Unchanged, _) => return None,
                (RegionKind::Conflict, None) => theme.merge_conflict_background,
                (RegionKind::Conflict, Some(_)) => theme.merge_resolved_background,
                _ => theme.merge_change_background,
            };
            let last_offset = end.max(start + 1) - 1;
            let (first_line, last_line) = (text.line_of_offset(start), text.line_of_offset(last_offset.max(start)));
            Some(Decoration::lines(text, first_line, last_line, DecorationKind::WholeLineBackground(to_rgba(color))))
        })
        .collect()
}

/// What the merge editor did this frame.
pub struct MergeResponse {
    pub result_changed: bool,
}

pub struct MergeEditor<'a> {
    document: &'a mut MergeDocument,
    view_config: &'a ViewConfig,
    theme: &'a EditorTheme,
    keymap: &'a Keymap,
    stylers: Option<[&'a mut dyn LineStyler; 3]>,
    labels: (&'a str, &'a str),
}

impl<'a> MergeEditor<'a> {
    pub fn new(
        document: &'a mut MergeDocument,
        view_config: &'a ViewConfig,
        theme: &'a EditorTheme,
        keymap: &'a Keymap,
    ) -> Self {
        Self { document, view_config, theme, keymap, stylers: None, labels: ("Ours", "Theirs") }
    }

    /// Highlighters for ours, theirs and the result.
    pub fn stylers(mut self, stylers: [&'a mut dyn LineStyler; 3]) -> Self {
        self.stylers = Some(stylers);
        self
    }

    /// Titles of the two inputs (`HEAD`, a branch name…).
    pub fn labels(mut self, ours: &'a str, theirs: &'a str) -> Self {
        self.labels = (ours, theirs);
        self
    }

    #[expect(clippy::too_many_lines, reason = "predates the complexity lints (D28)")]
    pub fn show(self, ui: &mut Ui) -> MergeResponse {
        let Self { document, view_config, theme, keymap, stylers, labels } = self;
        let [mut ours_styler, mut theirs_styler, mut result_styler] = match stylers {
            Some([ours, theirs, result]) => [Some(ours), Some(theirs), Some(result)],
            None => [None, None, None],
        };

        show_toolbar(ui, document);
        let rect = ui.available_rect_before_wrap();
        let header_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        let inputs_bottom = rect.top() + rect.height() * 0.5;
        let split_x = rect.center().x;
        let ours_rect =
            Rect::from_min_max(pos2(rect.left(), rect.top() + header_height), pos2(split_x - 1.0, inputs_bottom));
        let theirs_rect =
            Rect::from_min_max(pos2(split_x + 1.0, rect.top() + header_height), pos2(rect.right(), inputs_bottom));
        let result_rect = Rect::from_min_max(pos2(rect.left(), inputs_bottom + header_height), rect.right_bottom());

        let painter = ui.painter();
        let header_font = egui::TextStyle::Body.resolve(ui.style());
        let header_color = theme.foreground;
        painter.text(
            pos2(rect.left() + 6.0, rect.top() + 3.0),
            egui::Align2::LEFT_TOP,
            labels.0,
            header_font.clone(),
            header_color,
        );
        painter.text(
            pos2(split_x + 6.0, rect.top() + 3.0),
            egui::Align2::LEFT_TOP,
            labels.1,
            header_font.clone(),
            header_color,
        );
        painter.text(
            pos2(rect.left() + 6.0, inputs_bottom + 3.0),
            egui::Align2::LEFT_TOP,
            "Result",
            header_font,
            header_color,
        );
        painter.vline(split_x, ours_rect.top()..=inputs_bottom, Stroke::new(2.0, theme.widget_border));
        painter.hline(rect.x_range(), inputs_bottom + 1.0, Stroke::new(2.0, theme.widget_border));

        let (ours_layout, theirs_layout) = input_layouts(document, theme);
        let shared_scroll = document.ours_view.scroll_offset;
        document.theirs_view.scroll_offset = shared_scroll;

        let ours_response = ui
            .scope_builder(UiBuilder::new().max_rect(ours_rect), |ui| {
                let mut editor =
                    CodeEditor::new(&mut document.ours, &mut document.ours_view, view_config, theme, keymap)
                        .id_salt("rustext-merge-ours")
                        .aria_label("Ours")
                        .display_layout(&ours_layout)
                        .markers(true);
                if let Some(styler) = ours_styler.take() {
                    editor = editor.styler(styler);
                }
                editor.show(ui)
            })
            .inner;
        let theirs_response = ui
            .scope_builder(UiBuilder::new().max_rect(theirs_rect), |ui| {
                let mut editor =
                    CodeEditor::new(&mut document.theirs, &mut document.theirs_view, view_config, theme, keymap)
                        .id_salt("rustext-merge-theirs")
                        .aria_label("Theirs")
                        .display_layout(&theirs_layout)
                        .markers(true);
                if let Some(styler) = theirs_styler.take() {
                    editor = editor.styler(styler);
                }
                editor.show(ui)
            })
            .inner;
        if document.theirs_view.scroll_offset != shared_scroll {
            document.ours_view.scroll_offset = document.theirs_view.scroll_offset;
            ui.ctx().request_repaint();
        } else if document.ours_view.scroll_offset != shared_scroll {
            ui.ctx().request_repaint();
        }

        let decorations = result_decorations(document, theme);
        let result_response = ui
            .scope_builder(UiBuilder::new().max_rect(result_rect), |ui| {
                let mut editor =
                    CodeEditor::new(&mut document.result, &mut document.result_view, view_config, theme, keymap)
                        .id_salt("rustext-merge-result")
                        .aria_label("Result")
                        .decorations(&decorations);
                if let Some(styler) = result_styler.take() {
                    editor = editor.styler(styler);
                }
                editor.show(ui)
            })
            .inner;
        ui.advance_cursor_after_rect(rect);

        for changes in &result_response.outcome.applied_changes {
            document.map_result_ranges(changes);
        }
        if result_response.pressed.is_some() {
            let head = document.result.selections().primary().head;
            if let Some(region) = document
                .region_at_result_offset(head)
                .filter(|&index| document.regions[index].kind == RegionKind::Conflict)
            {
                document.current_conflict = Some(region);
            }
        }
        for (response, is_ours) in [(&ours_response, true), (&theirs_response, false)] {
            if response.pressed.is_none() {
                continue;
            }
            let state = if is_ours { &document.ours } else { &document.theirs };
            let line = state.text().line_of_offset(state.selections().primary().head);
            let region = document.regions.iter().position(|region| {
                let range = if is_ours { region.ours } else { region.theirs };
                region.kind == RegionKind::Conflict && (range.contains(line) || range.is_empty() && range.start == line)
            });
            if region.is_some() {
                document.current_conflict = region;
            }
        }
        let row_height = document.result_view.geometry().map_or(0.0, |geometry| geometry.row_height);
        if row_height > 0.0 && document.sync_result_scroll(row_height) {
            ui.ctx().request_repaint();
        }
        MergeResponse { result_changed: result_response.outcome.text_changed() }
    }
}

fn show_toolbar(ui: &mut Ui, document: &mut MergeDocument) {
    ui.horizontal(|ui| {
        let conflicts = document.conflict_indexes();
        let unresolved = document.unresolved_conflict_count();
        let position =
            document.current_conflict.and_then(|current| conflicts.iter().position(|&index| index == current));
        match position {
            Some(position) => {
                ui.label(format!("Conflict {} of {} ({unresolved} unresolved)", position + 1, conflicts.len()))
            }
            None if conflicts.is_empty() => ui.label("No conflicts"),
            None => ui.label(format!("{} conflicts ({unresolved} unresolved)", conflicts.len())),
        };
        if ui.button("Previous").clicked() {
            document.go_to_conflict(false);
        }
        if ui.button("Next").clicked() {
            document.go_to_conflict(true);
        }
        ui.separator();
        if let Some(current) = document.current_conflict {
            let choices = [
                ("Accept ours", Resolution::Ours),
                ("Accept theirs", Resolution::Theirs),
                ("Ours then theirs", Resolution::OursThenTheirs),
                ("Theirs then ours", Resolution::TheirsThenOurs),
                ("Base", Resolution::Base),
            ];
            for (label, resolution) in choices {
                let is_chosen = document.resolution(current) == Some(resolution);
                if ui.selectable_label(is_chosen, label).clicked() {
                    document.accept(current, resolution);
                }
            }
        }
        ui.separator();
        if ui.button("All ours").clicked() {
            document.accept_all(Resolution::Ours);
        }
        if ui.button("All theirs").clicked() {
            document.accept_all(Resolution::Theirs);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepting_resolutions_rewrites_the_result() {
        let mut document = MergeDocument::new("a\nx\nb\ny\nc\n", "a\nX1\nb\ny\nc\n", "a\nX2\nb\nY\nc\n");
        assert_eq!(document.result_text(), "a\nx\nb\nY\nc\n");
        assert_eq!(document.unresolved_conflict_count(), 1);

        let conflict = document.conflict_indexes()[0];
        document.accept(conflict, Resolution::OursThenTheirs);
        assert_eq!(document.result_text(), "a\nX1\nX2\nb\nY\nc\n");
        assert_eq!(document.unresolved_conflict_count(), 0);

        document.accept(conflict, Resolution::Theirs);
        assert_eq!(document.result_text(), "a\nX2\nb\nY\nc\n");
    }

    #[test]
    fn regions_follow_manual_edits_of_the_result() {
        let mut document = MergeDocument::new("top\nx\n", "top\nours\n", "top\ntheirs\n");
        document.result.execute(Command::SetSelections(Selections::cursor(0)));
        let outcome = document.result.execute(Command::Type("// note\n".into()));
        for changes in &outcome.applied_changes {
            document.map_result_ranges(changes);
        }

        let conflict = document.conflict_indexes()[0];
        document.accept(conflict, Resolution::Ours);

        assert_eq!(document.result_text(), "// note\ntop\nours\n");
    }
}
