//! Positions and ranges in a text, both 0-based, columns counted in chars (DECISIONS D5).

use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

impl fmt::Display for Position {
    /// `line:column`, 1-based as editors show them to people.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.line + 1, self.column + 1)
    }
}

/// An ordered range: `start <= end` always holds, the constructor orders its ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(first: Position, second: Position) -> Self {
        Self { start: first.min(second), end: first.max(second) }
    }

    pub fn empty(position: Position) -> Self {
        Self { start: position, end: position }
    }

    /// Lines `first_line..=last_line`, from the start of the first to the end of the last.
    pub fn lines(first_line: usize, last_line: usize, last_line_len: usize) -> Self {
        Self::new(Position::new(first_line, 0), Position::new(last_line, last_line_len))
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn is_single_line(&self) -> bool {
        self.start.line == self.end.line
    }

    pub fn contains(&self, position: Position) -> bool {
        self.start <= position && position <= self.end
    }

    pub fn intersects(&self, other: &Range) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_orders_its_ends() {
        let range = Range::new(Position::new(3, 1), Position::new(1, 4));

        assert_eq!(range.start, Position::new(1, 4));
        assert_eq!(range.end, Position::new(3, 1));
    }

    #[test]
    fn displays_one_based() {
        assert_eq!(Position::new(0, 0).to_string(), "1:1");
    }
}
