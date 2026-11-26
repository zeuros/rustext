//! Turning egui input into commands: pointer, keyboard (through the keymap), clipboard, IME,
//! wheel.

use egui::{Event, ImeEvent, Key, Modifiers, PointerButton, Pos2, Response, Ui, Vec2, vec2};
use rustext_core::cursors::{VerticalDirection, word_range_at};
use rustext_core::display::RowContent;
use rustext_core::{Command, Motion, Outcome, Position, Selection, Selections};

use super::{CodeEditor, Drag, DragUnit, Metrics, MouseTarget, whole_line};
use crate::keymap::{Action, KeyResolution, Keymap};

impl CodeEditor<'_> {
    /// The text position nearest to `pos`, and whether `pos` is past the end of its line.
    fn position_at(&mut self, ui: &Ui, metrics: &Metrics, pos: Pos2) -> (Position, bool) {
        let row = self.row_at_y(metrics, pos.y);
        let Some((line, start_column, end_column)) = self.row_segment(row) else {
            return (Position::new(self.nearest_line(row), 0), true);
        };
        let is_below_text = pos.y > self.row_top(metrics, row) + metrics.row_height;
        let layout = self.layout_segment(ui, metrics, line, start_column, end_column);
        let x = pos.x - self.text_left(metrics);
        let column = layout.column_at_x(x);
        (Position::new(line, column), x > layout.width() || is_below_text)
    }

    /// Char offset of the char under `pos` (not the nearest boundary), if `pos` is over text.
    pub(super) fn char_under(&mut self, ui: &Ui, metrics: &Metrics, pos: Pos2) -> Option<usize> {
        let row = self.row_at_y(metrics, pos.y);
        let (line, start_column, end_column) = self.row_segment(row)?;
        let layout = self.layout_segment(ui, metrics, line, start_column, end_column);
        let x = pos.x - self.text_left(metrics);
        let nearest_column = layout.column_at_x(x);
        let column = if nearest_column > 0 && x < layout.x_of_column(nearest_column) {
            nearest_column - 1
        } else {
            nearest_column
        };
        (column < self.state.text().line_len(line)).then(|| self.state.text().line_start(line) + column)
    }

    pub(super) fn target_at(&mut self, ui: &Ui, metrics: &Metrics, pos: Pos2) -> MouseTarget {
        if pos.x >= metrics.scrollbar_rect.left() {
            return MouseTarget::Scrollbar;
        }
        if metrics.minimap_rect.contains(pos) {
            return MouseTarget::Minimap;
        }
        if self.horizontal_scrollbar_track(metrics).is_some_and(|track| track.contains(pos)) {
            return MouseTarget::HorizontalScrollbar;
        }
        let sticky_slot = ((pos.y - metrics.text_rect.top()) / metrics.row_height).floor();
        let sticky_line =
            (sticky_slot >= 0.0).then(|| self.view.sticky_lines.get(sticky_slot as usize).copied()).flatten();
        if let Some(line) = sticky_line {
            return MouseTarget::StickyLine { line };
        }
        let row = self.row_at_y(metrics, pos.y);
        match self.row_content(row) {
            RowContent::Collapsed { first_line, last_line } => return MouseTarget::Collapsed { first_line, last_line },
            RowContent::Foreign(line) => return MouseTarget::Foreign { line },
            RowContent::Padding => return MouseTarget::Padding,
            RowContent::CodeLens(line) => {
                return self
                    .code_lens_at(metrics, line, pos.x)
                    .map_or(MouseTarget::Padding, |index| MouseTarget::CodeLens { index });
            }
            RowContent::Line(_) | RowContent::Wrapped { .. } => {}
        }
        let line = self.nearest_line(row);
        let gutter_left = metrics.gutter_rect().left();
        if pos.x < gutter_left + metrics.glyph_margin_width {
            return MouseTarget::GlyphMargin { line };
        }
        let fold_column_right = metrics.text_rect.left() - metrics.char_width;
        let is_in_fold_column = (fold_column_right - metrics.fold_marker_width..fold_column_right).contains(&pos.x);
        if is_in_fold_column && self.is_fold_header(line) {
            return MouseTarget::FoldMarker { line };
        }
        if pos.x < metrics.text_rect.left() {
            return MouseTarget::LineNumber { line };
        }
        let (position, is_past_line_end) = self.position_at(ui, metrics, pos);
        if is_past_line_end { MouseTarget::Empty { position } } else { MouseTarget::Text { position } }
    }

    fn execute(&mut self, outcome: &mut Outcome, command: Command) {
        let result = self.state.execute(command);
        *outcome = std::mem::take(outcome).merged(result);
    }

    pub(super) fn handle_pointer(
        &mut self,
        ui: &Ui,
        metrics: &Metrics,
        response: &Response,
        outcome: &mut Outcome,
    ) -> Option<MouseTarget> {
        let (pointer_pos, modifiers, primary_pressed, primary_down, time) = ui.input(|input| {
            (
                input.pointer.interact_pos(),
                input.modifiers,
                input.pointer.button_pressed(PointerButton::Primary),
                input.pointer.button_down(PointerButton::Primary),
                input.time,
            )
        });
        let pointer_pos = pointer_pos?;

        let mut pressed_target = None;
        if primary_pressed && metrics.full_rect().contains(pointer_pos) && response.hovered() {
            let target = self.target_at(ui, metrics, pointer_pos);
            pressed_target = Some(target);
            let click_count = self.click_count(time, pointer_pos);
            self.press(ui, metrics, target, pointer_pos, click_count, modifiers, outcome);
        }

        if primary_down {
            self.drag_to(ui, metrics, pointer_pos, outcome);
        } else {
            self.view.drag = None;
        }
        pressed_target
    }

    fn click_count(&mut self, time: f64, pos: Pos2) -> u8 {
        const MULTI_CLICK_SECONDS: f64 = 0.4;
        const MULTI_CLICK_DISTANCE: f32 = 4.0;
        let count = match self.view.last_press {
            Some((last_time, last_pos, last_count))
                if time - last_time < MULTI_CLICK_SECONDS && last_pos.distance(pos) < MULTI_CLICK_DISTANCE =>
            {
                last_count % 3 + 1
            }
            _ => 1,
        };
        self.view.last_press = Some((time, pos, count));
        count
    }

    #[allow(clippy::too_many_arguments)]
    fn press(
        &mut self,
        ui: &Ui,
        metrics: &Metrics,
        target: MouseTarget,
        pos: Pos2,
        click_count: u8,
        modifiers: Modifiers,
        outcome: &mut Outcome,
    ) {
        let text = self.state.text().clone();
        match target {
            MouseTarget::Scrollbar => {
                let thumb = self.scrollbar_thumb(metrics);
                let grab_offset_y = if thumb.contains(pos) { pos.y - thumb.top() } else { thumb.height() / 2.0 };
                self.view.drag = Some(Drag::VerticalScrollbar { grab_offset_y });
            }
            MouseTarget::HorizontalScrollbar => {
                if let Some(thumb) = self.horizontal_scrollbar_thumb(metrics) {
                    let grab_offset_x = if thumb.contains(pos) { pos.x - thumb.left() } else { thumb.width() / 2.0 };
                    self.view.drag = Some(Drag::HorizontalScrollbar { grab_offset_x });
                }
            }
            MouseTarget::Collapsed { .. } | MouseTarget::Foreign { .. } | MouseTarget::Padding => {}
            MouseTarget::CodeLens { .. } => {}
            MouseTarget::FoldMarker { line } => self.toggle_fold(line),
            MouseTarget::Minimap => {
                self.scroll_to_minimap_y(metrics, pos.y);
                self.view.drag = Some(Drag::Minimap);
            }
            MouseTarget::StickyLine { line } => {
                self.view.scroll_offset.y = self.row_of_line(line) as f32 * metrics.row_height;
                self.execute(outcome, Command::SetSelections(Selections::cursor(text.line_start(line))));
                self.view.sticky_lines.clear();
            }
            MouseTarget::LineNumber { line } | MouseTarget::GlyphMargin { line } => {
                let (start, end) = whole_line(&text, line);
                self.view.drag = Some(Drag::Selecting {
                    anchor_start: start,
                    anchor_end: end,
                    unit: DragUnit::Line,
                    adds_cursor: false,
                });
                self.execute(outcome, Command::SetSelections(Selections::single(Selection::new(start, end))));
            }
            MouseTarget::Text { .. } | MouseTarget::Empty { .. } => {
                self.view.completion.close();
                let (position, _) = self.position_at(ui, metrics, pos);
                let offset = text.offset(position);
                let is_definition_click = modifiers.command && !modifiers.shift && !modifiers.alt;
                if let Some(url) = self.link_at(offset).filter(|_| is_definition_click) {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    return;
                }
                if is_definition_click && self.services.is_some() {
                    self.request_definition(position);
                }
                if modifiers.shift && modifiers.alt {
                    let anchor = text.position(self.state.selections().primary().anchor);
                    self.view.drag = Some(Drag::Column { anchor });
                    self.drag_to(ui, metrics, pos, outcome);
                    return;
                }
                if modifiers.shift {
                    let anchor = self.state.selections().primary().anchor;
                    self.view.drag = Some(Drag::Selecting {
                        anchor_start: anchor,
                        anchor_end: anchor,
                        unit: DragUnit::Char,
                        adds_cursor: false,
                    });
                    self.execute(outcome, Command::SetSelections(Selections::single(Selection::new(anchor, offset))));
                    return;
                }
                let (unit, (start, end)) = match click_count {
                    2 => (DragUnit::Word, word_range_at(&text, &self.state.config, offset).unwrap_or((offset, offset))),
                    3 => (DragUnit::Line, whole_line(&text, position.line)),
                    _ => (DragUnit::Char, (offset, offset)),
                };
                let adds_cursor = modifiers.alt;
                self.view.drag = Some(Drag::Selecting { anchor_start: start, anchor_end: end, unit, adds_cursor });
                let selection = Selection::new(start, end);
                let selections =
                    if adds_cursor { self.state.selections().added(selection) } else { Selections::single(selection) };
                self.execute(outcome, Command::SetSelections(selections));
            }
        }
    }

    fn drag_to(&mut self, ui: &Ui, metrics: &Metrics, pos: Pos2, outcome: &mut Outcome) {
        let Some(drag) = self.view.drag else { return };
        let text = self.state.text().clone();
        match drag {
            Drag::VerticalScrollbar { grab_offset_y } => {
                let track = metrics.scrollbar_rect;
                let thumb_height = self.scrollbar_thumb(metrics).height();
                let fraction =
                    ((pos.y - grab_offset_y - track.top()) / (track.height() - thumb_height).max(1.0)).clamp(0.0, 1.0);
                self.view.scroll_offset.y = fraction * self.max_scroll_y(metrics);
            }
            Drag::Minimap => self.scroll_to_minimap_y(metrics, pos.y),
            Drag::HorizontalScrollbar { grab_offset_x } => {
                let (Some(track), Some(thumb)) =
                    (self.horizontal_scrollbar_track(metrics), self.horizontal_scrollbar_thumb(metrics))
                else {
                    return;
                };
                let fraction =
                    ((pos.x - grab_offset_x - track.left()) / (track.width() - thumb.width()).max(1.0)).clamp(0.0, 1.0);
                self.view.scroll_offset.x = fraction * self.max_scroll_x(metrics);
            }
            Drag::Column { anchor } => {
                let line = self.nearest_line(self.row_at_y(metrics, pos.y));
                let visual_column = ((pos.x - self.text_left(metrics)) / metrics.char_width).round().max(0.0) as usize;
                self.execute(
                    outcome,
                    Command::ColumnSelect { anchor, head_line: line, head_visual_column: visual_column },
                );
                self.autoscroll_towards(metrics, pos);
            }
            Drag::Selecting { anchor_start, anchor_end, unit, adds_cursor } => {
                let (position, _) = self.position_at(ui, metrics, pos);
                let offset = text.offset(position);
                let (head_start, head_end) = match unit {
                    DragUnit::Char => (offset, offset),
                    DragUnit::Word => word_range_at(&text, &self.state.config, offset).unwrap_or((offset, offset)),
                    DragUnit::Line => whole_line(&text, position.line),
                };
                let selection = if head_start < anchor_start {
                    Selection::new(anchor_end, head_start)
                } else {
                    Selection::new(anchor_start, head_end.max(anchor_end))
                };
                let selections = if adds_cursor {
                    let mut ranges: Vec<Selection> = self
                        .state
                        .selections()
                        .iter()
                        .copied()
                        .filter(|existing| existing.start() != anchor_start && existing.start() != selection.start())
                        .collect();
                    ranges.push(selection);
                    let primary_index = ranges.len() - 1;
                    Selections::new(ranges, primary_index)
                } else {
                    Selections::single(selection)
                };
                if &selections != self.state.selections() {
                    self.execute(outcome, Command::SetSelections(selections));
                }
                self.autoscroll_towards(metrics, pos);
            }
        }
    }

    /// Dragging past the text's edges scrolls towards the pointer.
    fn autoscroll_towards(&mut self, metrics: &Metrics, pos: Pos2) {
        let text_rect = metrics.text_rect;
        if pos.y < text_rect.top() {
            self.view.scroll_offset.y -= (text_rect.top() - pos.y).min(metrics.row_height * 3.0);
        } else if pos.y > text_rect.bottom() {
            self.view.scroll_offset.y += (pos.y - text_rect.bottom()).min(metrics.row_height * 3.0);
        }
        if pos.x > text_rect.right() {
            self.view.scroll_offset.x += (pos.x - text_rect.right()).min(metrics.char_width * 4.0);
        } else if pos.x < text_rect.left() && self.view.scroll_offset.x > 0.0 {
            self.view.scroll_offset.x -= (text_rect.left() - pos.x).min(metrics.char_width * 4.0);
        }
    }

    pub(super) fn handle_keyboard(
        &mut self,
        ui: &Ui,
        metrics: &Metrics,
        outcome: &mut Outcome,
        unhandled_actions: &mut Vec<Action>,
    ) {
        let events = ui.input(|input| input.events.clone());
        for event in events {
            match event {
                Event::Text(typed) => {
                    let printable: String = typed.chars().filter(|character| !character.is_control()).collect();
                    if !printable.is_empty() && self.view.pending_chords.is_empty() {
                        self.typed_this_frame.push_str(&printable);
                        self.execute(outcome, Command::Type(printable));
                    }
                }
                Event::Key { key, pressed: true, modifiers, .. } => {
                    self.key_pressed(metrics, key, modifiers, outcome, unhandled_actions)
                }
                Event::Copy => self.copy(ui, outcome, Command::Copy),
                Event::Cut => self.copy(ui, outcome, Command::Cut),
                Event::Paste(pasted) => self.execute(outcome, Command::Paste(pasted)),
                Event::Ime(ImeEvent::Preedit { text, .. }) => {
                    self.view.ime_preedit = (!text.is_empty()).then_some(text)
                }
                Event::Ime(ImeEvent::Commit(committed)) => {
                    self.view.ime_preedit = None;
                    if !committed.is_empty() {
                        self.execute(outcome, Command::Type(committed));
                    }
                }
                _ => {}
            }
        }
    }

    fn copy(&mut self, ui: &Ui, outcome: &mut Outcome, command: Command) {
        let result = self.state.execute(command);
        if let Some(clipboard) = &result.clipboard {
            ui.ctx().copy_text(clipboard.text.clone());
        }
        *outcome = std::mem::take(outcome).merged(result);
    }

    fn key_pressed(
        &mut self,
        metrics: &Metrics,
        key: Key,
        modifiers: Modifiers,
        outcome: &mut Outcome,
        unhandled_actions: &mut Vec<Action>,
    ) {
        if modifiers.is_none()
            && (self.completion_key(key, outcome)
                || self.references_key(key, outcome)
                || self.ghost_text_key(key, outcome))
        {
            return;
        }
        if key == Key::Escape && modifiers.is_none() && self.close_signature_help() {
            return;
        }
        let has_open_widget = self.view.find.is_open || self.view.go_to_line.is_open;
        if key == Key::Escape && modifiers.is_none() && has_open_widget {
            self.close_find();
            self.view.go_to_line.is_open = false;
            return;
        }
        match self.keymap.resolve(&self.view.pending_chords, modifiers, key) {
            KeyResolution::Pending => self.view.pending_chords.push(Keymap::chord(modifiers, key)),
            KeyResolution::Unbound => self.view.pending_chords.clear(),
            KeyResolution::Action(action) => {
                self.view.pending_chords.clear();
                self.run_action(metrics, action, outcome, unhandled_actions);
            }
        }
    }

    pub(super) fn run_action(
        &mut self,
        metrics: &Metrics,
        action: Action,
        outcome: &mut Outcome,
        unhandled_actions: &mut Vec<Action>,
    ) {
        let lines_per_page =
            ((metrics.text_rect.height() / metrics.row_height).floor() as usize).saturating_sub(1).max(1);
        if let Action::Edit(Command::Move { motion: motion @ (Motion::Up | Motion::Down), extend }) = action
            && self.view.wraps.is_wrapping()
        {
            let moved = self.selections_moved_by_row(motion == Motion::Down, extend);
            self.execute(outcome, Command::SetSelections(moved));
            return;
        }
        match action {
            Action::Edit(command) => self.execute(outcome, command),
            Action::MovePage { direction, extend } => {
                let motion = match direction {
                    VerticalDirection::Up => Motion::PageUp { lines: lines_per_page },
                    VerticalDirection::Down => Motion::PageDown { lines: lines_per_page },
                };
                let page_px = lines_per_page as f32 * metrics.row_height;
                self.view.scroll_offset.y += if direction == VerticalDirection::Up { -page_px } else { page_px };
                self.execute(outcome, Command::Move { motion, extend });
            }
            Action::ScrollLine(direction) => {
                self.view.scroll_offset.y +=
                    if direction == VerticalDirection::Up { -metrics.row_height } else { metrics.row_height };
            }
            Action::ZoomIn => self.view.font_size_delta += 1.0,
            Action::ZoomOut => self.view.font_size_delta -= 1.0,
            Action::ZoomReset => self.view.font_size_delta = 0.0,
            Action::TriggerSuggest => self.view.completion.request(),
            Action::CommandPalette => self.view.palette.open(),
            Action::Rename => self.open_rename(),
            Action::FindReferences => self.open_references(),
            Action::GrowSelection => self.grow_selections(outcome),
            Action::ShrinkSelection => self.shrink_selections(outcome),
            Action::GoToDefinition => {
                let head = self.state.selections().primary().head;
                self.request_definition(self.state.text().position(head));
            }
            Action::NextProblem | Action::PreviousProblem => {
                if let Some(start) = self.go_to_problem(action == Action::NextProblem) {
                    self.execute(outcome, Command::SetSelections(Selections::cursor(start)));
                }
            }
            Action::JumpToBracket => {
                let head = self.state.selections().primary().head;
                // An explicit jump may compute every line state before the cursor.
                let mut unlimited = usize::MAX;
                let pair = rustext_core::structure::matching_bracket_pair(
                    self.state.text(),
                    head,
                    &self.state.language,
                    &mut self.view.bracket_line_starts,
                    &mut unlimited,
                );
                if let Some((open, close)) = pair {
                    let target = if head == open || head == open + 1 { close } else { open };
                    self.execute(outcome, Command::SetSelections(Selections::cursor(target)));
                }
            }
            other => {
                unhandled_actions.extend(self.run_find_action(other).and_then(|other| self.run_fold_action(other)))
            }
        }
    }

    pub(super) fn handle_scroll(&mut self, ui: &Ui, response: &Response) {
        if !response.hovered() {
            return;
        }
        let (scroll_delta, zoom_delta, modifiers) =
            ui.input(|input| (input.smooth_scroll_delta, input.zoom_delta(), input.modifiers));
        if self.config.mouse_wheel_zoom && modifiers.command && zoom_delta != 1.0 {
            self.view.font_size_delta += (zoom_delta - 1.0) * 10.0;
            return;
        }
        if scroll_delta == Vec2::ZERO {
            return;
        }
        let speed = if modifiers.alt { self.config.fast_scroll_sensitivity } else { 1.0 };
        let mut delta = scroll_delta * speed;
        if modifiers.shift && delta.x == 0.0 {
            delta = vec2(delta.y, 0.0);
        }
        self.view.scroll_offset -= delta;
        ui.ctx().input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
    }

    pub(super) fn after_outcome(&mut self, ui: &Ui, outcome: &Outcome) {
        if let Some(styler) = self.styler.as_deref_mut() {
            let edits = outcome.texts_before.iter().zip(&outcome.applied_changes).zip(&outcome.edited_lines);
            if edits.len() == outcome.edited_lines.len() {
                for ((text_before, changes), edited) in edits {
                    styler.text_edited(text_before, changes, edited);
                }
            } else if let Some(first_line) = outcome.edited_lines.iter().map(|edited| edited.first_line).min() {
                styler.invalidate_from(first_line);
            }
        }
        for changes in &outcome.applied_changes {
            self.view.service_diagnostics =
                self.view.service_diagnostics.iter().filter_map(|decoration| decoration.map(changes)).collect();
            self.view.inlay_hints.map(changes);
            self.map_find_scope(changes);
        }
        for edited in &outcome.edited_lines {
            self.view.folds.follow_edit(edited);
            self.view.wraps.follow_edit(edited, self.state.text());
            self.view.bracket_line_starts.invalidate_from(edited.first_line);
        }
        if outcome.text_changed() {
            self.view.seen_text_revision = Some(self.state.text_revision());
            if let Some(services) = self.services.as_deref_mut() {
                services.text_changed(&outcome.texts_before, &outcome.applied_changes, self.state.text_revision());
            }
        }
        if let [changes] = outcome.applied_changes.as_slice() {
            self.update_find_matches_after(changes);
        }
        let is_dragging_scrollbar = matches!(
            self.view.drag,
            Some(Drag::VerticalScrollbar { .. } | Drag::HorizontalScrollbar { .. } | Drag::Minimap)
        );
        if (outcome.text_changed() || outcome.selections_changed) && !is_dragging_scrollbar {
            self.view.reveal_primary_cursor = true;
            self.view.last_interaction_time = ui.input(|input| input.time);
        }
    }
}
