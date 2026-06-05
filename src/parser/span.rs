/// Source location span for an AST node.
///
/// All line and column values are 1-indexed, matching the scanner's convention.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

impl Span {
    pub fn new(start_line: usize, start_column: usize, end_line: usize, end_column: usize) -> Self {
        Span {
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }

    /// Merge two spans into one that covers both.
    pub fn merge(a: &Span, b: &Span) -> Span {
        let (start_line, start_column) = if a.start_line < b.start_line
            || (a.start_line == b.start_line && a.start_column <= b.start_column)
        {
            (a.start_line, a.start_column)
        } else {
            (b.start_line, b.start_column)
        };

        let (end_line, end_column) = if a.end_line > b.end_line
            || (a.end_line == b.end_line && a.end_column >= b.end_column)
        {
            (a.end_line, a.end_column)
        } else {
            (b.end_line, b.end_column)
        };

        Span {
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }

    /// A placeholder span for synthetic/generated nodes.
    pub fn zero() -> Span {
        Span {
            start_line: 0,
            start_column: 0,
            end_line: 0,
            end_column: 0,
        }
    }
}
