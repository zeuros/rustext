//! The `CodeEditor` widget: paints an `EditorState` and turns input into commands.
//!
//! The application owns the `EditorState` (text, selections, history) and an `EditorView`
//! (scroll, zoom, in-progress mouse drag…: Monaco's view state), and calls
//! `CodeEditor::new(...).show(ui)` every frame. What happened comes back in the
//! `EditorResponse` (DECISIONS D10).
//!
//! The editor paints display rows (DECISIONS D17): one per line by default, or the rows of a
//! `DisplayLayout` (padding, collapsed and foreign rows, for diffs and folding).

mod assist;
mod code_lens;
mod completion;
mod find;
mod input;
mod layout;
mod paint;
mod palette;
mod references;
mod smart_select;
mod structure;
mod wrapping;

use egui::{CursorIcon, EventFilter, FontId, Pos2, Rect, Response, Sense, Ui, Vec2, pos2, vec2};
use rustext_core::decoration::{Decoration, DecorationKind};
use rustext_core::display::{DisplayLayout, RowContent};
use rustext_core::services::CodeLens;
use rustext_core::style::{LineStyler, Rgba};
use rustext_core::{EditorState, Outcome, Position, Text};

use crate::keymap::{Action, KeyChord, Keymap};
use crate::theme::EditorTheme;

pub use assist::{RenameState, SignatureState};
pub use completion::CompletionState;
pub use find::{FindState, GoToLineState, parse_line_and_column};
pub use layout::{to_color, to_rgba};
pub use palette::PaletteState;
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

/// What folds (Monaco's `foldingStrategy`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FoldingStrategy {
    /// The syntax tree's blocks when the styler parses the text (D22), else indentation.
    #[default]
    Auto,
    Indentation,
}

/// When lines wrap (Monaco's `wordWrap`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WordWrap {
    #[default]
    Off,
    /// At the editor's width.
    Viewport,
    /// At this many columns.
    Column(usize),
    /// At the smaller of the editor's width and this many columns.
    Bounded(usize),
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
    /// Folding, with fold markers in the gutter.
    pub folding: bool,
    pub folding_strategy: FoldingStrategy,
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
    /// A line in the bracket's color along the multi-line bracket pair holding the cursor
    /// (Monaco's `guides.bracketPairs: 'active'`).
    pub bracket_pair_guides: bool,
    pub word_wrap: WordWrap,
    /// Pins the headers of the blocks enclosing the top line (Monaco's `stickyScroll`).
    /// Ask the language services for inlay hints (parameter names, inferred types).
    pub inlay_hints: bool,
    pub sticky_scroll: bool,
    pub sticky_scroll_max_lines: usize,
    /// A zoomed-out map of the text right of it (Monaco's `minimap`).
    pub minimap: bool,
    /// Shown while the text is empty ("Commit message").
    pub placeholder: Option<String>,
    /// What screen readers call the editor (Monaco's `ariaLabel`).
    pub aria_label: String,
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
            folding_strategy: FoldingStrategy::Auto,
            indent_guides: true,
            match_brackets: true,
            occurrences_highlight: true,
            selection_highlight: true,
            render_whitespace: WhitespaceRendering::default(),
            show_carriage_returns: false,
            bracket_pair_colorization: true,
            bracket_pair_guides: false,
            word_wrap: WordWrap::default(),
            inlay_hints: true,
            sticky_scroll: true,
            sticky_scroll_max_lines: 5,
            minimap: false,
            placeholder: None,
            aria_label: "Editor".to_string(),
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
    HorizontalScrollbar {
        grab_offset_x: f32,
    },
    Minimap,
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
    /// The completion list (Ctrl+Space, typing).
    pub completion: CompletionState,
    /// The command palette (F1, Ctrl+Shift+P).
    pub palette: PaletteState,
    /// The rename box (F2).
    pub rename: RenameState,
    /// The signature help tooltip.
    pub signature: SignatureState,
    pub inlay_hints: assist::InlayHintsState,
    /// The placeholders of the snippet being filled in, as decorations.
    snippet_placeholders: Vec<Decoration>,
    pub document_highlights: assist::DocumentHighlightsState,
    /// The references list (Shift+F12).
    pub references: references::ReferencesState,
    /// Smart select's history (Shift+Alt+Right / Left).
    pub smart_select: smart_select::SmartSelectState,
    wraps: wrapping::WrapState,
    /// Lines pinned at the top by sticky scroll in the last frame.
    sticky_lines: Vec<usize>,
    /// The language services' latest diagnostics, as decorations that follow edits.
    service_diagnostics: Vec<Decoration>,
    /// Suggested text at the cursor (inline completion), and the request waiting for one.
    ghost_text: Option<GhostText>,
    ghost_request: Option<(u64, usize)>,
    bracket_line_starts: rustext_core::structure::BracketLineStarts,
    /// The bracket pair holding the primary cursor, for (text revision, cursor offset).
    active_bracket_pair: Option<((u64, usize), Option<rustext_core::structure::EnclosingBracketPair>)>,
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

    /// Lines pinned at the top by sticky scroll in the last frame.
    pub fn sticky_lines(&self) -> &[usize] {
        &self.sticky_lines
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

/// An inline completion shown at `offset` for text revision `text_revision`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct GhostText {
    offset: usize,
    text_revision: u64,
    text: String,
}

/// Where the pointer is over the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseTarget {
    /// The horizontal scrollbar, over the bottom of the text when lines overflow.
    HorizontalScrollbar,
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
    /// A code lens: its index in the slice given to `CodeEditor::code_lenses`.
    CodeLens {
        index: usize,
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
    /// The minimap.
    Minimap,
    /// A header pinned at the top by sticky scroll.
    StickyLine {
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
    /// The index (in the slice given to `code_lenses`) of the code lens clicked this frame.
    pub clicked_code_lens: Option<usize>,
}

pub use rustext_core::services::Severity;

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
    code_lenses: &'a [CodeLens],
    /// The code lens under the pointer this frame, drawn brighter.
    hovered_code_lens: Option<usize>,
    display_layout: Option<&'a DisplayLayout>,
    foreign: Option<Foreign<'a>>,
    glyph_margin: bool,
    markers: bool,
    id_salt: Option<String>,
    /// What commands run outside the keyboard and pointer handlers did this frame (the find
    /// widget's replacements), to go through `after_outcome` like the others.
    tracked_outcome: Outcome,
    /// Lines of bracket depths this frame may still compute (D19); when it runs out, the next
    /// frame continues.
    bracket_budget_lines: usize,
    services: Option<&'a mut dyn rustext_core::services::LanguageServices>,
    /// What was typed this frame, for completion triggers.
    typed_this_frame: String,
    /// Overrides `ViewConfig::aria_label` (each side of a diff has its own name).
    aria_label: Option<String>,
}

const MINIMAP_WIDTH: f32 = 90.0;
/// Height of a row in the minimap, in points.
const MINIMAP_ROW_HEIGHT: f32 = 2.0;

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
    /// Empty (zero width) when the minimap is off.
    minimap_rect: Rect,
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
    const BRACKET_BUDGET_LINES_PER_FRAME: usize = 10_000;

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
            code_lenses: &[],
            hovered_code_lens: None,
            display_layout: None,
            foreign: None,
            glyph_margin: false,
            markers: false,
            id_salt: None,
            tracked_outcome: Outcome::default(),
            bracket_budget_lines: Self::BRACKET_BUDGET_LINES_PER_FRAME,
            services: None,
            typed_this_frame: String::new(),
            aria_label: None,
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

    /// Completion, hover and definitions for the text (DECISIONS D9).
    pub fn services(mut self, services: &'a mut dyn rustext_core::services::LanguageServices) -> Self {
        self.services = Some(services);
        self
    }

    /// Labels in a row above their lines; a click on one is `EditorResponse::clicked_code_lens`.
    pub fn code_lenses(mut self, code_lenses: &'a [CodeLens]) -> Self {
        self.code_lenses = code_lenses;
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

    /// What screen readers call this editor (overrides `ViewConfig::aria_label`).
    pub fn aria_label(mut self, aria_label: impl Into<String>) -> Self {
        self.aria_label = Some(aria_label.into());
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
            self.view.bracket_line_starts = Default::default();
            if let Some(styler) = self.styler.as_deref_mut() {
                styler.invalidate_from(0);
            }
        }
        self.refresh_rows(&metrics);
        let mut outcome = Outcome::default();
        let mut unhandled_actions = Vec::new();
        let pointer_pos = ui.input(|input| input.pointer.hover_pos());
        let hovered = pointer_pos.filter(|pos| rect.contains(*pos)).map(|pos| self.target_at(ui, &metrics, pos));
        self.hovered_code_lens = match hovered {
            Some(MouseTarget::CodeLens { index }) => Some(index),
            _ => None,
        };
        let pressed = self.handle_pointer(ui, &metrics, &response, &mut outcome);
        if has_focus || response.has_focus() {
            self.handle_keyboard(ui, &metrics, &mut outcome, &mut unhandled_actions);
        }
        self.after_outcome(ui, &outcome);
        self.refresh_service_diagnostics();
        let placeholder_background = to_rgba(self.theme.snippet_tab_stop_background);
        self.view.snippet_placeholders = self
            .state
            .snippet_placeholders()
            .into_iter()
            .map(|(start, end)| Decoration::new(start, end, DecorationKind::Background(placeholder_background)))
            .collect();
        if self.refresh_inlay_hints(&metrics) | self.refresh_document_highlights() | self.refresh_references() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
        }
        let typed = std::mem::take(&mut self.typed_this_frame);
        self.refresh_completion(&typed);
        self.refresh_ghost_text(&typed);
        self.refresh_signature_help(&typed);
        if self.view.ghost_request.is_some() || self.view.signature_is_waiting() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
        }
        if self.view.completion.is_waiting() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
        }
        self.handle_scroll(ui, &response);
        self.refresh_find_matches();
        if self.show_find_widgets(ui, &metrics, id) {
            response.request_focus();
        }
        let mut completion_outcome = Outcome::default();
        self.show_completion(ui, &metrics, id, &mut completion_outcome);
        if self.show_rename(ui, &metrics, id, &mut completion_outcome) {
            response.request_focus();
        }
        self.show_signature_help(ui, &metrics, id);
        self.show_references(ui, &metrics, id, &mut completion_outcome);
        let (picked_action, palette_closed) = self.show_palette(ui, &metrics, id);
        if palette_closed {
            response.request_focus();
        }
        if let Some(action) = picked_action {
            self.run_action(&metrics, action, &mut completion_outcome, &mut unhandled_actions);
        }
        let widget_outcome = std::mem::take(&mut self.tracked_outcome).merged(completion_outcome);
        self.after_outcome(ui, &widget_outcome);
        let outcome = outcome.merged(widget_outcome);
        self.refresh_rows(&metrics);
        if self.view.reveal_primary_cursor {
            self.reveal_primary_cursor(ui, &metrics);
            self.view.reveal_primary_cursor = false;
        }
        self.clamp_scroll(&metrics);

        if matches!(hovered, Some(MouseTarget::Text { .. } | MouseTarget::Empty { .. })) {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }
        if matches!(hovered, Some(MouseTarget::Collapsed { .. } | MouseTarget::CodeLens { .. })) {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        // Screen readers see a text input named after `aria_label`, valued with the cursor's line.
        let cursor_line =
            self.state.text().line_content(self.state.text().line_of_offset(self.state.selections().primary().head));
        let is_editable = !self.state.config.read_only;
        let aria_label = self.aria_label.clone().unwrap_or_else(|| self.config.aria_label.clone());
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::text_edit(is_editable, "", &cursor_line, "");
            info.label = Some(aria_label.clone());
            info
        });
        let hovered_char = pointer_pos
            .filter(|pos| metrics.text_rect.contains(*pos))
            .and_then(|pos| self.char_under(ui, &metrics, pos));
        let mut hover_messages = hovered.map(|target| self.hover_messages(target, hovered_char)).unwrap_or_default();
        if let (Some(offset), Some(services)) = (hovered_char, self.services.as_deref_mut()) {
            let (text, revision) = (self.state.text(), self.state.text_revision());
            if let rustext_core::services::Reply::Ready(Some(message)) =
                services.hover(text, revision, text.position(offset))
            {
                hover_messages.push(message);
            }
        }
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
        if self.bracket_budget_lines == 0 {
            ui.ctx().request_repaint();
        }
        self.view.seen_text_revision = Some(self.state.text_revision());
        self.view.geometry = Some(EditorGeometry {
            text_rect: metrics.text_rect,
            row_height: metrics.row_height,
            char_width: metrics.char_width,
            scroll_offset: self.view.scroll_offset,
            padding_top: self.config.padding_top,
        });

        let clicked_code_lens = match hovered {
            Some(MouseTarget::CodeLens { index }) if response.clicked() => Some(index),
            _ => None,
        };
        EditorResponse { response, outcome, pressed, hovered, unhandled_actions, clicked_code_lens }
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
        let minimap_width = if self.config.minimap && self.display_layout.is_none() { MINIMAP_WIDTH } else { 0.0 };
        let minimap_rect = Rect::from_min_max(
            pos2(scrollbar_rect.left() - minimap_width, rect.top()),
            pos2(scrollbar_rect.left(), rect.bottom()),
        );
        let text_rect =
            Rect::from_min_max(pos2(rect.left() + gutter_width, rect.top()), pos2(minimap_rect.left(), rect.bottom()));
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
            minimap_rect,
        }
    }

    /// Runs a command whose outcome goes through `after_outcome` at the end of the frame.
    fn execute_tracked(&mut self, command: rustext_core::Command) {
        let result = self.state.execute(command);
        self.tracked_outcome = std::mem::take(&mut self.tracked_outcome).merged(result);
    }

    /// The application's decorations and the language services' diagnostics and inlay hints.
    fn all_decorations(&self) -> impl Iterator<Item = &Decoration> {
        self.decorations
            .iter()
            .chain(self.view.service_diagnostics.iter())
            .chain(self.view.inlay_hints.decorations())
            .chain(self.view.snippet_placeholders.iter())
    }

    /// Asks for an inline completion after typing; drops a suggestion the cursor left.
    fn refresh_ghost_text(&mut self, typed: &str) {
        let primary = self.state.selections().primary();
        let (revision, head) = (self.state.text_revision(), primary.head);
        let is_still_valid = |ghost: &GhostText| ghost.offset == head && ghost.text_revision == revision;
        if self.view.ghost_text.as_ref().is_some_and(|ghost| !is_still_valid(ghost)) {
            self.view.ghost_text = None;
        }
        let can_suggest = primary.is_empty() && !self.state.selections().is_multiple();
        if !typed.is_empty() && can_suggest {
            self.view.ghost_request = Some((revision, head));
        }
        if self.view.ghost_request.is_some_and(|request| request != (revision, head)) && typed.is_empty() {
            self.view.ghost_request = None;
        }
        let (Some(request), Some(services)) = (self.view.ghost_request, self.services.as_deref_mut()) else { return };
        match services.inline_completion(self.state.text(), revision, self.state.text().position(head)) {
            rustext_core::services::Reply::Ready(suggestion) => {
                self.view.ghost_request = None;
                self.view.ghost_text = suggestion.filter(|suggestion| !suggestion.is_empty()).map(|text| GhostText {
                    offset: request.1,
                    text_revision: request.0,
                    text,
                });
            }
            rustext_core::services::Reply::Pending => {}
        }
    }

    /// Tab accepts the ghost text, Escape dismisses it; returns whether it took the key.
    fn ghost_text_key(&mut self, key: egui::Key, outcome: &mut Outcome) -> bool {
        let Some(ghost) = self.view.ghost_text.clone() else { return false };
        match key {
            egui::Key::Tab => {
                self.view.ghost_text = None;
                let result = self.state.execute(rustext_core::Command::Type(ghost.text));
                *outcome = std::mem::take(outcome).merged(result);
                true
            }
            egui::Key::Escape => {
                self.view.ghost_text = None;
                true
            }
            _ => false,
        }
    }

    /// Takes the diagnostics the services published, as decorations.
    fn refresh_service_diagnostics(&mut self) {
        let Some(services) = self.services.as_deref_mut() else { return };
        let text = self.state.text();
        let Some(diagnostics) = services.take_diagnostics(text) else { return };
        self.view.service_diagnostics = diagnostics
            .into_iter()
            .map(|item| {
                let (start, end) = text.range_offsets(item.range);
                diagnostic(
                    start,
                    end.max(start + 1).min(text.len_chars().max(start)),
                    item.severity,
                    item.message,
                    self.theme,
                )
            })
            .collect();
    }

    /// Messages of the decorations under the pointer: on the text, or in the gutter for glyphs.
    fn hover_messages(&self, target: MouseTarget, hovered_char: Option<usize>) -> Vec<String> {
        let text = self.state.text();
        let decorations = self.all_decorations().filter(|decoration| decoration.hover_message.is_some());
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

    /// The definition of the symbol at `position`, from the services.
    fn definition_of(&mut self, position: Position) -> Option<Position> {
        let services = self.services.as_deref_mut()?;
        match services.definition(self.state.text(), self.state.text_revision(), position) {
            rustext_core::services::Reply::Ready(definition) => definition,
            rustext_core::services::Reply::Pending => None,
        }
    }

    /// Moves the cursor to the next (or previous) squiggle, wrapping around (F8).
    fn go_to_problem(&mut self, forward: bool) -> Option<usize> {
        let mut starts: Vec<usize> = self
            .all_decorations()
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

    /// The line and columns a row paints: `None` for rows that don't show the editor's text.
    fn row_segment(&self, row: usize) -> Option<(usize, usize, usize)> {
        let (line, start_column) = match self.row_content(row) {
            RowContent::Line(line) => (line, 0),
            RowContent::Wrapped { line, start_column } => (line, start_column),
            _ => return None,
        };
        let end_column =
            self.view.wraps.points(line).iter().copied().find(|&point| point > start_column).unwrap_or(usize::MAX);
        Some((line, start_column, end_column))
    }

    /// The row showing column `column` of `line` (the later row at a wrap point).
    fn row_of_position(&self, line: usize, column: usize) -> usize {
        let segment_index = self.view.wraps.points(line).iter().take_while(|&&point| point <= column).count();
        self.row_of_line(line) + segment_index
    }

    /// Headers to pin at the top: those enclosing the line under the pinned area, scrolled
    /// above their place in it.
    fn compute_sticky_lines(&self, metrics: &Metrics) -> Vec<usize> {
        const MAX_SCANNED_LINES: usize = 10_000;
        if !self.config.sticky_scroll || self.display_layout.is_some() || self.view.scroll_offset.y <= 0.0 {
            return Vec::new();
        }
        let (text, indentation, max_lines) =
            (self.state.text(), &self.state.config.indentation, self.config.sticky_scroll_max_lines);
        let first_row = self.row_at_y(metrics, metrics.text_rect.top());
        let headers_at = |row: usize| {
            rustext_core::structure::enclosing_headers(
                text,
                indentation,
                self.nearest_line(row),
                max_lines,
                MAX_SCANNED_LINES,
            )
        };
        let headers = headers_at(first_row);
        let headers = headers_at(first_row + headers.len());
        headers
            .into_iter()
            .enumerate()
            .take_while(|&(slot, line)| self.row_of_line(line) < first_row + slot + 1)
            .map(|(_, line)| line)
            .collect()
    }

    /// The first row the minimap shows: it scrolls proportionally when the text is taller.
    fn minimap_first_row(&self, metrics: &Metrics) -> usize {
        let minimap_rows = (metrics.minimap_rect.height() / MINIMAP_ROW_HEIGHT) as usize;
        let hidden_rows = self.row_count().saturating_sub(minimap_rows);
        let fraction =
            if self.max_scroll_y(metrics) > 0.0 { self.view.scroll_offset.y / self.max_scroll_y(metrics) } else { 0.0 };
        (fraction * hidden_rows as f32).round() as usize
    }

    /// Scrolls so the row under `y` in the minimap is in the middle of the viewport.
    fn scroll_to_minimap_y(&mut self, metrics: &Metrics, y: f32) {
        let row =
            self.minimap_first_row(metrics) as f32 + ((y - metrics.minimap_rect.top()) / MINIMAP_ROW_HEIGHT).max(0.0);
        self.view.scroll_offset.y = row * metrics.row_height - metrics.text_rect.height() / 2.0;
    }

    /// The width rows wrap at, in columns (0: no wrapping).
    fn wrap_width_columns(&self, metrics: &Metrics) -> usize {
        if self.display_layout.is_some() {
            return 0;
        }
        let viewport_columns =
            ((metrics.text_rect.width() / metrics.char_width).floor() as usize).saturating_sub(1).max(10);
        match self.config.word_wrap {
            WordWrap::Off => 0,
            WordWrap::Viewport => viewport_columns,
            WordWrap::Column(columns) => columns.max(1),
            WordWrap::Bounded(columns) => columns.max(1).min(viewport_columns),
        }
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

    /// The lines the visible rows show (`None` when they show none of the editor's lines).
    fn visible_lines(&self, metrics: &Metrics) -> Option<std::ops::RangeInclusive<usize>> {
        let mut lines = self.visible_rows(metrics).filter_map(|row| self.row_segment(row).map(|(line, _, _)| line));
        let first = lines.next()?;
        Some(first..=lines.next_back().unwrap_or(first))
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

    fn max_scroll_x(&self, metrics: &Metrics) -> f32 {
        if self.view.wraps.is_wrapping() {
            return 0.0;
        }
        (self.view.widest_line_px + metrics.char_width * 4.0 - metrics.text_rect.width()).max(0.0)
    }

    /// The horizontal scrollbar's track: a strip over the bottom of the text, when lines overflow.
    fn horizontal_scrollbar_track(&self, metrics: &Metrics) -> Option<Rect> {
        const HEIGHT: f32 = 10.0;
        (self.max_scroll_x(metrics) > 0.0).then(|| {
            Rect::from_min_max(
                pos2(metrics.text_rect.left(), metrics.text_rect.bottom() - HEIGHT),
                metrics.text_rect.right_bottom(),
            )
        })
    }

    fn horizontal_scrollbar_thumb(&self, metrics: &Metrics) -> Option<Rect> {
        let track = self.horizontal_scrollbar_track(metrics)?;
        let content_width = self.max_scroll_x(metrics) + metrics.text_rect.width();
        let thumb_width = (track.width() * metrics.text_rect.width() / content_width)
            .clamp(20.0_f32.min(track.width()), track.width());
        let fraction = self.view.scroll_offset.x / self.max_scroll_x(metrics);
        let left = track.left() + fraction * (track.width() - thumb_width);
        Some(Rect::from_min_size(pos2(left, track.top()), vec2(thumb_width, track.height())))
    }

    fn clamp_scroll(&mut self, metrics: &Metrics) {
        let max_scroll_x = self.max_scroll_x(metrics);
        self.view.scroll_offset.x = self.view.scroll_offset.x.clamp(0.0, max_scroll_x);
        self.view.scroll_offset.y = self.view.scroll_offset.y.clamp(0.0, self.max_scroll_y(metrics));
    }

    fn reveal_primary_cursor(&mut self, ui: &Ui, metrics: &Metrics) {
        let text = self.state.text().clone();
        let head = text.position(self.state.selections().primary().head);
        let margin = self.config.cursor_surrounding_lines as f32 * metrics.row_height;
        let cursor_top =
            self.config.padding_top + self.row_of_position(head.line, head.column) as f32 * metrics.row_height;
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
