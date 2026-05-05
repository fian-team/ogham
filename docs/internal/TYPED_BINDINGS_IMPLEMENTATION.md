# Typed Bindings — Phase 1 Implementation Plan

> **Status: Working plan (in-progress).**
>
> Concrete, merge-by-merge implementation plan for Phase 1 of the
> typed-bindings work. Builds on
> [`TYPED_BINDINGS.md`](TYPED_BINDINGS.md) (the design contract)
> and [`TYPED_BINDINGS_UL_AUDIT.md`](TYPED_BINDINGS_UL_AUDIT.md)
> (the validation against Untold Lore). This doc adds the file-
> level "what to change, in what order, with what validation
> gate." It assumes the design and the open-question answers in
> those docs are settled.
>
> **Scope:** the seven merges (M0 through M6) that ship Phase 1
> end-to-end, plus cross-cutting concerns (workspace
> restructure, test corpus, branch strategy, risk register).
> Does not cover Phase 2+ work (custom widget property schemas,
> body type-checking, sum types, `draft<T>`, signals, scenes).

---

## At a glance

```
M0  Foundational: extend SyntaxError with note/help; widen LSP diagnostic shape
    └── unblocks every later merge; ~1 week; low risk

M1  Front-end: scanner + parser for record / host_state / events / type-refs
    └── ModuleSchema data structure; two-pass resolver; self-reference check
    └── unblocks M2 and M3 in parallel; ~2-3 weeks; medium risk

M2  Strict-mode resolution + diagnostics in the compiler
    └── identifier resolution rules; field access; event-call checking; suggestions
    └── unblocks meaningful authoring; ~2-3 weeks; medium risk

M3  LSP integration: schema-aware hover, completion, go-to-definition
    └── makes the schema declarations feel worth writing
    └── parallelizable with M2 once M1 lands; ~2 weeks; low risk

M4  Cargo workspace restructure + ogham-derive crate
    └── OghamRecord, OghamState, OghamMsg derives; field/variant attributes
    └── longest pole; ~3-4 weeks; medium-high risk

M5  TypedOgham<S, M> handle + watch_typed + startup schema-match check
    └── runtime-side wiring; MPSC event channel; first end-to-end path
    └── depends on M1+M2+M4; ~1-2 weeks; low risk

M6  Hot-reload schema preservation + chest_ui migration smoke test
    └── exercises the full system against real production code
    └── ~1 week; low risk; ships Phase 1
```

Total: 12–17 weeks of focused work for a single contributor.
Parallelizable to ~9–12 weeks across two contributors split at
the M3/M4 boundary.

---

## Merge 0 — Foundational diagnostic infrastructure

**Goal:** make `SyntaxError` carry the structured fields that
`TYPED_BINDINGS.md`'s rich error shapes assume, and surface them
through the LSP.

**Context:** today's `SyntaxError` (`src/parser/syntax_error.rs`)
is `{ line, column, message }`. The LSP's `collect_diagnostics`
(`src/lsp/server.rs:223`) renders it as a one-character-range
`Diagnostic` with a single message. The strict-mode errors I
sketched in TYPED_BINDINGS.md — with `note:` and `help:` and
levenshtein suggestions — need richer plumbing.

`tower_lsp::Diagnostic` already has the fields we need
(`related_information: Option<Vec<DiagnosticRelatedInformation>>`,
plus `code` and `code_description`). The work is just on our
side.

### Files touched

- `src/parser/syntax_error.rs` — extend struct.
- `src/parser/mod.rs` — update construction sites to use the new
  fields (most stay default; a handful of high-value existing
  errors gain `note:` / `help:`).
- `src/lsp/server.rs` — `collect_diagnostics` renders the new
  fields into `Diagnostic`.

### New shape

```rust
// src/parser/syntax_error.rs
#[derive(Clone, Debug, PartialEq)]
pub struct SyntaxError {
    pub line: usize,
    pub column: usize,
    pub length: usize,                 // 0 means "unknown / one char"
    pub message: String,
    pub note: Option<String>,
    pub help: Option<String>,
}

impl SyntaxError {
    pub fn new(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self { line, column, length: 0, message: message.into(), note: None, help: None }
    }
    pub fn with_length(mut self, length: usize) -> Self { self.length = length; self }
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into()); self
    }
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into()); self
    }
}
```

### LSP rendering (`collect_diagnostics`)

```rust
let length = if err.length > 0 { err.length as u32 } else { 1 };
let mut diag = Diagnostic {
    range: Range::new(Position::new(line, col), Position::new(line, col + length)),
    severity: Some(DiagnosticSeverity::ERROR),
    source: Some("ogham".to_string()),
    message: err.message.clone(),
    ..Default::default()
};
if let (Some(n), Some(h)) = (&err.note, &err.help) {
    diag.message = format!("{}\n\nnote: {}\nhelp: {}", err.message, n, h);
} else if let Some(n) = &err.note {
    diag.message = format!("{}\n\nnote: {}", err.message, n);
} else if let Some(h) = &err.help {
    diag.message = format!("{}\n\nhelp: {}", err.message, h);
}
diagnostics.push(diag);
```

(Stuffing `note:` / `help:` into the message field is the simplest
v1 — `related_information` is fancier but requires a `Location`
pointing at related source, which strict-mode errors don't always
have. Revisit if needed.)

### Tests

- Unit tests on `SyntaxError` construction (builder pattern works).
- One LSP integration test asserting the message format for an
  error with `note:` and `help:`.

### Validation gate

- `cargo build --workspace` clean.
- `cargo test --workspace` passes (no existing parser tests break).
- Manual LSP smoke test: open an `.ogh` with a known parse error
  in VSCode, confirm the existing diagnostic still renders.

### Estimated complexity: low
~1 week. Mostly mechanical. The risk is forgetting to update one
of the ~30 existing `SyntaxError` construction sites in
`src/parser/mod.rs` — a `cargo check` will catch that
immediately because of the new required `length` field, but I'd
prefer to keep `length: 0` as default-via-constructor and have
`with_length` be additive, which keeps existing call sites
unchanged.

---

## Merge 1 — Front-end: scanner, parser, ModuleSchema

**Goal:** parse `record`, `host_state`, `events`, `array<>`,
`map<>`, `Self`, and `T?` syntax; emit `ModuleSchema` data
structure; resolve cross-references with a two-pass walk; reject
direct self-reference.

### Sub-merges (can land separately)

**M1a — Scanner additions** (~2 days)

- `src/scanner/token_type.rs`: add keyword tokens
  `Record`, `HostState`, `Events`, `SelfTy`. (`Self` clashes with
  Rust naming; use `SelfTy` internally, recognize `"Self"` in
  source.) Note that `array` and `map` are NOT keywords — they
  remain `Identifier` tokens and are recognized contextually only
  in `TypeRef` positions. This avoids breaking any user code that
  currently uses `array` or `map` as variable names.
- `src/scanner/mod.rs`: add the keyword recognizers.
- Tests: `tests/scanner_typed_bindings.rs` covering each new
  keyword + the `host_state` two-word sequence (which is
  scanned as a single `HostState` keyword by checking the
  `host_state` literal — single token).

  **Decision needed:** is `host_state` one token or two? Option A
  (one token, scanned as a special multi-char keyword) is uglier
  but symmetric with how `==` and `=>` are scanned. Option B (two
  tokens: `host` identifier + underscore + `state` keyword)
  collides with `state`. **Recommend a single `HostState` token
  matching the literal `host_state`.** Same for `events` (just
  one word, no decision).

**M1b — Type-ref grammar + AST** (~3 days)

- New AST node in `src/parser/`:
  ```rust
  // src/parser/typed_bindings.rs (new file)
  pub enum TypeRef {
      Primitive(PrimType),
      Record(String),                  // unresolved at parse time
      Array(Box<TypeRef>),
      Map(KeyType, Box<TypeRef>),
      Optional(Box<TypeRef>),
      SelfRef,                         // legal only inside RecordDecl
  }
  pub enum PrimType { Int, Float, Bool, String }
  pub enum KeyType { String, Int }
  ```
- New parser entry: `parse_type_ref()` — recursive-descent over
  `TypeRef` grammar with postfix `?`, generic `<T>`, etc.
- Tests: every type form, including nested
  `array<map<string, array<Foo?>>>`, including illegal forms
  (e.g. `map<float, T>` rejected with a useful error).

**M1c — Top-level declarations** (~3 days)

- `RecordDecl`, `HostStateDecl`, `EventsDecl` AST nodes.
- Add to `Statement` enum (treating them as top-level statements
  is the path of least resistance; rejection of non-top-level
  use happens in `parse_statement` via a flag, mirroring the
  existing `allow_import` pattern).
- `parse_record_decl`, `parse_host_state_decl`, `parse_events_decl`
  in `src/parser/mod.rs`.
- Trailing-comma support; one-block-per-module enforcement
  (parser tracks "have I seen `host_state` already?" and errors
  on the second).
- Tests: positive cases for each declaration; negative cases for
  duplicate blocks, missing `;`, missing fields, non-top-level
  use.

**M1d — `ModuleSchema` data structure + two-pass resolver** (~4 days)

- New module `src/runtime/schema.rs`:
  ```rust
  pub struct ModuleSchema {
      pub records:    HashMap<String, RecordSchema>,
      pub host_state: Option<RecordSchema>,
      pub events:     HashMap<String, EventSig>,
      pub imports:    HashMap<String, ImportedTy>,
  }
  pub struct RecordSchema { pub fields: BTreeMap<String, FieldSchema> }
  pub struct FieldSchema  { pub ty: TypeRef, pub default: Option<Literal> }
  pub struct EventSig     { pub args: Vec<TypeRef> }
  pub enum ImportedTy     { Record(RecordSchema) }
  ```
- Resolver: collect all `RecordDecl`s; walk every `RecordRef` in
  every field type; verify the name resolves; verify direct
  self-reference is absent.
- Self-reference check: walk a record's field types; if any path
  reaches `Self` or a `RecordRef` to the enclosing record without
  crossing `Array`, `Map`, or `Optional`, error.
- `Function` (the existing module wrapper) gains `Option<ModuleSchema>`.
- Tests: cross-record references resolve; forward references
  resolve; self-reference via `array<Self>` and `Self?` work;
  direct self-reference (e.g. `record N { next: N }`) is rejected.

**M1e — Standalone schema loader** (~2 days)

- `src/runtime/schema.rs`: `pub fn load_schema(path: &Path) -> Result<ModuleSchema, _>`.
- Used by both the LSP (schema-only parse for cross-module record
  imports) and the `Ogham::watch_typed` startup path. Does not
  compile bytecode.
- Tests: load a file with imports; verify imported records
  appear in the resulting schema.

### Files touched

| File | Change |
|---|---|
| `src/scanner/token_type.rs` | Add 4 keyword variants |
| `src/scanner/mod.rs` | Recognize new keywords |
| `src/parser/typed_bindings.rs` (new) | TypeRef, PrimType, KeyType, RecordDecl, HostStateDecl, EventsDecl |
| `src/parser/statement.rs` | Add 3 Statement variants |
| `src/parser/mod.rs` | parse_record_decl / parse_host_state_decl / parse_events_decl / parse_type_ref + duplicate-block detection |
| `src/runtime/schema.rs` (new) | ModuleSchema, RecordSchema, FieldSchema, EventSig, resolver, load_schema |
| `src/runtime/mod.rs` | `Function` carries `Option<ModuleSchema>` |
| `tests/scanner_typed_bindings.rs` (new) | Scanner test corpus |
| `tests/parser_typed_bindings.rs` (new) | Parser test corpus + audit-derived schema fixtures |
| `tests/schema_resolver.rs` (new) | Resolver test corpus |

### Validation gate

- All existing tests pass unchanged (no regression in the loose
  path).
- New tests pass.
- Pluck the inferred schemas from `TYPED_BINDINGS_UL_AUDIT.md`
  for migration-sequence items 1–8 (chest, tip_log, crafting,
  console, blueprint_editor, talents, dm_inventory, dm_hud) and
  use them as `tests/fixtures/typed_bindings/*.ogh` parser
  fixtures. Each must parse and resolve without error.
- The hot-reload-schema fixture (one `.ogh` importing a record
  from another) must load via `load_schema()` end-to-end.

### Estimated complexity: medium
2–3 weeks for a single contributor. The two-pass resolver and
the self-reference check are the only non-mechanical pieces; the
rest is straightforward grammar work.

### Risks

- **Sub-token ambiguity for `Self`**: today `Self` would scan as
  `Identifier("Self")`. We need to either keyword-promote it
  (cleaner) or contextually recognize it inside type positions
  (less invasive but more complex). **Recommend keyword
  promotion**, matching how `Record`/`HostState`/`Events` work.
- **Duplicate-block detection**: needs to span the whole module.
  Easiest implementation: track in the parser's mutable state.

---

## Merge 2 — Strict-mode resolution + diagnostics

**Goal:** when a module has `host_state {}`, the compiler enforces
strict identifier resolution and event-call signatures. Errors
surface via the M0-extended `SyntaxError` shape.

### Sub-merges

**M2a — Strict-mode flag plumbing** (~1 day)

- `Compiler::compile_module` checks `module.schema.host_state.is_some()`
  to enable strict mode for that compilation.
- A `CompilerContext` carries the active schema down through
  identifier resolution.

**M2b — Strict identifier resolution** (~5 days)

- Modify the compiler's identifier-resolution path (currently
  emits `GetGlobal` opcodes for unknown identifiers) to:
  - In loose mode: emit `GetGlobal` as today (resolves at runtime).
  - In strict mode: check the schema's `host_state.fields`; check
    declared `record` names (in type-position contexts); check
    locals/parameters/state/imports/built-ins; if none match,
    emit a `SyntaxError` with note+help+suggestion.
- Built-in identifiers (`event`, `mutation`, `log`, `rgb`, `rgba`)
  are listed explicitly in a `BUILTINS: &[&str]` constant.
- Suggestion: levenshtein-1 over the union of all in-scope
  identifiers.

**M2c — Field-access checking** (~3 days)

- When the compiler emits a field access (`a.b`), and the
  receiver's type is known (because `a` is a host_state field),
  check that `b` is a member of the receiver's record.
- Receiver type inference is *only* the trivial case: bare
  identifier resolves to a host_state field, that field's type
  is a record, the immediate `.field` access is checked. Chained
  access (`a.b.c`) works recursively as long as each step resolves.
- Container types (`array<>`, `map<>`) reject `.` access entirely.
- Optional types (`T?`) reject `.` access — must `match` first.
- Primitives reject `.` access.

**M2d — Event-call checking** (~3 days)

- The compiler recognizes `event(...)` as a built-in.
- In strict mode, the first arg must be a string literal. If
  not, error: "computed event names not allowed in strict mode."
- Look up the event name in `module.schema.events`. If absent:
  error with suggestion.
- Check arg count. If wrong: error with the declared signature.
- Check arg types where statically knowable (the easy cases:
  literals, bare host_state field references). Skip type checks
  for arbitrary expressions.

**M2e — Diagnostic polish + suggestion infra** (~2 days)

- `src/parser/typed_bindings.rs` (or sibling): `levenshtein_1(query, candidates)`.
- Standard error message templates with consistent shape.
- Tests asserting the error messages match expected strings
  (these double as documentation of what authors will see).

### Files touched

| File | Change |
|---|---|
| `src/runtime/compiler.rs` | Strict-mode flag; identifier resolution branch; field-access check; event-call check |
| `src/runtime/schema.rs` | Type lookup helpers (`field_type(record, field)`, `event_sig(name)`) |
| `src/parser/typed_bindings.rs` | `levenshtein_1`, builtin list, error templates |
| `tests/strict_mode_resolution.rs` (new) | Resolution tests with audit fixtures |
| `tests/strict_mode_diagnostics.rs` (new) | Error-message snapshot tests |

### Validation gate

- All loose-mode tests still pass (regression safety).
- Strict-mode positive tests: every audit-derived schema fixture
  compiles without error.
- Strict-mode negative tests for each error class:
  - Unknown identifier (with suggestion)
  - Field not on record (with suggestion)
  - Computed event name
  - Unknown event (with suggestion)
  - Wrong arg count
  - Wrong arg type (where statically inferable)
  - Field access on primitive / array / Optional
- Dogfood: take 2–3 audit `.ogh` schemas, deliberately introduce
  typos in the body, confirm the diagnostics are useful.

### Estimated complexity: medium
2–3 weeks. The compiler changes touch a load-bearing path
(identifier resolution) which has subtleties around state
declarations, captured upvalues, and the built-in list. Test
coverage matters here.

### Risks

- **Identifier-resolution interactions with closures**: today's
  closure-capture code may resolve identifiers at capture time
  rather than use time. Need to verify strict-mode works with
  closures that capture host_state.
- **Built-in list maintenance drift**: hard-coding the built-in
  list in two places (the compiler's resolver, the LSP's
  completion) is a smell. Recommend a single source of truth in
  `src/runtime/builtins.rs`.

---

## Merge 3 — LSP integration

**Goal:** schema declarations feel worth writing because the LSP
surfaces them in hover, completion, and go-to-definition.

Parallelizable with M2 once M1 lands — the schema data structure
is the only dependency.

### Sub-merges

**M3a — Hover** (~3 days)

- `src/lsp/hover.rs`: add schema-aware paths.
  - Hover on a host_state field reference: show declared type
    + default + "(declared in `host_state`)".
  - Hover on a record name: show the record's fields.
  - Hover on a record-typed field access (`player.hp`): show
    field type and owning record.
  - Hover on `event("name", ...)`: show the declared signature.
- Reuses the existing `hover::hover_at(ast, line, col)` entry
  point; adds new branches.

**M3b — Completion** (~4 days)

- `src/lsp/completion.rs` (new) — completion is currently
  unimplemented per AGENTS.md.
- Add `completion_provider: Some(...)` to the LSP's
  `ServerCapabilities` initialization.
- Cases:
  - Bare identifier in expression position: suggest host_state
    fields, locals, params, state, imports, built-ins.
  - After `event("`: suggest declared event names.
  - After `.` on a record-typed value: suggest fields.
  - In a `TypeRef` position inside `host_state {}` / `events {}` /
    `record { ... }`: suggest primitives, declared records,
    `array<>`, `map<>`.
- Built-in list comes from the M2-introduced `builtins.rs`.

**M3c — Go-to-definition** (~2 days)

- `src/lsp/goto_definition.rs`: add cases for
  - Record name → declaration site
  - Cross-module imported record → declaration in imported file
    (uses `load_schema` from M1 to find the source)

**M3d — Diagnostics** (~1 day)

- Already free: M0+M2 strict-mode errors flow through
  `collect_diagnostics` automatically.
- Add capability advertisement and confirm in VSCode.

**M3e — Loose-mode hint** (~1 day, optional polish)

- A status-bar message or workspace-edit notification when a
  module is in loose mode: "loose mode — declare `host_state {}`
  for type-checked bindings." Skippable in v1; nice for adoption.

### Files touched

| File | Change |
|---|---|
| `src/lsp/server.rs` | `completion_provider` capability |
| `src/lsp/hover.rs` | Schema-aware branches |
| `src/lsp/completion.rs` (new) | New module |
| `src/lsp/goto_definition.rs` | Record / cross-module record resolution |
| `editors/vscode/` | Confirm completion capability registration; bump version |
| `tests/lsp_typed_bindings.rs` (new) | LSP integration tests |

### Validation gate

- Manual: open a typed `.ogh` in VSCode, confirm hover on a
  host_state field shows the type, completion suggests fields,
  go-to-definition jumps to the record.
- Automated tests where feasible.

### Estimated complexity: low-medium
2 weeks. LSP work is mostly additive; the test infrastructure
already exists. Risk is minimal because regressions surface
immediately during manual testing.

### Risks

- **Completion never existed before**: capability negotiation
  must be added; any pre-existing client config (`editors/vscode`)
  may need a version bump.
- **`load_schema` cross-module performance**: re-parsing imported
  files on every LSP query could be slow if a file imports many
  modules. Phase 1 measure: cache parsed schemas in the
  `DocumentStore` keyed by path + mtime.

---

## Merge 4 — Cargo workspace + ogham-derive crate

**Goal:** new crate at `crates/ogham-derive/` providing
`OghamRecord`, `OghamState`, `OghamMsg`. Restructure the repo as
a Cargo workspace so the derive crate can depend on a shared
schema-types crate without circular dependencies.

This is the longest single merge and the highest macro-
specific risk. Worth its own dedicated implementation push.

### Sub-merges

**M4a — Workspace restructure** (~2 days)

- Move existing crate to `crates/ogham/`.
- Top-level `Cargo.toml` becomes a workspace manifest with
  `members = ["crates/ogham", "crates/ogham-derive", "crates/ogham-types"]`.
- New crate `crates/ogham-types/` carries `TypeRef`, `PrimType`,
  `KeyType`, `RecordSchema`, `EventSig`, `ModuleSchema`. Both
  `ogham` (lib) and `ogham-derive` depend on it.
  - **Why a separate types crate:** the derive macro needs to
    emit code referencing `RecordSchema` etc., but cannot depend
    on `ogham` itself (which depends on `skia-safe` and a stack
    of GUI dependencies that proc-macros don't need or want).
- Update all internal `use` paths.
- Validation: `cargo build --workspace`, `cargo test --workspace`
  pass.

**M4b — `OghamRecord` derive** (~5 days)

- `crates/ogham-derive/src/lib.rs`: `#[proc_macro_derive(OghamRecord, attributes(ogham))]`.
- Generates:
  - `impl OghamRecord for Foo` with associated `OGHAM_SCHEMA`
    (lazy via `LazyLock` since `BTreeMap::new()` is not const).
  - `impl IntoHostValue for &Foo` returning `Value::Map`.
- Field attributes: `#[ogham(rename = "...")]`, `#[ogham(skip)]`.
- Tests: `crates/ogham-derive/tests/`. Compile-time and runtime
  assertions. Use `trybuild` for "this should fail to compile"
  cases (wrong field type for the derive, etc.).

**M4c — `OghamState` derive** (~3 days)

- Extends `OghamRecord` with:
  - `fn snapshot_into(&self, sink: &mut impl HostStateSink)`
  - `fn diff_apply(&self, prev: &Self, sink: &mut impl HostStateSink)`
- Both walk the struct fields and call `sink.set(name, &self.field)`.
- `diff_apply` compares each field with `PartialEq` and skips
  unchanged ones — matches `inject_host_state_if_changed` semantics.
- `#[ogham(default)]` is purely cosmetic on this side (the schema
  carries it; the macro does not consume it for codegen).

**M4d — `OghamMsg` derive** (~5 days)

- `#[proc_macro_derive(OghamMsg, attributes(ogham))]` on enums.
- Generates:
  - `const OGHAM_EVENTS: &[(&'static str, EventSig)]` — declared
    events. (Or a `LazyLock` HashMap if perf demands.)
  - `fn try_from_event(name: &str, args: &[Value]) -> Option<Self>`
    — switch over variant names; for each, parse `args` into the
    variant's payload using `TryFromValue`.
  - `fn register(config: &mut RuntimeConfig, tx: Sender<Self>)`
    — for each declared variant, `config.with_event_handler(name, ...)`
    where the closure parses args and pushes to `tx`.
- Variant attributes: `#[ogham(rename = "...")]`.
- Tests: `trybuild` for compile-fail cases (variant with
  unsupported field type, missing rename matching schema, etc.).

**M4e — Reverse direction: `TryFromValue` trait** (~2 days)

- Used by `OghamMsg::try_from_event` to parse args.
- Lives in `ogham-types` (to avoid a circular dep with the derive
  crate).
- Impls for primitives (i32, f32, f64, bool, String) and records
  (auto via `#[derive(OghamRecord)]`'s reverse path).
- Container impls: `Vec<T> where T: TryFromValue`,
  `HashMap<String, V>`, `Option<T>`.

**M4f — Macro error messages** (~3 days, polish)

- Proc-macro errors are infamously unfriendly without effort.
  Use `syn`'s span machinery to point errors at the right field
  / variant.
- Common cases: "field `foo: Bar` — `Bar` does not implement
  `OghamRecord`" should highlight `Bar`.
- This is iteratively polishable; ship with adequate errors and
  improve over time.

### Files touched

| File | Change |
|---|---|
| `Cargo.toml` (root) | Workspace manifest |
| `crates/ogham/Cargo.toml` | Existing crate, moved |
| `crates/ogham-types/` (new crate) | TypeRef, schemas, traits |
| `crates/ogham-derive/` (new crate) | Three derives |
| `crates/ogham/src/runtime/schema.rs` | Re-exports from `ogham-types` |
| `crates/ogham/src/runtime/host_state.rs` | `IntoHostValue` impls move to or coexist with the new traits |

### Validation gate

- Workspace builds cleanly.
- All existing tests pass.
- Derive tests pass.
- `trybuild` cases produce expected error messages.
- A standalone integration test:
  ```rust
  #[derive(OghamRecord)]
  struct Item { name: String, count: i32 }

  #[derive(OghamState)]
  struct State { items: Vec<Item>, count: i32 }

  #[derive(OghamMsg)]
  enum Msg { Add(String), Remove(i32) }

  // Assert OGHAM_SCHEMA matches expected RecordSchema.
  // Assert OghamMsg::try_from_event correctly parses arg lists.
  ```

### Estimated complexity: medium-high
3–4 weeks. Proc-macro work has high boilerplate cost and a long
tail of edge cases (nested records, optional fields, generic
collection types). The workspace restructure is mechanical but
touches every internal `use`.

### Risks

- **`syn`/`quote`/`proc-macro2` version churn**: pin versions
  early; these crates evolve and breaking changes are common.
- **Lazy-init pattern for `RecordSchema`**: `LazyLock` requires
  Rust 1.80+. Confirm minimum Rust version (current: edition
  2021, no MSRV declared). Add an explicit MSRV if needed.
- **Dependency direction**: `ogham-derive` *cannot* depend on
  `ogham` (proc-macros can't pull in skia/winit/etc. — both for
  build-time cost and because some of those deps don't compile
  in proc-macro contexts). The `ogham-types` extraction is
  load-bearing for this reason.
- **Cyclic crate-name confusion**: `ogham-types` could collide
  in name with future crates. Consider `ogham-schema` or
  `ogham-bindings` as alternatives. **Recommend `ogham-schema`**
  — clearer purpose.

---

## Merge 5 — TypedOgham handle + watch_typed + schema match

**Goal:** the typed-runtime path opens. `Ogham::watch_typed::<S, M>`
constructs a `TypedOgham<S, M>`; `set_state` and `poll_msg` work.

This is the smallest merge by line count but the highest-value
gate: it's the first time end-to-end typed bindings work in a
real program.

### Sub-merges

**M5a — `TypedOgham` struct** (~2 days)

- New module `crates/ogham/src/typed.rs`.
- Struct, `set_state`, `poll_msg`, `drain_msgs`, `inner`,
  `inner_mut` per TYPED_BINDINGS.md §"`TypedOgham<S, M>`".
- MPSC channel: `std::sync::mpsc::channel()` (open question O5
  in TYPED_BINDINGS.md — go with `std`; profile later if needed).

**M5b — `Ogham::watch_typed` and `Ogham::from_source_typed`** (~3 days)

- Lives on `Ogham` in `crates/ogham/src/lib.rs`.
- Loads source, parses, builds `ModuleSchema` (M1's path).
- Registers handlers via `M::register(&mut config, tx)` (M4's path).
- Calls `Ogham::watch` / `Ogham::from_source` internally with the
  enriched config.
- Pushes initial state via `S::snapshot_into(...)` (M4's path).
- Wraps the result in `TypedOgham`.

**M5c — Schema-match check** (~2 days)

- New function in `crates/ogham/src/typed.rs`:
  ```rust
  fn assert_schemas_match(
      module: &ModuleSchema,
      state_schema: &RecordSchema,
      events_schema: &[(&str, EventSig)],
  ) -> Result<(), RuntimeError>;
  ```
- Runs at `watch_typed` startup. Diff-style error messages on
  mismatch:
  - Field present in module but missing in state struct
  - Field type mismatch
  - Event present in module but missing in enum
  - Event arg-list mismatch
  - Both sides recursive (record-of-record-of-record).
- Tests: positive + each mismatch class.

**M5d — `RuntimeError::SchemaMissing`, `RuntimeError::SchemaMismatch`** (~1 day)

- Extend `RuntimeError` enum with new variants.
- Format implementations produce the diff-style output.

### Files touched

| File | Change |
|---|---|
| `crates/ogham/src/typed.rs` (new) | `TypedOgham<S, M>`, `assert_schemas_match` |
| `crates/ogham/src/lib.rs` | `watch_typed`, `from_source_typed` constructors |
| `crates/ogham/src/runtime/error.rs` | New error variants |
| `tests/typed_ogham_basic.rs` (new) | End-to-end test using a tiny `.ogh` and a derived state/msg pair |

### Validation gate

- All schema-match positive and negative tests pass.
- A minimal end-to-end test: parse a schema, derive matching
  Rust types, construct `TypedOgham`, push state, drain a
  message. No widget rendering required for this test.
- The chest_ui smoke test is still M6's job.

### Estimated complexity: low
1–2 weeks. The work is mostly wiring; the hard parts (schema
data structure, derives, parser) are done by now.

### Risks

- **Mismatch error message quality**: easy to produce
  unreadable structural diffs. Invest in good formatting
  upfront — diff-style output with field-by-field comparison
  scales better than "expected X, got Y" walls.

---

## Merge 6 — Hot-reload preservation + chest_ui smoke test

**Goal:** the file-watcher reload path correctly handles typed
modules (re-checks schema, preserves last_state if compatible,
fails loudly otherwise). Chest_ui in Untold Lore migrates to
typed bindings end-to-end and renders correctly.

### Sub-merges

**M6a — Hot-reload schema preservation** (~3 days)

- `Ogham::reload` already exists and rebuilds the runtime. For
  typed modules, after the reload:
  - Re-run `assert_schemas_match` against the new schema.
  - If compatible: re-push `last_state` via `snapshot_into`.
  - If incompatible: surface a `RuntimeError::SchemaMismatch`
    (caller can choose to log + keep the old runtime, or panic).
- The `TypedOgham::reload()` pass-through wraps this.
- Tests: edit a typed `.ogh` file in a way that keeps schema
  stable (just rearrange the body) → state preserved. Edit it
  in a way that changes the schema → loud error.

**M6b — chest_ui migration** (~2 days)

- In Untold Lore (separate repo, separate branch).
- Add `events { chest_pick_up(); chest_cancel(); }` to `chest.ogh`.
- Rewrite `src/ui/chest_ui.rs` per the worked example in
  TYPED_BINDINGS.md §"Worked example".
- Verify the chest UI still works in-game. No visible behavioral
  change expected.

**M6c — Migration cookbook entries (TYPED_BINDINGS.md updates)** (~1 day)

- Add the F1 / F5 / F13 cookbook entries identified in the
  audit:
  - F1: refactoring computed event names.
  - F5: numeric-as-string draft pattern is the Phase 1 idiom.
  - F13: optional-as-empty-string fields can migrate to `T?`
    opportunistically.
- Promote `TYPED_BINDINGS.md` from "Design draft" to "Live
  contract" per its own §"Status" footer.
- Move per-UI detail from `TYPED_BINDINGS_UL_AUDIT.md` to the
  Untold Lore repo per the audit's "Next steps" §3.

### Files touched

**Ogham:**
| File | Change |
|---|---|
| `crates/ogham/src/typed.rs` | `reload` pass-through with schema re-check |
| `crates/ogham/src/lib.rs` | If `Ogham::reload` needs hooks for typed paths, add them |
| `tests/typed_hot_reload.rs` (new) | Schema-stable + schema-incompatible reload tests |
| `docs/internal/TYPED_BINDINGS.md` | Promote to Live contract; add cookbook |
| `docs/internal/TYPED_BINDINGS_UL_AUDIT.md` | Trim to design-relevant findings; rest moves out |

**Untold Lore:**
| File | Change |
|---|---|
| `data/engine/ui/chest.ogh` | Add `events {}` block |
| `src/ui/chest_ui.rs` | Rewrite per worked example |
| `Cargo.toml` | Depend on `ogham-derive` |

### Validation gate

- Schema-stable reload: edit `chest.ogh` to add a comment, save,
  game continues working without restart.
- Schema-incompatible reload: edit `chest.ogh` to add an
  undeclared event call, save, error surfaces in the game's
  log; the typed handle does not crash.
- Chest UI works in-game: open a chest, click pick-up, item
  appears in inventory; click cancel, chest closes.

### Estimated complexity: low
1 week. Most work is verification.

### Risks

- **`Ogham::reload` couples to host state**: today's reload
  re-applies host state from `RuntimeConfig` (per
  `INTENT.md` §7). The typed path needs to re-apply
  `last_state` instead of/in addition to that. Verify the
  ordering doesn't double-write or skip-write.
- **Untold Lore consumer-side surprise**: chest_ui is the
  smallest UI; if migration is harder than expected here, every
  larger UI is exponentially harder. **Treat migration friction
  in M6 as a Phase 1 design signal**, not an Untold Lore
  problem.

---

## Cross-cutting concerns

### Branch strategy

- `main` stays releasable.
- `phase1-typed-bindings` long-lived feature branch off `main`.
- Each merge (M0 through M6) is a separate PR into the feature
  branch, reviewed independently.
- Rebase the feature branch onto `main` weekly to stay current.
- Final merge of the whole feature branch into `main` is the
  Phase-1-ships milestone.

Rationale: the merges have well-defined contracts and
validation gates; merging each into `main` directly would put
half-built typed-bindings code on `main` for weeks. The
feature branch lets the work integrate continuously without
blocking other Ogham work.

### Test corpus

A `tests/fixtures/typed_bindings/` directory becomes the
canonical "real-world" test corpus. Populated from the audit's
per-UI schemas. Used by M1 (parser), M2 (compiler), M3 (LSP),
and M5 (schema-match) tests.

Initial contents:
```
tests/fixtures/typed_bindings/
  chest.ogh                    (M1: parses; M2: resolves)
  tip_log.ogh
  crafting.ogh
  console.ogh
  blueprint_editor.ogh
  talents.ogh
  dm_inventory.ogh
  dm_hud.ogh
  shared/
    inventory_types.ogh        (M1: cross-module record import)
    social_types.ogh
  invalid/                     (M2: each .ogh has a deliberate strict-mode error)
    typo_in_field.ogh
    typo_in_event.ogh
    computed_event_name.ogh
    field_on_primitive.ogh
    duplicate_host_state.ogh
    direct_self_reference.ogh
```

The `invalid/` corpus doubles as snapshot tests for diagnostic
quality. Each `.ogh` has a `// EXPECT:` comment with the
expected error message; tests read the comment and assert.

### Workspace name resolution

Open question O3 in TYPED_BINDINGS.md (import alias syntax for
record name conflicts) needs resolution before M1 lands.
Recommend extending the existing import grammar:

```ogh
import [Item] from "./inventory.ogh";
import [Item as ShopItem] from "./shop.ogh";
```

Mirrors Rust's `use foo::Item as ShopItem`. Small grammar
addition; M1's parser changes can include it.

### Minimum supported Rust version

Currently undeclared. The derive crate's lazy-init pattern needs
`LazyLock` (Rust 1.80, 2024-07). Two options:
1. **Declare `rust-version = "1.80"` in workspace.** Cleanest.
   Likely fine — Untold Lore presumably uses recent stable.
2. **Use `once_cell` crate instead of `LazyLock`.** Adds a
   dependency but unblocks older Rust versions.

Recommend option 1. Verify Untold Lore's Rust version first.

### Continuous integration

Add a typed-bindings test job that runs:
- `cargo build --workspace`
- `cargo test --workspace`
- `cargo test --package ogham-derive` with `trybuild` cases
- The fixture-based parser/resolver/diagnostic tests

If CI doesn't exist yet, M0 is the right time to add it (at
least for the typed-bindings work). If CI exists, extend it.

### Documentation maintenance

Three docs evolve together:
- `TYPED_BINDINGS.md` — design contract; updates with cookbook
  entries during M6 and graduates to Live contract.
- `TYPED_BINDINGS_UL_AUDIT.md` — working notes; trimmed during
  M6, residual content moves to Untold Lore repo.
- `TYPED_BINDINGS_IMPLEMENTATION.md` — this doc; archived or
  deleted after Phase 1 ships.

The relevant existing docs to update at Phase-1 ship:
- `LANGUAGE.md` — add the new declaration forms to the grammar
  reference.
- `RUNTIME.md` — add `TypedOgham` and the typed construction
  path; cross-reference `INTENT.md` §2 (asymmetry preserved).
- `LSP.md` — update the capabilities table (Completion: ✓ added;
  schema-aware Hover/Definition).
- `AGENTS.md` — add a "Typed bindings" section for user-facing
  integration.

---

## Risk register (consolidated)

Ranked by likelihood × impact.

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R1 | Proc-macro work overruns the M4 estimate | High | Medium | Time-box; ship M4 with rough error messages, polish later. Reuse `serde`'s patterns. |
| R2 | Untold Lore migration uncovers a design hole the audit missed | Medium | High | Migrate chest_ui in M6 *before* declaring Phase 1 ship. If something breaks, adjust before the audit's other 19 UIs touch the new path. |
| R3 | Compiler identifier-resolution changes break existing closure capture | Medium | High | M2 negative tests must cover loose-mode closures; full regression suite must pass. |
| R4 | LSP cross-module schema loading is too slow | Medium | Low | Cache parsed schemas in `DocumentStore` keyed by mtime. Easy to add reactively. |
| R5 | Workspace restructure breaks Untold Lore's `Cargo.toml` dependency on Ogham | Low | Medium | Communicate ahead; the dep path is `ogham = "..."` either way; the internal restructure is invisible. |
| R6 | Diagnostic infra (M0) needs more than the planned 50-line extension | Low | Low | M0 is its own merge; if it grows, it grows in isolation without blocking others. |
| R7 | Macro-generated code conflicts with consumer's `use` statements | Low | Low | Use absolute paths (`::ogham_schema::*`) in macro output. Standard practice. |
| R8 | A future Ogham feature (e.g., generic functions) requires changing the schema model | Low | Medium | The `ModuleSchema` is internal; revising it is allowed. The grammar is harder to walk back; design conservatively. |

---

## Summary timeline

```
Week  1     2     3     4     5     6     7     8     9    10    11    12    13    14    15
      |     |     |     |     |     |     |     |     |     |     |     |     |     |     |
M0   [==]
M1         [================]
M2                          [================]
M3                          [============]            (parallel with M2)
M4                                     [======================]
M5                                                              [======]
M6                                                                       [====]

M0: 1wk   M1: 2-3wk   M2: 2-3wk   M3: 2wk (parallel)   M4: 3-4wk   M5: 1-2wk   M6: 1wk

Single contributor: 12-17 weeks
Two contributors split at M3/M4: ~9-12 weeks
```

---

## Decision points before starting

Resolve these before M0 ships:

1. **Workspace Rust version**: declare `rust-version = "1.80"` or
   use `once_cell`? — recommend 1.80 (verify Untold Lore allows it).
2. **`ogham-types` crate name**: vs. `ogham-schema` or
   `ogham-bindings`? — recommend `ogham-schema`.
3. **`host_state` token shape**: single keyword token or
   identifier+keyword? — recommend single keyword.
4. **`Self` keyword promotion**: keyword or contextual? —
   recommend keyword.
5. **Import alias syntax** (`import [Item as Foo]`): land in M1
   or defer? — recommend land in M1 (small grammar change).
6. **MPSC choice**: `std::sync::mpsc` or `crossbeam`? — recommend
   `std`; revisit on perf data.

All six are low-stakes calls but each has rabbit-hole potential
if discovered mid-implementation.

---

## What "Phase 1 ships" looks like

After M6 merges to `main`:
- `record`, `host_state`, `events` declarations parseable in
  any `.ogh` module.
- Strict mode (declaring `host_state {}`) catches identifier
  typos, missing fields, wrong event signatures, computed event
  names — all with helpful diagnostics.
- LSP surfaces all of the above as hover, completion, and
  go-to-definition.
- `#[derive(OghamRecord)]`, `#[derive(OghamState)]`,
  `#[derive(OghamMsg)]` all work.
- `Ogham::watch_typed::<S, M>` constructs a typed scene, runs
  the schema-match check, and returns a `TypedOgham<S, M>`.
- `set_state(&S)` diffs internally; `poll_msg() -> Option<M>`
  drains the event queue.
- Hot-reload preserves typed state when the schema is unchanged;
  fails loudly when it isn't.
- `chest_ui` in Untold Lore runs on the typed path in production.
- All existing loose-mode `.ogh` files and Rust callers continue
  working unchanged.

The other 19 Untold Lore UIs migrate in the order specified by
`TYPED_BINDINGS_UL_AUDIT.md` §"Suggested migration sequence",
post-Phase-1, on Untold Lore's own schedule.

End of plan.
