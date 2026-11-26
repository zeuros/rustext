//! egui widgets of the rustext editor. See `docs/DECISIONS.md` (D3, D10, D15).

pub mod code_editor;
pub mod diff_editor;
pub mod keymap;
pub mod merge_editor;
pub mod theme;

pub use code_editor::{
    CodeEditor, EditorGeometry, EditorResponse, EditorView, Foreign, MouseTarget, Severity, ViewConfig, diagnostic,
};
pub use diff_editor::{DiffDocument, DiffEditor, DiffResponse, DiffSide, DiffViewConfig, DiffViewMode};
pub use keymap::{Action, Keymap};
pub use merge_editor::{MergeDocument, MergeEditor, MergeResponse};
pub use theme::EditorTheme;
