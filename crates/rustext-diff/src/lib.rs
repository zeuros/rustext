//! Diff computation for the rustext diff editor (DECISIONS D14): line hunks with inner word
//! changes, side-by-side and inline row layouts, collapsed unchanged regions, and patches of
//! selected lines for staging with git.

pub mod display;
pub mod layout;
pub mod line_diff;
pub mod merge;
pub mod moves;
pub mod patch;

pub use display::{
    Collapsing, DiffColors, ReadLine, ReadLineKind, WrapPoints, hunk_read_lines, inline_layout,
    inner_change_decorations, move_code_lenses, side_by_side_layouts,
};
pub use layout::{
    CollapseOptions, InlineRow, RowKind, Segment, SideBySideRow, collapse_unchanged, inline_rows, side_by_side_rows,
};
pub use line_diff::{DiffOptions, Hunk, InnerChange, LineDiff, LineRange, diff_lines};
pub use moves::MovedBlock;
pub use patch::{LineSelection, PatchDirection, build_patch};
