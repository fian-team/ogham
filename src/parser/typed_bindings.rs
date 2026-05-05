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
