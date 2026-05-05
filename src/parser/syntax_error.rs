/// A diagnostic produced by the scanner or parser.
///
/// `line` and `column` are 1-indexed and point at the start of the error.
/// `length` is the number of source characters spanned by the error
/// (0 means "unknown / render as a single character" — the LSP falls
/// back to a one-character range in that case).
///
/// `note` and `help` are optional secondary lines shown alongside the
/// primary `message`. They follow the convention of Rust's diagnostic
/// formatter: `note:` is contextual ("this rule exists because…"),
/// `help:` is actionable ("did you mean…").
///
/// Construct via the [`new`](Self::new) constructor and chain the
/// `with_*` builder methods to attach optional context. The struct
/// fields are kept `pub` for backward-compatible reading by downstream
/// code (the LSP renders them directly), but new construction sites
/// should prefer the builders.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyntaxError {
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub length: usize,
    pub note: Option<String>,
    pub help: Option<String>,
}

impl SyntaxError {
    /// Build a new error at the given 1-indexed line/column with the
    /// given message. `length` defaults to 0 (single-char span);
    /// `note` and `help` default to `None`. Use the `with_*` builders
    /// to attach them.
    pub fn new(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            column,
            message: message.into(),
            length: 0,
            note: None,
            help: None,
        }
    }

    /// Set the source-character span this error covers. The LSP uses
    /// this to highlight the offending range; a value of 0 falls back
    /// to a one-character highlight.
    pub fn with_length(mut self, length: usize) -> Self {
        self.length = length;
        self
    }

    /// Attach a `note:` line — context for *why* this error exists.
    /// Convention: explain the rule, not the fix (use `with_help` for
    /// the fix).
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Attach a `help:` line — an actionable suggestion. Common shape:
    /// "did you mean `foo`?" for typos resolved via Levenshtein-1
    /// matching.
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sets_required_fields_and_defaults_optional_ones() {
        let err = SyntaxError::new(3, 7, "unexpected token");
        assert_eq!(err.line, 3);
        assert_eq!(err.column, 7);
        assert_eq!(err.message, "unexpected token");
        assert_eq!(err.length, 0);
        assert!(err.note.is_none());
        assert!(err.help.is_none());
    }

    #[test]
    fn with_length_sets_span() {
        let err = SyntaxError::new(1, 1, "msg").with_length(5);
        assert_eq!(err.length, 5);
    }

    #[test]
    fn with_note_attaches_note() {
        let err = SyntaxError::new(1, 1, "msg").with_note("explanation");
        assert_eq!(err.note.as_deref(), Some("explanation"));
    }

    #[test]
    fn with_help_attaches_help() {
        let err = SyntaxError::new(1, 1, "msg").with_help("did you mean `foo`?");
        assert_eq!(err.help.as_deref(), Some("did you mean `foo`?"));
    }

    #[test]
    fn builder_chain_composes() {
        let err = SyntaxError::new(2, 4, "oops")
            .with_length(3)
            .with_note("rule")
            .with_help("fix");
        assert_eq!(err.line, 2);
        assert_eq!(err.column, 4);
        assert_eq!(err.message, "oops");
        assert_eq!(err.length, 3);
        assert_eq!(err.note.as_deref(), Some("rule"));
        assert_eq!(err.help.as_deref(), Some("fix"));
    }

    #[test]
    fn message_accepts_owned_and_borrowed_strings() {
        let _ = SyntaxError::new(1, 1, "literal");
        let _ = SyntaxError::new(1, 1, "owned".to_string());
        let _ = SyntaxError::new(1, 1, String::from("borrowed-string"));
    }
}
