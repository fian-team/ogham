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
//! Cross-module record imports (`import { Item } from
//! "./inv.ogh"`) arrive in the supplied `imports` map, which the
//! resolver trusts. Who fills it is
//! [`crate::runtime::imports`], the one walk of the graph; every
//! entry point here that reads a *file* (rather than a string)
//! goes through it, so a document split across files resolves the
//! same way it will at run time.
//!
//! ## Strict mode is detected by presence
//!
//! A module is in *strict mode* iff it has a `host_state {}`
//! declaration. [`ModuleSchema::is_strict`] is the single source
//! of truth — the rest of the runtime never carries a separate
//! flag.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::Path;

use crate::runtime::imports::ImportSpace;
use crate::runtime::value::Value;

use crate::parser::span::Span;
use crate::parser::typed_bindings::{EventsDecl, FieldDecl, HostStateDecl, RecordDecl};
use crate::parser::{Parser, SyntaxError};
use crate::scanner::Scanner;

// Re-export the type-universe items so callers (and the
// derive macros) have one canonical import path:
// `use ogham::runtime::schema::{TypeRef, PrimType, KeyType, ...};`.
pub use crate::parser::typed_bindings::{KeyType, PrimType, SchemaLiteral, TypeRef};
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
    /// Screens declared by this module, keyed by id. Each carries the
    /// slice of host state that screen alone reads; the module's
    /// `host_state {}` remains readable from every screen and is the
    /// only scope they share.
    pub screens: BTreeMap<String, ScreenSchema>,
}

/// One declared `screen`, resolved.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScreenSchema {
    /// The screen's own host-state slice. Reads of these names inside
    /// this screen's view compile to the namespaced key
    /// `"<id>::<field>"`, which is what keeps two screens' identically
    /// named fields apart.
    pub state: RecordSchema,
    pub decl_span: Option<Span>,
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

    /// The ids this module declares a `screen` for, in sorted order.
    pub fn screen_ids(&self) -> Vec<&str> {
        self.screens.keys().map(|s| s.as_str()).collect()
    }

    /// The event names this module declares it raises, sorted.
    pub fn event_names(&self) -> Vec<&str> {
        self.events.keys().map(|s| s.as_str()).collect()
    }

    /// Check this module's declared events against the names a host has
    /// registered handlers for.
    ///
    /// A declared raise with no handler is a button that draws, clicks and
    /// reaches nobody — the exact failure `screen`/route-id validation
    /// prevents for surfaces, at the other end of the same wire. A
    /// registered handler the document never raises is the milder half and
    /// is reported too, because it is usually a rename that only landed on
    /// one side.
    ///
    /// A module declaring no `events {}` is vacuously fine: it has opted
    /// out of static checking of its raises entirely.
    pub fn validate_events(&self, registered: &[&str]) -> Result<(), String> {
        if self.events.is_empty() {
            return Ok(());
        }
        let declared: std::collections::BTreeSet<&str> = self.event_names().into_iter().collect();
        let registered: std::collections::BTreeSet<&str> = registered.iter().copied().collect();

        let unhandled: Vec<&str> = declared.difference(&registered).copied().collect();
        let unraised: Vec<&str> = registered.difference(&declared).copied().collect();
        if unhandled.is_empty() && unraised.is_empty() {
            return Ok(());
        }

        let mut msg = String::from("the host and this document disagree about events");
        if !unhandled.is_empty() {
            msg.push_str(&format!(
                "\n  declared but no handler registered: {}",
                unhandled.join(", ")
            ));
        }
        if !unraised.is_empty() {
            msg.push_str(&format!(
                "\n  handler registered but never declared: {}",
                unraised.join(", ")
            ));
        }
        Err(msg)
    }

    /// Check this module's screens against the ids a host's route table
    /// registered. A registered id with no `screen` block, or a block
    /// with no registered id, is an error naming both — the drift that
    /// is silent today, where a `.ogh` accumulates modes nobody routes
    /// to and a router names modes nobody drew.
    ///
    /// Called once at load. A module that declares no screens at all is
    /// vacuously fine: it is a document that predates routing, or one
    /// that a host mounts whole.
    pub fn validate_screens(&self, registered: &[&str]) -> Result<(), String> {
        if self.screens.is_empty() {
            return Ok(());
        }
        let declared: std::collections::BTreeSet<&str> = self.screen_ids().into_iter().collect();
        let registered: std::collections::BTreeSet<&str> = registered.iter().copied().collect();

        let undrawn: Vec<&str> = registered.difference(&declared).copied().collect();
        let unrouted: Vec<&str> = declared.difference(&registered).copied().collect();
        if undrawn.is_empty() && unrouted.is_empty() {
            return Ok(());
        }

        let mut msg = String::from("the route table and this document disagree about screens");
        if !undrawn.is_empty() {
            msg.push_str(&format!(
                "\n  registered with no `screen` block: {}",
                undrawn.join(", ")
            ));
        }
        if !unrouted.is_empty() {
            msg.push_str(&format!(
                "\n  declared but not registered: {}",
                unrouted.join(", ")
            ));
        }
        Err(msg)
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
        let mut screens: BTreeMap<String, ScreenSchema> = BTreeMap::new();

        for stmt in &module.body.statement_list {
            match stmt {
                Statement::ScreenDeclaration(decl) => {
                    // Id uniqueness is the parser's; here we just convert.
                    screens.insert(
                        decl.id.clone(),
                        ScreenSchema {
                            state: RecordSchema {
                                fields: collect_fields(
                                    &decl.state,
                                    &format!("screen \"{}\"", decl.id),
                                )?,
                                decl_span: Some(decl.span),
                            },
                            decl_span: Some(decl.span),
                        },
                    );
                }
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
            screens,
        };

        // -----------------------------------------------------------------
        // Pass 2 — resolve every Record(name) reference and detect
        // direct self-reference inside record bodies.
        // -----------------------------------------------------------------
        for decl in &record_decl_order {
            let local_record_name = &decl.name;
            for field in &decl.fields {
                resolve_type_ref(&field.ty, Some(local_record_name), &schema, field.span)?;
                // Also detect direct self-reference: a record may
                // not contain itself non-optionally,
                // non-collectionally.
                check_no_direct_self_reference(&field.ty, local_record_name, field.span)?;
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
        for (_, screen) in &schema.screens {
            for (_, field) in &screen.state.fields {
                resolve_type_ref(&field.ty, None, &schema, field.decl_span)?;
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

/// One `record` declaration, resolved on its own.
///
/// The import walk ([`crate::runtime::imports`]) needs a record's shape
/// without building the whole module it was declared in — the module it was
/// declared in may not even resolve, and an unresolvable neighbour must not
/// take the record with it.
pub(crate) fn record_schema_of(decl: &RecordDecl) -> Result<RecordSchema, SyntaxError> {
    build_record_schema(decl)
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
                format!("duplicate field `{}` in `{}`", field.name, container_name),
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
                    format!("record `{}` directly contains itself", enclosing_record),
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
// Runtime value validation
// ---------------------------------------------------------------------

/// One mismatch between an injected host-state `Value` tree and the
/// declared schema. `path` is the dotted route from the host_state
/// root (`lobby.roster[3].name`); `message` says what disagreed.
#[derive(Clone, Debug, PartialEq)]
pub struct SchemaValueError {
    pub path: String,
    pub message: String,
}

impl std::fmt::Display for SchemaValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl ModuleSchema {
    /// Validate a host-injected state map against this module's
    /// declared `host_state {}` schema. The `.ogh` declaration is
    /// the single source of truth (INTENT: no rival Rust-side
    /// description); this walk lets a host's tests assert its
    /// injected `Value` trees conform — the runtime-value
    /// counterpart of the compiler's strict-mode read checking.
    ///
    /// Checked both ways: a declared non-optional field missing
    /// from the map is an error, and a map key the schema doesn't
    /// declare is an error (that's drift, the thing this exists to
    /// catch). All mismatches are collected, not first-error.
    ///
    /// A loose module (no `host_state {}`) vacuously passes —
    /// callers that require a schema should assert
    /// [`has_host_state`](Self::has_host_state) first.
    pub fn validate_host_state(
        &self,
        state: &HashMap<String, Value>,
    ) -> Result<(), Vec<SchemaValueError>> {
        let mut errors = Vec::new();
        if let Some(hs) = &self.host_state {
            self.validate_record_value(hs, None, state, "", &mut errors);
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Validate a single `Value` against one declared type — the
    /// per-key form of [`validate_host_state`](Self::validate_host_state),
    /// for hosts that inject top-level keys individually.
    pub fn validate_value(
        &self,
        ty: &TypeRef,
        value: &Value,
        path: &str,
    ) -> Result<(), Vec<SchemaValueError>> {
        let mut errors = Vec::new();
        self.validate_type(ty, None, value, path, &mut errors);
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn validate_record_value(
        &self,
        record: &RecordSchema,
        enclosing_record: Option<&str>,
        map: &HashMap<String, Value>,
        path: &str,
        errors: &mut Vec<SchemaValueError>,
    ) {
        for (name, field) in &record.fields {
            let field_path = join_path(path, name);
            match map.get(name) {
                Some(value) => {
                    self.validate_type(&field.ty, enclosing_record, value, &field_path, errors)
                }
                None => {
                    // A missing optional field reads as Void — fine.
                    if !matches!(field.ty, TypeRef::Optional(_)) {
                        errors.push(SchemaValueError {
                            path: field_path,
                            message: format!(
                                "missing: declared `{}` but the injected map has no such key",
                                field.ty.to_canonical_string()
                            ),
                        });
                    }
                }
            }
        }
        for key in map.keys() {
            if !record.fields.contains_key(key) {
                errors.push(SchemaValueError {
                    path: join_path(path, key),
                    message: "undeclared: present in the injected map but not in the schema"
                        .to_string(),
                });
            }
        }
    }

    fn validate_type(
        &self,
        ty: &TypeRef,
        enclosing_record: Option<&str>,
        value: &Value,
        path: &str,
        errors: &mut Vec<SchemaValueError>,
    ) {
        let mismatch = |errors: &mut Vec<SchemaValueError>| {
            errors.push(SchemaValueError {
                path: path.to_string(),
                message: format!(
                    "expected `{}`, found {}",
                    ty.to_canonical_string(),
                    value_kind(value)
                ),
            });
        };
        match ty {
            TypeRef::Primitive(PrimType::Int) => {
                if !matches!(value, Value::Integer(_)) {
                    mismatch(errors);
                }
            }
            // An integer where a float is declared is accepted —
            // the VM's arithmetic already treats the two as one
            // numeric tower, and hosts routinely inject whole
            // numbers into float slots.
            TypeRef::Primitive(PrimType::Float) => {
                if !matches!(value, Value::Float(_) | Value::Integer(_)) {
                    mismatch(errors);
                }
            }
            TypeRef::Primitive(PrimType::Bool) => {
                if !matches!(value, Value::Boolean(_)) {
                    mismatch(errors);
                }
            }
            TypeRef::Primitive(PrimType::String) => {
                if !matches!(value, Value::String(_)) {
                    mismatch(errors);
                }
            }
            TypeRef::Record(name) => match (self.lookup_record(name), value) {
                (Some(record), Value::Map(map)) => {
                    self.validate_record_value(record, Some(name), map, path, errors)
                }
                (Some(_), _) => mismatch(errors),
                (None, _) => errors.push(SchemaValueError {
                    path: path.to_string(),
                    message: format!("unknown record `{}` (schema not resolved?)", name),
                }),
            },
            TypeRef::SelfRef => match enclosing_record {
                Some(name) => self.validate_type(
                    &TypeRef::Record(name.to_string()),
                    None,
                    value,
                    path,
                    errors,
                ),
                None => errors.push(SchemaValueError {
                    path: path.to_string(),
                    message: "`Self` outside a record (schema not resolved?)".to_string(),
                }),
            },
            TypeRef::Array(inner) => match value {
                Value::Array(items) => {
                    for (i, item) in items.iter().enumerate() {
                        self.validate_type(
                            inner,
                            enclosing_record,
                            item,
                            &format!("{path}[{i}]"),
                            errors,
                        );
                    }
                }
                _ => mismatch(errors),
            },
            TypeRef::Map(key_ty, value_ty) => match value {
                Value::Map(map) => {
                    for (key, item) in map {
                        if matches!(key_ty, KeyType::Int) && key.parse::<i32>().is_err() {
                            errors.push(SchemaValueError {
                                path: join_path(path, key),
                                message: format!("map key `{key}` is not an int"),
                            });
                        }
                        self.validate_type(
                            value_ty,
                            enclosing_record,
                            item,
                            &join_path(path, key),
                            errors,
                        );
                    }
                }
                _ => mismatch(errors),
            },
            TypeRef::Optional(inner) => {
                if !matches!(value, Value::Void) {
                    self.validate_type(inner, enclosing_record, value, path, errors);
                }
            }
        }
    }
}

fn join_path(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

/// The schema-vocabulary name for a `Value`'s runtime shape, for
/// mismatch messages.
fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Integer(_) => "int",
        Value::Float(_) => "float",
        Value::Boolean(_) => "bool",
        Value::String(_) => "string",
        Value::Map(_) => "a map",
        Value::Array(_) => "an array",
        Value::Void => "void",
        Value::BytecodeClosure(_) => "a closure",
        Value::Widget(_) => "a widget",
        Value::WidgetRef(_) => "a widget ref",
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
    let root = path.parent().unwrap_or(Path::new("."));
    load_schema_in(path, &ImportSpace::rooted_at(root))
}

/// [`load_schema`] with the import space a host has configured, so a
/// document split across files resolves the same way it will at run time
/// (`APPLICATION_BUILD.md` WP-3.1).
///
/// The bare [`load_schema`] roots the space at the document's own
/// directory, which is where a `./sibling.ogh` lives in every shipped
/// document today. A host that maps prefixes or embeds its sources passes
/// its own space and gets its own answers.
pub fn load_schema_in(path: &Path, space: &ImportSpace) -> Result<ModuleSchema, SchemaLoadError> {
    let source = fs::read_to_string(path)?;
    let tokens = scan(&source)?;
    let module = Parser::new(tokens).parse()?;
    let crossing = crate::runtime::imports::walk(&module, space);
    let schema = ModuleSchema::from_module_with_imports(&module, &crossing.records)?;
    Ok(schema)
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
    let tokens = scan(source)?;
    let module = Parser::new(tokens).parse()?;
    let schema = ModuleSchema::from_module_with_imports(&module, imports)?;
    Ok(schema)
}

/// Scan, surfacing the first scanner `Error` token as a load failure so a
/// caller sees it rather than a parse error further downstream.
fn scan(source: &str) -> Result<Vec<crate::scanner::Token>, SchemaLoadError> {
    let tokens = Scanner::new(source.to_string()).scan();
    for token in &tokens {
        if let crate::scanner::TokenType::Error(msg) = &token.token_type {
            return Err(SchemaLoadError::Scanner(format!(
                "{} at line {} col {}",
                msg, token.line, token.column
            )));
        }
    }
    Ok(tokens)
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
        let err =
            load_schema_from_source("host_state { x: int };\nhost_state { y: int };").unwrap_err();
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
        let err = load_schema_from_source("host_state { player: UnknownRecord };").unwrap_err();
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
        let s =
            load_schema_from_source_with_imports("host_state { item: ImportedItem };", &imports)
                .unwrap();
        assert!(s.lookup_record("ImportedItem").is_some());
    }

    #[test]
    fn load_schema_loose_module_returns_non_strict() {
        let s = load_schema_from_source("let x = 5;").unwrap();
        assert!(!s.is_strict());
    }

    // -----------------------------------------------------------------
    // Runtime value validation
    // -----------------------------------------------------------------

    fn plate(name: &str) -> Value {
        let mut m = HashMap::new();
        m.insert("name".to_string(), Value::String(name.to_string()));
        m.insert("count".to_string(), Value::Integer(2));
        Value::Map(m)
    }

    fn roster_schema() -> ModuleSchema {
        schema_of(
            r#"
            record Item { name: string, count: int };
            host_state {
                title: string,
                weight: float,
                open: bool,
                items: array<Item>,
                pending: Item?,
            };
            "#,
        )
        .unwrap()
    }

    fn valid_state() -> HashMap<String, Value> {
        HashMap::from([
            ("title".to_string(), Value::String("hands".to_string())),
            ("weight".to_string(), Value::Float(1.2)),
            ("open".to_string(), Value::Boolean(true)),
            (
                "items".to_string(),
                Value::Array(vec![plate("candle"), plate("key")]),
            ),
            ("pending".to_string(), Value::Void),
        ])
    }

    #[test]
    fn validate_conforming_state_passes() {
        roster_schema().validate_host_state(&valid_state()).unwrap();
    }

    #[test]
    fn validate_missing_key_names_the_path() {
        let mut state = valid_state();
        state.remove("open");
        let errs = roster_schema().validate_host_state(&state).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].path, "open");
        assert!(errs[0].message.contains("missing"));
    }

    #[test]
    fn validate_missing_optional_passes() {
        let mut state = valid_state();
        state.remove("pending");
        roster_schema().validate_host_state(&state).unwrap();
    }

    #[test]
    fn validate_present_optional_is_checked() {
        let mut state = valid_state();
        state.insert("pending".to_string(), plate("candle"));
        roster_schema().validate_host_state(&state).unwrap();
        state.insert("pending".to_string(), Value::Integer(3));
        let errs = roster_schema().validate_host_state(&state).unwrap_err();
        assert_eq!(errs[0].path, "pending");
    }

    #[test]
    fn validate_undeclared_key_is_drift() {
        let mut state = valid_state();
        state.insert("stray".to_string(), Value::Void);
        let errs = roster_schema().validate_host_state(&state).unwrap_err();
        assert_eq!(errs[0].path, "stray");
        assert!(errs[0].message.contains("undeclared"));
    }

    #[test]
    fn validate_wrong_type_in_array_element_names_index() {
        let mut state = valid_state();
        let mut bad = HashMap::new();
        bad.insert("name".to_string(), Value::String("candle".to_string()));
        bad.insert("count".to_string(), Value::String("two".to_string()));
        state.insert(
            "items".to_string(),
            Value::Array(vec![plate("key"), Value::Map(bad)]),
        );
        let errs = roster_schema().validate_host_state(&state).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].path, "items[1].count");
        assert!(errs[0].message.contains("expected `int`, found string"));
    }

    #[test]
    fn validate_int_widens_to_float_but_not_reverse() {
        let mut state = valid_state();
        state.insert("weight".to_string(), Value::Integer(1));
        roster_schema().validate_host_state(&state).unwrap();

        let s = schema_of("host_state { n: int };").unwrap();
        let errs = s
            .validate_host_state(&HashMap::from([("n".to_string(), Value::Float(1.5))]))
            .unwrap_err();
        assert_eq!(errs[0].path, "n");
    }

    #[test]
    fn validate_collects_all_errors_not_first() {
        let mut state = valid_state();
        state.remove("title");
        state.insert("open".to_string(), Value::Integer(1));
        let errs = roster_schema().validate_host_state(&state).unwrap_err();
        assert_eq!(errs.len(), 2);
    }

    #[test]
    fn validate_loose_module_vacuously_passes() {
        let s = schema_of("let x = 5;").unwrap();
        s.validate_host_state(&HashMap::from([("anything".to_string(), Value::Void)]))
            .unwrap();
    }

    #[test]
    fn validate_single_value_per_key_form() {
        let s = roster_schema();
        let ty = TypeRef::Array(Box::new(TypeRef::Record("Item".to_string())));
        s.validate_value(&ty, &Value::Array(vec![plate("candle")]), "items")
            .unwrap();
        let errs = s
            .validate_value(&ty, &Value::String("nope".to_string()), "items")
            .unwrap_err();
        assert_eq!(errs[0].path, "items");
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
        assert!(
            err.line >= 4,
            "expected error on line ≥ 4 (the field), got line {}",
            err.line
        );
    }
}
