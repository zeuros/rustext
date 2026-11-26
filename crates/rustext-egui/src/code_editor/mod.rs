//! The `CodeEditor` widget: paints an `EditorState` and turns input into commands.
//!
//! The application owns the `EditorState` (text, selections, history) and an `EditorView`
//! (scroll, zoom, in-progress mouse drag…: Monaco's view state), and calls
//! `CodeEditor::new(...).show(ui)` every frame. What happened comes back in the
//! `EditorResponse` (DECISIONS D10).
//!
//! The editor paints display rows (DECISIONS D17): one per line by default, or the rows of a
//! `DisplayLayout` (padding, collapsed and foreign rows, for diffs and folding).

mod find;
mod input;
mod layout;
mod paint;
mod structure;

use egui::{CursorIcon, EventFilter, FontId, Pos2, Rect, Response, Sense, Ui, Vec2, pos2, vec2};
use rustext_core::decoration::{Decoration, DecorationKind};
use rustext_core::display::{DisplayLayout, RowContent};
use rustext_core::style::{LineStyler, Rgba};
use rustext_core::{EditorState, Outcome, Position, Text};

use crate::keymap::{Action, KeyChord, Keymap};
use crate::theme::EditorTheme;

pub use find::{FindState, GoToLineState, parse_line_and_column};
pub use layout::{to_color, to_rgba};
pub use structure::FoldState;

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

/// Which whitespace chars get a visible mark (`·` for a space, `→` for a tab).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WhitespaceRendering {
    None,
    /// Inside selections only.
    #[default]
    Selection,
    /// At the end of lines only.
    Trailing,
    /// All but single spaces between words.
    Boundary,
    All,
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
    /// Indentation folding, with fold markers in the gutter.
    pub folding: bool,
    pub indent_guides: bool,
    pub match_brackets: bool,
    /// Highlights the other occurrences of the word at the cursor.
    pub occurrences_highlight: bool,
    /// Highlights the other occurrences of the selected text.
    pub selection_highlight: bool,
    pub render_whitespace: WhitespaceRendering,
    /// Marks the `\r` of lines ending with `\r\n` (gitgud's visible CRLF).
    pub show_carriage_returns: bool,
    /// Colors brackets by nesting depth.
    pub bracket_pair_colorization: bool,
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
            folding: true,
            indent_guides: true,
            match_brackets: true,
            occurrences_highlight: true,
            selection_highlight: true,
            render_whitespace: WhitespaceRendering::default(),
            show_carriage_returns: false,
            bracket_pair_colorization: true,
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
    /// The find / replace widget (Ctrl+F, Ctrl+H).
    pub find: FindState,
    /// The go-to-line box (Ctrl+G).
    pub go_to_line: GoToLineState,
    pub folds: FoldState,
    bracket_depths: rustext_core::structure::BracketDepths,
    /// The text revision the view's caches follow; another one means the text changed outside
    /// the widget (`set_text`, an application command) and the caches start over.
    seen_text_revision: Option<u64>,
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
    /// Top of a display row (a line's row when there is one row per line).
    pub fn row_top(&self, row: usize) -> f32 {
        self.text_rect.top() + self.padding_top + row as f32 * self.row_height - self.scroll_offset.y
    }

    /// Center of the cell at `visual_column` on a display row, assuming a monospace font.
    pub fn cell_center(&self, row: usize, visual_column: usize) -> Pos2 {
        let x = self.text_rect.left() - self.scroll_offset.x + (visual_column as f32 + 0.5) * self.char_width;
        pos2(x, self.row_top(row) + self.row_height / 2.0)
    }

    /// Left edge of the cell at `visual_column` on a display row, vertically centered.
    pub fn cell_left(&self, row: usize, visual_column: usize) -> Pos2 {
        self.cell_center(row, visual_column) - vec2(self.char_width / 2.0, 0.0)
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
    /// A collapsed row standing for hidden lines.
    Collapsed {
        first_line: usize,
        last_line: usize,
    },
    /// A line of the foreign text (an inline diff's removed line).
    Foreign {
        line: usize,
    },
    /// A padding row.
    Padding,
    /// The fold marker of a fold range's header line.
    FoldMarker {
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

/// How bad a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

/// A diagnostic (Monaco's marker) as a decoration: a squiggle in the severity's color, an
/// overview mark, and the message on hover.
pub fn diagnostic(
    start: usize,
    end: usize,
    severity: Severity,
    message: impl Into<String>,
    theme: &EditorTheme,
) -> Decoration {
    let color = to_rgba(match severity {
        Severity::Error => theme.error,
        Severity::Warning => theme.warning,
        Severity::Info => theme.info,
        Severity::Hint => theme.hint,
    });
    Decoration::new(start, end, DecorationKind::Squiggle(color)).with_overview_ruler(color).with_hover_message(message)
}

/// Lines of another text shown in this editor's foreign rows.
pub struct Foreign<'a> {
    pub text: &'a Text,
    pub styler: Option<&'a mut dyn LineStyler>,
    pub decorations: &'a [Decoration],
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
    display_layout: Option<&'a DisplayLayout>,
    foreign: Option<Foreign<'a>>,
    glyph_margin: bool,
    markers: bool,
    id_salt: Option<String>,
    /// What commands run outside the keyboard and pointer handlers did this frame (the find
    /// widget's replacements), to go through `after_outcome` like the others.
    tracked_outcome: Outcome,
}

/// Sizes of one frame's layout.
#[derive(Clone, Copy, Debug)]
struct Metrics {
    font_size: f32,
    row_height: f32,
    char_width: f32,
    glyph_margin_width: f32,
    line_numbers_width: f32,
    marker_width: f32,
    fold_marker_width: f32,
    gutter_width: f32,
    text_rect: Rect,
    scrollbar_rect: Rect,
}

impl Metrics {
    fn font_id(&self) -> FontId {
        FontId::monospace(self.font_size)
    }

    fn full_rect(&self) -> Rect {
        Rect::from_min_max(
            pos2(self.text_rect.left() - self.gutter_width, self.text_rect.top()),
            self.scrollbar_rect.right_bottom(),
        )
    }

    fn gutter_rect(&self) -> Rect {
        Rect::from_min_max(
            pos2(self.text_rect.left() - self.gutter_width, self.text_rect.top()),
            self.text_rect.left_bottom(),
        )
    }
}

impl<'a> CodeEditor<'a> {
    pub fn new(
        state: &'a mut EditorState,
        view: &'a mut EditorView,
        config: &'a ViewConfig,
        theme: &'a EditorTheme,
        keymap: &'a Keymap,
    ) -> Self {
        Self {
            state,
            view,
            config,
            theme,
            keymap,
            styler: None,
            decorations: &[],
            display_layout: None,
            foreign: None,
            glyph_margin: false,
            markers: false,
            id_salt: None,
            tracked_outcome: Outcome::default(),
        }
    }

    pub fn styler(mut self, styler: &'a mut dyn LineStyler) -> Self {
        self.styler = Some(styler);
        self
    }

    pub fn decorations(mut self, decorations: &'a [Decoration]) -> Self {
        self.decorations = decorations;
        self
    }

    /// Paints these rows instead of one row per line.
    pub fn display_layout(mut self, display_layout: &'a DisplayLayout) -> Self {
        self.display_layout = Some(display_layout);
        self
    }

    /// The text that the layout's foreign rows show.
    pub fn foreign(mut self, foreign: Foreign<'a>) -> Self {
        self.foreign = Some(foreign);
        self
    }

    /// Reserves a margin left of the line numbers for glyph decorations.
    pub fn glyph_margin(mut self, glyph_margin: bool) -> Self {
        self.glyph_margin = glyph_margin;
        self
    }

    /// Reserves a column right of the line numbers for the rows' markers (`+`, `-`).
    pub fn markers(mut self, markers: bool) -> Self {
        self.markers = markers;
        self
    }

    /// Distinguishes editors laid out in the same place (each side of a diff).
    pub fn id_salt(mut self, id_salt: impl Into<String>) -> Self {
        self.id_salt = Some(id_salt.into());
        self
    }

    pub fn show(mut self, ui: &mut Ui) -> EditorResponse {
        let rect = ui.available_rect_before_wrap();
        let response = match &self.id_salt {
            Some(id_salt) => {
                let id = ui.make_persistent_id(id_salt.as_str());
                ui.advance_cursor_after_rect(rect);
                ui.interact(rect, id, Sense::click_and_drag())
            }
            None => ui.allocate_rect(rect, Sense::click_and_drag()),
        };
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

        if self.view.seen_text_revision != Some(self.state.text_revision()) {
            self.view.bracket_depths = Default::default();
            if let Some(styler) = self.styler.as_deref_mut() {
                styler.invalidate_from(0);
            }
        }
        self.refresh_folding();
        let mut outcome = Outcome::default();
        let mut unhandled_actions = Vec::new();
        let pointer_pos = ui.input(|input| input.pointer.hover_pos());
        let hovered = pointer_pos.filter(|pos| rect.contains(*pos)).map(|pos| self.target_at(ui, &metrics, pos));
        let pressed = self.handle_pointer(ui, &metrics, &response, &mut outcome);
        if has_focus || response.has_focus() {
            self.handle_keyboard(ui, &metrics, &mut outcome, &mut unhandled_actions);
        }
        self.after_outcome(ui, &outcome);
        self.handle_scroll(ui, &response);
        self.refresh_find_matches();
        if self.show_find_widgets(ui, &metrics, id) {
            response.request_focus();
        }
        let widget_outcome = std::mem::take(&mut self.tracked_outcome);
        self.after_outcome(ui, &widget_outcome);
        let outcome = outcome.merged(widget_outcome);
        self.refresh_folding();
        if self.view.reveal_primary_cursor {
            self.reveal_primary_cursor(ui, &metrics);
            self.view.reveal_primary_cursor = false;
        }
        self.clamp_scroll(&metrics);

        if matches!(hovered, Some(MouseTarget::Text { .. } | MouseTarget::Empty { .. })) {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }
        if matches!(hovered, Some(MouseTarget::Collapsed { .. })) {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let hovered_char = pointer_pos
            .filter(|pos| metrics.text_rect.contains(*pos))
            .and_then(|pos| self.char_under(ui, &metrics, pos));
        let hover_messages = hovered.map(|target| self.hover_messages(target, hovered_char)).unwrap_or_default();
        let response = if hover_messages.is_empty() {
            response
        } else {
            response.on_hover_ui_at_pointer(|ui| {
                for message in &hover_messages {
                    ui.label(message);
                }
            })
        };
        self.paint(ui, &metrics, response.has_focus());
        self.view.seen_text_revision = Some(self.state.text_revision());
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
        let foreign_line_count = self.foreign.as_ref().map_or(0, |foreign| foreign.text.line_count());
        let digits = self.state.text().line_count().max(foreign_line_count).to_string().len().max(2) as f32;
        let line_numbers_width = match self.config.line_numbers {
            LineNumbers::Off => char_width,
            LineNumbers::On | LineNumbers::Relative => (digits + 2.0) * char_width,
        };
        let glyph_margin_width = if self.glyph_margin { row_height } else { 0.0 };
        let marker_width = if self.markers { char_width * 2.0 } else { 0.0 };
        let fold_marker_width =
            if self.config.folding && self.display_layout.is_none() { char_width * 1.8 } else { 0.0 };
        let gutter_width = glyph_margin_width + line_numbers_width + marker_width + fold_marker_width + char_width;
        let scrollbar_rect =
            Rect::from_min_max(pos2(rect.right() - self.config.scrollbar_width, rect.top()), rect.right_bottom());
        let text_rect = Rect::from_min_max(
            pos2(rect.left() + gutter_width, rect.top()),
            pos2(scrollbar_rect.left(), rect.bottom()),
        );
        Metrics {
            font_size,
            row_height,
            char_width,
            glyph_margin_width,
            line_numbers_width,
            marker_width,
            fold_marker_width,
            gutter_width,
            text_rect,
            scrollbar_rect,
        }
    }

    /// Runs a command whose outcome goes through `after_outcome` at the end of the frame.
    fn execute_tracked(&mut self, command: rustext_core::Command) {
        let result = self.state.execute(command);
        self.tracked_outcome = std::mem::take(&mut self.tracked_outcome).merged(result);
    }

    /// Messages of the decorations under the pointer: on the text, or in the gutter for glyphs.
    fn hover_messages(&self, target: MouseTarget, hovered_char: Option<usize>) -> Vec<String> {
        let text = self.state.text();
        let decorations = self.decorations.iter().filter(|decoration| decoration.hover_message.is_some());
        let hovered: Vec<&Decoration> = match target {
            MouseTarget::Text { .. } => match hovered_char {
                Some(offset) => decorations
                    .filter(|decoration| {
                        decoration.start <= offset && offset < decoration.end.max(decoration.start + 1)
                    })
                    .collect(),
                None => Vec::new(),
            },
            MouseTarget::GlyphMargin { line } | MouseTarget::LineNumber { line } => decorations
                .filter(|decoration| {
                    let is_gutter_kind =
                        matches!(decoration.kind, DecorationKind::Glyph(_) | DecorationKind::LineNumberColor(_));
                    is_gutter_kind && decoration.first_line(text) == line
                })
                .collect(),
            _ => Vec::new(),
        };
        hovered.into_iter().filter_map(|decoration| decoration.hover_message.clone()).collect()
    }

    /// Moves the cursor to the next (or previous) squiggle, wrapping around (F8).
    fn go_to_problem(&mut self, forward: bool) -> Option<usize> {
        let mut starts: Vec<usize> = self
            .decorations
            .iter()
            .filter(|decoration| matches!(decoration.kind, DecorationKind::Squiggle(_)))
            .map(|decoration| decoration.start)
            .collect();
        starts.sort_unstable();
        starts.dedup();
        let head = self.state.selections().primary().head;
        if forward {
            starts.iter().find(|&&start| start > head).or(starts.first()).copied()
        } else {
            starts.iter().rev().find(|&&start| start < head).or(starts.last()).copied()
        }
    }

    // ---- rows ----

    /// The rows painted: the application's layout, else the folding one, else one per line.
    fn layout(&self) -> Option<&DisplayLayout> {
        self.display_layout.or(self.view.folds.layout.as_ref())
    }

    fn row_count(&self) -> usize {
        self.layout().map_or(self.state.text().line_count(), DisplayLayout::row_count)
    }

    fn row_content(&self, row: usize) -> RowContent {
        match self.layout() {
            Some(display_layout) => {
                display_layout.row(row).map_or(RowContent::Padding, |display_row| display_row.content)
            }
            None => RowContent::Line(row),
        }
    }

    fn row_style(&self, row: usize) -> (Option<Rgba>, Option<char>) {
        self.layout()
            .and_then(|display_layout| display_layout.row(row))
            .map_or((None, None), |display_row| (display_row.background, display_row.marker))
    }

    fn row_of_line(&self, line: usize) -> usize {
        self.layout().map_or(line, |display_layout| display_layout.row_of_line(line))
    }

    /// The text line a row stands for, or the nearest one for padding and foreign rows.
    fn nearest_line(&self, row: usize) -> usize {
        match self.layout() {
            Some(display_layout) => display_layout.nearest_line(row),
            None => row.min(self.state.text().last_line()),
        }
    }

    fn row_top(&self, metrics: &Metrics, row: usize) -> f32 {
        metrics.text_rect.top() + self.config.padding_top + row as f32 * metrics.row_height - self.view.scroll_offset.y
    }

    fn row_at_y(&self, metrics: &Metrics, y: f32) -> usize {
        let row = ((y - metrics.text_rect.top() - self.config.padding_top + self.view.scroll_offset.y)
            / metrics.row_height)
            .floor();
        (row.max(0.0) as usize).min(self.row_count().saturating_sub(1))
    }

    fn visible_rows(&self, metrics: &Metrics) -> std::ops::Range<usize> {
        let first = self.row_at_y(metrics, metrics.text_rect.top());
        let last = self.row_at_y(metrics, metrics.text_rect.bottom());
        first..last + 1
    }

    fn text_left(&self, metrics: &Metrics) -> f32 {
        metrics.text_rect.left() - self.view.scroll_offset.x
    }

    // ---- scrolling ----

    fn content_height(&self, metrics: &Metrics) -> f32 {
        self.row_count() as f32 * metrics.row_height + self.config.padding_top
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
        let cursor_top = self.config.padding_top + self.row_of_line(head.line) as f32 * metrics.row_height;
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
}

/// The line with its line ending, as a selection range (triple click, gutter click).
fn whole_line(text: &Text, line: usize) -> (usize, usize) {
    let end = if line < text.last_line() { text.line_start(line + 1) } else { text.len_chars() };
    (text.line_start(line), end)
}
