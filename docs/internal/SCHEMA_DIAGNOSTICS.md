# Ogham — Schema-Diagnostic Surface

> **Status: Phase 0 Ogham-side shipped 2026-05-05; UL adoption +
> Phase 1 LSP integration pending.** See
> [`SCHEMA_DIAGNOSTICS_IMPLEMENTATION.md`](SCHEMA_DIAGNOSTICS_IMPLEMENTATION.md)
> for the per-merge status (P0-M1..M5 ✅; P0-M6 UL gate and
> P1-M1..M6 LSP wiring ⏳).
>
> A cross-side diagnostic layer that catches drift between a `.ogh`
> module and the Rust types declared to bind to it
> (`#[derive(OghamState)]` / `#[derive(OghamMsg)]`) at edit time, in
> any tool that consumes Ogham — editors, agents, hooks, CI. Phase 0
> ships the engine in `src/diagnostics/` and the `ogham check` CLI
> consumer; Phase 1 will wire the same engine through `ogham-lsp` so
> diagnostics surface in editors as the user types (today the LSP
> emits scanner / parser / typed-bindings AST validation /
> lifecycle-warning diagnostics, but not schema-drift errors).
>
> Builds on [`TYPED_BINDINGS.md`](TYPED_BINDINGS.md) (the boundary
> contract this checks) and [`LSP.md`](LSP.md) (the diagnostic pipeline
> this extends). Companion to
> [`TYPED_BINDINGS_IMPLEMENTATION.md`](TYPED_BINDINGS_IMPLEMENTATION.md)
> — Phase 1 shipped `check_schemas_match` at runtime; this doc lifts
> the same check to edit time.

---

## Motivation

Today's diagnostic surface is asymmetric:

- **Inside a `.ogh` module:** `ogham-lsp` flags scanner errors, parse
  errors, schema-resolver errors, and strict-mode compile errors as the
  user types. Coverage is good.
- **Across the Rust↔.ogh boundary:** the only check is
  `check_schemas_match::<S, M>` in `src/typed.rs`, which runs at
  `TypedOgham::watch_typed` construction. By that point the binary has
  already compiled and started.

The drift class this misses is the most common one: rename a field on
the Rust state struct, the `.ogh` module still parses fine, the LSP
shows green, the agent claims success, and the failure surfaces only
when the binary launches and the runtime errors out.

Worse, agents working in consumer repos (Untold Lore today) have no
in-loop feedback that the binding still matches. They edit Rust, edit
`.ogh`, claim done. The drift gets caught by a human.

This is solvable: the schema-match logic already exists; it just needs
to run earlier, on data instead of generics, fed by a manifest the
proc-macros emit at compile time.

---

## At a glance

```
┌─────────────────────┐                    ┌─────────────────────┐
│ Rust source         │                    │ .ogh source         │
│   #[derive(...)]    │                    │   host_state {...}  │
│   #[ogham(          │                    │   events {...}      │
│     binding_module  │                    │                     │
│     = "ui/x.ogh")]  │                    │                     │
└──────────┬──────────┘                    └──────────┬──────────┘
           │ cargo build                              │ ogham-lsp / ogham check
           ▼                                          ▼
   ┌───────────────────┐                     ┌──────────────────┐
   │ proc-macro emits  │                     │ ModuleSchema     │
   │ target/ogham/     │                     │ (parsed)         │
   │   <binding>.json  │                     │                  │
   └─────────┬─────────┘                     └────────┬─────────┘
             │                                        │
             └────────────────┬───────────────────────┘
                              ▼
                  ┌──────────────────────────┐
                  │ ogham::diagnostics::     │
                  │   check_against_manifest │
                  └────────────┬─────────────┘
                               ▼
                       Vec<Diagnostic>
                  (LSP publishes / CLI prints)
```

Two front-ends (LSP + CLI), one backend, one source of truth (the
manifest), one new authoring concept (`#[ogham(binding_module = …)]`).

---

## Architecture

### Schema manifest

One JSON file per derived binding, written by the proc-macro at
expansion time to `<CARGO_MANIFEST_DIR>/target/ogham/<binding-id>.json`.
The path is derived from the consumer crate's `CARGO_MANIFEST_DIR`
(set by cargo when proc-macros expand), so the manifest co-locates with
the crate that owns the binding.

Two shapes — one for `OghamState`, one for `OghamMsg`:

```jsonc
// state binding
{
  "kind": "state",
  "binding": "untold_lore::ui::chest::ChestUiState",
  "ogh_module": "data/engine/ui/chest_ui.ogh",
  "rust_source": { "file": "src/ui/chest_ui.rs", "line": 42, "column": 10 },
  "host_state": {
    "fields": {
      "items":    { "ty": "array<Item>" },
      "selected": { "ty": "int" }
    }
  }
}

// events binding
{
  "kind": "events",
  "binding": "untold_lore::ui::chest::ChestUiMsg",
  "ogh_module": "data/engine/ui/chest_ui.ogh",
  "rust_source": { "file": "src/ui/chest_ui.rs", "line": 73, "column": 10 },
  "events": {
    "open_chest":  { "args": [] },
    "take_item":   { "args": ["int"] },
    "close_chest": { "args": [] }
  }
}
```

`TypeRef` serializes as its canonical string form (`int`,
`array<Item>`, `map<string, Player>`, `T?`) — the exact surface
syntax users type in `.ogh` files, already rendered by Ogham for
diagnostic and hover messages. The format adds a parser back from
that string, so the macro side can hand-emit JSON without depending
on serde. The reader side uses serde, which is already in Ogham's
transitive graph via `tower-lsp`.

`ogh_module` is stored relative to `CARGO_MANIFEST_DIR`. The diagnostic
backend resolves it absolute when matching against a `.ogh` file URI.

### Binding linkage

A new attribute extends the existing `#[ogham(...)]` parser in
`crates/ogham-derive/src/attrs.rs`:

```rust
#[derive(OghamState)]
#[ogham(binding_module = "data/engine/ui/chest_ui.ogh")]
pub struct ChestUiState {
    pub items: Vec<Item>,
    pub selected: i64,
}

#[derive(OghamMsg)]
#[ogham(binding_module = "data/engine/ui/chest_ui.ogh")]
pub enum ChestUiMsg {
    OpenChest,
    TakeItem(i64),
    CloseChest,
}
```

State and Msg are linked by **shared `binding_module` value**, not by a
cross-reference attribute. The diagnostic backend groups manifests by
`ogh_module` and runs both checks. Multiple bindings (state or events)
targeting the same module are *legal* — see "Multi-binding modules"
below.

The attribute is **optional**. State/Msg derives without it still work
exactly as today — they just don't participate in cross-side
diagnostics. This preserves the "migration is per-file, not
per-codebase" property from the typed-bindings design.

### Diagnostic backend

`check_schemas_match` (today, `src/typed.rs:246`) is generic over `S:
OghamState, M: OghamMsg`. Lift its body into a data-shaped function:

```rust
// src/diagnostics.rs (new)
pub fn check_against_manifest(
    parsed: &ModuleSchema,
    state_manifest: Option<&StateManifest>,
    events_manifest: Option<&EventsManifest>,
    binding_id: &str,             // tags every emitted Diagnostic
) -> Vec<Diagnostic>;
```

The function checks one binding pair (state + events) at a time.
`binding_id` is the fully-qualified Rust type path (e.g.
`untold_lore::ui::chest::ChestUiState`); the function tags every
emitted diagnostic with it so multi-binding output stays readable.

`Diagnostic` is the existing Ogham diagnostic type used elsewhere in
the LSP (or a thin wrapper); each entry carries a span pointing at the
offending `.ogh` location plus a `related_information` pointer at the
Rust source span recorded in the manifest. The existing `diff_record`
+ event-set-diff logic in `typed.rs` becomes the body, with diff lines
turned into structured diagnostics rather than concatenated strings.

The runtime path keeps using the generic `check_schemas_match` —
unchanged — so `TypedOgham::watch_typed` still fails at construction
when the manifest path was skipped (e.g. someone deleted the manifest
file). Belt and suspenders.

#### Multi-binding modules

A `.ogh` module can have more than one Rust binding targeting it —
legitimately. Examples: a test crate runs property tests against the
same module the main crate consumes; a shared engine `.ogh` is
imported by two product crates. Both should validate.

Resolution rule: **all bindings must agree**. The LSP and CLI loop
`check_against_manifest` once per binding discovered for a given
`ogh_module` and concatenate the diagnostics. Each emitted entry
carries its `binding_id`, so when bindings *disagree* with the `.ogh`
the user sees per-binding output ("Rust binding A is missing field X;
Rust binding B has type mismatch on field Y") rather than one blurred
message. The `.ogh` is correct iff every binding's check returns
empty.

This handles the accidental-duplicate case the same way as the
intentional case: a copy-paste mistake surfaces as one binding
agreeing, the other not — the diagnostic points at the offending
binding directly. No special "duplicate binding" error.

### LSP front-end

`ogham-lsp`'s `collect_diagnostics` (`src/lsp/server.rs:225`) gains a
fifth pipeline stage after the existing schema-resolver step:

```
  1. Scanner errors
  2. Parser errors
  3. Schema-resolver errors
  4. Strict-mode compile errors
  5. *Binding mismatch diagnostics  ← new
```

Stage 5 runs only when the open file's path matches a `ogh_module`
field in some discovered manifest. If no manifest exists *and* the
module declares `host_state {}` (i.e. it's a strict module that wants
a binding), emit a single info-severity hint: "no Rust binding
manifest found for this module — Rust crate may not be built yet."
Loose modules with no `host_state` get no hint.

Manifest discovery: at `initialize`, walk every workspace folder
(`InitializeParams::workspace_folders`) and every cargo workspace
member rooted under it, looking for `target/ogham/*.json`. Cache the
parsed manifests, keyed by absolute `ogh_module` path. Refresh on a
debounced filesystem watcher (manifests change at cargo-build cadence,
not keystroke cadence — debounce ~500 ms is fine).

### CLI front-end

A new subcommand on the existing `ogham` binary:

```
ogham check <path-to-ogh-file> [--workspace <dir>]
ogham check --all                     # every .ogh under cwd
```

Output format mirrors `cargo`'s `--message-format=short` so it's
familiar to agents already used to reading Rust errors:

```
data/engine/ui/chest_ui.ogh:14:5: error[ogham:binding]: host_state field
  `selecte` declared in .ogh but missing from Rust struct ChestUiState
  --> src/ui/chest_ui.rs:42:10
  help: did you mean `selected`?
```

Same backend, same diagnostics; just rendered as text. Exit code is
non-zero on ERROR-severity diagnostics so CI / hooks can gate on it.

### Staleness handling

If the consumer edits `src/ui/chest_ui.rs` but doesn't rebuild,
`target/ogham/chest_ui-state.json` keeps describing the old shape.
Silent staleness is the worst outcome — false-green is worse than
false-red.

Mitigation: at manifest load, the diagnostic backend `stat`s both the
manifest file and the `rust_source.file` it points to. If the source
mtime is newer than the manifest mtime, emit a single
WARNING-severity diagnostic at the top of the `.ogh` file:

```
warning[ogham:stale-manifest]: binding manifest is older than its
  Rust source — diagnostics may be inaccurate
  --> target/ogham/chest_ui-state.json (mtime 2026-05-05T14:22:01Z)
  --> src/ui/chest_ui.rs (mtime 2026-05-05T14:31:48Z)
  help: run `cargo check -p untold_lore` to refresh
```

The mismatch diagnostics still publish — staleness doesn't suppress
them — but the warning makes clear that the result reflects the last
build, not the current Rust source. This is the "manifest stale
warnings feel safer" decision: we'd rather over-warn than silently
green-light drift.

---

## Phasing

### Phase 0 — CLI + manifest emit

**Goal:** Ship the manifest format, the proc-macro emit, and the CLI
front-end. No LSP wiring. The CLI is enough for hook-based agent
gates and for human invocation.

Merges:

- **P0-M1** — `#[ogham(binding_module = "...")]` attribute parser in
  `ogham-derive`. No emit yet; just accept and ignore. Lets us land
  the consumer-side annotation without moving anything else.
- **P0-M2** — Manifest format crate-internal types (`StateManifest`,
  `EventsManifest`), TypeRef canonical-string round-trip. Tests for
  every `TypeRef` variant.
- **P0-M3** — Proc-macro emit: `OghamState` / `OghamMsg` derives
  write `target/ogham/<binding-id>.json` when `binding_module` is
  set. ID = sanitized `kind + "-" + binding_module + "-" + type-name`
  to avoid collisions across crates in a workspace.
- **P0-M4** — `ogham::diagnostics::check_against_manifest`
  data-shaped backend; refactor `check_schemas_match` to share.
- **P0-M5** — `ogham check` subcommand. Workspace discovery,
  manifest grouping by `ogh_module`, staleness detection, exit-code
  semantics.
- **P0-M6** — Untold Lore validation: annotate one binding
  (chest_ui), verify the CLI catches a synthetic field rename, wire
  `ogham check --all` into UL's `cargo test` or pre-commit hook.

Validation gate for Phase 0: a UL agent can edit a Rust state struct,
run `ogham check`, and see a structured drift diagnostic without
having to launch the binary.

### Phase 1 — LSP integration

**Goal:** The same diagnostics surface in any LSP client (editor +
LSP-aware agent harnesses) without a separate invocation.

Merges:

- **P1-M1** — Manifest discovery + caching in `ogham-lsp`: walk
  `workspace_folders` at `initialize`, populate a manifest registry,
  watch `target/ogham/` for changes.
- **P1-M2** — Wire stage 5 (binding mismatch) into
  `collect_diagnostics`. Diagnostic source field = `ogham:binding`.
  Use `related_information` to point at the Rust source span.
- **P1-M3** — Stale-manifest WARNING diagnostic.
- **P1-M4** — "No manifest found" INFO hint for strict modules with
  no discovered binding. Suppressible via LSP setting.
- **P1-M5** — Settings: an LSP `initializationOptions` field
  (`oghamBinding.checkEnabled`, default true) and a per-document
  comment pragma (`// @ogham:binding-check off`) for scoped opt-out.
- **P1-M6** — Hover affordance. When the cursor is on a `host_state`
  field name in a `.ogh` file, the LSP hover already renders the
  resolved `TypeRef`; extend it to append "bound to
  `<RustType>::<field>` at `<rust_source.file>:<line>`" pulled from
  the manifest registry (with one entry per binding when multiple
  bindings target the module). Same registry, same lookup pattern as
  P1-M2; purely additive ergonomics. Symmetric extension on event
  names, where they're declared in the `events {}` block.

Validation gate for Phase 1: open a `.ogh` file in any LSP-aware
editor, edit a host_state field name to a typo, see the diagnostic
appear in the problems pane within one keystroke debounce; hover the
field name and see the bound Rust type.

---

## Non-goals

- **Body type-checking.** Whether a `.ogh` expression's runtime type
  matches a host_state field's declared type is Phase 2+ territory in
  [`TYPED_BINDINGS.md`](TYPED_BINDINGS.md). This doc only types the
  *boundary*.
- **Cross-module record validation against the Rust side.** Imports
  between `.ogh` modules (`import [Item] from "./inv.ogh"`) are
  already validated by the schema resolver. Validating an imported
  record against a Rust `OghamRecord` derive is a clean extension but
  not in scope for the first cut.
- **Generating `.ogh` skeletons from Rust types** (or vice versa).
  Useful, but a separate workflow.
- **Replacing the runtime `check_schemas_match` check.** Belt and
  suspenders — the runtime check is cheap and catches the
  "manifest deleted / never emitted" case the static path can't.

---

## Cross-references

- [`SCHEMA_DIAGNOSTICS_IMPLEMENTATION.md`](SCHEMA_DIAGNOSTICS_IMPLEMENTATION.md)
  — merge-by-merge implementation plan for the twelve P0/P1 merges
  this design specifies.
- [`TYPED_BINDINGS.md`](TYPED_BINDINGS.md) — the boundary contract
  this layer checks. Read first if you're new to the typed-bindings
  story.
- [`TYPED_BINDINGS_IMPLEMENTATION.md`](TYPED_BINDINGS_IMPLEMENTATION.md)
  — Phase 1 implementation history; `check_schemas_match` shipped in
  M5. The cadence and doc shape here mirror that document.
- [`LSP.md`](LSP.md) — the diagnostic pipeline this extends. Stage 5
  drops in after the existing four.
- [`INTENT.md`](INTENT.md) §7 ("Hot reload preserves what it can,
  drops what it can't") — the same fail-loud principle applied to
  edit-time drift.
