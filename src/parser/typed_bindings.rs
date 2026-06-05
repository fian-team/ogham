//! AST nodes for the typed-bindings declarations introduced in
//! Phase 1: `record Foo { ... }`, `host_state { ... }`, and
//! `events { ... }`. Lives next to the rest of the parser so the
//! AST stays in one place; the *resolved* form (`ModuleSchema`)
//! lives in [`crate::runtime::schema`] and is built from these
//! nodes after parsing finishes.
//!
//! See [`docs/internal/TYPED_BINDINGS.md`](../../docs/internal/TYPED_BINDINGS.md)
//! for the design contract; this is the implementation of the
//! grammar listed there.
//!
//! The grammar these AST nodes correspond to (EBNF):
//!
//! ```ebnf
//! RecordDecl    ::= 'record' Ident '{' FieldList '}' ';'
//! HostStateDecl ::= 'host_state' '{' FieldList '}' ';'
//! EventsDecl    ::= 'events' '{' EventList '}' ';'
//!
//! FieldList     ::= (FieldDecl (',' FieldDecl)* ','?)?
//! FieldDecl     ::= Ident ':' TypeRef ('=' Literal)?
//!
//! EventList     ::= (EventSig (',' EventSig)* ','?)?
//! EventSig      ::= Ident '(' (TypeRef (',' TypeRef)* ','?)? ')'
//!
//! TypeRef       ::= PrimType
//!                 | RecordRef
//!                 | ContainerType
//!                 | TypeRef '?'
//! PrimType      ::= 'int' | 'float' | 'string' | 'bool'
//! RecordRef     ::= Ident
//! ContainerType ::= 'array' '<' TypeRef '>'
//!                 | 'map' '<' KeyType ',' TypeRef '>'
//!                 | 'Self'
//! KeyType       ::= 'string' | 'int'
//! ```

use super::span::Span;

/// A reference to a type, as written in source. May be unresolved
/// (a `Record(name)` whose declaration hasn't been validated yet);
/// the resolver in [`crate::runtime::schema`] checks that every
/// `Record` reference names a declared record.
#[derive(Clone, Debug, PartialEq)]
pub enum TypeRef {
    /// A primitive scalar type.
    Primitive(PrimType),
    /// A reference to a named record declared elsewhere in the
    /// module (or imported).
    Record(String),
    /// `array<T>`: an ordered collection of `T`.
    Array(Box<TypeRef>),
    /// `map<K, V>`: a hash map from `K` to `V`. `K` is restricted
    /// to `KeyType`.
    Map(KeyType, Box<TypeRef>),
    /// `T?`: optional. None on the wire is `Value::Void`; Some(v)
    /// is the underlying `Value`.
    Optional(Box<TypeRef>),
    /// `Self`: refers to the enclosing record. Legal *only* inside
    /// a `RecordDecl`'s field types; the resolver rejects it
    /// elsewhere.
    SelfRef,
}

/// The four primitive types in the schema's type universe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrimType {
    Int,
    Float,
    Bool,
    String,
}

/// Map key types. Floats are forbidden (NaN equality); records
/// are forbidden (v1 scope).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyType {
    String,
    Int,
}

/// A literal value that can appear as a default for a `FieldDecl`.
/// Phase 1 restricts defaults to literals only; expressions wait
/// for a compelling use case.
#[derive(Clone, Debug, PartialEq)]
pub enum SchemaLiteral {
    Int(i32),
    Float(f64),
    Bool(bool),
    String(String),
}

/// A single field within a `record` or `host_state` block.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldDecl {
    pub name: String,
    pub ty: TypeRef,
    pub default: Option<SchemaLiteral>,
    pub span: Span,
}

/// A single event signature within an `events` block.
#[derive(Clone, Debug, PartialEq)]
pub struct EventSigDecl {
    pub name: String,
    pub args: Vec<TypeRef>,
    pub span: Span,
}

/// A `record Foo { ... };` declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordDecl {
    pub name: String,
    pub fields: Vec<FieldDecl>,
    pub span: Span,
}

/// A `host_state { ... };` declaration. At most one per module.
#[derive(Clone, Debug, PartialEq)]
pub struct HostStateDecl {
    pub fields: Vec<FieldDecl>,
    pub span: Span,
}

/// An `events { ... };` declaration. At most one per module.
#[derive(Clone, Debug, PartialEq)]
pub struct EventsDecl {
    pub events: Vec<EventSigDecl>,
    pub span: Span,
}

// ---------------------------------------------------------------------
// TypeRef canonical-string rendering and round-trip parser.
//
// The canonical form is the exact surface syntax a user types in `.ogh`
// source: `int`, `array<T>`, `map<K, V>`, `T?`, `Self`, or a bare
// identifier for a record reference. This single source of truth is
// consumed by:
//
// - LSP hover and compiler error messages (via the existing
//   `format_type_ref` helpers, which now delegate here).
// - The schema-diagnostics manifest format (P0-M2 onwards): macros
//   serialize TypeRefs to canonical strings; the diagnostic backend
//   parses them back when running schema-match against a `.ogh` file.
//
// Round-trip identity is a property test invariant: every constructible
// TypeRef must satisfy `from_canonical_string(t.to_canonical_string()) == t`.
// ---------------------------------------------------------------------

impl TypeRef {
    /// Render this type to its canonical string form — the exact
    /// surface syntax users type in `.ogh` source. Output is
    /// deterministic and stable; callers may persist it.
    pub fn to_canonical_string(&self) -> String {
        match self {
            TypeRef::Primitive(PrimType::Int) => "int".to_string(),
            TypeRef::Primitive(PrimType::Float) => "float".to_string(),
            TypeRef::Primitive(PrimType::Bool) => "bool".to_string(),
            TypeRef::Primitive(PrimType::String) => "string".to_string(),
            TypeRef::Record(name) => name.clone(),
            TypeRef::Array(inner) => format!("array<{}>", inner.to_canonical_string()),
            TypeRef::Map(k, v) => {
                let k = match k {
                    KeyType::String => "string",
                    KeyType::Int => "int",
                };
                format!("map<{}, {}>", k, v.to_canonical_string())
            }
            TypeRef::Optional(inner) => format!("{}?", inner.to_canonical_string()),
            TypeRef::SelfRef => "Self".to_string(),
        }
    }

    /// Parse a canonical-string TypeRef. Whitespace-tolerant on read;
    /// the renderer never emits stray whitespace except the `, ` after
    /// the comma in `map<K, V>`. Returns a structured error so callers
    /// can render position-bearing diagnostics rather than raw strings.
    ///
    /// **Edge case — keyword-named records.** A `TypeRef::Record(name)`
    /// whose `name` matches a reserved word (`int`, `float`, `bool`,
    /// `string`, `array`, `map`, `Self`) won't round-trip through this
    /// parser — the rendered string would be re-parsed as the keyword.
    /// In practice this can't happen from a real `.ogh` source because
    /// those words are reserved on the front-end too; it's only
    /// reachable if a user constructs `TypeRef::Record("array")`
    /// programmatically or names a Rust struct `array` (legal but
    /// vanishingly unusual). The schema-diagnostic backend would
    /// surface the resulting mismatch as a regular binding error.
    pub fn from_canonical_string(s: &str) -> Result<Self, TypeRefParseError> {
        if s.trim().is_empty() {
            return Err(TypeRefParseError::Empty);
        }
        let mut parser = TypeRefParser { input: s, pos: 0 };
        let result = parser.parse_type_ref()?;
        parser.skip_ws();
        if parser.pos < s.len() {
            return Err(TypeRefParseError::TrailingInput {
                rest: s[parser.pos..].to_string(),
            });
        }
        Ok(result)
    }
}

impl std::fmt::Display for TypeRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_canonical_string())
    }
}

/// Structured error returned by [`TypeRef::from_canonical_string`].
#[derive(Clone, Debug, PartialEq)]
pub enum TypeRefParseError {
    /// Input was empty or only whitespace.
    Empty,
    /// A non-identifier character appeared where an identifier or
    /// keyword was expected.
    UnexpectedChar { ch: char, pos: usize },
    /// Input ended mid-construct (e.g. `array<int` with no closing
    /// `>`).
    UnexpectedEnd,
    /// `map<K, …>` with `K` outside `{string, int}`.
    InvalidMapKey(String),
    /// `array` or `map` not followed by `<…>`.
    MissingAngleBrackets { keyword: &'static str },
    /// Valid TypeRef parsed but extra characters remained.
    TrailingInput { rest: String },
}

impl std::fmt::Display for TypeRefParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("empty TypeRef"),
            Self::UnexpectedChar { ch, pos } => {
                write!(f, "unexpected character `{ch}` at position {pos}")
            }
            Self::UnexpectedEnd => f.write_str("unexpected end of input"),
            Self::InvalidMapKey(k) => {
                write!(f, "invalid map key type `{k}` (expected `string` or `int`)")
            }
            Self::MissingAngleBrackets { keyword } => {
                write!(f, "`{keyword}` requires angle-bracketed type arguments")
            }
            Self::TrailingInput { rest } => {
                write!(f, "trailing input after TypeRef: `{rest}`")
            }
        }
    }
}

impl std::error::Error for TypeRefParseError {}

struct TypeRefParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> TypeRefParser<'a> {
    fn parse_type_ref(&mut self) -> Result<TypeRef, TypeRefParseError> {
        let mut t = self.parse_base()?;
        loop {
            self.skip_ws();
            if self.peek_char() == Some('?') {
                self.bump_char();
                t = TypeRef::Optional(Box::new(t));
            } else {
                break;
            }
        }
        Ok(t)
    }

    fn parse_base(&mut self) -> Result<TypeRef, TypeRefParseError> {
        self.skip_ws();
        let ident = self.parse_ident()?;
        match ident.as_str() {
            "int" => Ok(TypeRef::Primitive(PrimType::Int)),
            "float" => Ok(TypeRef::Primitive(PrimType::Float)),
            "bool" => Ok(TypeRef::Primitive(PrimType::Bool)),
            "string" => Ok(TypeRef::Primitive(PrimType::String)),
            "Self" => Ok(TypeRef::SelfRef),
            "array" => {
                self.skip_ws();
                if self.peek_char() != Some('<') {
                    return Err(TypeRefParseError::MissingAngleBrackets { keyword: "array" });
                }
                self.bump_char();
                let inner = self.parse_type_ref()?;
                self.skip_ws();
                self.expect_char('>')?;
                Ok(TypeRef::Array(Box::new(inner)))
            }
            "map" => {
                self.skip_ws();
                if self.peek_char() != Some('<') {
                    return Err(TypeRefParseError::MissingAngleBrackets { keyword: "map" });
                }
                self.bump_char();
                let key = self.parse_key_type()?;
                self.skip_ws();
                self.expect_char(',')?;
                let value = self.parse_type_ref()?;
                self.skip_ws();
                self.expect_char('>')?;
                Ok(TypeRef::Map(key, Box::new(value)))
            }
            _ => Ok(TypeRef::Record(ident)),
        }
    }

    fn parse_key_type(&mut self) -> Result<KeyType, TypeRefParseError> {
        self.skip_ws();
        let ident = self.parse_ident()?;
        match ident.as_str() {
            "string" => Ok(KeyType::String),
            "int" => Ok(KeyType::Int),
            other => Err(TypeRefParseError::InvalidMapKey(other.to_string())),
        }
    }

    fn parse_ident(&mut self) -> Result<String, TypeRefParseError> {
        self.skip_ws();
        let bytes = self.input.as_bytes();
        if self.pos >= bytes.len() {
            return Err(TypeRefParseError::UnexpectedEnd);
        }
        let first = bytes[self.pos];
        if !(first.is_ascii_alphabetic() || first == b'_') {
            return Err(TypeRefParseError::UnexpectedChar {
                ch: self.input[self.pos..].chars().next().unwrap(),
                pos: self.pos,
            });
        }
        let start = self.pos;
        self.pos += 1;
        while self.pos < bytes.len() {
            let b = bytes[self.pos];
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        Ok(self.input[start..self.pos].to_string())
    }

    fn skip_ws(&mut self) {
        let bytes = self.input.as_bytes();
        while self.pos < bytes.len() && bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn bump_char(&mut self) {
        if let Some(c) = self.peek_char() {
            self.pos += c.len_utf8();
        }
    }

    fn expect_char(&mut self, ch: char) -> Result<(), TypeRefParseError> {
        self.skip_ws();
        match self.peek_char() {
            Some(c) if c == ch => {
                self.pos += c.len_utf8();
                Ok(())
            }
            Some(c) => Err(TypeRefParseError::UnexpectedChar {
                ch: c,
                pos: self.pos,
            }),
            None => Err(TypeRefParseError::UnexpectedEnd),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(t: TypeRef, expected: &str) {
        // Round-trip in both directions: rendering produces the
        // expected canonical form, and parsing it back yields the
        // original value.
        let rendered = t.to_canonical_string();
        assert_eq!(rendered, expected, "render mismatch for {t:?}");
        let parsed = TypeRef::from_canonical_string(&rendered)
            .unwrap_or_else(|e| panic!("parse failed for `{rendered}`: {e}"));
        assert_eq!(parsed, t, "round-trip lost identity for `{rendered}`");
    }

    #[test]
    fn round_trip_primitives() {
        rt(TypeRef::Primitive(PrimType::Int), "int");
        rt(TypeRef::Primitive(PrimType::Float), "float");
        rt(TypeRef::Primitive(PrimType::Bool), "bool");
        rt(TypeRef::Primitive(PrimType::String), "string");
    }

    #[test]
    fn round_trip_self_ref() {
        rt(TypeRef::SelfRef, "Self");
    }

    #[test]
    fn round_trip_records() {
        rt(TypeRef::Record("Item".to_string()), "Item");
        rt(
            TypeRef::Record("PlayerCharacter".to_string()),
            "PlayerCharacter",
        );
        rt(TypeRef::Record("snake_record".to_string()), "snake_record");
    }

    #[test]
    fn round_trip_array() {
        rt(
            TypeRef::Array(Box::new(TypeRef::Primitive(PrimType::Int))),
            "array<int>",
        );
        rt(
            TypeRef::Array(Box::new(TypeRef::Record("Item".to_string()))),
            "array<Item>",
        );
        rt(
            TypeRef::Array(Box::new(TypeRef::Array(Box::new(TypeRef::Primitive(
                PrimType::Int,
            ))))),
            "array<array<int>>",
        );
    }

    #[test]
    fn round_trip_map_each_key_type() {
        rt(
            TypeRef::Map(KeyType::String, Box::new(TypeRef::Primitive(PrimType::Int))),
            "map<string, int>",
        );
        rt(
            TypeRef::Map(KeyType::Int, Box::new(TypeRef::Record("Item".to_string()))),
            "map<int, Item>",
        );
    }

    #[test]
    fn round_trip_optional() {
        rt(
            TypeRef::Optional(Box::new(TypeRef::Primitive(PrimType::Int))),
            "int?",
        );
        rt(
            TypeRef::Optional(Box::new(TypeRef::Array(Box::new(TypeRef::Record(
                "Item".to_string(),
            ))))),
            "array<Item>?",
        );
        // T?? is well-formed though semantically silly; we accept
        // for round-trip simplicity.
        rt(
            TypeRef::Optional(Box::new(TypeRef::Optional(Box::new(TypeRef::Primitive(
                PrimType::Int,
            ))))),
            "int??",
        );
    }

    #[test]
    fn round_trip_combo() {
        // array<map<string, Item>?>
        let combo = TypeRef::Array(Box::new(TypeRef::Optional(Box::new(TypeRef::Map(
            KeyType::String,
            Box::new(TypeRef::Record("Item".to_string())),
        )))));
        rt(combo, "array<map<string, Item>?>");
    }

    #[test]
    fn parser_tolerates_internal_whitespace() {
        // The renderer never emits this much whitespace, but the
        // parser accepts it on the read path.
        let parsed = TypeRef::from_canonical_string(" array < map < string , Item > ? > ").unwrap();
        assert_eq!(
            parsed,
            TypeRef::Array(Box::new(TypeRef::Optional(Box::new(TypeRef::Map(
                KeyType::String,
                Box::new(TypeRef::Record("Item".to_string())),
            ))))),
        );
    }

    #[test]
    fn parser_rejects_empty() {
        assert_eq!(
            TypeRef::from_canonical_string(""),
            Err(TypeRefParseError::Empty),
        );
        assert_eq!(
            TypeRef::from_canonical_string("   "),
            Err(TypeRefParseError::Empty),
        );
    }

    #[test]
    fn parser_rejects_malformed_array() {
        assert!(matches!(
            TypeRef::from_canonical_string("array"),
            Err(TypeRefParseError::MissingAngleBrackets { keyword: "array" }),
        ));
        // `array<>` — bare close angle where a TypeRef is expected.
        assert!(matches!(
            TypeRef::from_canonical_string("array<>"),
            Err(TypeRefParseError::UnexpectedChar { .. }),
        ));
        // `array<int` — missing close angle.
        assert!(matches!(
            TypeRef::from_canonical_string("array<int"),
            Err(TypeRefParseError::UnexpectedEnd),
        ));
    }

    #[test]
    fn parser_rejects_invalid_map_key() {
        assert!(matches!(
            TypeRef::from_canonical_string("map<float, int>"),
            Err(TypeRefParseError::InvalidMapKey(_)),
        ));
        // bool also forbidden as map key in this grammar
        assert!(matches!(
            TypeRef::from_canonical_string("map<bool, int>"),
            Err(TypeRefParseError::InvalidMapKey(_)),
        ));
    }

    #[test]
    fn parser_rejects_malformed_map() {
        assert!(matches!(
            TypeRef::from_canonical_string("map"),
            Err(TypeRefParseError::MissingAngleBrackets { keyword: "map" }),
        ));
        // `map<int>` — missing comma + value.
        assert!(matches!(
            TypeRef::from_canonical_string("map<int>"),
            Err(TypeRefParseError::UnexpectedChar { .. }),
        ));
    }

    #[test]
    fn parser_rejects_trailing_input() {
        assert!(matches!(
            TypeRef::from_canonical_string("int garbage"),
            Err(TypeRefParseError::TrailingInput { .. }),
        ));
    }

    #[test]
    fn parser_rejects_leading_punctuation() {
        // `<int>` — angle brackets aren't a top-level construct.
        assert!(matches!(
            TypeRef::from_canonical_string("<int>"),
            Err(TypeRefParseError::UnexpectedChar { .. }),
        ));
    }
}
