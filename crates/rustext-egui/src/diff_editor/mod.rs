//! The `DiffEditor` widget: two code editors side by side with aligned rows, or one editor
//! showing removed lines inline, built on display rows (DECISIONS D17). Monaco's
//! `createDiffEditor`.
//!
//! The application owns a `DiffDocument` (both texts, their views, the diff) and shows a
//! `DiffEditor` over it every frame.

use std::collections::BTreeSet;

use egui::{Rect, Stroke, Ui, UiBuilder, pos2};
use rustext_core::decoration::{Decoration, DecorationKind};
use rustext_core::display::DisplayLayout;
use rustext_core::services::CodeLens;
use rustext_core::style::LineStyler;
use rustext_core::{Command, EditingConfig, EditorState, Selection, Selections, Text};
use rustext_diff::{
    CollapseOptions, Collapsing, DiffColors, DiffOptions, LineDiff, LineSelection, RowKind, WrapPoints, diff_lines,
    inline_layout, inner_change_decorations, move_code_lenses, side_by_side_layouts, side_by_side_rows,
};

use crate::code_editor::{CodeEditor, EditorResponse, EditorView, Foreign, MouseTarget, ViewConfig, to_rgba};
use crate::keymap::{Action, Keymap};
use crate::theme::EditorTheme;

mod accessible_viewer;
mod document;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DiffViewMode {
    #[default]
    SideBySide,
    Inline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiffSide {
    Old,
    New,
}

/// Options of a diff editor (Monaco's `IDiffEditorOptions`).
#[derive(Clone, Debug, PartialEq)]
pub struct DiffViewConfig {
    pub mode: DiffViewMode,
    /// Collapses unchanged regions (Monaco's `hideUnchangedRegions`); `None` shows everything.
    pub collapse_unchanged: Option<CollapseOptions>,
    pub diff_options: DiffOptions,
    /// Lets the old side be edited (Monaco's `originalEditable`).
    pub old_editable: bool,
    /// Below this width, a side-by-side view shows inline instead.
    pub inline_below_width: f32,
    /// Share of the width the old side takes in a side-by-side view, until the user drags the
    /// separator (the dragged ratio is kept in the `DiffDocument`).
    pub split_ratio: f32,
}

impl Default for DiffViewConfig {
    fn default() -> Self {
        Self {
            mode: DiffViewMode::default(),
            collapse_unchanged: None,
            diff_options: DiffOptions::default(),
            old_editable: false,
            inline_below_width: 700.0,
            split_ratio: 0.5,
        }
    }
}

/// Rows and decorations derived from the diff, rebuilt when the diff or how it is shown changes.
struct DiffDisplay {
    /// What the layouts were built from: the mode, the collapsing (expanded regions and folds
    /// included), the colors, and the wrap generations of both sides.
    key: (DiffViewMode, Collapsing, DiffColors, (u64, u64)),
    old_layout: DisplayLayout,
    new_layout: DisplayLayout,
    inline_layout: DisplayLayout,
    old_decorations: Vec<Decoration>,
    new_decorations: Vec<Decoration>,
    /// "Moved to line N" above each moved block of the old side, "Moved from line N" of the
    /// new side, in the order of the diff's moves.
    old_move_lenses: Vec<CodeLens>,
    new_move_lenses: Vec<CodeLens>,
}

/// Both texts of a diff, their views, and the diff between them.
pub struct DiffDocument {
    pub old: EditorState,
    pub new: EditorState,
    pub old_view: EditorView,
    pub new_view: EditorView,
    diff: LineDiff,
    diff_options: DiffOptions,
    is_diff_stale: bool,
    expanded_old_lines: Vec<usize>,
    display: Option<DiffDisplay>,
    focused_side: DiffSide,
    /// Removed lines picked in an inline view (they are not text of the editor shown).
    selected_removed_lines: BTreeSet<usize>,
    last_clicked_removed_line: Option<usize>,
    /// The split the user dragged to; the config's ratio until then.
    pub dragged_split_ratio: Option<f32>,
    /// The accessible diff viewer (Alt+F2) is open: the change at the cursor, line by line.
    pub shows_accessible_viewer: bool,
}

/// What a diff editor did this frame.
pub struct DiffResponse {
    /// The old side's editor; `None` in an inline view.
    pub old: Option<EditorResponse>,
    pub new: EditorResponse,
    /// The diff was computed again (a side was edited, or new texts were set).
    pub diff_changed: bool,
    pub mode_shown: DiffViewMode,
}

pub struct DiffEditor<'a> {
    document: &'a mut DiffDocument,
    config: &'a DiffViewConfig,
    view_config: &'a ViewConfig,
    theme: &'a EditorTheme,
    keymap: &'a Keymap,
    old_styler: Option<&'a mut dyn LineStyler>,
    new_styler: Option<&'a mut dyn LineStyler>,
}

impl<'a> DiffEditor<'a> {
    pub fn new(
        document: &'a mut DiffDocument,
        config: &'a DiffViewConfig,
        view_config: &'a ViewConfig,
        theme: &'a EditorTheme,
        keymap: &'a Keymap,
    ) -> Self {
        Self { document, config, view_config, theme, keymap, old_styler: None, new_styler: None }
    }

    pub fn stylers(mut self, old_styler: &'a mut dyn LineStyler, new_styler: &'a mut dyn LineStyler) -> Self {
        self.old_styler = Some(old_styler);
        self.new_styler = Some(new_styler);
        self
    }

    fn colors(&self) -> DiffColors {
        DiffColors {
            removed_line: to_rgba(self.theme.removed_line_background),
            added_line: to_rgba(self.theme.inserted_line_background),
            removed_text: to_rgba(self.theme.removed_text_background),
            added_text: to_rgba(self.theme.inserted_text_background),
            moved_line: to_rgba(self.theme.moved_line_background),
        }
    }

    fn ensure_display(&mut self, mode: DiffViewMode) {
        let colors = self.colors();
        let document = &mut *self.document;
        let wrap_generations = (document.old_view.wraps.generation(), document.new_view.wraps.generation());
        let collapsing = document.collapsing(self.config.collapse_unchanged);
        let key = (mode, collapsing, colors, wrap_generations);
        if document.display.as_ref().is_some_and(|display| display.key == key) {
            return;
        }
        let collapsing = &key.1;
        let (old_text, new_text) = (document.old.text(), document.new.text());
        let wraps = WrapPoints { old: document.old_view.wraps.all_points(), new: document.new_view.wraps.all_points() };
        let (old_layout, new_layout) =
            side_by_side_layouts(&document.diff, old_text, new_text, collapsing, &colors, &wraps);
        let inline = inline_layout(&document.diff, new_text, collapsing, &colors, wraps.new);
        let (old_decorations, new_decorations) = inner_change_decorations(&document.diff, old_text, new_text, &colors);
        let (old_move_lenses, new_move_lenses) = move_code_lenses(&document.diff);
        document.display = Some(DiffDisplay {
            key,
            old_layout,
            new_layout,
            inline_layout: inline,
            old_decorations,
            new_decorations,
            old_move_lenses,
            new_move_lenses,
        });
    }

    pub fn show(mut self, ui: &mut Ui) -> DiffResponse {
        self.document.old.config.read_only = !self.config.old_editable;
        self.document.refresh_diff(&self.config.diff_options);
        let rect = ui.available_rect_before_wrap();
        let mode = if self.config.mode == DiffViewMode::SideBySide && rect.width() < self.config.inline_below_width {
            DiffViewMode::Inline
        } else {
            self.config.mode
        };
        self.ensure_display(mode);

        let response = match mode {
            DiffViewMode::SideBySide => self.show_side_by_side(ui, rect),
            DiffViewMode::Inline => self.show_inline(ui, rect),
        };
        // The editors wrapped their lines, or folded a block, this frame: lay the rows out with
        // it next frame.
        let document = &*self.document;
        let wrap_generations = (document.old_view.wraps.generation(), document.new_view.wraps.generation());
        let collapsing = document.collapsing(self.config.collapse_unchanged);
        if document
            .display
            .as_ref()
            .is_some_and(|display| display.key.3 != wrap_generations || display.key.1 != collapsing)
        {
            ui.ctx().request_repaint();
        }
        self.show_accessible_viewer(ui, rect);
        ui.advance_cursor_after_rect(rect);
        response
    }

    #[expect(clippy::too_many_lines, reason = "predates the complexity lints (D28)")]
    fn show_side_by_side(&mut self, ui: &mut Ui, rect: Rect) -> DiffResponse {
        let ratio = self.document.dragged_split_ratio.unwrap_or(self.config.split_ratio).clamp(0.1, 0.9);
        let split_x = rect.left() + rect.width() * ratio;
        let handle = Rect::from_center_size(pos2(split_x, rect.center().y), egui::vec2(6.0, rect.height()));
        let handle_response = ui.interact(handle, ui.make_persistent_id("rustext-diff-split"), egui::Sense::drag());
        if handle_response.hovered() || handle_response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if let Some(pointer) = handle_response.dragged().then(|| handle_response.interact_pointer_pos()).flatten() {
            self.document.dragged_split_ratio = Some(((pointer.x - rect.left()) / rect.width()).clamp(0.1, 0.9));
        }
        let old_rect = Rect::from_min_max(rect.left_top(), pos2(split_x - 1.0, rect.bottom()));
        let new_rect = Rect::from_min_max(pos2(split_x + 1.0, rect.top()), rect.right_bottom());
        let document = &mut *self.document;
        let display = document.display.as_ref().expect("display built before showing");

        let shared_scroll = document.new_view.scroll_offset;
        document.old_view.scroll_offset = shared_scroll;
        let old_response = ui
            .scope_builder(UiBuilder::new().max_rect(old_rect), |ui| {
                let mut editor = CodeEditor::new(
                    &mut document.old,
                    &mut document.old_view,
                    self.view_config,
                    self.theme,
                    self.keymap,
                )
                .id_salt("rustext-diff-old")
                .aria_label("Original")
                .wrap_under_given_layout()
                .fold_under_given_layout()
                .display_layout(&display.old_layout)
                .decorations(&display.old_decorations)
                .code_lenses(&display.old_move_lenses)
                .markers(true);
                if let Some(styler) = self.old_styler.as_deref_mut() {
                    editor = editor.styler(styler);
                }
                editor.show(ui)
            })
            .inner;
        if document.old_view.scroll_offset != shared_scroll {
            document.new_view.scroll_offset = document.old_view.scroll_offset;
            ui.ctx().request_repaint();
        }
        let new_response = ui
            .scope_builder(UiBuilder::new().max_rect(new_rect), |ui| {
                let mut editor = CodeEditor::new(
                    &mut document.new,
                    &mut document.new_view,
                    self.view_config,
                    self.theme,
                    self.keymap,
                )
                .id_salt("rustext-diff-new")
                .aria_label("Modified")
                .wrap_under_given_layout()
                .fold_under_given_layout()
                .display_layout(&display.new_layout)
                .decorations(&display.new_decorations)
                .code_lenses(&display.new_move_lenses)
                .markers(true);
                if let Some(styler) = self.new_styler.as_deref_mut() {
                    editor = editor.styler(styler);
                }
                editor.show(ui)
            })
            .inner;
        if document.new_view.scroll_offset != shared_scroll {
            document.old_view.scroll_offset = document.new_view.scroll_offset;
            ui.ctx().request_repaint();
        }
        ui.painter().vline(split_x, rect.y_range(), Stroke::new(2.0, self.theme.widget_border));

        self.after_show(Some(old_response), new_response, DiffViewMode::SideBySide, ui)
    }

    fn show_inline(&mut self, ui: &mut Ui, rect: Rect) -> DiffResponse {
        let document = &mut *self.document;
        let display = document.display.as_ref().expect("display built before showing");
        let selected_removed: Vec<Decoration> = document
            .selected_removed_lines
            .iter()
            .filter(|&&line| line < document.old.text().line_count())
            .map(|&line| {
                Decoration::lines(
                    document.old.text(),
                    line,
                    line,
                    DecorationKind::WholeLineBackground(to_rgba(self.theme.selection_background)),
                )
            })
            .collect();
        let foreign_decorations: Vec<Decoration> =
            display.old_decorations.iter().cloned().chain(selected_removed).collect();

        let new_response = ui
            .scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                let foreign = Foreign {
                    text: document.old.text(),
                    styler: self.old_styler.as_deref_mut().map(|styler| styler as &mut dyn LineStyler),
                    decorations: &foreign_decorations,
                };
                let mut editor = CodeEditor::new(
                    &mut document.new,
                    &mut document.new_view,
                    self.view_config,
                    self.theme,
                    self.keymap,
                )
                .id_salt("rustext-diff-new")
                .aria_label("Modified")
                .wrap_under_given_layout()
                .fold_under_given_layout()
                .display_layout(&display.inline_layout)
                .decorations(&display.new_decorations)
                .code_lenses(&display.new_move_lenses)
                .foreign(foreign)
                .markers(true);
                if let Some(styler) = self.new_styler.as_deref_mut() {
                    editor = editor.styler(styler);
                }
                editor.show(ui)
            })
            .inner;
        self.after_show(None, new_response, DiffViewMode::Inline, ui)
    }

    fn after_show(
        &mut self,
        old: Option<EditorResponse>,
        new: EditorResponse,
        mode: DiffViewMode,
        ui: &Ui,
    ) -> DiffResponse {
        let document = &mut *self.document;
        let shift = ui.input(|input| input.modifiers.shift);
        if old
            .as_ref()
            .is_some_and(|old| old.pressed.is_some() || old.response.has_focus() && old.outcome.selections_changed)
        {
            document.focused_side = DiffSide::Old;
        }
        if new.pressed.is_some() || new.response.has_focus() && new.outcome.selections_changed {
            document.focused_side = DiffSide::New;
            if !matches!(new.pressed, Some(MouseTarget::Foreign { .. })) && new.pressed.is_some() {
                document.selected_removed_lines.clear();
            }
        }

        let pressed = [(DiffSide::Old, old.as_ref().and_then(|old| old.pressed)), (DiffSide::New, new.pressed)];
        for (side, target) in pressed {
            match target {
                Some(MouseTarget::Collapsed { first_line, .. }) => document.expand_collapsed(side, first_line),
                Some(MouseTarget::Foreign { line }) => document.click_removed_line(line, shift),
                _ => {}
            }
        }

        if let Some(index) = old.as_ref().and_then(|old| old.clicked_code_lens) {
            document.go_to_move(index, DiffSide::New);
        }
        if let Some(index) = new.clicked_code_lens.filter(|_| mode == DiffViewMode::SideBySide) {
            document.go_to_move(index, DiffSide::Old);
        }

        let actions = old.iter().flat_map(|old| old.unhandled_actions.iter()).chain(new.unhandled_actions.iter());
        for action in actions.cloned().collect::<Vec<_>>() {
            match action {
                Action::NextChange => document.go_to_change(true),
                Action::PreviousChange => document.go_to_change(false),
                Action::AccessibleDiffViewer => document.shows_accessible_viewer = true,
                _ => {}
            }
        }

        let text_changed = new.outcome.text_changed() || old.as_ref().is_some_and(|old| old.outcome.text_changed());
        if text_changed {
            document.is_diff_stale = true;
            document.refresh_diff(&self.config.diff_options);
            ui.ctx().request_repaint();
        }
        DiffResponse { old, new, diff_changed: text_changed, mode_shown: mode }
    }
}

/// Selects whole lines `first_line..=last_line` of a state (for tests and applications that
/// pick lines programmatically).
pub fn select_lines(state: &mut EditorState, first_line: usize, last_line: usize) {
    let text = state.text();
    let selection = Selection::new(text.line_start(first_line), text.line_content_end(last_line));
    state.execute(Command::SetSelections(Selections::single(selection)));
}
