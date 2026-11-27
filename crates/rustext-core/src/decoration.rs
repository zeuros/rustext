//! Decorations: styled ranges the application puts on the text (DECISIONS D11). Their look
//! is data, and a `DecorationSet` follows edits through `ChangeSet`s.

use crate::change::{Bias, ChangeSet};
use crate::style::Rgba;
use crate::text::Text;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DecorationKind {
    /// Background behind the range's chars.
    Background(Rgba),
    /// Background of the whole lines the range covers, gutter included.
    WholeLineBackground(Rgba),
    Border(Rgba),
    Underline(Rgba),
    /// A wavy underline (diagnostics).
    Squiggle(Rgba),
    /// Color of the line number of the lines the range covers.
    LineNumberColor(Rgba),
    /// A dot in the glyph margin of the range's first line.
    Glyph(Rgba),
    /// A bar in the gutter, just left of the text, along the lines the range covers (VS Code's
    /// added / modified lines; Monaco's `linesDecorationsClassName`).
    GutterBar(Rgba),
    /// Text shown before the char at `start`, not part of the document: inlay hints, ghost
    /// text (Monaco's `before` / injected text). The cursor stays before it.
    InjectedText {
        text: String,
        color: Rgba,
    },
}

/// Whether typing at a decoration's edges extends it (Monaco's `TrackedRangeStickiness`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Stickiness {
    pub grows_at_start: bool,
    pub grows_at_end: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Decoration {
    /// Char offsets.
    pub start: usize,
    pub end: usize,
    pub kind: DecorationKind,
    /// A mark on the scrollbar at the range's first line.
    pub overview_ruler: Option<Rgba>,
    /// A mark behind the range in the minimap.
    pub minimap: Option<Rgba>,
    pub hover_message: Option<String>,
    pub stickiness: Stickiness,
}

impl Decoration {
    pub fn new(start: usize, end: usize, kind: DecorationKind) -> Self {
        Self {
            start: start.min(end),
            end: start.max(end),
            kind,
            overview_ruler: None,
            minimap: None,
            hover_message: None,
            stickiness: Stickiness::default(),
        }
    }

    /// Text shown at char offset `offset` without being part of the document.
    pub fn injected_text(offset: usize, text: impl Into<String>, color: Rgba) -> Self {
        Self::new(offset, offset, DecorationKind::InjectedText { text: text.into(), color })
    }

    /// A decoration covering lines `first_line..=last_line`.
    pub fn lines(text: &Text, first_line: usize, last_line: usize, kind: DecorationKind) -> Self {
        Self::new(text.line_start(first_line), text.line_content_end(last_line), kind)
    }

    pub fn with_overview_ruler(self, color: Rgba) -> Self {
        Self { overview_ruler: Some(color), ..self }
    }

    pub fn with_minimap(self, color: Rgba) -> Self {
        Self { minimap: Some(color), ..self }
    }

    pub fn with_hover_message(self, message: impl Into<String>) -> Self {
        Self { hover_message: Some(message.into()), ..self }
    }

    pub fn with_stickiness(self, stickiness: Stickiness) -> Self {
        Self { stickiness, ..self }
    }

    pub fn first_line(&self, text: &Text) -> usize {
        text.line_of_offset(self.start)
    }

    pub fn last_line(&self, text: &Text) -> usize {
        text.line_of_offset(self.end)
    }

    pub fn covers_line(&self, text: &Text, line: usize) -> bool {
        (self.first_line(text)..=self.last_line(text)).contains(&line)
    }

    /// The decoration after `changes`; `None` when its whole range was deleted (an empty
    /// decoration stays: a cursor-like marker).
    pub fn map(&self, changes: &ChangeSet) -> Option<Decoration> {
        let start_bias = if self.stickiness.grows_at_start { Bias::Before } else { Bias::After };
        let end_bias = if self.stickiness.grows_at_end { Bias::After } else { Bias::Before };
        let start = changes.map_offset(self.start, start_bias);
        let end = changes.map_offset(self.end, end_bias).max(start);
        let was_deleted = self.start < self.end && start == end;
        (!was_deleted).then(|| Decoration { start, end, ..self.clone() })
    }
}

/// Decorations an application keeps for one text, sorted by start.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DecorationSet {
    decorations: Vec<Decoration>,
}

impl DecorationSet {
    pub fn new(mut decorations: Vec<Decoration>) -> Self {
        decorations.sort_by_key(|decoration| (decoration.start, decoration.end));
        Self { decorations }
    }

    pub fn as_slice(&self) -> &[Decoration] {
        &self.decorations
    }

    pub fn is_empty(&self) -> bool {
        self.decorations.is_empty()
    }

    /// The set after `changes`, dropping decorations whose text was deleted.
    pub fn map(&self, changes: &ChangeSet) -> Self {
        Self::new(self.decorations.iter().filter_map(|decoration| decoration.map(changes)).collect())
    }

    /// Decorations touching char offsets `start..=end`.
    pub fn in_range(&self, start: usize, end: usize) -> impl Iterator<Item = &Decoration> {
        self.decorations
            .iter()
            .take_while(move |decoration| decoration.start <= end)
            .filter(move |decoration| decoration.end >= start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::change::Change;

    const RED: Rgba = Rgba::rgb(255, 0, 0);

    #[test]
    fn follows_edits_before_it() {
        let decoration = Decoration::new(4, 6, DecorationKind::Background(RED));
        let changes = ChangeSet::single(Change::insert(0, "ab"));

        let mapped = decoration.map(&changes).unwrap();

        assert_eq!((mapped.start, mapped.end), (6, 8));
    }

    #[test]
    fn does_not_grow_at_its_edges_by_default() {
        let decoration = Decoration::new(2, 4, DecorationKind::Background(RED));
        let changes = ChangeSet::new([Change::insert(2, "x"), Change::insert(4, "y")]);

        let mapped = decoration.map(&changes).unwrap();

        assert_eq!((mapped.start, mapped.end), (3, 5));
    }

    #[test]
    fn grows_when_sticky() {
        let sticky = Stickiness { grows_at_start: true, grows_at_end: true };
        let decoration = Decoration::new(2, 4, DecorationKind::Background(RED)).with_stickiness(sticky);
        let changes = ChangeSet::new([Change::insert(2, "x"), Change::insert(4, "y")]);

        let mapped = decoration.map(&changes).unwrap();

        assert_eq!((mapped.start, mapped.end), (2, 6));
    }

    #[test]
    fn is_dropped_when_its_text_is_deleted() {
        let decoration = Decoration::new(2, 4, DecorationKind::Background(RED));

        assert!(decoration.map(&ChangeSet::single(Change::delete(1, 5))).is_none());
    }

    #[test]
    fn in_range_finds_overlapping_decorations() {
        let set = DecorationSet::new(vec![
            Decoration::new(0, 2, DecorationKind::Background(RED)),
            Decoration::new(5, 9, DecorationKind::Background(RED)),
            Decoration::new(20, 22, DecorationKind::Background(RED)),
        ]);

        assert_eq!(set.in_range(3, 6).count(), 1);
    }
}
