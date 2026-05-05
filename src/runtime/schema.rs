//! Module schema — the resolved form of a `.ogh` module's
//! typed-bindings declarations (`record`, `host_state`, `events`).
//!
//! The parser produces an AST under
//! [`crate::parser::typed_bindings`]; this module walks that AST
//! to produce a [`ModuleSchema`] suitable for:
//!
//! - The compiler (M2): strict-mode identifier resolution,
//!   field-access checking, event-call signature checking.
//! - The LSP (M3): hover, completion, go-to-definition.
//! - `TypedOgham::watch_typed` (M5): startup schema-match check
//!   against `#[derive(OghamState)]` / `#[derive(OghamMsg)]` Rust
//!   types.
//!
//! ## Schema construction is two-pass
//!
//! Pass 1 collects all `record` declarations into a name → schema
//! map (so forward references work).
//! Pass 2 walks every `TypeRef` and verifies that each
//! `Record(name)` reference resolves to a declared (or imported)
//! record name. Pass 2 also detects direct self-references.
//!
//! Cross-module record imports (`import [Item] from
//! "./inv.ogh"`) are wired in by the parser-level import path; the
//! resolver here just trusts that imported records are present in
//! the supplied `imports` map.
//!
//! ## Strict mode is detected by presence
//!
//! A module is in *strict mode* iff it has a `host_state {}`
//! declaration. [`ModuleSchema::is_strict`] is the single source
//! of truth — the rest of the runtime never carries a separate
//! flag.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use crate::parser::span::Span;
use crate::parser::{Parser, SyntaxError};
use crate::scanner::Scanner;
use crate::parser::typed_bindings::{
    EventsDecl, FieldDecl, HostStateDecl, KeyType, PrimType, RecordDecl, SchemaLiteral, TypeRef,
};
use crate::parser::{Function, Statement};

/// The resolved schema for one module.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModuleSchema {
    /// Records declared in this module, keyed by name. `BTreeMap`
    /// for stable ordering in error messages and debug output.
    pub records: BTreeMap<String, RecordSchema>,
    /// The module's host_state schema, or `None` in loose mode.
    pub host_state: Option<RecordSchema>,
    /// Events the module declares it emits, keyed by name.
    pub events: BTreeMap<String, EventSig>,
    /// Records imported from other modules, keyed by their name in
    /// *this* module's namespace (after any `as` aliasing). Phase 1
    /// scope: aliasing is not yet wired through the import grammar;
    /// names match the source module's declarations.
    pub imports: BTreeMap<String, RecordSchema>,
}

/// A resolved record: ordered by field name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecordSchema {
    /// Fields keyed by name. `BTreeMap` makes iteration order stable
    /// so derived snapshot/diff code on the macro side can rely on
    /// it.
    pub fields: BTreeMap<String, FieldSchema>,
    /// Span of the originating declaration, for diagnostic
    /// rendering. `None` for synthesized schemas (e.g.
    /// `host_state` doesn't have an enclosing `record` name).
    pub decl_span: Option<Span>,
}

/// A single field within a record or host_state.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldSchema {
    pub ty: TypeRef,
    /// Phase 1: literal default. Documented in the schema but
    /// only consulted on the loose path; typed mode supplies all
    /// fields from the Rust struct.
    pub default: Option<SchemaLiteral>,
    pub decl_span: Span,
}

/// A single declared event signature.
#[derive(Clone, Debug, PartialEq)]
pub struct EventSig {
    pub args: Vec<TypeRef>,
    pub decl_span: Span,
}

impl ModuleSchema {
    /// True iff the source declared *any* schema block — either
    /// `host_state {}` or `events {}`. The compiler's event-call
    /// validation keys off this (a module that declares its events
    /// shouldn't be allowed to emit undeclared ones, even if it
    /// hasn't also declared host_state).
    pub fn is_strict(&self) -> bool {
        self.host_state.is_some() || !self.events.is_empty()
    }

    /// True iff the source declared `host_state {}`. Identifier
    /// resolution against a host_state field list requires this —
    /// without it, the compiler can't enumerate valid bare
    /// identifiers, so it stays loose.
    pub fn has_host_state(&self) -> bool {
        self.host_state.is_some()
    }

    /// Look up a record by name in this module. Walks the local
    /// declarations first, then imports.
    pub fn lookup_record(&self, name: &str) -> Option<&RecordSchema> {
        self.records.get(name).or_else(|| self.imports.get(name))
    }

    /// Build a schema from a parsed module. Walks the top-level
    /// statement list, collects declarations, and runs the
    /// two-pass resolver. Returns either a complete schema or a
    /// `SyntaxError` describing the first resolution failure.
    ///
    /// This is the canonical entry point used by the Runtime when
    /// it loads a module. The LSP uses the standalone
    /// [`load_schema`] for cross-module imports without touching
    /// the bytecode pipeline.
    pub fn from_module(module: &Function) -> Result<Self, SyntaxError> {
        Self::from_module_with_imports(module, &BTreeMap::new())
    }

    /// Like [`from_module`] but with a pre-built map of imported
    /// records (keyed by their in-scope name). The import resolver
    /// (M3+) provides this map so cross-module record references
    /// resolve.
    pub fn from_module_with_imports(
        module: &Function,
        imports: &BTreeMap<String, RecordSchema>,
    ) -> Result<Self, SyntaxError> {
        // -----------------------------------------------------------------
        // Pass 1 — collect all top-level declarations.
        // -----------------------------------------------------------------
        let mut records: BTreeMap<String, RecordSchema> = BTreeMap::new();
        let mut host_state: Option<RecordSchema> = None;
        let mut events: BTreeMap<String, EventSig> = BTreeMap::new();
        let mut record_decl_order: Vec<RecordDecl> = Vec::new();

        for stmt in &module.body.statement_list {
            match stmt {
                Statement::RecordDeclaration(decl) => {
                    if records.contains_key(&decl.name) {
                        return Err(SyntaxError::new(
                            decl.span.start_line,
                            decl.span.start_column,
                            format!("duplicate record `{}`", decl.name),
                        )
                        .with_length(decl.name.len() + "record ".len())
                        .with_note("record names must be unique within a module"));
                    }
                    let schema = build_record_schema(decl)?;
                    records.insert(decl.name.clone(), schema);
                    record_decl_order.push(decl.clone());
                }
                Statement::HostStateDeclaration(decl) => {
                    // Uniqueness is already enforced by the parser;
                    // here we just convert.
                    host_state = Some(build_host_state_schema(decl)?);
                }
                Statement::EventsDeclaration(decl) => {
                    build_events(decl, &mut events)?;
                }
                _ => {}
            }
        }

        let schema = ModuleSchema {
            records,
            host_state,
            events,
            imports: imports.clone(),
        };

        // -----------------------------------------------------------------
        // Pass 2 — resolve every Record(name) reference and detect
        // direct self-reference inside record bodies.
        // -----------------------------------------------------------------
        for decl in &record_decl_order {
            let local_record_name = &decl.name;
            for field in &decl.fields {
                resolve_type_ref(
                    &field.ty,
                    Some(local_record_name),
                    &schema,
                    field.span,
                )?;
                // Also detect direct self-reference: a record may
                // not contain itself non-optionally,
                // non-collectionally.
                check_no_direct_self_reference(
                    &field.ty,
                    local_record_name,
                    field.span,
                )?;
            }
        }
        if let Some(hs) = &schema.host_state {
            for (_, field) in &hs.fields {
                resolve_type_ref(&field.ty, None, &schema, field.decl_span)?;
            }
        }
        for (_, sig) in &schema.events {
            for ty in &sig.args {
                resolve_type_ref(ty, None, &schema, sig.decl_span)?;
            }
        }

        Ok(schema)
    }
}

// ---------------------------------------------------------------------
// Builders (Pass 1)
// ---------------------------------------------------------------------

fn build_record_schema(decl: &RecordDecl) -> Result<RecordSchema, SyntaxError> {
    let fields = collect_fields(&decl.fields, &decl.name)?;
    Ok(RecordSchema {
        fields,
        decl_span: Some(decl.span),
    })
}

fn build_host_state_schema(decl: &HostStateDecl) -> Result<RecordSchema, SyntaxError> {
    let fields = collect_fields(&decl.fields, "host_state")?;
    Ok(RecordSchema {
        fields,
        decl_span: Some(decl.span),
    })
}

/// Collect a `Vec<FieldDecl>` into the schema's `BTreeMap<String,
/// FieldSchema>`, rejecting duplicate field names.
fn collect_fields(
    decls: &[FieldDecl],
    container_name: &str,
) -> Result<BTreeMap<String, FieldSchema>, SyntaxError> {
    let mut out: BTreeMap<String, FieldSchema> = BTreeMap::new();
    for field in decls {
        if out.contains_key(&field.name) {
            return Err(SyntaxError::new(
                field.span.start_line,
                field.span.start_column,
                format!(
                    "duplicate field `{}` in `{}`",
                    field.name, container_name
                ),
            )
            .with_length(field.name.len())
            .with_note("each field name in a record or host_state must be unique"));
        }
        out.insert(
            field.name.clone(),
            FieldSchema {
                ty: field.ty.clone(),
                default: field.default.clone(),
                decl_span: field.span,
            },
        );
    }
    Ok(out)
}

fn build_events(
    decl: &EventsDecl,
    out: &mut BTreeMap<String, EventSig>,
) -> Result<(), SyntaxError> {
    for ev in &decl.events {
        if out.contains_key(&ev.name) {
            return Err(SyntaxError::new(
                ev.span.start_line,
                ev.span.start_column,
                format!("duplicate event `{}`", ev.name),
            )
            .with_length(ev.name.len())
            .with_note("each event name within `events {}` must be unique"));
        }
        out.insert(
            ev.name.clone(),
            EventSig {
                args: ev.args.clone(),
                decl_span: ev.span,
            },
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------
// Resolver (Pass 2)
// ---------------------------------------------------------------------

/// Walk a `TypeRef` and verify every `Record(name)` reference
/// resolves to a declared (or imported) record. `enclosing_record`
/// is `Some(name)` when called from inside a record's field types
/// (so `Self` is legal); otherwise `None`.
///
/// Pass 1 already rejected `Self` in disallowed positions at parse
/// time, so this function should never see a `SelfRef` with
/// `enclosing_record == None`. The check here is defense-in-depth.
fn resolve_type_ref(
    ty: &TypeRef,
    enclosing_record: Option<&str>,
    schema: &ModuleSchema,
    span: Span,
) -> Result<(), SyntaxError> {
    match ty {
        TypeRef::Primitive(_) => Ok(()),
        TypeRef::Record(name) => {
            if schema.lookup_record(name).is_some() {
                Ok(())
            } else {
                let candidates: Vec<&str> = schema
                    .records
                    .keys()
                    .map(|s| s.as_str())
                    .chain(schema.imports.keys().map(|s| s.as_str()))
                    .collect();
                let mut err = SyntaxError::new(
                    span.start_line,
                    span.start_column,
                    format!("unknown record `{}`", name),
                )
                .with_length(name.len())
                .with_note(
                    "the record must be declared in this module via \
                     `record Foo { ... };` or imported from another module",
                );
                if let Some(suggestion) = levenshtein_1(name, &candidates) {
                    err = err.with_help(format!("did you mean `{}`?", suggestion));
                }
                Err(err)
            }
        }
        TypeRef::Array(inner) => resolve_type_ref(inner, enclosing_record, schema, span),
        TypeRef::Map(_, value) => resolve_type_ref(value, enclosing_record, schema, span),
        TypeRef::Optional(inner) => resolve_type_ref(inner, enclosing_record, schema, span),
        TypeRef::SelfRef => {
            if enclosing_record.is_none() {
                Err(SyntaxError::new(
                    span.start_line,
                    span.start_column,
                    "`Self` used outside a record declaration",
                )
                .with_length(4)
                .with_note(
                    "`Self` refers to the enclosing record; it cannot appear in \
                     `host_state` fields or `events` argument types",
                ))
            } else {
                Ok(())
            }
        }
    }
}

/// Reject `record Foo { f: Foo }` — direct self-reference makes
/// the type infinite-size. Indirect references through `array<>`,
/// `map<,>`, or `?` are fine because they introduce indirection.
///
/// `Self` and a `Record(name)` to the enclosing record are
/// equivalent for this check.
fn check_no_direct_self_reference(
    ty: &TypeRef,
    enclosing_record: &str,
    span: Span,
) -> Result<(), SyntaxError> {
    match ty {
        TypeRef::SelfRef | TypeRef::Record(_) => {
            // Cross indirection if any below; here at the top
            // level there is none — this is the failing case.
            let names_self = matches!(ty, TypeRef::SelfRef)
                || matches!(ty, TypeRef::Record(name) if name == enclosing_record);
            if names_self {
                Err(SyntaxError::new(
                    span.start_line,
                    span.start_column,
                    format!(
                        "record `{}` directly contains itself",
                        enclosing_record
                    ),
                )
                .with_length(enclosing_record.len())
                .with_note(
                    "a record cannot contain itself without indirection \
                     (size would be infinite)",
                )
                .with_help(
                    "wrap the field in `array<...>`, `map<K, ...>`, or make it \
                     optional with `?` to introduce indirection",
                ))
            } else {
                Ok(())
            }
        }
        // These all introduce indirection — self-reference under
        // any of them is fine.
        TypeRef::Array(_) | TypeRef::Map(_, _) | TypeRef::Optional(_) => Ok(()),
        TypeRef::Primitive(_) => Ok(()),
    }
}

// ---------------------------------------------------------------------
// Standalone schema loader (M1e)
// ---------------------------------------------------------------------

/// Errors that can arise from [`load_schema`]. Distinct from
/// [`SyntaxError`] because IO failures aren't syntax errors.
#[derive(Debug)]
pub enum SchemaLoadError {
    /// The file couldn't be opened or read.
    Io(io::Error),
    /// A scanner produced an `Error` token.
    Scanner(String),
    /// The parser or schema resolver rejected the file.
    Syntax(SyntaxError),
}

impl std::fmt::Display for SchemaLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SchemaLoadError::Io(e) => write!(f, "io error: {}", e),
            SchemaLoadError::Scanner(msg) => write!(f, "scanner error: {}", msg),
            SchemaLoadError::Syntax(e) => write!(f, "{}", e.message),
        }
    }
}

impl std::error::Error for SchemaLoadError {}

impl From<io::Error> for SchemaLoadError {
    fn from(e: io::Error) -> Self {
        SchemaLoadError::Io(e)
    }
}

impl From<SyntaxError> for SchemaLoadError {
    fn from(e: SyntaxError) -> Self {
        SchemaLoadError::Syntax(e)
    }
}

/// Read a `.ogh` file from disk and produce its [`ModuleSchema`]
/// without compiling bytecode. Used by:
///
/// - The LSP for cross-module record imports (so hover and
///   completion work across files).
/// - `Ogham::watch_typed` (M5) at startup, alongside the normal
///   compilation path, to obtain the schema for the schema-match
///   check.
///
/// The function is intentionally side-effect-free apart from
/// reading the file: no caching, no global state, no runtime
/// instance required. Callers that want caching layer it on top
/// (the LSP keys by path + mtime).
pub fn load_schema(path: &Path) -> Result<ModuleSchema, SchemaLoadError> {
    load_schema_with_imports(path, &BTreeMap::new())
}

/// Like [`load_schema`] but with a pre-supplied import map. The
/// LSP uses this to inject already-loaded imported records when
/// resolving a module that imports them.
pub fn load_schema_with_imports(
    path: &Path,
    imports: &BTreeMap<String, RecordSchema>,
) -> Result<ModuleSchema, SchemaLoadError> {
    let source = fs::read_to_string(path)?;
    load_schema_from_source_with_imports(&source, imports)
}

/// Load a schema from an in-memory source string. Useful for
/// tests and for the LSP's "current document" path (which has
/// the source already in its `DocumentStore` and shouldn't re-read
/// from disk).
pub fn load_schema_from_source(source: &str) -> Result<ModuleSchema, SchemaLoadError> {
    load_schema_from_source_with_imports(source, &BTreeMap::new())
}

/// Inner form taking explicit imports. The other entry points
/// delegate here.
pub fn load_schema_from_source_with_imports(
    source: &str,
    imports: &BTreeMap<String, RecordSchema>,
) -> Result<ModuleSchema, SchemaLoadError> {
    let tokens = Scanner::new(source.to_string()).scan();
    // Surface scanner errors as a SchemaLoadError so callers see
    // them. The first scanner Error token wins.
    for token in &tokens {
        if let crate::scanner::TokenType::Error(msg) = &token.token_type {
            return Err(SchemaLoadError::Scanner(format!(
                "{} at line {} col {}",
                msg, token.line, token.column
            )));
        }
    }
    let module = Parser::new(tokens).parse()?;
    let schema = ModuleSchema::from_module_with_imports(&module, imports)?;
    Ok(schema)
}

// ---------------------------------------------------------------------
// Diagnostics helpers
// ---------------------------------------------------------------------

/// Return the closest candidate within Levenshtein distance 1, if
/// any. Used for `did you mean...` hints in unknown-record /
/// unknown-event diagnostics.
fn levenshtein_1<'a>(query: &str, candidates: &'a [&'a str]) -> Option<&'a str> {
    candidates
        .iter()
        .find(|c| levenshtein_le_1(query, c))
        .copied()
}

/// Public re-export of [`levenshtein_1`] for use from the
/// compiler's strict-mode diagnostics. Keeps a single source of
/// truth for the suggestion algorithm.
pub fn levenshtein_1_pub<'a>(query: &str, candidates: &'a [&'a str]) -> Option<&'a str> {
    levenshtein_1(query, candidates)
}

fn levenshtein_le_1(a: &str, b: &str) -> bool {
    if a == b {
        return false;
    }
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (la, lb) = (a.len(), b.len());
    if la.abs_diff(lb) > 1 {
        return false;
    }
    if la == lb {
        // Substitution only.
        let mut diffs = 0;
        for i in 0..la {
            if a[i] != b[i] {
                diffs += 1;
                if diffs > 1 {
                    return false;
                }
            }
        }
        return diffs == 1;
    }
    // Insertion/deletion.
    let (short, long) = if la < lb { (&a, &b) } else { (&b, &a) };
    let mut i = 0;
    let mut j = 0;
    let mut diffs = 0;
    while i < short.len() && j < long.len() {
        if short[i] == long[j] {
            i += 1;
            j += 1;
        } else {
            diffs += 1;
            if diffs > 1 {
                return false;
            }
            j += 1;
        }
    }
    true
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::scanner::Scanner;

    fn schema_of(source: &str) -> Result<ModuleSchema, SyntaxError> {
        let tokens = Scanner::new(source.to_string()).scan();
        let module = Parser::new(tokens)
            .parse()
            .expect("parse should succeed in test fixtures unless asserting otherwise");
        ModuleSchema::from_module(&module)
    }

    #[test]
    fn loose_module_has_no_strict_mode() {
        let s = schema_of("let x = 5;").unwrap();
        assert!(!s.is_strict());
        assert!(s.host_state.is_none());
    }

    #[test]
    fn host_state_module_is_strict() {
        let s = schema_of("host_state { count: int };").unwrap();
        assert!(s.is_strict());
        assert!(s.host_state.is_some());
        assert!(s.host_state.unwrap().fields.contains_key("count"));
    }

    #[test]
    fn record_collected_from_pass_1() {
        let s = schema_of("record Item { name: string, count: int };").unwrap();
        let item = s.lookup_record("Item").expect("Item should resolve");
        assert_eq!(item.fields.len(), 2);
        assert!(item.fields.contains_key("name"));
        assert!(item.fields.contains_key("count"));
    }

    #[test]
    fn forward_record_reference_resolves() {
        // host_state declared *before* record — must still resolve.
        let s = schema_of(
            r#"
            host_state { player: Player };
            record Player { name: string };
            "#,
        )
        .unwrap();
        let hs = s.host_state.unwrap();
        // The TypeRef stays as Record("Player"); resolution is a
        // pass-2 check, not a rewrite.
        assert!(matches!(hs.fields["player"].ty, TypeRef::Record(ref n) if n == "Player"));
    }

    #[test]
    fn unknown_record_fails_with_suggestion() {
        let err = schema_of(
            r#"
            record Player { name: string };
            host_state { player: Plyer };
            "#,
        )
        .unwrap_err();
        assert!(err.message.contains("unknown record `Plyer`"));
        assert_eq!(err.help.as_deref(), Some("did you mean `Player`?"));
    }

    #[test]
    fn unknown_record_no_close_match_omits_help() {
        let err = schema_of(
            r#"
            record Player { name: string };
            host_state { weird: Z };
            "#,
        )
        .unwrap_err();
        assert!(err.message.contains("unknown record `Z`"));
        assert!(err.help.is_none());
    }

    #[test]
    fn duplicate_record_name_fails() {
        let err = schema_of(
            r#"
            record Foo { a: int };
            record Foo { b: int };
            "#,
        )
        .unwrap_err();
        assert!(err.message.contains("duplicate record `Foo`"));
    }

    #[test]
    fn duplicate_field_name_fails() {
        let err = schema_of("record Foo { a: int, a: string };").unwrap_err();
        assert!(err.message.contains("duplicate field `a`"));
    }

    #[test]
    fn duplicate_event_name_fails() {
        let err = schema_of("events { close(), close() };").unwrap_err();
        assert!(err.message.contains("duplicate event `close`"));
    }

    #[test]
    fn direct_self_reference_rejected() {
        let err = schema_of("record Node { next: Self };").unwrap_err();
        assert!(err.message.contains("directly contains itself"));
        assert!(err.help.is_some());
    }

    #[test]
    fn direct_self_via_record_name_rejected() {
        let err = schema_of("record Node { next: Node };").unwrap_err();
        assert!(err.message.contains("directly contains itself"));
    }

    #[test]
    fn self_reference_via_array_allowed() {
        let _ = schema_of("record Tree { children: array<Self> };").unwrap();
    }

    #[test]
    fn self_reference_via_optional_allowed() {
        let _ = schema_of("record Node { parent: Self? };").unwrap();
    }

    #[test]
    fn self_reference_via_map_value_allowed() {
        let _ = schema_of("record Node { children: map<string, Self> };").unwrap();
    }

    #[test]
    fn levenshtein_1_basic() {
        assert!(levenshtein_le_1("Plyer", "Player")); // insertion
        assert!(levenshtein_le_1("Player", "Plyer")); // deletion
        assert!(levenshtein_le_1("Player", "Pleyer")); // substitution
        assert!(!levenshtein_le_1("Player", "Player")); // identical
        assert!(!levenshtein_le_1("Player", "Plyrr")); // > 1 edits
        assert!(!levenshtein_le_1("Player", "ABC")); // length diff > 1
    }

    #[test]
    fn audit_dm_hud_optional_record_resolves() {
        let _ = schema_of(
            r#"
            record EntityInspector {
                name: string,
                kind: string,
                position_text: string,
                detail_lines: array<string>,
                can_possess: bool,
                is_possessing: bool,
                can_open_inventory: bool,
            };
            host_state {
                paused: bool,
                selection_count: int,
                selected_entity: EntityInspector?,
            };
            events {
                dm_toggle_pause(),
                dm_open_inventory(),
                dm_possess(),
                dm_release(),
                dm_deselect(),
            };
            "#,
        )
        .unwrap();
    }

    #[test]
    fn audit_inventory_hud_partial_resolves() {
        let _ = schema_of(
            r#"
            record InvItem {
                name: string,
                x: int, y: int, w: int, h: int,
                description: string,
                weight: int,
                item_type: string,
                size_text: string,
                sell_price_text: string,
                can_sell: bool,
                dormant: bool,
            };
            record Skill { name: string, level: int, xp: int, xp_needed: int };
            record Player { name: string };
            host_state {
                show_inventory: bool,
                player_count: int,
                players: array<Player>,
                tool_name: string,
                inv_items: array<InvItem>,
                skills: array<Skill>,
            };
            events {
                inv_cell_click(int, int),
                shop_buy(string, string),
                shop_sell_item(string, int, int),
            };
            "#,
        )
        .unwrap();
    }

    // -----------------------------------------------------------------
    // Standalone loader (M1e)
    // -----------------------------------------------------------------

    #[test]
    fn load_schema_from_source_basic() {
        let s = load_schema_from_source(
            r#"
            record Item { name: string };
            host_state { items: array<Item> };
            events { close() };
            "#,
        )
        .unwrap();
        assert!(s.is_strict());
        assert!(s.lookup_record("Item").is_some());
        assert!(s.events.contains_key("close"));
    }

    #[test]
    fn load_schema_surfaces_syntax_errors() {
        let err = load_schema_from_source("host_state { x: int };\nhost_state { y: int };")
            .unwrap_err();
        match err {
            SchemaLoadError::Syntax(s) => {
                assert!(s.message.contains("duplicate `host_state`"))
            }
            other => panic!("expected Syntax error, got {:?}", other),
        }
    }

    #[test]
    fn load_schema_surfaces_scanner_errors() {
        let err = load_schema_from_source("host_state { x: int = & };").unwrap_err();
        match err {
            SchemaLoadError::Scanner(_) => {}
            other => panic!("expected Scanner error, got {:?}", other),
        }
    }

    #[test]
    fn load_schema_surfaces_resolver_errors() {
        let err = load_schema_from_source(
            "host_state { player: UnknownRecord };",
        )
        .unwrap_err();
        match err {
            SchemaLoadError::Syntax(s) => {
                assert!(s.message.contains("unknown record `UnknownRecord`"))
            }
            other => panic!("expected Syntax error, got {:?}", other),
        }
    }

    #[test]
    fn load_schema_with_pre_supplied_imports() {
        let mut imports: BTreeMap<String, RecordSchema> = BTreeMap::new();
        let mut fields: BTreeMap<String, FieldSchema> = BTreeMap::new();
        fields.insert(
            "name".to_string(),
            FieldSchema {
                ty: TypeRef::Primitive(PrimType::String),
                default: None,
                decl_span: Span::zero(),
            },
        );
        imports.insert(
            "ImportedItem".to_string(),
            RecordSchema {
                fields,
                decl_span: None,
            },
        );
        let s = load_schema_from_source_with_imports(
            "host_state { item: ImportedItem };",
            &imports,
        )
        .unwrap();
        assert!(s.lookup_record("ImportedItem").is_some());
    }

    #[test]
    fn load_schema_loose_module_returns_non_strict() {
        let s = load_schema_from_source("let x = 5;").unwrap();
        assert!(!s.is_strict());
    }

    // Catch the case where a record is declared but `host_state`
    // references a typo of it — must fail with a clean diagnostic
    // pointing at the right field.
    #[test]
    fn typo_in_record_reference_pointed_at_correctly() {
        let err = schema_of(
            r#"
            record Player { name: string };
            host_state {
                player: Plyer,
            };
            "#,
        )
        .unwrap_err();
        assert!(err.message.contains("unknown record `Plyer`"));
        // The error should point at the field's span, not the
        // record declaration's span.
        assert!(err.line >= 4, "expected error on line ≥ 4 (the field), got line {}", err.line);
    }

    // The HashSet import below is unused but kept to demonstrate
    // that this module compiles cleanly with the existing import
    // surface. Suppress the unused-import warning.
    #[allow(dead_code)]
    fn _unused_imports_anchor() {
        let _: HashSet<()> = HashSet::new();
        let _: HashMap<(), ()> = HashMap::new();
        let _ = PrimType::Int;
        let _ = KeyType::String;
    }
}
