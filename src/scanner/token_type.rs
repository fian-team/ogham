#[derive(Clone, Debug, PartialEq)]
pub enum TokenType {
    EOF,
    // Punctuation
    LeftParenthesis,
    RightParenthesis,
    LeftBracket,
    RightBracket,
    LeftSquareBracket,
    RightSquareBracket,
    Dot,
    Equal,
    EqualEqual, // ==
    Colon,
    Semicolon,
    Comma,
    // Arithmetic operators
    Plus,      // +
    Minus,     // -
    Multiply,  // *
    Divide,    // /
    Modulo,    // %
    Power,     // ^
    Increment, // ++
    Range,     // ..
    Spread,    // ...
    // Comparison operators
    GreaterThan,          // >
    GreaterThanOrEqualTo, // >=
    LessThan,             // <
    LessThanOrEqualTo,    // <=
    Not,                  // !
    NotEqual,             // !=
    /// `?` — postfix Optional marker in type positions
    /// (`string?`, `Item?`). Outside type positions the parser
    /// rejects it; the scanner just emits the token.
    Question,
    // Logical operators
    And, // &&
    Or,  // ||
    // Keywords
    State,
    Let,
    If,
    Else,
    Return,
    Log,
    Fn,
    For,
    In,
    Match,
    Import,
    From,
    // Typed-bindings keywords (Phase 1).
    /// `record` — declares a named structural type for use in
    /// `host_state` / `events` / record-field positions.
    Record,
    /// `host_state` — declares the schema of host-injected state
    /// for the module. Scanned as a single multi-character keyword
    /// (parallel to how `==` and `=>` scan as single tokens).
    HostState,
    /// `events` — declares the event signatures the module
    /// emits via `event(...)`.
    Events,
    /// `Self` — refers to the enclosing record type from inside
    /// its own field declarations. Legal only there.
    SelfTy,
    // Lifecycle hook keywords (Phase 2).
    /// `on_mount` — block statement inside an fn body. The body
    /// runs once when the function's call-stack path first
    /// becomes active. See `LIFECYCLE_AND_PORTAL.md`.
    OnMount,
    /// `on_unmount` — block statement inside an fn body. The body
    /// runs at drain-time when the function's call-stack path
    /// stops being active (after any exit animation completes).
    OnUnmount,
    // Match arms
    FatArrow, // =>
    // String delimiters
    Quote,
    // Literals
    Identifier(String),
    String(String),
    Integer(i32),
    Float(f64),
    Boolean(bool),
    // Other
    /// Scanner error token.
    ///
    /// This is emitted when the scanner encounters an unexpected character or
    /// an unterminated construct (e.g. string/comment).
    Error(String),
}
