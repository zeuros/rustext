//! egui widgets of the rustext editor. See `docs/DECISIONS.md` (D3, D10, D15).

pub mod code_editor;
pub mod keymap;
pub mod theme;

pub use code_editor::{CodeEditor, EditorGeometry, EditorResponse, EditorView, MouseTarget, ViewConfig};
pub use keymap::{Action, Keymap};
pub use theme::EditorTheme;
