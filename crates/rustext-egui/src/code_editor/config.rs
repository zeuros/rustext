//! What a code editor shows and how (Monaco's `IEditorOptions` that concern the view).

use rustext_core::unicode::UnicodeHighlight;

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
#[expect(clippy::struct_excessive_bools, reason = "predates the complexity lints (D28)")]
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
    /// Which invisible and ambiguous characters are boxed (Monaco's `unicodeHighlight`, D32).
    pub unicode_highlight: UnicodeHighlight,
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
    /// Asks the language services for code lenses (Monaco's `codeLens`).
    pub code_lens: bool,
    /// Formats the code around the cursor after a char the language services name is typed
    /// (`}`, `;`, Enter; Monaco's `formatOnType`).
    pub format_on_type: bool,
    /// Formats pasted text (Monaco's `formatOnPaste`).
    pub format_on_paste: bool,
    /// A lightbulb in the gutter where the cursor rests on code actions (Monaco's
    /// `lightbulb.enabled`); a click opens them.
    pub lightbulb: bool,
    /// Underlines URLs; Ctrl+click opens them (Monaco's `links`).
    pub links: bool,
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
            unicode_highlight: UnicodeHighlight::default(),
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
            links: true,
            lightbulb: true,
            code_lens: true,
            format_on_type: false,
            format_on_paste: false,
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
