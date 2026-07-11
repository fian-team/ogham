# Ogham — Typed Rust↔Ogham Bindings (Phase 1)

> **Status: PARTIALLY CUT (2026-06-07, `addd9da`) — read this before
> planning against anything below.**
>
> The Rust-side half of this design — `ogham-derive`
> (`OghamState`/`OghamMsg`/`OghamRecord`), `TypedOgham<S, M>`,
> `watch_typed`/`from_source_typed`, `ogham check`, and the
> diagnostics manifest system — was built (M4–M6, landed 2026-05),
> then **deliberately removed** as a rival typed layer per
> [`IDENTITY_AND_SCOPE.md`](IDENTITY_AND_SCOPE.md) §1–2: one
> adoption site ever, superseded by `editable` for the
> typed-introspectable-state need. Do not re-implement it from this
> document.
>
> What **survives, load-bearing** (see the cut commit for the
> split): the `.ogh`-source schema — `record` / `host_state {}` /
> `events {}` declarations, `ModuleSchema`/`RecordSchema`/
> `load_schema*`, compiler strict-mode resolution + event-call
> validation, LSP hover/diagnostics, the view layer's Tenet-6
> host-state requirements, and `ModuleSchema::validate_host_state`
> (runtime value validation for host conformance tests). The
> `.ogh` declaration is the **single** description of the
> contract; hosts inject loose `Value`s against it. Sections below
> describing the `.ogh` grammar and strict mode remain accurate;
> sections describing derives, `TypedOgham`, manifests, and the
> CLI are **historical**.
>
> This document specifies Phase 1 of the typed-bindings work: a
> per-module schema declared in `.ogh` source, paired with Rust
> derive macros and a typed scene handle. It supersedes none of
> the existing live contracts; it adds an opt-in mode that
> coexists with today's loose, stringly-typed `set_host_state`
> / `event(...)` path.
>
> See [`INTENT.md`](INTENT.md) §2 ("Host state flows in, events
> flow out") for the asymmetry this design preserves. See
> [`RUNTIME.md`](RUNTIME.md) for the loose path being extended.
> See [`LANGUAGE.md`](LANGUAGE.md) for the front-end this design
> grafts onto. See
> [`TYPED_BINDINGS_IMPLEMENTATION.md`](TYPED_BINDINGS_IMPLEMENTATION.md)
> for the per-merge implementation history.
>
> **Implementation status (2026-05):** Phase 1 is implemented
> end-to-end in the `phase1-typed-bindings` branch. Merges M0–M6
> are landed:
> - M0: SyntaxError carries length/note/help; LSP renders rich
>   diagnostics.
> - M1: scanner keywords (`record`, `host_state`, `events`,
>   `Self`, `?`); parser declarations + grammar; ModuleSchema +
>   two-pass resolver + standalone load_schema.
> - M2: strict-mode compiler integration (identifier resolution,
>   event-call validation, Levenshtein-1 suggestions).
> - M3 v1: LSP diagnostics flow through the full pipeline;
>   schema-aware hover for host_state fields and record names.
>   (Schema-aware completion + record goto-definition are a
>   future M3.5.)
> - M4: Cargo workspace; `ogham-derive` proc-macro crate with
>   OghamRecord/OghamState/OghamMsg derives.
> - M5: TypedOgham<S, M> handle, watch_typed/from_source_typed,
>   startup schema-match check, MPSC event channel.
> - M6: hot-reload preserves typed state across schema-stable
>   reloads, fails loud on schema-incompatible reloads.
>
> One Untold-Lore-side smoke-test step (chest_ui migration) is
> the natural validation gate but lives in a separate repo;
> coordinate that migration when ready to start landing the
> 17-UI migration sequence in
> [`TYPED_BINDINGS_UL_AUDIT.md`](TYPED_BINDINGS_UL_AUDIT.md).

---

## Motivation

Today's Rust↔Ogham boundary is a `HashMap<String, Value>` for
state and a string-keyed `Fn(&[Value])` for events. Renames are
silent; typos are silent; missing fields manifest as runtime
crashes rather than build errors. The `Ogham` consumer
(currently Untold Lore) carries large amounts of boilerplate to
patch around this — closure factories cloning `Arc<Mutex<...>>`
form-state shadows, hand-rolled `push_state` field-by-field
shovels, and runtime `Value::String(s) = args.first()` matches
repeated across every event handler.

Phase 1 introduces an *opt-in* typed contract per `.ogh` module:
the module declares its host-state schema and event signatures;
the Rust caller derives a matching pair of types and calls a
typed constructor that verifies them at startup and exposes a
typed `set_state` / `poll_msg` API. The loose path stays
unchanged. Migration is per-file, not per-codebase.

Phase 1 deliberately does not try to type-check the *body* of
`.ogh` expressions (no arithmetic-type inference, no `let`-type
inference, no function-signature checking). It types the
*boundary* and the resolution of host-state identifiers at the
boundary.

---

## Conceptual overview

```mermaid
flowchart LR
    subgraph .ogh ["settings.ogh"]
        rec["record SettingsState { ... }"]
        hs["host_state { ... }"]
        ev["events { ... }"]
        body["body uses master_volume,<br/>event(\"set_master_volume\", v)"]
    end
    subgraph rust ["Rust"]
        s["#[derive(OghamState)]<br/>struct SettingsState"]
        m["#[derive(OghamMsg)]<br/>enum SettingsMsg"]
        h["TypedOgham&lt;S, M&gt;"]
    end
    rec -.matches.-> s
    ev  -.matches.-> m
    s & m --> h
    body -- compiled against schema --> ms[ModuleSchema]
    ms -- startup match check --> h
```

Three new declaration forms in `.ogh` (`record`, `host_state`,
`events`), three derive macros in Rust (`OghamRecord`,
`OghamState`, `OghamMsg`), one new constructor (`watch_typed`).
The compiler emits a `ModuleSchema` from the declarations; the
typed constructor compares it against the macro-generated
schemas on the Rust side and refuses to start if they disagree.

---

## The four contract surfaces

For symmetry of this doc with the audit that produced it, the
fixes line up surface-by-surface:

| Surface | Today | Phase 1 |
|---|---|---|
| Host state | `runtime.set_host_state("k", v)`; bare globals in `.ogh` | `host_state { … }` block; `TypedOgham::set_state(&S)` |
| Events     | `event("name", a, b)`; `Fn(&[Value])` handlers       | `events { name(T, T) }`; `TypedOgham::poll_msg() -> Option<M>` |
| Nested data | implicit `Value::Map`/`Value::Array` shapes | `record Foo { … }` declarations; `#[derive(OghamRecord)]` |
| Custom widget props | `WidgetDescriptor.properties: HashMap<String, Value>` | **Phase 4** (out of scope here; same machinery applies) |

Event-handler return values are dropped on the typed path; see
"Out of scope" §below.

---

## Grammar additions

Three new top-level declaration forms. EBNF, alongside the
existing module grammar:

```ebnf
Module        ::= TopLevel*
TopLevel      ::= ImportStmt
                | RecordDecl
                | HostStateDecl
                | EventsDecl
                | Statement                  (* existing: let, state, fn, expr *)

RecordDecl    ::= 'record' Ident '{' FieldList '}' ';'
HostStateDecl ::= 'host_state' '{' FieldList '}' ';'
EventsDecl    ::= 'events' '{' EventList '}' ';'

FieldList     ::= (FieldDecl (',' FieldDecl)* ','?)?
FieldDecl     ::= Ident ':' TypeRef ('=' Literal)?

EventList     ::= (EventSig (',' EventSig)* ','?)?
EventSig      ::= Ident '(' (TypeRef (',' TypeRef)* ','?)? ')'

TypeRef       ::= PrimType
                | RecordRef
                | ContainerType
                | TypeRef '?'                (* Optional, postfix *)
PrimType      ::= 'int' | 'float' | 'string' | 'bool'
RecordRef     ::= Ident
ContainerType ::= 'array' '<' TypeRef '>'
                | 'map' '<' KeyType ',' TypeRef '>'
                | 'Self'                     (* only inside RecordDecl *)
KeyType       ::= 'string' | 'int'

Literal       ::= IntegerLit | FloatLit | StringLit | BoolLit
```

### Grammar tenets

- **Trailing commas allowed** in field lists, event lists, and
  argument lists. Matches map/array literal conventions used by
  the rest of the language.

  *Why:* removes diff noise on add/remove; matches Rust's habit;
  no parsing ambiguity.

- **Each declaration ends with `;`.** `record Foo { ... };` not
  `record Foo { ... }`. Symmetrical with the existing `let` /
  `state` statement terminators; the parser already keys off `;`
  to recover from errors.

- **At most one `host_state {}` and one `events {}` per module.**
  Multiple `record` declarations are fine.

  *Why:* the schema for a module is singular; nothing useful is
  expressed by splitting it across blocks. Two blocks would just
  invite the question "which one wins for an overlapping field?"

- **Declarations may appear in any order.** Resolution is
  two-pass: collect all `record` / `host_state` / `events`
  declarations first, then resolve type names in field types.
  Forward references between records are legal.

  *Why:* lets authors put the API surface at the top of the file
  (`host_state`, `events`) and the supporting record decls
  underneath, or vice versa, without grammar contortions.

- **`Self` is a valid `TypeRef` only inside a `RecordDecl`'s
  field types.** It refers to the enclosing record type.

  *Why:* enables `record Tree { children: array<Self> }` and
  `record Node { parent: Self? }` without forward-reference
  ceremony. Rejecting `Self` outside a record keeps the rule
  one-sentence-stateable.

### Self-reference rule

A record may reference itself transitively only through `array<>`,
`map<>`, or `?` (i.e. through indirection). Direct
`record Node { next: Node }` is rejected at compile time.

*Why:* mirrors Rust's "infinite size" rule. `array<Self>` is
heap-indirect; `Self?` is `None`-terminable; a bare `Self` field
would have unbounded size by construction. The check is local
(walk the field type tree, error if any `RecordRef` to the
enclosing record is reached without crossing an indirection).

---

## The type universe

```rust
// All three live in a new module: src/runtime/schema.rs

pub enum TypeRef {
    Primitive(PrimType),
    Record(String),                          // resolved at use, not declared
    Array(Box<TypeRef>),
    Map(KeyType, Box<TypeRef>),
    Optional(Box<TypeRef>),
}

pub enum PrimType { Int, Float, Bool, String }

pub enum KeyType { String, Int }
```

### Type universe tenets

- **Map keys are `string` or `int` only.** Floats are forbidden
  (NaN-equality), bools are silly, records are forbidden in v1.

  *Why:* matches what `HashMap` can sanely key on; the
  `Value::Map` runtime representation is `HashMap<String, Value>`
  today, so int keys will require a small runtime extension —
  flagged below as an open question for first implementation
  pass.

- **Every type is either a primitive, a named record, a
  container of types, or an Optional of a type.** No anonymous
  records (Approach B from the audit discussion). No tuples, no
  unions, no generic parameters.

  *Why:* keeps the schema small enough to type-check trivially.
  Anonymous structures fail the cross-module-share test (you
  can't `import` an anonymous shape) and damage error-message
  quality (the diff is a wall of fields rather than "expected
  Player, got Item"). Named records also leave a clean extension
  point for a future `enum`/sum type story when proper
  Optional/nullability gets revisited.

- **`Optional<T>` is the only nullable form.** A field declared
  `string` is required; `string?` is `Some(s)` or `None`.

  *Why:* eliminates the today-idiom `match (s != "") { … }`
  which conflates "empty string" with "missing." Forces a
  decision at declaration time.

- **The runtime `Value` enum gains nothing.** `T?` round-trips as
  the existing `Value`s; `None` is represented as `Value::Void`
  on the wire, `Some(v)` as `v` itself.

  *Why:* Phase 1 should not require touching the VM. The schema
  layer interprets `Value::Void` as `None` only when the schema
  for the slot is `Optional`; everywhere else `Void` keeps its
  current meaning ("no value produced," e.g. by an event handler
  return).

---

## Strict-mode resolution

A module that declares `host_state {}` enters **strict mode**.
Strict mode changes one thing: the rules for resolving bare
identifier references in expressions. Everything else (parsing,
bytecode, runtime semantics) is unchanged.

In strict mode, a bare identifier in an expression must resolve
to one of:

1. A local `let` binding in scope.
2. A function parameter in scope.
3. A `state` declaration in scope.
4. A field declared in `host_state {}`.
5. An imported symbol (function or record).
6. A built-in (`event`, `mutation`, `log`, `rgb`, `rgba`, etc.).
7. A declared `record` name (only legal in `TypeRef` positions —
   `host_state {}`, `events {}`, `record { ... }`).

Any other identifier reference is a compile error ("unknown
identifier `foo`"). In loose mode (no `host_state {}` block),
resolution falls through to the runtime as today.

### Strict-mode field access

Field access on a typed value (e.g. `player.hp`) is checked
against the schema in strict mode:

- If `player` resolves to a record-typed slot, `hp` must be a
  field of that record.
- If `player` resolves to an `array<T>` or `map<K, V>`, dot
  access is an error (use `[]` for arrays, `.get(k)` for maps —
  same as today).
- If `player` resolves to an `Optional<T>`, dot access is an
  error: must be matched first.
- If `player` resolves to a primitive, dot access is an error.

### What strict mode does NOT check

- Arithmetic types (`"foo" + 5` keeps coercing as today).
- Function argument types or return types.
- `let` binding types.
- Style property values (these go through the existing
  `apply_flex_style_from_map` path).
- The contents of `state` initial values.

*Why:* the goal is a typed boundary, not a typed language. Body
type-checking is a multi-quarter project that would gate every
other improvement on its design. Phase 1 stops at "the host and
the module agree on the shape of what crosses between them."

### Event-call checking

In strict mode, every `event("name", arg1, arg2, ...)` call is
checked against the `events {}` declarations:

- `"name"` must be a string literal that matches a declared
  event. Computed event names (`event(some_var, ...)`) are an
  error in strict mode.
- The number of arguments must match the declared signature.
- Each argument's expression need not be type-inferred, but if
  it *is* a bare identifier whose schema type is known, that
  type is checked against the declared parameter type.

*Why:* string-literal event names are the 100% case in
production code; allowing computed names defeats the schema
entirely. Argument-arity is cheap and catches the most common
class of bug. Argument-type checking is best-effort because
expression typing is out of scope, but the easy cases (passing a
host-state field through directly) come for free.

---

## Defaults and Optional

```ogh
host_state {
  count:        int = 0,           // defaulted: never missing at use site
  player_name:  string,            // required: must be supplied before render
  selection:    string?,           // optional: must be matched before use
};
```

### Required (no default, no `?`)

The host *must* supply a value before the first render. The
typed constructor (`watch_typed`) takes the initial state struct
by value; the macro proves at compile time that every required
field is populated. There is no "uninitialized first frame"
window.

In loose mode (no schema or no Rust derive), behavior is
unchanged: missing keys read as undefined and probably crash.
Strict mode makes that impossible.

### Defaulted

The default is a literal that becomes the initial value if the
host has not pushed one before first render.

In typed mode (`watch_typed` + `OghamState`), defaults are
**purely declarative**: the Rust struct always supplies a value
because `OghamState` requires every field. The default is a
documentation aid that survives in the schema and shows up in
LSP hovers (`count: int (default: 0)`) but is never actually
consulted.

In loose mode (no Rust derive but `host_state {}` present), the
default is consulted at module load time to seed the host_state
map.

*Why two semantics:* the literal-default form is useful for
loose-mode authors who want safety without going all-in on the
derive-macro path. Typed mode renders it cosmetic, which is
fine. Forbidding defaults in typed mode would be a needless
inconsistency.

### Optional (`T?`)

A field that may or may not be present. Represented as
`Option<T>` on the Rust side, matched in `.ogh` via:

```ogh
match selection {
  Some(s) => Text { text: "Selected: " + s, ... },
  None    => Text { text: "Nothing selected", ... },
}
```

No `?.` chaining, no `unwrap`, no `.is_some()`. Just match.
Phase 1 keeps Optional ergonomics minimal — sweetening (`?.`,
unwrap, default-fallback `??`) waits for the larger
nullability/sum-type pass agreed in the audit.

`Option<T>` on the wire is `Value::Void` for `None` and the
underlying `Value` for `Some`.

---

## Diagnostics

Strict mode's value comes from the diagnostics it surfaces. The
target shape:

```text
error: unknown identifier `master_voloume`
  --> settings.ogh:42:14
   |
42 |   text: master_voloume -> string,
   |         ^^^^^^^^^^^^^^^
   = note: this module declares `host_state {}`; identifiers
     resolve only to declared fields, locals, parameters, state,
     imports, and built-ins
   = help: did you mean `master_volume`?
```

```text
error: argument type mismatch in `event("set_master_volume", ...)`
  --> settings.ogh:20:38
   |
20 |     mouse_down: fn() { event("set_master_volume", v); },
   |                                                   ^
   = note: declared signature: set_master_volume(float)
   = note: argument 1 has type `string`, expected `float`
```

```text
error: unknown event `closse`
  --> settings.ogh:5:25
   |
5  |     mouse_down: fn() { event("closse"); },
   |                              ^^^^^^^^
   = note: declared events: close, set_master_volume,
     set_music_volume, rebind
   = help: did you mean `close`?
```

```text
error: field `xp_needd` not found on record `Skill`
  --> talents.ogh:30:14
   |
30 |   text: skill.xp_needd -> string,
   |               ^^^^^^^^
   = note: `Skill` has fields: name, level, xp, xp_needed
   = help: did you mean `xp_needed`?
```

### Diagnostic tenets

- **Every strict-mode error includes a `note:` explaining why
  this rule exists.** The first time an author hits a rule,
  the message itself teaches the model.

- **Levenshtein-1 suggestions are required for all
  identifier-not-found errors** (typo'd field names, typo'd
  event names, typo'd record names). The set being suggested
  from is small and known; this is cheap and high-value.

- **All strict-mode errors flow through the existing
  `SyntaxError` plumbing** so the LSP picks them up via
  `collect_diagnostics` automatically. No new plumbing.

  *Why:* the LSP reads parse errors today; piggy-backing on the
  same path means LSP integration is partly free.

---

## Compiled artifact: `ModuleSchema`

The parser, when it encounters `host_state {}` / `events {}` /
`record` declarations, builds a `ModuleSchema` and attaches it
to the compiled `Function` (which today is the module-level
opaque proto consumed by the VM).

```rust
// src/runtime/schema.rs (new module)

pub struct ModuleSchema {
    pub records:    HashMap<String, RecordSchema>,
    pub host_state: Option<RecordSchema>,         // None in loose mode
    pub events:     HashMap<String, EventSig>,
    pub imports:    HashMap<String, ImportedTy>,  // e.g. Item from "./inv.ogh"
}

pub struct RecordSchema {
    pub fields: BTreeMap<String, FieldSchema>,    // BTree → stable ordering
}

pub struct FieldSchema {
    pub ty:      TypeRef,
    pub default: Option<Literal>,
}

pub struct EventSig {
    pub args: Vec<TypeRef>,
}

pub enum ImportedTy {
    Record(RecordSchema),
}
```

### `ModuleSchema` tenets

- **`ModuleSchema` is `None` in loose mode and `Some(...)` in
  strict.** Strict mode is detected by the *presence* of a
  `host_state {}` declaration, not a flag.

  *Why:* one source of truth for "is this module strict";
  removes the question of inconsistent state ("strict flag set
  but no schema").

- **The schema is bytecode-cache-stable.** `ModuleSchema` is
  built at parse time, lives alongside the cached
  `FunctionProto`, and is reused across rerenders without
  regeneration.

  *Why:* matches the existing bytecode-caching model in
  `RUNTIME.md`; the schema is about the *module*, not about any
  particular execution.

- **Imports of records are resolved at parse time, not at
  execution time.** When a module does
  `import [Item] from "./inventory.ogh"`, the parser loads the
  imported module's schema and stitches `Item` into the local
  schema's `imports` map.

  *Why:* the LSP needs imported-record information to do
  hover/completion without running the VM. The existing import
  machinery (`runtime::execute_module`) is execution-time; we
  add a parse-time schema-only pass parallel to it.

- **Record name conflicts on import are errors.** If
  `inv.ogh` and `shop.ogh` both export `Item` and a third file
  imports both, the importer must alias one (`import [Item as
  ShopItem] from "./shop.ogh"` — extends today's import
  grammar).

  *Why:* matches Rust's `use` semantics; silently shadowing one
  is the surprise-everyone-most behavior.

---

## Rust side: derive macros

Three derives, in priority order of impact.

### `#[derive(OghamState)]`

Top-level. Marks a struct as the host_state for a typed module.
Generates:

- `impl OghamState for SettingsState` with:
  - `const OGHAM_SCHEMA: RecordSchema` — the schema this struct
    expects, computed at compile time from field types.
  - `fn snapshot_into(&self, sink: &mut impl HostStateSink)` —
    field-by-field invocation of `sink.set("name", &self.field)`,
    using the existing `IntoHostValue` / `HostStateSinkExt`
    infrastructure (so existing `inject_host_state_if_changed`
    diffs apply and there's no rerender churn on no-op frames).
  - `fn diff_apply(&self, prev: &Self, sink: &mut impl HostStateSink)`
    — only sets fields that changed against the previous
    snapshot; called by `TypedOgham::set_state`.

```rust
#[derive(OghamState)]
struct SettingsState {
    master_volume: f32,
    music_volume:  f32,
    keybinds:      HashMap<String, String>,
    server_name:   Option<String>,         // matches `server_name: string?`
    #[ogham(default)]
    pending_count: i32,                    // documents the .ogh-side default
}
```

### `#[derive(OghamRecord)]`

For nested record types referenced from a `host_state` field
or another record. Generates:

- `impl OghamRecord for Item` with:
  - `const OGHAM_SCHEMA: RecordSchema`
  - `fn into_host_value(&self) -> Value` — converts to
    `Value::Map`.
  - `impl IntoHostValue for &Item` so it composes with
    `HostStateSinkExt::set` and array/map field translation.

```rust
#[derive(OghamRecord)]
struct Item {
    name:  String,
    count: i32,
    icon:  Option<String>,
}
```

### `#[derive(OghamMsg)]`

For the events enum. Generates:

- `impl OghamMsg for SettingsMsg` with:
  - `const OGHAM_EVENTS: HashMap<&'static str, EventSig>` (or
    equivalent) — declared event signatures.
  - `fn try_from_event(name: &str, args: &[Value]) -> Option<Self>`
    — parse args into a variant.
  - `fn register(config: &mut RuntimeConfig, tx: Sender<Self>)`
    — registers a handler for each declared variant that pushes
    parsed messages into `tx`.

```rust
#[derive(OghamMsg)]
enum SettingsMsg {
    SetMasterVolume(f32),
    SetMusicVolume(f32),
    Rebind(String, String),
    Close,
}
```

### Field attributes

| Attribute | Applies to | Effect |
|---|---|---|
| `#[ogham(rename = "...")]` | record / state field | Use a different name on the `.ogh` side |
| `#[ogham(skip)]` | state field | Field is Rust-only; not part of the schema, not pushed |
| `#[ogham(default)]` | state field | Documents that the matching `.ogh` field has a default; no behavior change on the Rust side |

Variant attributes for `OghamMsg`:

| Attribute | Applies to | Effect |
|---|---|---|
| `#[ogham(rename = "...")]` | enum variant | Use a different event name on the `.ogh` side |

### Macro tenets

- **The macros do not call into the runtime.** They emit code
  that uses the existing `IntoHostValue` / `HostStateSink` API.

  *Why:* keeps macro complexity local; no need to teach the
  proc-macro about runtime internals.

- **Schema generation is `const` where possible.** `RecordSchema`
  has a `BTreeMap` of fields, which is not const-constructible
  on stable Rust today, so the schema is built lazily on first
  access (`OnceLock`). The `const` ideal can be revisited if
  `BTreeMap::new()` ever becomes const.

- **Generated code is private by default.** The derives produce
  trait impls; they do not introduce new public items on the
  user's struct. Callers get the surface they explicitly opt
  into via `TypedOgham`.

---

## Rust side: `TypedOgham<S, M>`

The typed constructor and handle. Wraps `Ogham` and adds the
typed surface.

```rust
// src/lib.rs (new methods alongside Ogham)

impl Ogham {
    pub fn watch_typed<S, M>(
        path:    impl Into<String>,
        initial: S,
        config:  RuntimeConfig,
    ) -> Result<TypedOgham<S, M>, RuntimeError>
    where
        S: OghamState + 'static,
        M: OghamMsg + Send + 'static,
    {
        // 1. Load source, parse → ModuleSchema.
        // 2. Strict-mode required: the .ogh must declare host_state {}.
        //    If not: RuntimeError::SchemaMissing.
        // 3. Match S::OGHAM_SCHEMA against ModuleSchema.host_state.
        // 4. Match M::OGHAM_EVENTS against ModuleSchema.events.
        // 5. Build the channel; register one handler per event
        //    via M::register(&mut config, tx).
        // 6. Construct the inner Ogham via watch().
        // 7. Snapshot `initial` into the runtime's host_state.
        // 8. Return TypedOgham { inner, rx, last_state: initial, .. }.
    }

    pub fn from_source_typed<S, M>( /* analog */ ) -> Result<TypedOgham<S, M>, _>;
}

pub struct TypedOgham<S, M> {
    inner:      Ogham,
    rx:         std::sync::mpsc::Receiver<M>,
    last_state: S,
    _phantom:   std::marker::PhantomData<(S, M)>,
}

impl<S: OghamState, M: OghamMsg> TypedOgham<S, M> {
    /// Diff `new` against the last snapshot and inject only changed fields.
    pub fn set_state(&mut self, new: S);

    /// Drain at most one message from the queue.
    pub fn poll_msg(&mut self) -> Option<M>;

    /// Drain all queued messages.
    pub fn drain_msgs(&mut self) -> impl Iterator<Item = M> + '_;

    /// Escape hatches for code that still wants the loose surface
    /// (e.g. for layout/render calls that don't need typing).
    pub fn inner(&self)         -> &Ogham;
    pub fn inner_mut(&mut self) -> &mut Ogham;
}
```

### `TypedOgham` tenets

- **The typed handle owns the loose `Ogham`; it doesn't replace
  it.** All existing methods on `Ogham` remain reachable via
  `inner_mut()`. Layout, animation tick, hot-reload, custom
  widget registration — all unchanged.

  *Why:* keeps the typed path additive. Untold Lore can migrate
  one UI to `TypedOgham` while the other 16 keep using `Ogham`.

- **Events flow through an MPSC channel, not direct callbacks.**
  Per-variant handlers parse `&[Value]` into a typed `M` and
  push into the receiver. Consumers drain on their own
  schedule.

  *Why:* the existing event handler signature
  (`Fn(&[Value]) -> Result<Value, String> + Send + Sync +
  'static`) cannot capture `&mut Self` ergonomically and forces
  the `Arc<Mutex<...>>` shadow-state pattern. A channel-decoupled
  poll model lets consumers process messages in their normal
  update loop with full mutable access to game state.

- **`set_state` diffs internally, leveraging
  `inject_host_state_if_changed`.** No-op frames don't trigger
  rerenders.

  *Why:* matches the contract of the existing
  `set_host_state` / `inject_host_state_batch` paths described
  in [`RUNTIME.md`](RUNTIME.md). Untold Lore's per-frame
  injection becomes free when nothing changed.

- **Hot reload preserves `last_state`.** When the `.ogh` file
  reloads, the typed handle re-runs the schema match against the
  new module. If the schema is still compatible, `last_state` is
  re-pushed to seed the new module. If not, the reload fails
  loudly with the schema-mismatch error.

  *Why:* matches the spirit of `INTENT.md` §7 ("Hot reload
  preserves what it can, drops what it can't"). Schema-incompatible
  reloads should *not* silently coerce.

---

## Startup schema-match check

When `watch_typed::<S, M>` runs after parsing produces a
`ModuleSchema`:

1. **Module must be strict.** `ModuleSchema.host_state` must be
   `Some(_)`. Loose-mode modules can't use the typed API.
2. **Field-by-field equality** of `S::OGHAM_SCHEMA` against
   `ModuleSchema.host_state.unwrap()`:
   - Same set of field names.
   - Each field's `TypeRef` matches recursively.
3. **Record references resolve** to the same `RecordSchema` on
   both sides (recursive walk, comparing `BTreeMap` of fields).
4. **Event-by-event equality** of `M::OGHAM_EVENTS` against
   `ModuleSchema.events`:
   - Same set of event names.
   - Each event's argument list matches recursively.

Any mismatch fails the constructor with a `RuntimeError`
diagnostic of the same shape as the strict-mode parse errors:

```text
schema mismatch: settings.ogh declares
    `master_volume: float`
but Rust SettingsState declares
    `master_volume: i32`

schema mismatch: settings.ogh declares event
    `set_master_volume(float)`
but Rust SettingsMsg has no matching variant

schema mismatch: Rust SettingsMsg has variant
    `Rebind(string, string)`
but settings.ogh does not declare this event
```

The check runs once at construction; the cost is paid at app
startup (and on every hot reload), not per frame.

---

## LSP integration

The strict-mode work is necessary for safety; the LSP work is
what makes it *feel* worth declaring. Phase 1 LSP additions:

- **Hover on a host_state field reference**: shows the declared
  type, the optional default, and a "(declared in `host_state`)"
  origin marker.
- **Hover on a record field access** (`player.hp`): shows the
  field's type and the owning record name.
- **Hover on `event("name", ...)`**: shows the declared
  signature.
- **Completion on bare identifier** in expression position:
  includes host_state fields, locals, parameters, state, imports,
  built-ins.
- **Completion on `event("`**: completes from declared event
  names.
- **Completion on `.` after a record-typed value**: completes
  from that record's fields.
- **Diagnostics**: every strict-mode error from the parser
  surfaces in the LSP via the existing `collect_diagnostics`
  pipeline.
- **Go-to-definition** on a record name jumps to its
  `record Foo { ... }` declaration; on an imported record name,
  jumps to the declaration in the imported file.

### LSP tenets

- **All LSP queries answerable from `ModuleSchema`** — the LSP
  must not need to run the VM to give a hover or completion.
  Matches `INTENT.md` §1.

  *Why:* answering hovers from `Runtime` would mean either
  running user code in the LSP process (terrifying) or
  duplicating the VM (worse). The schema, like the AST, is
  pure-data and safe to interrogate.

- **The LSP loads imported modules' schemas, but not their
  bytecode.** A parse-only path that builds `ModuleSchema`
  without compiling is needed for cross-module record imports.

  *Why:* keeps the LSP fast. It already re-parses on every
  keystroke; compiling on every keystroke is heavier and
  unnecessary for these queries.

---

## Migration story

Per-file, in any order. The shape:

1. Pick a small `.ogh` file (suggested: `chest.ogh`, ~35 lines
   with no host_state today).
2. Add a `host_state { ... }` block declaring the fields the
   file actually reads. The LSP/parser flags missing
   declarations.
3. Add an `events { ... }` block declaring the events the file
   actually emits.
4. On the Rust side, replace the `Ogham::watch(path, config)`
   call with `Ogham::watch_typed::<MyUiState, MyUiMsg>(path,
   initial, config)`. Define the two types with derives.
5. Replace per-frame `rt.set_host_state(...)` calls with a
   single `typed.set_state(new_state)`.
6. Replace `with_event_handler(...)` chains with a `while let
   Some(msg) = typed.poll_msg() { match msg { ... } }` loop in
   the existing client update path.
7. Delete the `Arc<Mutex<FormState>>` shadow if there was one;
   the typed state struct is now the source of truth.

### Migration tenets

- **No file is forced to migrate.** Loose mode persists
  indefinitely.
- **Migration is safe to do incrementally** — half a Rust
  application can hold typed UIs and the other half loose ones.
  The Rust-level dispatch (Untold Lore's `active_ogham()`) sees
  `&Ogham` either way (via `TypedOgham::inner()`).
- **Tooling (a future LSP code action "extract host_state
  schema from current set_host_state call sites") is out of
  scope for Phase 1** but is the obvious follow-on if migration
  proves slow.

---

## Out of scope (explicitly deferred)

These are the things this design *does not* attempt, even if
they came up in the audit. Each gets its own phase or doc when
prioritized.

- **Body type-checking.** No arithmetic typing, no `let` typing,
  no function-signature typing. Strict mode checks the boundary
  and field access; the body remains as dynamic as today.
- **Type inference.** All types are written, not inferred.
- **Generics.** `array<T>` and `map<K, V>` are not user-extensible;
  `record Foo<T> { ... }` is not a thing.
- **Methods on records.** Records are pure data.
- **Sum types / `enum`.** A future revisit, paired with proper
  Optional ergonomics.
- **Optional sweetening.** No `?.`, no `unwrap`, no `??`. Only
  `match`.
- **Build-script-time schema check.** Runtime-startup check is
  the v1 plan; build-time is a follow-on if useful.
- **Custom widget property schemas.** Same machinery, scheduled
  as Phase 4.
- **Migration tooling** (LSP code action, source rewrite).
- **Removal of the loose API.** `set_host_state`,
  `with_event_handler`, `inject_host_state_batch` all stay.
- **Event-handler return values on the typed path.** Dropped
  entirely on `OghamMsg`'s registrar; nothing observable in
  Untold Lore today depends on them. Loose path keeps them.
- **Performance work on style re-parsing, signal-based
  reactivity, scenes/portals/lifecycle.** Independent tracks.

---

## Open implementation questions

These are the calls that still need a decision *during*
implementation. None block starting Phase 1.

1. **`map<int, V>` requires extending `Value::Map`.** Today's
   `Value::Map` is `HashMap<String, Value>`. Options: (a) widen
   to `HashMap<MapKey, Value>` where `MapKey ∈ {String, Int}`;
   (b) string-encode int keys at the boundary (`"42"`);
   (c) defer int-keyed maps to a follow-on. Recommend (c) for
   v1 to keep VM changes minimal — most real Untold Lore use
   cases are string-keyed (keybinds, named-asset lookups).
2. **Diagnostic infra: how rich can the existing `SyntaxError`
   plumbing be?** The error shapes shown above assume Rust-style
   spanned diagnostics with `note:` and `help:`. The current
   parser surfaces simpler messages. Phase 1 may need a small
   diagnostic-enrichment pass; if so, it should land first as a
   foundational change.
3. **Imported-record alias syntax.** `import [Item] from
   "./inv.ogh"` works for non-conflicting names; `import [Item
   as ShopItem]` for conflicts. Current import grammar may not
   support `as`; if not, Phase 1 needs to extend it.
4. **`OnceLock` / lazy schema initialization on the macro
   side.** `RecordSchema` uses `BTreeMap`, which is not
   const-constructible. The schema must be lazily initialized.
   Determine whether `LazyLock` (1.80+) or `once_cell` is the
   right dependency choice for the proc-macro output.
5. **Channel choice for `OghamMsg` events.** `std::sync::mpsc`
   is the obvious default; `crossbeam` is faster but adds a
   dependency. Recommend `std::sync::mpsc` unless profiling
   later disagrees.
6. **Where does the schema-only parse for cross-module imports
   live?** A new `Runtime::load_schema(path)` method? A
   standalone function in `runtime/schema.rs`? Recommend
   standalone function — schema loading does not need a
   runtime instance.
7. **What does the LSP show when a module is in loose mode?**
   Probably a status-bar hint: "loose mode — no schema
   declared." Not blocking; nice-to-have.

---

## Phase 1 deliverables

Concrete checklist for "Phase 1 ships":

### Front-end (`src/scanner/`, `src/parser/`)

- [ ] Scanner: `record`, `host_state`, `events`, `Self` keywords;
      `array`, `map` are contextual identifiers (parse as
      `TypeRef` only inside type positions).
- [ ] Parser: `RecordDecl`, `HostStateDecl`, `EventsDecl`
      productions; trailing-comma handling; one-block-per-module
      enforcement.
- [ ] Parser: `TypeRef` parser including postfix `?`, `array<>`,
      `map<,>`, `Self`.
- [ ] AST: new statement variants; the existing `Function`
      module wrapper grows an optional `ModuleSchema`.

### Schema (`src/runtime/schema.rs` — new module)

- [ ] `ModuleSchema`, `RecordSchema`, `FieldSchema`, `EventSig`,
      `TypeRef`, `PrimType`, `KeyType` types.
- [ ] Two-pass resolver: collect declarations, then resolve
      `RecordRef`s and `Self`s; detect direct self-reference and
      error.
- [ ] Standalone `load_schema(path) -> Result<ModuleSchema, _>`
      that parses without compiling, used by both the LSP and
      cross-module record imports.

### Strict-mode resolution (`src/runtime/compiler.rs`)

- [ ] When the module's `ModuleSchema.host_state` is `Some`,
      enable strict-mode identifier resolution.
- [ ] Field-access checking against record types known from the
      schema.
- [ ] Event-call checking against `ModuleSchema.events`.
- [ ] Diagnostics: identifier-not-found, unknown-event,
      arg-arity-mismatch, field-not-found-on-record, with
      Levenshtein-1 suggestions.

### Runtime (`src/runtime/host_state.rs`, `src/runtime/mod.rs`)

- [ ] No changes required for v1; the typed path uses the
      existing `IntoHostValue` / `HostStateSink` /
      `inject_host_state_if_changed` infrastructure.

### Macros (new crate: `ogham-derive` or similar)

- [ ] `OghamRecord` derive.
- [ ] `OghamState` derive (extends `OghamRecord` with
      `snapshot_into` / `diff_apply`).
- [ ] `OghamMsg` derive.
- [ ] `#[ogham(rename = ...)]`, `#[ogham(skip)]`,
      `#[ogham(default)]` field/variant attributes.

### Typed handle (`src/lib.rs`)

- [ ] `TypedOgham<S, M>` struct.
- [ ] `Ogham::watch_typed`, `Ogham::from_source_typed`.
- [ ] Startup schema-match check with diff-style error messages.
- [ ] Hot-reload path that re-runs the schema check.

### LSP (`src/lsp/`)

- [ ] Schema-aware hover for host_state, record fields, events.
- [ ] Schema-aware completion in the same positions.
- [ ] Strict-mode diagnostics surface via existing
      `collect_diagnostics`.
- [ ] Cross-module record go-to-definition.

### Tests

- [ ] Parser tests for each new declaration form, trailing
      commas, ordering, forward references, self-reference
      detection.
- [ ] Strict-mode resolution tests: valid resolution, missing
      identifier, wrong-type field access, event-call
      arg-arity, event-call computed-name rejection.
- [ ] Schema-match tests: matching pair succeeds; each shape of
      mismatch (missing field, wrong type, missing event, extra
      event, recursive record mismatch) produces the right
      error.
- [ ] Integration tests with a real `OghamState` /
      `OghamMsg` derive and a representative module.
- [ ] Hot-reload preserves typed-state when schema unchanged;
      fails loudly when schema-incompatible.

### Documentation

- [ ] Update `LANGUAGE.md` with the new declarations.
- [ ] Update `RUNTIME.md` with `TypedOgham` and the typed
      construction path.
- [ ] Update `LSP.md` with the new query capabilities.
- [ ] Update `AGENTS.md` (the user-facing integration guide)
      with a "Typed bindings" section showing the migration of
      one Untold Lore UI as the worked example.
- [ ] Promote *this* doc from "Design draft" to "Live contract"
      with the standard "Tenets + Drift indicators" shape.

---

## Worked example: Untold Lore's `chest.ogh` migration

The simplest UI in the codebase, used as the migration smoke
test. Today's shape:

- `chest.ogh` reads no host_state, fires two events:
  `chest_pick_up`, `chest_cancel`.
- `chest_ui.rs` registers two `with_event_handler` closures
  cloning an `ActionSender<ClientAction>`.

After migration:

```ogh
// chest.ogh — adds at top:
events {
  chest_pick_up(),
  chest_cancel(),
};
```

```rust
// chest_ui.rs — replaces ~50 lines with ~15

#[derive(OghamState, Default)]
struct ChestState {}                          // empty, but typed

#[derive(OghamMsg)]
enum ChestMsg {
    ChestPickUp,
    ChestCancel,
}

pub struct ChestUI {
    typed: TypedOgham<ChestState, ChestMsg>,
}

impl ChestUI {
    pub fn new(sender: ActionSender<ClientAction>) -> Result<Self, String> {
        let config = crate::util::font_config();
        let typed  = Ogham::watch_typed::<ChestState, ChestMsg>(
            CHEST_PATH, ChestState::default(), config,
        ).map_err(|e| format!("Failed to load chest.ogh: {e}"))?;
        Ok(Self { typed })
    }

    pub fn drain(&mut self, sender: &ActionSender<ClientAction>) {
        for msg in self.typed.drain_msgs() {
            match msg {
                ChestMsg::ChestPickUp => sender.send(ClientAction::ChestPickUp),
                ChestMsg::ChestCancel => sender.send(ClientAction::ChestCancel),
            }
        }
    }

    pub fn ogham(&self)         -> &Ogham    { self.typed.inner() }
    pub fn ogham_mut(&mut self) -> &mut Ogham { self.typed.inner_mut() }
}
```

The `ChestState` being empty is intentional — it forces the
macro / typed-handle path to handle the trivially-empty case
cleanly. If that works, the larger UIs (SettingsUI's ~30 fields,
DmInventoryUI's nested records) follow the same pattern with
more fields and one or two `#[derive(OghamRecord)]` types.

---

## Estimated phasing within Phase 1

Suggested sequencing for implementation; each item ships
independently and earns value before the next lands.

1. **Front-end + schema** (parser, scanner, `ModuleSchema`,
   resolver). No Rust API changes; `.ogh` files can declare
   schemas and the LSP can hover them via diagnostics-only
   integration. **First merge.**
2. **Strict-mode resolution + diagnostics** (compiler changes;
   identifier-not-found, event-call checks, field-access checks).
   `.ogh` authors see real errors. **Second merge.**
3. **LSP completion + hover** (schema-aware). Authoring
   experience lands. **Third merge.**
4. **Derive macros** (`OghamRecord`, `OghamState`,
   `OghamMsg`). Rust authors can declare typed counterparts.
   **Fourth merge.**
5. **`TypedOgham` + `watch_typed` + schema-match check**. The
   typed runtime path opens. **Fifth merge.**
6. **Hot-reload schema preservation**. Migrates the chest_ui
   smoke test end-to-end. **Sixth merge.**

Each merge is independently shippable and incrementally
validated. The longest-pole item is (4) — proc-macro work tends
to surface scope creep around field attributes.

---

## What this design does *not* solve, restated

Worth carrying over from the audit so the doc is honest with
itself:

- Untold Lore's 17 runtimes do not become 1; each gets ~70%
  smaller. The merge-runtimes story is Shift B (Scenes).
- No lifecycle hooks: resources still live in Rust.
- No portals/modals/tooltips.
- No animation completion callbacks.
- No fine-grained reactivity.
- No richer Optional ergonomics until the broader sum-type
  pass.

If any of those becomes a blocker for Phase 1's value (e.g.
typed bindings *require* Scenes to be useful), the design needs
to be revisited. None look like blockers from the audit
evidence.

---

## Migration cookbook

Three patterns surfaced by the Untold Lore audit
([`TYPED_BINDINGS_UL_AUDIT.md`](TYPED_BINDINGS_UL_AUDIT.md))
need explicit handling during the per-UI migration. None
require Ogham changes; they're rewrites in `.ogh` and Rust
calling code.

### F1 — Computed event names (strict-mode blocker)

**Pattern in production code** (place_inspector.ogh):

```ogh
let pill_btn = fn (label: string, is_active: bool, evt: string, key: string) {
  Flex {
    mouse_down: fn () { event(evt, key); },  // ← computed name
    ...
  }
};
```

Strict mode requires the first arg to `event(...)` to be a
string literal so the schema can validate the call statically.
Three migration options:

1. **Inline expansion (recommended for ≤ 4 call sites)** —
   replace each `pill_btn(...)` with an explicit
   `Flex { mouse_down: fn () { event("set_arrival_mode", key); }, ... }`
   block. Adds line count but keeps the closure pattern.
2. **Per-event helpers** — define
   `arrival_pill_btn(label, is_active, key)` and similar
   per-group helpers, each hardcoding its event name.
   Preserves abstraction; small line cost.
3. **Tagged dispatch event** — `set_pill(string, string)`
   taking `(group, key)` with the game side switching on
   group. Trades event count for type-safety value; only
   worth it for large groups.

**Recommendation:** option 2. Keeps closure ergonomics, costs
~10 lines, no Ogham changes.

### F5 — Numeric-as-string draft fields

**Pattern across ~5 form-heavy UIs** (settings_ui, sea_config_ui,
ruleset_editor_ui, life_stages_editor_ui, map_editor_ui):

```rust
struct SettingsState {
    master_volume: String,    // not f32 — preserves "0." mid-edit
    base_xp: String,
    fov: String,
}
```

These fields hold raw user input strings during editing
(`"0."`, `"3.14e"`, etc.) so partial input doesn't snap or
parse to NaN. The Phase 1 idiom: declare them as `string` in
both the schema and the Rust struct. Pair with a sibling
validation flag when the field has constraints:

```ogh
host_state {
    base_xp: string,
    base_xp_invalid: bool,
};
```

```rust
#[derive(OghamState, ...)]
struct State {
    base_xp: String,
    base_xp_invalid: bool,
}
```

**Recommendation:** keep the draft-as-string convention for
form fields. A future `draft<T>` type with built-in validation
flags is on the roadmap but doesn't block Phase 1.

### F13 — Optional fields stored as empty-string sentinels

**Pattern across ~10 UIs** (place_inspector, character_select,
ruleset_editor, dm_hud, etc.):

```rust
// today
selected_character_id: String,    // "" means "no selection"
selected_entity: HashMap<...>,    // empty map means "no entity"
```

```ogh
// today's idiom
match (selected_character_id != "") {
    true => render_selected(),
    false => render_empty_state(),
}
```

These can migrate opportunistically to `T?`:

```rust
selected_character_id: Option<String>,
selected_entity: Option<EntityInspector>,
```

```ogh
host_state {
    selected_character_id: string?,
    selected_entity: EntityInspector?,
};

// .ogh becomes
match selected_character_id {
    Some(id) => render_selected(),
    None     => render_empty_state(),
}
```

**Recommendation:** opportunistic — migrate when touching the
UI for other reasons. Don't block the typed-bindings migration
on Optional adoption; the empty-string sentinel still works in
strict mode.

### How the cookbook applies to the migration sequence

The audit's 21-step migration sequence (in
[`TYPED_BINDINGS_UL_AUDIT.md`](TYPED_BINDINGS_UL_AUDIT.md))
flags F1 against `place_inspector_ui` (slot 11). F5 applies
to settings/sea_config/ruleset/life_stages/map_editor (slots
13, 14, 17, 18, 21). F13 applies to roughly half the UIs.
None of the F-patterns block any other migration; each can be
fixed in isolation when its UI is migrated.
