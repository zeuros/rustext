//! UI-agnostic core of the rustext editor: text, positions, selections, edits, history and
//! commands. See `docs/DECISIONS.md` at the repository root for the design.

pub mod change;
pub mod comment;
pub mod config;
pub mod cursors;
pub mod decoration;
pub mod display;
pub mod edit;
pub mod find;
pub mod history;
pub mod indentation;
pub mod lexical;
pub mod lines;
pub mod motion;
pub mod position;
pub mod selection;
pub mod services;
pub mod snippet;
pub mod state;
pub mod structure;
pub mod style;
pub mod text;
pub mod words;
pub mod wrap;

pub use change::{Bias, Change, ChangeSet};
pub use config::{EditingConfig, LanguageConfiguration};
pub use cursors::VerticalDirection;
pub use edit::{DeleteUnit, Transaction};
pub use motion::Motion;
pub use position::{Position, Range};
pub use selection::{Selection, Selections};
pub use state::{Command, EditedLines, EditorState, Outcome};
pub use text::{LineEnding, Text};
