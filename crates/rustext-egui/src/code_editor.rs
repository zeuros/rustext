//! The `CodeEditor` widget: paints an `EditorState` and turns input into commands.
//!
//! The application owns the `EditorState` (text, selections, history) and an `EditorView`
//! (scroll, zoom, in-progress mouse drag…: Monaco's view state), and calls
//! `CodeEditor::new(...).show(ui)` every frame. What happened comes back in the
//! `EditorResponse` (DECISIONS D10).

use std::sync::Arc;

use egui::text::{ByteIndex, CCursor, LayoutJob, TextFormat};
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, Event, EventFilter, FontId, Galley, ImeEvent, Key, Modifiers,
    PointerButton, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2,
};
use rustext_core::cursors::{VerticalDirection, word_range_at};
use rustext_core::decoration::{Decoration, DecorationKind};
use rustext_core::style::{LineStyler, Rgba, StyledSpan};
use rustext_core::{Command, EditorState, Motion, Outcome, Position, Selection, Selections, Text};

use crate::keymap::{Action, KeyChord, KeyResolution, Keymap};
use crate::theme::EditorTheme;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineNumbers {
    #[default]
    On,
    Off,
    /// Distance to the cursor's line, the cursor's line showing its own number.
    Relative,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CursorStyle {
    #[default]
    Line,
    Block,
    Underline,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CurrentLineHighlight {
    None,
    Gutter,
    Line,
    #[default]
    All,
}

/// Display options of a code editor (DECISIONS D12).
#[derive(Clone, Debug, PartialEq)]
pub struct ViewConfig {
    pub font_size: f32,
    /// Line height as a multiple of the font's row height.
    pub line_height_factor: f32,
    pub line_numbers: LineNumbers,
    pub current_line_highlight: CurrentLineHighlight,
    pub cursor_style: CursorStyle,
    pub cursor_blinking: bool,
    /// Lets the last line scroll up to the top of the viewport.
    pub scroll_beyond_last_line: bool,
    /// Lines kept visible above and below the cursor when revealing it.
    pub cursor_surrounding_lines: usize,
    pub rulers: Vec<usize>,
    pub padding_top: f32,
    /// Lines longer than this many chars are cut, for speed (Monaco's `stopRenderingLineAfter`).
    pub max_rendered_line_chars: usize,
    pub scrollbar_width: f32,
    /// Alt + wheel scrolls this many times faster.
    pub fast_scroll_sensitivity: f32,
    pub mouse_wheel_zoom: bool,
    /// Draws the cursor positions and marks on the vertical scrollbar.
    pub overview_ruler: bool,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            line_height_factor: 1.35,
            line_numbers: LineNumbers::default(),
            current_line_highlight: CurrentLineHighlight::default(),
            cursor_style: CursorStyle::default(),
            cursor_blinking: true,
            scroll_beyond_last_line: true,
            cursor_surrounding_lines: 2,
            rulers: Vec::new(),
            padding_top: 4.0,
            max_rendered_line_chars: 10_000,
            scrollbar_width: 14.0,
            fast_scroll_sensitivity: 5.0,
            mouse_wheel_zoom: true,
            overview_ruler: true,
        }
    }
}

/// How a mouse drag extends the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragUnit {
    Char,
    Word,
    Line,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Selecting text from `anchor_start..anchor_end` (a word or line for double / triple clicks).
    Selecting {
        anchor_start: usize,
        anchor_end: usize,
        unit: DragUnit,
        adds_cursor: bool,
    },
    /// Shift+Alt drag: box selection from `anchor`.
    Column {
        anchor: Position,
    },
    VerticalScrollbar {
        grab_offset_y: f32,
    },
}

/// Per-view state the application keeps between frames: scroll, zoom, mouse drag, chord in
/// progress. Saving and restoring it with the `EditorState`'s selections is Monaco's
/// `saveViewState` / `restoreViewState`.
#[derive(Clone, Debug, Default)]
pub struct EditorView {
    /// Content offset in points: `x` horizontal, `y` vertical.
    pub scroll_offset: Vec2,
    /// Font size change from zooming, added to `ViewConfig::font_size`.
    pub font_size_delta: f32,
    drag: Option<Drag>,
    pending_chords: Vec<KeyChord>,
    ime_preedit: Option<String>,
    last_interaction_time: f64,
    last_press: Option<(f64, Pos2, u8)>,
    widest_line_px: f32,
    reveal_primary_cursor: bool,
    geometry: Option<EditorGeometry>,
}

/// Where things were painted in the last frame, in screen points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorGeometry {
    pub text_rect: Rect,
    pub row_height: f32,
    /// Width of `M` in the editor's monospace font.
    pub char_width: f32,
    pub scroll_offset: Vec2,
    pub padding_top: f32,
}

impl EditorGeometry {
    /// Top of `line`'s row.
    pub fn line_top(&self, line: usize) -> f32 {
        self.text_rect.top() + self.padding_top + line as f32 * self.row_height - self.scroll_offset.y
    }

    /// Center of the cell at `visual_column` on `line`, assuming a monospace font.
    pub fn cell_center(&self, line: usize, visual_column: usize) -> Pos2 {
        let x = self.text_rect.left() - self.scroll_offset.x + (visual_column as f32 + 0.5) * self.char_width;
        pos2(x, self.line_top(line) + self.row_height / 2.0)
    }

    /// Left edge of the cell at `visual_column` on `line`, vertically centered.
    pub fn cell_left(&self, line: usize, visual_column: usize) -> Pos2 {
        self.cell_center(line, visual_column) - vec2(self.char_width / 2.0, 0.0)
    }
}

impl EditorView {
    /// Scrolls so the primary cursor is visible on the next frame.
    pub fn reveal_cursor(&mut self) {
        self.reveal_primary_cursor = true;
    }

    pub fn pending_chords(&self) -> &[KeyChord] {
        &self.pending_chords
    }

    /// Geometry of the last frame; `None` before the first one.
    pub fn geometry(&self) -> Option<EditorGeometry> {
        self.geometry
    }
}

/// Where the pointer is over the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseTarget {
    /// Over text: the nearest position.
    Text {
        position: Position,
    },
    /// Past the end of a line, or below the last line.
    Empty {
        position: Position,
    },
    LineNumber {
        line: usize,
    },
    /// The margin left of the line numbers (glyphs, breakpoints).
    GlyphMargin {
        line: usize,
    },
    Scrollbar,
}

pub struct EditorResponse {
    pub response: Response,
    pub outcome: Outcome,
    /// What the primary button was pressed on this frame.
    pub pressed: Option<MouseTarget>,
    pub hovered: Option<MouseTarget>,
    /// An action the widget doesn't handle itself (find, go to line, folding…), for the
    /// application or a wrapping widget.
    pub unhandled_actions: Vec<Action>,
}

/// The widget. Borrowed state, built every frame.
pub struct CodeEditor<'a> {
    state: &'a mut EditorState,
    view: &'a mut EditorView,
    config: &'a ViewConfig,
    theme: &'a EditorTheme,
    keymap: &'a Keymap,
    styler: Option<&'a mut dyn LineStyler>,
    decorations: &'a [Decoration],
    glyph_margin: bool,
}

/// Sizes of one frame's layout.
#[derive(Clone, Copy, Debug)]
struct Metrics {
    font_id_size: f32,
    row_height: f32,
    char_width: f32,
    glyph_margin_width: f32,
    line_numbers_width: f32,
    gutter_width: f32,
    text_rect: Rect,
    scrollbar_rect: Rect,
}

impl Metrics {
    fn font_id(&self) -> FontId {
        FontId::monospace(self.font_id_size)
    }
}

/// One laid-out line: its galley and where each char column lands in it (tabs expanded).
struct LineLayout {
    galley: Arc<Galley>,
    display_index_of_column: Vec<usize>,
}

impl LineLayout {
    fn x_of_column(&self, column: usize) -> f32 {
        let index =
            self.display_index_of_column.get(column).or(self.display_index_of_column.last()).copied().unwrap_or(0);
        self.galley.pos_from_cursor(CCursor::new(index)).min.x
    }

    fn column_at_x(&self, x: f32) -> usize {
        let display_index = self.galley.cursor_from_pos(vec2(x, self.galley.size().y / 2.0)).index.0;
        self.display_index_of_column
            .iter()
            .position(|&index| index >= display_index)
            .unwrap_or(self.display_index_of_column.len() - 1)
    }

    fn width(&self) -> f32 {
        self.galley.size().x
    }
}

pub fn to_color(rgba: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(rgba.red, rgba.green, rgba.blue, rgba.alpha)
}

impl<'a> CodeEditor<'a> {
    pub fn new(
        state: &'a mut EditorState,
        view: &'a mut EditorView,
        config: &'a ViewConfig,
        theme: &'a EditorTheme,
        keymap: &'a Keymap,
    ) -> Self {
        Self { state, view, config, theme, keymap, styler: None, decorations: &[], glyph_margin: false }
    }

    pub fn styler(mut self, styler: &'a mut dyn LineStyler) -> Self {
        self.styler = Some(styler);
        self
    }

    pub fn decorations(mut self, decorations: &'a [Decoration]) -> Self {
        self.decorations = decorations;
        self
    }

    /// Reserves a margin left of the line numbers for glyph decorations.
    pub fn glyph_margin(mut self, glyph_margin: bool) -> Self {
        self.glyph_margin = glyph_margin;
        self
    }

    pub fn show(mut self, ui: &mut Ui) -> EditorResponse {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let id = response.id;
        let metrics = self.metrics(ui, rect);
        let has_focus = response.has_focus();
        if response.clicked() || response.drag_started() {
            response.request_focus();
        }
        if has_focus {
            let filter = EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true };
            ui.memory_mut(|memory| memory.set_focus_lock_filter(id, filter));
        }

        let mut outcome = Outcome::default();
        let mut unhandled_actions = Vec::new();
        let pointer_pos = ui.input(|input| input.pointer.hover_pos());
        let hovered = pointer_pos.filter(|pos| rect.contains(*pos)).map(|pos| self.target_at(ui, &metrics, pos));
        let pressed = self.handle_pointer(ui, &metrics, &response, &mut outcome);
        if has_focus || response.has_focus() {
            self.handle_keyboard(ui, &metrics, &mut outcome, &mut unhandled_actions);
        }
        self.handle_scroll(ui, &response);
        self.after_outcome(ui, &outcome);
        if self.view.reveal_primary_cursor {
            self.reveal_primary_cursor(ui, &metrics);
            self.view.reveal_primary_cursor = false;
        }
        self.clamp_scroll(&metrics);

        if matches!(hovered, Some(MouseTarget::Text { .. } | MouseTarget::Empty { .. })) {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }
        self.paint(ui, &metrics, response.has_focus());
        self.view.geometry = Some(EditorGeometry {
            text_rect: metrics.text_rect,
            row_height: metrics.row_height,
            char_width: metrics.char_width,
            scroll_offset: self.view.scroll_offset,
            padding_top: self.config.padding_top,
        });

        EditorResponse { response, outcome, pressed, hovered, unhandled_actions }
    }

    fn metrics(&self, ui: &Ui, rect: Rect) -> Metrics {
        let font_size = (self.config.font_size + self.view.font_size_delta).clamp(6.0, 72.0);
        let font_id = FontId::monospace(font_size);
        let (font_row_height, char_width) =
            ui.ctx().fonts_mut(|fonts| (fonts.row_height(&font_id), fonts.glyph_width(&font_id, 'M')));
        let row_height = (font_row_height * self.config.line_height_factor).round();
        let digits = self.state.text().line_count().to_string().len().max(2) as f32;
        let line_numbers_width = match self.config.line_numbers {
            LineNumbers::Off => char_width,
            LineNumbers::On | LineNumbers::Relative => (digits + 2.0) * char_width,
        };
        let glyph_margin_width = if self.glyph_margin { row_height } else { 0.0 };
        let gutter_width = glyph_margin_width + line_numbers_width + char_width;
        let scrollbar_rect =
            Rect::from_min_max(pos2(rect.right() - self.config.scrollbar_width, rect.top()), rect.right_bottom());
        let text_rect = Rect::from_min_max(
            pos2(rect.left() + gutter_width, rect.top()),
            pos2(scrollbar_rect.left(), rect.bottom()),
        );
        Metrics {
            font_id_size: font_size,
            row_height,
            char_width,
            glyph_margin_width,
            line_numbers_width,
            gutter_width,
            text_rect,
            scrollbar_rect,
        }
    }

    fn full_rect(metrics: &Metrics) -> Rect {
        Rect::from_min_max(
            pos2(metrics.text_rect.left() - metrics.gutter_width, metrics.text_rect.top()),
            metrics.scrollbar_rect.right_bottom(),
        )
    }

    // ---- layout ----

    fn layout_line(&mut self, ui: &Ui, metrics: &Metrics, line: usize) -> LineLayout {
        let text = self.state.text();
        let content = text.line_content(line);
        let spans = match self.styler.as_deref_mut() {
            Some(styler) => styler.style_line(text, line),
            None => Vec::new(),
        };
        let job = line_layout_job(
            &content,
            &spans,
            metrics.font_id(),
            self.theme.foreground,
            self.state.config.indentation.tab_size,
            self.config.max_rendered_line_chars,
        );
        let display_index_of_column = job.1;
        let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job.0));
        LineLayout { galley, display_index_of_column }
    }

    fn line_top(&self, metrics: &Metrics, line: usize) -> f32 {
        metrics.text_rect.top() + self.config.padding_top + line as f32 * metrics.row_height - self.view.scroll_offset.y
    }

    fn text_left(&self, metrics: &Metrics) -> f32 {
        metrics.text_rect.left() - self.view.scroll_offset.x
    }

    fn line_at_y(&self, metrics: &Metrics, y: f32) -> usize {
        let row = ((y - metrics.text_rect.top() - self.config.padding_top + self.view.scroll_offset.y)
            / metrics.row_height)
            .floor();
        (row.max(0.0) as usize).min(self.state.text().last_line())
    }

    fn visible_lines(&self, metrics: &Metrics) -> std::ops::Range<usize> {
        let first = self.line_at_y(metrics, metrics.text_rect.top());
        let last = self.line_at_y(metrics, metrics.text_rect.bottom());
        first..last + 1
    }

    fn position_at(&mut self, ui: &Ui, metrics: &Metrics, pos: Pos2) -> (Position, bool) {
        let line = self.line_at_y(metrics, pos.y);
        let is_below_text = pos.y > self.line_top(metrics, line) + metrics.row_height;
        let layout = self.layout_line(ui, metrics, line);
        let x = pos.x - self.text_left(metrics);
        let column = layout.column_at_x(x);
        let is_past_line_end = x > layout.width() || is_below_text;
        (Position::new(line, column), is_past_line_end)
    }

    fn target_at(&mut self, ui: &Ui, metrics: &Metrics, pos: Pos2) -> MouseTarget {
        if pos.x >= metrics.scrollbar_rect.left() {
            return MouseTarget::Scrollbar;
        }
        let line = self.line_at_y(metrics, pos.y);
        let gutter_left = metrics.text_rect.left() - metrics.gutter_width;
        if pos.x < gutter_left + metrics.glyph_margin_width {
            return MouseTarget::GlyphMargin { line };
        }
        if pos.x < metrics.text_rect.left() {
            return MouseTarget::LineNumber { line };
        }
        let (position, is_past_line_end) = self.position_at(ui, metrics, pos);
        if is_past_line_end { MouseTarget::Empty { position } } else { MouseTarget::Text { position } }
    }

    // ---- input ----

    fn execute(&mut self, outcome: &mut Outcome, command: Command) {
        let result = self.state.execute(command);
        *outcome = std::mem::take(outcome).merged(result);
    }

    fn handle_pointer(
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
        let full_rect = Self::full_rect(metrics);

        let mut pressed_target = None;
        if primary_pressed && full_rect.contains(pointer_pos) && response.hovered() {
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
                let (position, _) = self.position_at(ui, metrics, pos);
                let offset = text.offset(position);
                if modifiers.shift && modifiers.alt {
                    let anchor = text.position(self.state.selections().primary().anchor);
                    self.view.drag = Some(Drag::Column { anchor });
                    self.drag_to(ui, metrics, pos, outcome);
                    return;
                }
                if modifiers.shift {
                    let primary = self.state.selections().primary();
                    let anchor = primary.anchor;
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
            Drag::Column { anchor } => {
                let line = self.line_at_y(metrics, pos.y);
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
                    let others =
                        self.state.selections().iter().copied().filter(|existing| {
                            existing.start() != anchor_start && existing.start() != selection.start()
                        });
                    let mut ranges: Vec<Selection> = others.collect();
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

    /// Dragging above or below the text scrolls towards the pointer.
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

    fn handle_keyboard(
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
                        self.execute(outcome, Command::Type(printable));
                    }
                }
                Event::Key { key, pressed: true, modifiers, .. } => {
                    self.key_pressed(ui, metrics, key, modifiers, outcome, unhandled_actions)
                }
                Event::Copy => self.copy(ui, outcome, Command::Copy),
                Event::Cut => self.copy(ui, outcome, Command::Cut),
                Event::Paste(pasted) => self.execute(outcome, Command::Paste(pasted)),
                Event::Ime(ImeEvent::Preedit { text, .. }) => {
                    self.view.ime_preedit = (!text.is_empty()).then_some(text);
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
        ui: &Ui,
        metrics: &Metrics,
        key: Key,
        modifiers: Modifiers,
        outcome: &mut Outcome,
        unhandled_actions: &mut Vec<Action>,
    ) {
        let resolution = self.keymap.resolve(&self.view.pending_chords, modifiers, key);
        match resolution {
            KeyResolution::Pending => self.view.pending_chords.push(Keymap::chord(modifiers, key)),
            KeyResolution::Unbound => self.view.pending_chords.clear(),
            KeyResolution::Action(action) => {
                self.view.pending_chords.clear();
                self.run_action(ui, metrics, action, outcome, unhandled_actions);
            }
        }
    }

    fn run_action(
        &mut self,
        _ui: &Ui,
        metrics: &Metrics,
        action: Action,
        outcome: &mut Outcome,
        unhandled_actions: &mut Vec<Action>,
    ) {
        let lines_per_page =
            ((metrics.text_rect.height() / metrics.row_height).floor() as usize).saturating_sub(1).max(1);
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
            other => unhandled_actions.push(other),
        }
    }

    fn handle_scroll(&mut self, ui: &Ui, response: &Response) {
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

    fn after_outcome(&mut self, ui: &Ui, outcome: &Outcome) {
        // The text before the earliest change is the same in every version, so its line is exact.
        let first_changed_offset = outcome
            .applied_changes
            .iter()
            .filter_map(|changes| changes.changes().first())
            .map(|change| change.start)
            .min();
        if let (Some(styler), Some(first_changed_offset)) = (self.styler.as_deref_mut(), first_changed_offset) {
            styler.invalidate_from(self.state.text().line_of_offset(first_changed_offset));
        }
        let is_dragging_scrollbar = matches!(self.view.drag, Some(Drag::VerticalScrollbar { .. }));
        if (outcome.text_changed() || outcome.selections_changed) && !is_dragging_scrollbar {
            self.view.reveal_primary_cursor = true;
            self.view.last_interaction_time = ui.input(|input| input.time);
        }
    }

    // ---- scrolling ----

    fn content_height(&self, metrics: &Metrics) -> f32 {
        self.state.text().line_count() as f32 * metrics.row_height + self.config.padding_top
    }

    fn max_scroll_y(&self, metrics: &Metrics) -> f32 {
        let content_height = self.content_height(metrics);
        if self.config.scroll_beyond_last_line {
            (content_height - metrics.row_height).max(0.0)
        } else {
            (content_height - metrics.text_rect.height()).max(0.0)
        }
    }

    fn clamp_scroll(&mut self, metrics: &Metrics) {
        let max_scroll_x = (self.view.widest_line_px + metrics.char_width * 4.0 - metrics.text_rect.width()).max(0.0);
        self.view.scroll_offset.x = self.view.scroll_offset.x.clamp(0.0, max_scroll_x);
        self.view.scroll_offset.y = self.view.scroll_offset.y.clamp(0.0, self.max_scroll_y(metrics));
    }

    fn reveal_primary_cursor(&mut self, ui: &Ui, metrics: &Metrics) {
        let text = self.state.text().clone();
        let head = text.position(self.state.selections().primary().head);
        let margin = self.config.cursor_surrounding_lines as f32 * metrics.row_height;
        let cursor_top = self.config.padding_top + head.line as f32 * metrics.row_height;
        let viewport_height = metrics.text_rect.height();
        let top_limit = cursor_top - margin;
        let bottom_limit = cursor_top + metrics.row_height + margin - viewport_height;
        if self.view.scroll_offset.y > top_limit {
            self.view.scroll_offset.y = top_limit.max(0.0);
        } else if self.view.scroll_offset.y < bottom_limit {
            self.view.scroll_offset.y = bottom_limit;
        }

        let layout = self.layout_line(ui, metrics, head.line);
        let cursor_x = layout.x_of_column(head.column);
        self.view.widest_line_px = self.view.widest_line_px.max(layout.width());
        let horizontal_margin = metrics.char_width * 4.0;
        let viewport_width = metrics.text_rect.width();
        if cursor_x - self.view.scroll_offset.x < horizontal_margin.min(cursor_x) {
            self.view.scroll_offset.x = (cursor_x - horizontal_margin).max(0.0);
        } else if cursor_x - self.view.scroll_offset.x > viewport_width - horizontal_margin {
            self.view.scroll_offset.x = cursor_x - viewport_width + horizontal_margin;
        }
    }

    fn scrollbar_thumb(&self, metrics: &Metrics) -> Rect {
        let track = metrics.scrollbar_rect;
        let total_height = self.max_scroll_y(metrics) + metrics.text_rect.height();
        let thumb_height = (track.height() * metrics.text_rect.height() / total_height)
            .clamp(20.0_f32.min(track.height()), track.height());
        let fraction =
            if self.max_scroll_y(metrics) > 0.0 { self.view.scroll_offset.y / self.max_scroll_y(metrics) } else { 0.0 };
        let top = track.top() + fraction * (track.height() - thumb_height);
        Rect::from_min_size(pos2(track.left(), top), vec2(track.width(), thumb_height))
    }

    // ---- painting ----

    fn paint(&mut self, ui: &mut Ui, metrics: &Metrics, has_focus: bool) {
        let full_rect = Self::full_rect(metrics);
        let painter = ui.painter_at(full_rect);
        let theme = self.theme.clone();
        painter.rect_filled(full_rect, CornerRadius::ZERO, theme.background);

        let text = self.state.text().clone();
        let selections = self.state.selections().clone();
        let primary_head = text.position(selections.primary().head);
        let cursor_lines: Vec<usize> = selections.iter().map(|selection| text.line_of_offset(selection.head)).collect();
        let all_selections_empty = selections.iter().all(Selection::is_empty);
        let text_painter = painter.with_clip_rect(metrics.text_rect);
        let text_left = self.text_left(metrics);
        let visible_lines = self.visible_lines(metrics);

        let highlights_line =
            matches!(self.config.current_line_highlight, CurrentLineHighlight::Line | CurrentLineHighlight::All);
        let highlights_gutter =
            matches!(self.config.current_line_highlight, CurrentLineHighlight::Gutter | CurrentLineHighlight::All);

        let mut layouts: Vec<(usize, LineLayout)> = Vec::with_capacity(visible_lines.len());
        for line in visible_lines.clone() {
            let layout = self.layout_line(ui, metrics, line);
            self.view.widest_line_px = self.view.widest_line_px.max(layout.width());
            layouts.push((line, layout));
        }

        for (line, layout) in &layouts {
            let top = self.line_top(metrics, *line);
            let row_rect = Rect::from_min_size(
                pos2(metrics.text_rect.left(), top),
                vec2(metrics.text_rect.width(), metrics.row_height),
            );
            if highlights_line && all_selections_empty && cursor_lines.contains(line) {
                text_painter.rect_filled(row_rect, CornerRadius::ZERO, theme.current_line_background);
            }

            for decoration in self.decorations.iter().filter(|decoration| decoration.covers_line(&text, *line)) {
                if let DecorationKind::WholeLineBackground(color) = decoration.kind {
                    text_painter.rect_filled(row_rect, CornerRadius::ZERO, to_color(color));
                }
            }

            let line_start = text.line_start(*line);
            let line_end = text.line_content_end(*line);
            for selection in selections.iter().filter(|selection| !selection.is_empty()) {
                let (start, end) = (selection.start().max(line_start), selection.end().min(line_end));
                let continues_past_line = selection.end() > line_end && selection.start() <= line_end;
                if start > end
                    || (start == end && !continues_past_line)
                    || selection.end() < line_start
                    || selection.start() > line_end
                {
                    continue;
                }
                let left = text_left + layout.x_of_column(start - line_start);
                let right = text_left
                    + layout.x_of_column(end - line_start)
                    + if continues_past_line { metrics.char_width * 0.6 } else { 0.0 };
                let color = if has_focus { theme.selection_background } else { theme.inactive_selection_background };
                text_painter.rect_filled(
                    Rect::from_min_max(pos2(left, top), pos2(right, top + metrics.row_height)),
                    CornerRadius::same(2),
                    color,
                );
            }

            for decoration in self.decorations.iter() {
                self.paint_inline_decoration(&text_painter, metrics, &text, *line, layout, top, decoration);
            }

            let text_top = top + (metrics.row_height - layout.galley.size().y) / 2.0;
            text_painter.galley(pos2(text_left, text_top), layout.galley.clone(), theme.foreground);
        }

        for column in &self.config.rulers {
            let x = text_left + *column as f32 * metrics.char_width;
            text_painter.vline(x, metrics.text_rect.y_range(), Stroke::new(1.0, theme.ruler));
        }

        let now = ui.input(|input| input.time);
        let blink_on = !self.config.cursor_blinking || ((now - self.view.last_interaction_time) % 1.0) < 0.55;
        if has_focus && blink_on {
            for selection in selections.iter() {
                let head = text.position(selection.head);
                let Some((_, layout)) = layouts.iter().find(|(line, _)| *line == head.line) else { continue };
                self.paint_cursor(&text_painter, metrics, layout, head, &theme);
            }
        }
        if has_focus {
            if self.config.cursor_blinking {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
            }
            self.paint_ime(ui, &text_painter, metrics, &layouts, primary_head, &theme);
        }

        self.paint_gutter(
            &painter,
            metrics,
            &text,
            &layouts,
            &cursor_lines,
            primary_head,
            highlights_gutter && all_selections_empty,
        );
        self.paint_scrollbar(ui, &painter, metrics, &text, &selections);
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_inline_decoration(
        &self,
        painter: &egui::Painter,
        metrics: &Metrics,
        text: &Text,
        line: usize,
        layout: &LineLayout,
        top: f32,
        decoration: &Decoration,
    ) {
        let line_start = text.line_start(line);
        let line_end = text.line_content_end(line);
        let (start, end) = (decoration.start.max(line_start), decoration.end.min(line_end));
        if decoration.end < line_start || decoration.start > line_end || start > end {
            return;
        }
        let text_left = self.text_left(metrics);
        let left = text_left + layout.x_of_column(start - line_start);
        let right = text_left + layout.x_of_column(end - line_start);
        let bottom = top + metrics.row_height;
        match decoration.kind {
            DecorationKind::Background(color) => {
                painter.rect_filled(
                    Rect::from_min_max(pos2(left, top), pos2(right, bottom)),
                    CornerRadius::ZERO,
                    to_color(color),
                );
            }
            DecorationKind::Border(color) => {
                let rect = Rect::from_min_max(pos2(left, top), pos2(right.max(left + 2.0), bottom));
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(2),
                    Stroke::new(1.0, to_color(color)),
                    egui::StrokeKind::Inside,
                );
            }
            DecorationKind::Squiggle(color) => {
                paint_squiggle(painter, left, right.max(left + metrics.char_width), bottom - 2.0, to_color(color));
            }
            DecorationKind::Underline(color) => {
                painter.hline(left..=right, bottom - 1.5, Stroke::new(1.0, to_color(color)));
            }
            DecorationKind::WholeLineBackground(_) | DecorationKind::Glyph(_) | DecorationKind::LineNumberColor(_) => {}
        }
    }

    fn paint_cursor(
        &self,
        painter: &egui::Painter,
        metrics: &Metrics,
        layout: &LineLayout,
        head: Position,
        theme: &EditorTheme,
    ) {
        let top = self.line_top(metrics, head.line);
        let x = self.text_left(metrics) + layout.x_of_column(head.column);
        let next_x = self.text_left(metrics) + layout.x_of_column(head.column + 1);
        let char_width = if next_x > x { next_x - x } else { metrics.char_width };
        let rect = match self.config.cursor_style {
            CursorStyle::Line => Rect::from_min_size(pos2(x - 1.0, top), vec2(2.0, metrics.row_height)),
            CursorStyle::Block => Rect::from_min_size(pos2(x, top), vec2(char_width, metrics.row_height)),
            CursorStyle::Underline => {
                Rect::from_min_size(pos2(x, top + metrics.row_height - 2.0), vec2(char_width, 2.0))
            }
        };
        let color = if self.config.cursor_style == CursorStyle::Block {
            theme.cursor.gamma_multiply(0.6)
        } else {
            theme.cursor
        };
        painter.rect_filled(rect, CornerRadius::ZERO, color);
    }

    /// Shows the IME composition text at the primary cursor and tells the platform where it is.
    fn paint_ime(
        &self,
        ui: &Ui,
        painter: &egui::Painter,
        metrics: &Metrics,
        layouts: &[(usize, LineLayout)],
        primary_head: Position,
        theme: &EditorTheme,
    ) {
        let Some((_, layout)) = layouts.iter().find(|(line, _)| *line == primary_head.line) else { return };
        let top = self.line_top(metrics, primary_head.line);
        let x = self.text_left(metrics) + layout.x_of_column(primary_head.column);
        let cursor_rect = Rect::from_min_size(pos2(x, top), vec2(2.0, metrics.row_height));
        if let Some(preedit) = &self.view.ime_preedit {
            let galley = painter.layout_no_wrap(preedit.clone(), metrics.font_id(), theme.foreground);
            let preedit_rect = Rect::from_min_size(pos2(x, top), vec2(galley.size().x, metrics.row_height));
            painter.rect_filled(preedit_rect, CornerRadius::ZERO, theme.background);
            painter.galley(pos2(x, top + (metrics.row_height - galley.size().y) / 2.0), galley, theme.foreground);
            painter.hline(preedit_rect.x_range(), preedit_rect.bottom() - 1.0, Stroke::new(1.0, theme.foreground));
        }
        let to_global = ui.ctx().layer_transform_to_global(ui.layer_id()).unwrap_or_default();
        ui.ctx().output_mut(|output| {
            output.ime = Some(egui::output::IMEOutput {
                purpose: egui::IMEPurpose::Normal,
                rect: to_global * metrics.text_rect,
                cursor_rect: to_global * cursor_rect,
                should_interrupt_composition: false,
            });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_gutter(
        &self,
        painter: &egui::Painter,
        metrics: &Metrics,
        text: &Text,
        layouts: &[(usize, LineLayout)],
        cursor_lines: &[usize],
        primary_head: Position,
        highlights_current_line: bool,
    ) {
        let theme = self.theme;
        let gutter_rect = Rect::from_min_max(
            pos2(metrics.text_rect.left() - metrics.gutter_width, metrics.text_rect.top()),
            metrics.text_rect.left_bottom(),
        );
        let gutter_painter = painter.with_clip_rect(gutter_rect);
        gutter_painter.rect_filled(gutter_rect, CornerRadius::ZERO, theme.gutter_background);
        let numbers_right = gutter_rect.left() + metrics.glyph_margin_width + metrics.line_numbers_width;
        let font_id = metrics.font_id();

        for (line, _) in layouts {
            let top = self.line_top(metrics, *line);
            let is_cursor_line = cursor_lines.contains(line);
            let row_rect =
                Rect::from_min_size(pos2(gutter_rect.left(), top), vec2(gutter_rect.width(), metrics.row_height));
            if highlights_current_line && is_cursor_line {
                gutter_painter.rect_filled(row_rect, CornerRadius::ZERO, theme.current_line_background);
            }
            for decoration in self.decorations.iter().filter(|decoration| decoration.covers_line(text, *line)) {
                match decoration.kind {
                    DecorationKind::WholeLineBackground(color) => {
                        gutter_painter.rect_filled(row_rect, CornerRadius::ZERO, to_color(color));
                    }
                    DecorationKind::Glyph(color)
                        if self.glyph_margin && text.line_of_offset(decoration.start) == *line =>
                    {
                        let center =
                            pos2(gutter_rect.left() + metrics.glyph_margin_width / 2.0, top + metrics.row_height / 2.0);
                        gutter_painter.circle_filled(center, metrics.row_height * 0.28, to_color(color));
                    }
                    _ => {}
                }
            }

            let number = match self.config.line_numbers {
                LineNumbers::Off => continue,
                LineNumbers::On => line + 1,
                LineNumbers::Relative if *line == primary_head.line => line + 1,
                LineNumbers::Relative => line.abs_diff(primary_head.line),
            };
            let decoration_color =
                self.decorations.iter().filter(|decoration| decoration.covers_line(text, *line)).find_map(
                    |decoration| match decoration.kind {
                        DecorationKind::LineNumberColor(color) => Some(to_color(color)),
                        _ => None,
                    },
                );
            let color =
                decoration_color.unwrap_or(if is_cursor_line { theme.active_line_number } else { theme.line_number });
            gutter_painter.text(
                pos2(numbers_right, top + metrics.row_height / 2.0),
                Align2::RIGHT_CENTER,
                number.to_string(),
                font_id.clone(),
                color,
            );
        }
    }

    fn paint_scrollbar(
        &self,
        ui: &Ui,
        painter: &egui::Painter,
        metrics: &Metrics,
        text: &Text,
        selections: &Selections,
    ) {
        let theme = self.theme;
        let track = metrics.scrollbar_rect;
        painter.rect_filled(track, CornerRadius::ZERO, theme.background);
        painter.vline(track.left(), track.y_range(), Stroke::new(1.0, theme.overview_ruler_border));

        if self.config.overview_ruler {
            let line_count = text.line_count().max(1) as f32;
            let y_of_line = |line: usize| track.top() + (line as f32 + 0.5) / line_count * track.height();
            for decoration in self.decorations {
                if let Some(color) = decoration.overview_ruler {
                    let y = y_of_line(text.line_of_offset(decoration.start));
                    let mark = Rect::from_center_size(pos2(track.center().x, y), vec2(track.width() - 4.0, 3.0));
                    painter.rect_filled(mark, CornerRadius::ZERO, to_color(color));
                }
            }
            for selection in selections.iter() {
                let y = y_of_line(text.line_of_offset(selection.head));
                painter.hline(track.x_range(), y, Stroke::new(2.0, theme.cursor.gamma_multiply(0.7)));
            }
        }

        if self.max_scroll_y(metrics) <= 0.0 {
            return;
        }
        let thumb = self.scrollbar_thumb(metrics);
        let is_hovered = ui.input(|input| input.pointer.hover_pos()).is_some_and(|pos| track.contains(pos));
        let is_dragged = matches!(self.view.drag, Some(Drag::VerticalScrollbar { .. }));
        let color = if is_hovered || is_dragged { theme.scrollbar_thumb_hovered } else { theme.scrollbar_thumb };
        painter.rect_filled(thumb.shrink2(vec2(1.0, 0.0)), CornerRadius::ZERO, color);
    }
}

/// The line with its line ending, as a selection range (triple click, gutter click).
fn whole_line(text: &Text, line: usize) -> (usize, usize) {
    let end = if line < text.last_line() { text.line_start(line + 1) } else { text.len_chars() };
    (text.line_start(line), end)
}

/// Builds the layout job of a line: tabs expanded to spaces, spans colored. Returns the job
/// and, for each char column (plus the end), its char index in the job's text.
pub(crate) fn line_layout_job(
    content: &str,
    spans: &[StyledSpan],
    font_id: FontId,
    default_color: Color32,
    tab_size: usize,
    max_chars: usize,
) -> (LayoutJob, Vec<usize>) {
    let default_format = TextFormat { font_id: font_id.clone(), color: default_color, ..TextFormat::default() };
    let mut job =
        LayoutJob { wrap: egui::text::TextWrapping::no_max_width(), break_on_newline: false, ..LayoutJob::default() };
    let mut display_index_of_column = Vec::with_capacity(content.len() + 1);
    let mut display_len = 0;
    let mut span_index = 0;

    for (column, character) in content.chars().enumerate().take(max_chars) {
        display_index_of_column.push(display_len);
        while spans.get(span_index).is_some_and(|span| span.end_column <= column) {
            span_index += 1;
        }
        let format = match spans.get(span_index).filter(|span| span.start_column <= column) {
            Some(span) => text_format(&span.style, &font_id, default_color),
            None => default_format.clone(),
        };
        let byte_start = job.text.len();
        if character == '\t' {
            let spaces = tab_size.max(1) - display_len % tab_size.max(1);
            job.text.extend(std::iter::repeat_n(' ', spaces));
            display_len += spaces;
        } else {
            job.text.push(character);
            display_len += 1;
        }
        match job.sections.last_mut() {
            Some(section) if section.format == format => section.byte_range.end = ByteIndex(job.text.len()),
            _ => job.sections.push(egui::text::LayoutSection {
                leading_space: 0.0,
                byte_range: ByteIndex(byte_start)..ByteIndex(job.text.len()),
                format,
            }),
        }
    }
    display_index_of_column.push(display_len);
    if job.sections.is_empty() {
        job.sections.push(egui::text::LayoutSection {
            leading_space: 0.0,
            byte_range: ByteIndex(0)..ByteIndex(0),
            format: default_format,
        });
    }
    (job, display_index_of_column)
}

fn text_format(style: &rustext_core::style::TextStyle, font_id: &FontId, default_color: Color32) -> TextFormat {
    TextFormat {
        font_id: font_id.clone(),
        color: style.foreground.map(to_color).unwrap_or(default_color),
        background: style.background.map(to_color).unwrap_or(Color32::TRANSPARENT),
        italics: style.italic,
        underline: if style.underline {
            Stroke::new(1.0, style.foreground.map(to_color).unwrap_or(default_color))
        } else {
            Stroke::NONE
        },
        strikethrough: if style.strikethrough {
            Stroke::new(1.0, style.foreground.map(to_color).unwrap_or(default_color))
        } else {
            Stroke::NONE
        },
        ..TextFormat::default()
    }
}

fn paint_squiggle(painter: &egui::Painter, left: f32, right: f32, y: f32, color: Color32) {
    const WAVE_WIDTH: f32 = 3.0;
    const WAVE_HEIGHT: f32 = 1.5;
    let mut points = Vec::new();
    let mut x = left;
    let mut is_up = true;
    while x <= right {
        points.push(pos2(x, if is_up { y - WAVE_HEIGHT } else { y + WAVE_HEIGHT }));
        x += WAVE_WIDTH;
        is_up = !is_up;
    }
    if points.len() > 1 {
        painter.add(egui::Shape::line(points, Stroke::new(1.0, color)));
    }
}
