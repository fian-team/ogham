//! The surface side of the contract seam: what a document *declares*, said
//! in the structure framework's vocabulary, and the harness that holds
//! every shipped document to it at `cargo test` time.
//!
//! `APPLICATION.md` §4.1 gives a provider the schema and a consumer a
//! selection. The provider's half lives in the structure framework — a
//! scope's [reflection](structure::Kind) and the
//! [intents](structure::Vocabulary) it accepts — and the grading of the two
//! against each other lives there too ([`structure::Validation`]). What
//! lives *here* is the translation, because a document is written in this
//! crate's language and the structure framework has never heard of it: a
//! `host_state {}` block becomes a list of [`Field`]s, an `events {}` block
//! becomes a list of [`Declared`] raises, and a `TypeRef` becomes a
//! [`Kind`].
//!
//! # The CI moment
//!
//! [`Documents`] is the reason this module exists rather than a
//! load-time-only check. Three games guarantee document/host agreement
//! today with hand-rolled tests that read their own `.ogh` files and parse
//! the blocks out as strings — untold_lore's
//! `every_declared_key_is_projected`, regency's schema-conformance test,
//! celia's `the_shipped_document_declares_exactly_the_registered_screens`.
//! Those delete only if something else holds the same guarantee at the same
//! moment, and the moment is `cargo test`, not first boot. So the harness
//! takes a [`Store`] with nothing but its registrations run — no path, no
//! mount, no frame, no window — reads the shipped documents off disk, and
//! answers in both grades. A consumer instantiates it in one test.
//!
//! # The mapping is an input
//!
//! Which scopes a given document selects against is the binding's answer to
//! give (`APPLICATION_BUILD.md` P4/P5), and the binding does not exist yet.
//! So a [`Mount`] is a consumer's declaration: this document, these scopes
//! nearest-first, these registered ids. Nothing here invents it, and when
//! the binding lands it supplies the same three things from the route
//! table instead of from a test.
//!
//! # What the document's side does *not* carry across
//!
//! Two declarations a `host_state {}` field can make are dropped on the way
//! in, and both for the same reason: they are the **provider's** to make
//! (§4.1). A `T?` becomes the shape of `T` — optionality is stated in the
//! schema, with the sentence saying *when*, and a consumer asserting its
//! own optionality is the inverted contract §1 threw out. A `= 3` default
//! likewise: a field's at-mount value is declared once, by the scope that
//! provides it. Neither is compared, so neither can drift; both simply stop
//! being the document's business.

use std::path::{Path, PathBuf};

use structure::schema::Field;

use crate::runtime::imports::ImportSpace;
use crate::runtime::schema::{
    load_schema_in, EventSig, ModuleSchema, PrimType, RecordSchema, SchemaLoadError, TypeRef,
};

/// The structure framework's half of the seam, re-exported so a consumer
/// naming a scope or reading a finding needs no dependency of its own —
/// the same service `route` does for the table (`APPLICATION_BUILD.md`
/// §0.5's declared scaffolding edge, and it dies with it in P4).
pub use structure::{Declared, Finding, Findings, Kind, RouteId, Scope, Store, Validation};

/// Where a document's imports resolve from, re-exported so a consumer
/// declaring a [`Mount`] for a split document needs no second import path.
pub use crate::runtime::imports::ImportSpace as Imports;

// --- the translation -------------------------------------------------------

/// One declared type, in the shape vocabulary a scope's reflection is
/// written in.
///
/// Structural throughout (§4.7): a `record Item` in a document and an
/// `Item` struct in Rust meet as two lists of named fields and never as two
/// names, because a name cannot cross that boundary and pretending it can
/// is what makes a rename look like agreement.
pub fn kind_of(ty: &TypeRef, schema: &ModuleSchema) -> Kind {
    kind_within(ty, schema, &mut Vec::new())
}

fn kind_within(ty: &TypeRef, schema: &ModuleSchema, expanding: &mut Vec<String>) -> Kind {
    match ty {
        TypeRef::Primitive(PrimType::Int) => Kind::Int,
        TypeRef::Primitive(PrimType::Float) => Kind::Float,
        TypeRef::Primitive(PrimType::Bool) => Kind::Bool,
        TypeRef::Primitive(PrimType::String) => Kind::Str,
        TypeRef::Array(inner) => Kind::List(Box::new(kind_within(inner, schema, expanding))),
        // A map's key type is not part of the shape: a document only ever
        // reads a key as text, which is the same call `Kind::Map` makes.
        TypeRef::Map(_, value) => Kind::Map(Box::new(kind_within(value, schema, expanding))),
        // Optionality is the provider's declaration, not the consumer's —
        // see the module doc.
        TypeRef::Optional(inner) => kind_within(inner, schema, expanding),
        TypeRef::SelfRef => Kind::Cycle,
        TypeRef::Record(name) => {
            if expanding.iter().any(|open| open == name) {
                return Kind::Cycle;
            }
            // A name that resolves to nothing cannot reach here: the module
            // schema's second pass refuses an unresolved record reference,
            // so a document that got this far has none. An empty record is
            // the safe answer either way — it refuses against every real
            // shape, naming the field.
            let Some(record) = schema.lookup_record(name) else {
                return Kind::Record(Vec::new());
            };
            expanding.push(name.clone());
            let fields = fields_within(record, schema, expanding);
            expanding.pop();
            Kind::Record(fields)
        }
    }
}

/// A declared block — a `host_state {}` or a `screen`'s `state {}` — as the
/// list of fields a selection names.
///
/// Per-field, because per-field selection is what gives the most specific
/// refusal (§4.2).
pub fn selection_of(record: &RecordSchema, schema: &ModuleSchema) -> Vec<Field> {
    fields_within(record, schema, &mut Vec::new())
}

fn fields_within(
    record: &RecordSchema,
    schema: &ModuleSchema,
    expanding: &mut Vec<String>,
) -> Vec<Field> {
    record
        .fields
        .iter()
        .map(|(name, field)| Field::new(name, kind_within(&field.ty, schema, expanding)))
        .collect()
}

/// A document's `events {}` block as the raises it declares (§4.4).
///
/// Positional, because that is what the block declares: `save(string, int)`
/// names no parameters, so [`Vocabulary::check_one`](structure::Vocabulary::check_one)
/// compares shapes in order and keeps the provider's names for the
/// diagnostic.
pub fn raises_of(schema: &ModuleSchema) -> Vec<Declared> {
    schema
        .events
        .iter()
        .map(|(name, sig): (&String, &EventSig)| {
            Declared::new(
                name,
                sig.args.iter().map(|ty| kind_of(ty, schema)).collect(),
            )
        })
        .collect()
}

// --- what a consumer declares ----------------------------------------------

/// One document, and where it mounts: the scopes its selection may name and
/// the registered ids it draws screens for.
///
/// This is the mapping the binding will own (§6.1) and does not yet. A
/// consumer states it once, in the test that instantiates [`Documents`],
/// and the same three facts are what a route table will answer when P4
/// lands: [`document_of`](structure::RouteTable::document_of) gives the
/// path, the walk gives the scopes, and the ids under the enclosing
/// instance give the screens.
#[derive(Clone, Debug)]
pub struct Mount {
    document: PathBuf,
    scopes: Vec<Scope>,
    screens: Vec<RouteId>,
    /// Where this document's imports resolve from. `None` roots the walk
    /// at the document's own directory, which is where a `./sibling.ogh`
    /// lives in every shipped document; a host that maps prefixes or
    /// embeds its sources says so with
    /// [`importing_from`](Mount::importing_from).
    space: Option<ImportSpace>,
}

impl Mount {
    /// The document at `path`, mounting under nothing yet.
    pub fn new(document: impl Into<PathBuf>) -> Self {
        Self {
            document: document.into(),
            scopes: Vec::new(),
            screens: Vec::new(),
            space: None,
        }
    }

    /// Resolve this document's imports the way the host that mounts it
    /// will (`APPLICATION_BUILD.md` WP-3.1).
    ///
    /// A document split across files is only readable if the reader knows
    /// where the other files are, and one consumer already answers that
    /// with a prefix map rather than with a directory (untold_lore's
    /// editor mounts the game's UI under a `UI_PREFIX`). Without this the
    /// harness would read those documents as unreadable and the repo would
    /// have to drop the check that WP-2.4 exists to keep.
    pub fn importing_from(mut self, space: ImportSpace) -> Self {
        self.space = Some(space);
        self
    }

    /// The import space this mount reads under.
    fn space(&self) -> ImportSpace {
        match &self.space {
            Some(space) => space.clone(),
            None => ImportSpace::rooted_at(self.document.parent().unwrap_or(Path::new("."))),
        }
    }

    /// Add a scope this document's selection may name. **Nearest first**:
    /// call order is resolution order, the way a mount path resolves — a
    /// view's own scope before the instance root's before the process rung.
    pub fn selecting(mut self, scope: Scope) -> Self {
        self.scopes.push(scope);
        self
    }

    /// The registered ids this document draws `screen` blocks for.
    pub fn drawing(mut self, screens: &[RouteId]) -> Self {
        self.screens.extend_from_slice(screens);
        self
    }

    /// The path this mount reads from.
    pub fn document(&self) -> &Path {
        &self.document
    }

    /// Offer one already-parsed document to a running check.
    ///
    /// Four questions, in §4.1's two grades: what the document's
    /// `host_state {}` selects, what each `screen`'s `state {}` selects,
    /// what its `events {}` raises, and whether its screens and the
    /// table's ids are the same set.
    ///
    /// A screen's own block additionally selects from **its node's scope**,
    /// when that node owns one — which is what today's `"{id}::{field}"`
    /// projection does by hand, and what the binding will do for real. A
    /// view node that owns no scope simply reads from the document's, and
    /// is not named, because naming a scope nothing publishes is a refusal
    /// and a screen reading from the root above it is not a mistake.
    pub fn check_into(&self, schema: &ModuleSchema, into: &mut Validation<'_>) {
        let document = self.document.display().to_string();
        if let Some(host_state) = &schema.host_state {
            into.selects(&document, &self.scopes, &selection_of(host_state, schema));
        }
        into.raises(&document, &self.scopes, &raises_of(schema));
        into.draws(&document, &schema.screen_ids(), &self.screens);

        for (id, screen) in &schema.screens {
            if screen.state.fields.is_empty() {
                continue;
            }
            let mut scopes = Vec::with_capacity(self.scopes.len() + 1);
            if let Some(node) = self.screens.iter().find(|node| **node == id.as_str()) {
                let own = Scope::Node(node);
                // Not if the document already named it: a duplicate would
                // read as one scope shadowing itself.
                if into.publishes(own) && !self.scopes.contains(&own) {
                    scopes.push(own);
                }
            }
            scopes.extend(self.scopes.iter().copied());
            into.selects(
                &format!("{document} screen \"{id}\""),
                &scopes,
                &selection_of(&screen.state, schema),
            );
        }
    }
}

/// A shipped document that would not read.
///
/// Separate from a [`Finding`](structure::Finding) on purpose: a document
/// that will not parse has a real error waiting with real diagnostics, and
/// burying it under a contract complaint is how the diagnostic that would
/// have helped gets lost.
#[derive(Debug)]
pub struct Unreadable {
    pub document: PathBuf,
    /// Boxed because a schema load failure carries an `io::Error` or a
    /// whole `SyntaxError`, and every `check` that succeeds would otherwise
    /// pay for the size of the one that did not.
    pub why: Box<SchemaLoadError>,
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} could not be read: {}",
            self.document.display(),
            self.why
        )
    }
}

impl std::error::Error for Unreadable {}

/// **Load every shipped document against its schemas** — the harness each
/// consumer instantiates (`APPLICATION_BUILD.md` WP-2.4).
///
/// It is deliberately the whole guarantee in one call, because what it
/// replaces is three games' worth of bespoke tests that each held one
/// corner of it:
///
/// | this asks | it replaces |
/// |---|---|
/// | every selected field is provided, at that shape | untold_lore's `every_declared_key_is_projected`, regency's `the_typed_chrome_conforms_to_the_declared_schema` |
/// | every raise is accepted, at that arity and those shapes | `every_declared_raise_reaches_a_handler` / `every_declared_raise_has_a_handler`, and the wire half of celia's `every_button_reaches_a_route` |
/// | the screens and the registered ids are the same set | `the_shipped_document_declares_exactly_the_registered_screens` |
/// | nothing is provided or accepted that no document uses | the reverse half of each of those, now a report rather than an assertion (§4.1) |
///
/// ```no_run
/// # use ogham::contract::{Documents, Mount, Scope, Store};
/// # fn store() -> Store { Store::new() }
/// # fn data_dir() -> std::path::PathBuf { ".".into() }
/// let store = store();
/// let found = Documents::new(&store)
///     .mounting(
///         Mount::new(data_dir().join("ui/lobby.ogh"))
///             .selecting(Scope::Node("lobby"))
///             .selecting(Scope::Process)
///             .drawing(&["lobby"]),
///     )
///     .check()
///     .expect("the shipped documents read");
/// assert!(!found.refuses(), "{found}");
/// ```
pub struct Documents<'a> {
    store: &'a Store,
    mounts: Vec<Mount>,
}

impl<'a> Documents<'a> {
    /// Start a check against the facts one store publishes. The store needs
    /// its registrations run and nothing else — no route walked, no scope
    /// mounted, no frame taken. That is what makes this answerable under
    /// `cargo test` rather than at first boot.
    pub fn new(store: &'a Store) -> Self {
        Self {
            store,
            mounts: Vec::new(),
        }
    }

    /// Add a document.
    pub fn mounting(mut self, mount: Mount) -> Self {
        self.mounts.push(mount);
        self
    }

    /// Read every document and check it, in §4.1's two grades.
    ///
    /// The unread directions — a field provided and selected by nothing, an
    /// intent accepted and raised by nothing — are answered over the whole
    /// set, which is why they are here and not on [`Mount`]: a field one
    /// document selects and its three siblings do not is read.
    pub fn check(&self) -> Result<Findings, Unreadable> {
        let mut check = Validation::new(self.store);
        for mount in &self.mounts {
            let schema =
                load_schema_in(&mount.document, &mount.space()).map_err(|why| Unreadable {
                    document: mount.document.clone(),
                    why: Box::new(why),
                })?;
            mount.check_into(&schema, &mut check);
        }
        Ok(check.finish())
    }
}

/// The one document a mounted instance is holding, checked against the
/// store — the load-time and hot-reload half of the same question
/// [`Documents`] asks at `cargo test` time.
///
/// Returns the refusals as one sentence, or `Ok(())`. Reports are not in
/// it: a reload must not be rejected over coverage drift (§4.1), and the
/// place drift is *read* is the harness, where somebody is looking.
pub fn refusals(schema: &ModuleSchema, store: &Store, mount: &Mount) -> Result<(), String> {
    let mut check = Validation::new(store);
    mount.check_into(schema, &mut check);
    let found = check.finish();
    let mut refusals = found.refusals().peekable();
    if refusals.peek().is_none() {
        return Ok(());
    }
    Err(refusals
        .map(|finding| finding.to_string())
        .collect::<Vec<_>>()
        .join("\n"))
}
