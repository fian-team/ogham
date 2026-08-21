//! The document contract: what a document *declares*, said in the
//! structure framework's vocabulary, and the harness that holds every
//! shipped document to it at `cargo test` time.
//!
//! `APPLICATION.md` §4.1 gives a provider the schema and a consumer a
//! selection. The provider's half lives in the structure framework — a
//! scope's [reflection](structure::Kind) and the
//! [intents](structure::Vocabulary) it accepts — and the grading of the two
//! against each other lives there too ([`structure::Validation`]). What
//! lives *here* is the translation, because a document is written in the
//! surface framework's language and the structure framework has never
//! heard of it: a `host_state {}` block becomes a list of [`Field`]s, an
//! `events {}` block becomes a list of [`Declared`] raises, and a
//! `TypeRef` becomes a [`Kind`].
//!
//! # Why this is a crate of its own
//!
//! Because §2's sentence is "only the binding depends on both", and this
//! is a binding by that sentence's own definition: it needs the parser to
//! read what a document says and the store to read what a provider
//! publishes, and it needs nothing else in the world. Two dependencies,
//! `ogham` and `structure`, and there will never be a third.
//!
//! It is deliberately **not** part of `lorekeeper/driver`, the other
//! binding. The whole property this harness was built for is that a
//! consumer answers "do my shipped documents agree with my schemas?"
//! under `cargo test` — no path walked, no instance mounted, no frame
//! taken, no window opened. A consumer that had to link `app`, `winit`
//! and Skia to ask that question would stop asking it.
//!
//! # The CI moment
//!
//! [`Documents`] is the reason this crate exists rather than a
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

use structure::schema::{Field, Lit};

use ogham::runtime::imports::ImportSpace;
use ogham::runtime::schema::{
    load_schema_in, EventSig, ModuleSchema, PrimType, RecordSchema, SchemaLoadError, TypeRef,
};
use ogham::runtime::value::Value;

/// The structure framework's half of the seam, re-exported so a consumer
/// naming a scope or reading a finding needs no dependency of its own.
///
/// Not scaffolding, unlike `ogham::route`'s re-exports: this crate depends
/// on `structure` for good, because a contract is exactly the place the
/// two frameworks are put side by side.
pub use structure::{Declared, Finding, Findings, Kind, RouteId, Scope, Store, Validation};

/// Where a document's imports resolve from, re-exported so a consumer
/// declaring a [`Mount`] for a split document needs no second import path.
pub use ogham::runtime::imports::ImportSpace as Imports;

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

    /// What this document's selection holds the moment it mounts, ready
    /// for [`RuntimeConfig::with_host_state`](ogham::runtime::config::RuntimeConfig::with_host_state).
    ///
    /// A `host_state {}` document seeds itself: it declares its own field
    /// shapes and its own defaults, and the runtime fills them in before
    /// the first execution. A selection declares neither — that is the
    /// inversion — so the seed comes from the providing scope's
    /// reflection instead, which is where §4.1 says an at-mount value
    /// lives. Without it a document that mounts and immediately draws
    /// fails on whichever field its host had not pushed yet, and every
    /// game would grow a hand-written pre-projection to work around it.
    ///
    /// This is the binding's job from P4 (§6.1: the binding mounts,
    /// projects, and tears down); it lives here until then, for the same
    /// reason [`Mount`] itself does.
    ///
    /// A document that will not read seeds nothing: it has a real error
    /// waiting with real diagnostics.
    pub fn at_mount(&self, store: &Store) -> std::collections::HashMap<String, Value> {
        let mut seeded = std::collections::HashMap::new();
        let Ok(schema) = load_schema_in(&self.document, &self.space()) else {
            return seeded;
        };
        for selection in &schema.selections {
            let scopes: Vec<Scope> = match &selection.scope {
                Some(name) => self
                    .scopes
                    .iter()
                    .filter(|scope| named(scope, name))
                    .copied()
                    .collect(),
                None => self.scopes.clone(),
            };
            for field in &selection.fields {
                let found = scopes.iter().find_map(|scope| {
                    store
                        .reflection(*scope)
                        .and_then(|kind| kind.field_at(field).ok())
                });
                if let Some(provided) = found {
                    seeded.insert(field.clone(), at_mount(provided));
                }
            }
        }
        seeded
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
        for selection in &schema.selections {
            let scopes = match &selection.scope {
                // A fragment (§4.7): stated once, checked against the
                // scopes of *this* mount. Mount it somewhere else and the
                // same three names are checked against those scopes
                // instead, which is the whole of the property — one
                // selection, one statement, validated at each mount.
                None => self.scopes.clone(),
                // A selection that names its scope (§4.6) is checked
                // against that scope alone, which is what makes its
                // refusal say which provider was meant and its agreement
                // report no shadowing.
                Some(name) => match self.scopes.iter().find(|scope| named(scope, name)) {
                    Some(scope) => vec![*scope],
                    None => {
                        into.unmounted(&document, name, &self.scopes);
                        continue;
                    }
                },
            };
            into.selects_named(&document, &scopes, &selection.fields);
            // And how far into those names the document actually reads.
            // A selection says `hud` and stops; the helpers say
            // `hud.clock`, and the shape that has to have a `clock` on it
            // is the provider's. Checked against *this* selection's
            // scopes, because §4.6 binds a name exactly once, so the
            // selection that bound the root is the one that resolves it.
            into.reads(
                &document,
                &scopes,
                &through(&schema.reads, &selection.fields),
            );
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

/// The value a field holds at mount, as the runtime's own [`Value`]
/// (§4.1: "a field's at-mount value is declared").
///
/// The provider declares it and the reflection carries it, so this is a
/// translation and not a decision — which is the point. Under
/// `host_state {}` the *document* declared its defaults and the runtime
/// seeded from them; a selection declares none, so the seed has to come
/// from the same place the shape does, or a document that mounts and
/// immediately draws fails on whichever field the host had not got to yet
/// (untold_lore's `launch_fade` lesson, one level up).
///
/// A union and a back-edge have no document-side encoding yet — how a
/// tagged union reaches a document is P5's to settle — so they seed as
/// absent rather than as a guess.
pub fn at_mount(field: &Field) -> Value {
    match field.initial.value() {
        Lit::Str(s) => Value::String(s.clone()),
        Lit::Int(n) => Value::Integer(*n as i32),
        Lit::Float(f) => Value::Float(*f),
        Lit::Bool(b) => Value::Boolean(*b),
        Lit::Absent => Value::Void,
        Lit::Composed => match &field.kind {
            Kind::Record(fields) => Value::Map(
                fields
                    .iter()
                    .map(|f| (f.name.clone(), at_mount(f)))
                    .collect(),
            ),
            Kind::List(_) => Value::Array(Vec::new()),
            Kind::Map(_) => Value::Map(std::collections::HashMap::new()),
            Kind::Tuple(kinds) => Value::Array(
                kinds
                    .iter()
                    .map(|kind| at_mount(&Field::new("", kind.clone())))
                    .collect(),
            ),
            Kind::Union(_) | Kind::Cycle => Value::Void,
            _ => Value::Void,
        },
    }
}

/// The document's reads that go *through* one selection's names.
///
/// A document may hold several selections against several scopes, and its
/// reads arrive as one list — the file does not say which `select` bound
/// which name. The root does: §4.6 binds a name exactly once across a
/// document, so a path's first segment names its selection unambiguously.
fn through(reads: &[String], selected: &[String]) -> Vec<String> {
    reads
        .iter()
        .filter(|path| {
            path.split_once('.')
                .is_some_and(|(root, _)| selected.iter().any(|name| name == root))
        })
        .cloned()
        .collect()
}

/// Whether `scope` is the one a document's `select <name>` means.
///
/// A route node's scope is named by its id, which is the name the route
/// table already gave it and the only name that crosses the seam. The
/// process rung has no id — it is not a node — so a document reaches it
/// through a fragment, which resolves nearest-first over the whole mount
/// and finds it there.
fn named(scope: &Scope, name: &str) -> bool {
    matches!(scope, Scope::Node(id) if *id == name)
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
/// # use contract::{Documents, Mount, Scope, Store};
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

// --- one mounted instance ---------------------------------------------------

/// The contract, asked of one **mounted** document — the load-time and
/// hot-reload half of what [`Documents`] asks of a whole repo at
/// `cargo test` time (§4.1: "validation runs at document load *and at
/// every hot reload*").
///
/// An extension trait rather than inherent methods because
/// [`Chrome`](ogham::route::Chrome) lives in the surface framework, which
/// depends on nothing of the structure framework (§2). `Chrome` owns the
/// machinery — a gate the reload must pass and a refusal held apart from
/// a compile error — and this is the question that goes into it.
pub trait Checked {
    /// Check the mounted document against what the store publishes, in the
    /// two grades.
    ///
    /// Refusals are kept, and [`Chrome::refusal`](ogham::route::Chrome::refusal)
    /// reports them once rather than per frame; the reports come back in
    /// the [`Findings`] for a host that wants to print them.
    fn check(&mut self, store: &Store, mount: &Mount) -> Findings;

    /// [`Chrome::frame`](ogham::route::Chrome::frame) with the hot reload
    /// held to the contract.
    ///
    /// A hot-reload refusal rejects the new document and names the field
    /// **without tearing down the running instance**: the edit is checked
    /// before the swap, so a refused one costs the candidate and nothing
    /// else.
    fn frame_checked(&mut self, store: &Store, mount: &Mount, width: f32, height: f32, dt: f32);
}

impl Checked for ogham::route::Chrome {
    fn check(&mut self, store: &Store, mount: &Mount) -> Findings {
        let mut check = Validation::new(store);
        if let Some(schema) = self.ui().module_schema() {
            mount.check_into(&schema, &mut check);
        }
        let found = check.finish();
        let refused: Vec<String> = found.refusals().map(ToString::to_string).collect();
        self.refuse(match refused.is_empty() {
            true => None,
            false => Some(refused.join("\n")),
        });
        found
    }

    fn frame_checked(&mut self, store: &Store, mount: &Mount, width: f32, height: f32, dt: f32) {
        self.frame_gated(|schema| refusals(schema, store, mount), width, height, dt);
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
