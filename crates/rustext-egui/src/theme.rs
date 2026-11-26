//! Editor colors as a value each editor holds (DECISIONS D6): no global theme.

use egui::Color32;

/// Colors of everything the editor paints except syntax tokens (which come with the tokens).
#[derive(Clone, Debug, PartialEq)]
pub struct EditorTheme {
    pub is_dark: bool,
    pub background: Color32,
    pub foreground: Color32,
    pub gutter_background: Color32,
    pub line_number: Color32,
    pub active_line_number: Color32,
    pub current_line_background: Color32,
    pub selection_background: Color32,
    pub inactive_selection_background: Color32,
    pub cursor: Color32,
    pub whitespace: Color32,
    pub indent_guide: Color32,
    pub active_indent_guide: Color32,
    pub ruler: Color32,
    pub matching_bracket_background: Color32,
    pub matching_bracket_border: Color32,
    pub fold_marker: Color32,
    pub find_match_background: Color32,
    pub current_find_match_background: Color32,
    pub occurrence_background: Color32,
    /// Behind the placeholders of the snippet being filled in.
    pub snippet_tab_stop_background: Color32,
    pub scrollbar_thumb: Color32,
    pub scrollbar_thumb_hovered: Color32,
    pub overview_ruler_border: Color32,
    pub fold_placeholder_background: Color32,
    pub widget_background: Color32,
    pub widget_border: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub info: Color32,
    pub hint: Color32,
    pub inserted_line_background: Color32,
    pub removed_line_background: Color32,
    pub inserted_text_background: Color32,
    pub removed_text_background: Color32,
    pub diff_padding_background: Color32,
    pub merge_change_background: Color32,
    pub merge_conflict_background: Color32,
    pub merge_resolved_background: Color32,
    /// Bracket pair colorization, cycled by nesting depth.
    pub bracket_pair_colors: Vec<Color32>,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

const fn hex_alpha(rgb: u32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, alpha)
}

impl EditorTheme {
    /// Close to VS Code's "Dark Modern".
    pub fn dark() -> Self {
        Self {
            is_dark: true,
            background: hex(0x1f1f1f),
            foreground: hex(0xcccccc),
            gutter_background: hex(0x1f1f1f),
            line_number: hex(0x6e7681),
            active_line_number: hex(0xcccccc),
            current_line_background: hex(0x282828),
            selection_background: hex_alpha(0x264f78, 255),
            inactive_selection_background: hex(0x3a3d41),
            cursor: hex(0xaeafad),
            whitespace: hex_alpha(0xe3e4e2, 0x29),
            indent_guide: hex(0x404040),
            active_indent_guide: hex(0x707070),
            ruler: hex(0x5a5a5a),
            matching_bracket_background: hex_alpha(0x006400, 0x60),
            matching_bracket_border: hex(0x888888),
            fold_marker: hex(0xc5c5c5),
            find_match_background: hex_alpha(0x623315, 0xb0),
            current_find_match_background: hex_alpha(0x9e6a03, 0xd0),
            occurrence_background: hex_alpha(0x575757, 0xb8),
            snippet_tab_stop_background: hex_alpha(0x7c7c7c, 0x4d),
            scrollbar_thumb: hex_alpha(0x797979, 0x66),
            scrollbar_thumb_hovered: hex_alpha(0x646464, 0xb3),
            overview_ruler_border: hex_alpha(0x7f7f7f, 0x4d),
            fold_placeholder_background: hex_alpha(0x264f78, 0x80),
            widget_background: hex(0x252526),
            widget_border: hex(0x454545),
            error: hex(0xf14c4c),
            warning: hex(0xcca700),
            info: hex(0x3794ff),
            hint: hex_alpha(0xeeeeee, 0xb3),
            inserted_line_background: hex_alpha(0x2a6a3a, 0x38),
            removed_line_background: hex_alpha(0x6a2a2a, 0x38),
            inserted_text_background: hex_alpha(0x2a6a3a, 0x80),
            removed_text_background: hex_alpha(0x8a2a2a, 0x80),
            diff_padding_background: hex(0x252525),
            merge_change_background: hex_alpha(0x1e82ff, 0x26),
            merge_conflict_background: hex_alpha(0xa04100, 0x48),
            merge_resolved_background: hex_alpha(0x64a04b, 0x40),
            bracket_pair_colors: vec![hex(0xffd700), hex(0xda70d6), hex(0x179fff)],
        }
    }

    /// Close to VS Code's "Light Modern".
    pub fn light() -> Self {
        Self {
            is_dark: false,
            background: hex(0xffffff),
            foreground: hex(0x3b3b3b),
            gutter_background: hex(0xffffff),
            line_number: hex(0x6e7681),
            active_line_number: hex(0x171184),
            current_line_background: hex(0xf3f3f3),
            selection_background: hex(0xadd6ff),
            inactive_selection_background: hex(0xe5ebf1),
            cursor: hex(0x000000),
            whitespace: hex_alpha(0x333333, 0x33),
            indent_guide: hex(0xd3d3d3),
            active_indent_guide: hex(0x939393),
            ruler: hex(0xd3d3d3),
            matching_bracket_background: hex_alpha(0x006400, 0x40),
            matching_bracket_border: hex(0xb9b9b9),
            fold_marker: hex(0x424242),
            find_match_background: hex_alpha(0xea5c00, 0x55),
            current_find_match_background: hex_alpha(0xa8ac94, 0xff),
            occurrence_background: hex_alpha(0x575757, 0x40),
            snippet_tab_stop_background: hex_alpha(0x0a3264, 0x33),
            scrollbar_thumb: hex_alpha(0x646464, 0x66),
            scrollbar_thumb_hovered: hex_alpha(0x646464, 0xb3),
            overview_ruler_border: hex_alpha(0x7f7f7f, 0x4d),
            fold_placeholder_background: hex_alpha(0xadd6ff, 0x80),
            widget_background: hex(0xf8f8f8),
            widget_border: hex(0xc8c8c8),
            error: hex(0xe51400),
            warning: hex(0xbf8803),
            info: hex(0x1a85ff),
            hint: hex(0x6c6c6c),
            inserted_line_background: hex_alpha(0x9ccc2c, 0x40),
            removed_line_background: hex_alpha(0xff0000, 0x26),
            inserted_text_background: hex_alpha(0x9ccc2c, 0x80),
            removed_text_background: hex_alpha(0xff0000, 0x50),
            diff_padding_background: hex(0xf3f3f3),
            merge_change_background: hex_alpha(0x1464dc, 0x1f),
            merge_conflict_background: hex_alpha(0xb45000, 0x2e),
            merge_resolved_background: hex_alpha(0x508c3c, 0x33),
            bracket_pair_colors: vec![hex(0x0431fa), hex(0x319331), hex(0x7b3814)],
        }
    }

    /// Gitgud's light theme (`gitgud-light` in its `monaco-utils.ts`).
    pub fn gitgud_light() -> Self {
        Self {
            background: hex(0xf8f6fc),
            gutter_background: hex(0xf8f6fc),
            current_line_background: hex(0xeee9f5),
            line_number: hex(0xb8a8c8),
            active_line_number: hex(0x8e4e8c),
            inserted_text_background: hex_alpha(0xc8f0d0, 0x80),
            removed_text_background: hex_alpha(0xf8d0d0, 0x80),
            inserted_line_background: hex_alpha(0xd4f0da, 0x60),
            removed_line_background: hex_alpha(0xf8dada, 0x60),
            ..Self::light()
        }
    }

    /// Gitgud's dark theme (`gitgud-dark` in its `monaco-utils.ts`).
    pub fn gitgud_dark() -> Self {
        Self {
            background: hex(0x1c1e23),
            gutter_background: hex(0x1c1e23),
            current_line_background: hex(0x2c2d37),
            line_number: hex(0x585e6e),
            active_line_number: hex(0xce93d8),
            inserted_text_background: hex_alpha(0x2a6a3a, 0x60),
            removed_text_background: hex_alpha(0x6a2a2a, 0x60),
            inserted_line_background: hex_alpha(0x2a6a3a, 0x38),
            removed_line_background: hex_alpha(0x6a2a2a, 0x38),
            ..Self::dark()
        }
    }
}

impl Default for EditorTheme {
    fn default() -> Self {
        Self::dark()
    }
}
