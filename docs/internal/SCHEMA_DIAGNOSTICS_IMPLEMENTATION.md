# Schema Diagnostics — Implementation Plan

> **Status: Plan — not yet started.**
>
> Concrete, merge-by-merge implementation plan for the
> Schema-Diagnostic Surface design. Builds on
> [`SCHEMA_DIAGNOSTICS.md`](SCHEMA_DIAGNOSTICS.md) (the design
> contract) and inherits the layout conventions of
> [`TYPED_BINDINGS_IMPLEMENTATION.md`](TYPED_BINDINGS_IMPLEMENTATION.md).
>
> **Scope:** twelve merges total — six for Phase 0 (CLI + manifest
> emit) and six for Phase 1 (LSP integration). All land on `main`
> directly (no feature branches; "rip and tear" cadence between
> merges).
>
> **Status by merge:**
> - P0-M1 — Attribute parser. ⏳ Not started.
> - P0-M2 — Manifest types + TypeRef canonical-string. ⏳
> - P0-M3 — Proc-macro emit. ⏳
> - P0-M4 — Diagnostic backend. ⏳
> - P0-M5 — `ogham check` CLI. ⏳
> - P0-M6 — UL validation gate. ⏳ (consumer-side)
> - P1-M1..M6 — Phase 1 LSP integration. ⏳

---

## At a glance

```
P0-M1  Attribute parser — accepts #[ogham(binding_module = "...")]
       └── crates/ogham-derive/src/attrs.rs; mechanical; ~½ day; low risk

P0-M2  Manifest types + TypeRef ↔ canonical-string round-trip
       └── src/diagnostics/manifest.rs (new); foundational; ~1 day; low risk

P0-M3  Proc-macro emits target/ogham/<binding-id>.json
       └── crates/ogham-derive emit path; ~1-2 days; medium risk (FS write from macro)

P0-M4  ogham::diagnostics::check_against_manifest backend
       └── lift check_schemas_match into data shape; ~1 day; low risk

P0-M5  `ogham check` CLI subcommand
       └── new bin; workspace discovery; staleness; ~2-3 days; medium risk

P0-M6  Untold Lore validation
       └── consumer-side annotate + verify drift caught; ~½ day; low risk

P1-M1  ogham-lsp manifest discovery + caching
       └── src/lsp/manifests.rs; FS watcher; ~1-2 days; low risk

P1-M2  Stage 5 binding diagnostics in collect_diagnostics
       └── wire backend through LSP; ~1 day; low risk

P1-M3  Stale-manifest WARNING through LSP
       └── reuse staleness logic; ~½ day; low risk

P1-M4  "No manifest found" INFO hint
       └── ~½ day; low risk

P1-M5  Settings + per-document opt-out pragma
       └── ~1 day; low risk

P1-M6  Hover affordance
       └── extend src/lsp/hover.rs; ~1 day; low risk
```

Total: ~10-15 working days for a single contributor. Phase 0 alone
(~5-7 days) delivers most of the agent-ergonomics value; Phase 1 is
additive polish for editor users.

---

## Cross-cutting

### Branch strategy

Land each merge on `main` directly. The pieces are individually
small and mostly additive — the new attribute is opt-in, the new
diagnostics fire only when manifests exist, the new CLI is a new
binary. Nothing destabilizes the existing build.

The one exception is P0-M4 (lifting `check_schemas_match` into a
shared backend); that's a refactor of the runtime path. Validate
with the existing TypedOgham tests before merge.

### Test posture

Mirror the existing distribution. New tests live in:
- `crates/ogham-derive/tests/` for derive-attribute / emit tests.
- `tests/diagnostics_*.rs` for backend tests.
- `tests/cli_check.rs` for CLI integration tests.
- `tests/lsp_binding_*.rs` for Phase 1 LSP integration tests.

Every merge ships its own tests; nothing relies on a future merge's
fixtures.

### Risk register

- **Proc-macro filesystem write** (P0-M3). Not the conventional
  pattern — proc-macros usually emit code, not files. Precedent:
  sqlx, diesel, askama. Mitigation: skip emit when env var
  `OGHAM_SKIP_MANIFEST_EMIT=1` is set, for hostile sandboxed
  environments. Default behaviour writes; opt-out is explicit.
- **Manifest staleness across cargo profiles.** `target/ogham/`
  lives outside `target/debug/` vs `target/release/`, so the
  manifest is profile-invariant. No action needed.
- **TypeRef canonical-string drift.** When TypeRef gains a new
  variant (Phase 2 sum types, etc.), the canonical-string parser
  must learn it. Mitigation: round-trip property test in P0-M2 that
  exhausts every `TypeRef` constructor; CI fails when a new variant
  isn't covered.
- **Multi-binding diagnostic noise.** With the all-must-agree rule
  (R2), a misconfigured second binding emits a parallel set of
  diagnostics for the same `.ogh` line. Acceptable — the `binding_id`
  prefix tells the user which side disagrees.

---

## Phase 0 — CLI + manifest emit

### P0-M1 — Attribute parser

**Goal:** the `#[ogham(binding_module = "...")]` attribute parses
without error on `OghamState` and `OghamMsg` derives. No emit yet —
the value is collected and discarded. This unblocks consumer-side
annotation work without changing build behaviour.

**Files touched:**
- `crates/ogham-derive/src/attrs.rs` — add a parser sibling to
  `record_name_override`.
- `crates/ogham-derive/src/lib.rs` — call the parser in
  `expand_ogham_state` and `expand_ogham_msg`; store the result in
  a temp variable for now (P0-M3 will use it).

**New shape:**

```rust
// crates/ogham-derive/src/attrs.rs

/// Parse `#[ogham(binding_module = "path/to/file.ogh")]` from a
/// list of attributes on a struct or enum. Returns `None` if no
/// such attribute is present. Errors only on malformed values
/// (non-string, missing equals, etc.).
pub fn binding_module_path(attrs: &[syn::Attribute]) -> syn::Result<Option<String>>;
```

The parser walks `#[ogham(...)]` items, matches `binding_module = LIT`,
emits `syn::Error` on shape violations. Co-existence with the existing
`record_name_override` parser is by inspection — both walk the same
attribute list and ignore non-matching keys.

**Tests** (`crates/ogham-derive/tests/binding_module_attr.rs`):
- absent → `Ok(None)`.
- present with valid string → `Ok(Some(path))`.
- non-string value → `Err`.
- combined with other `#[ogham(...)]` keys (e.g. `record = "..."`) →
  parses both correctly.
- bad shape (`#[ogham(binding_module)]` no `=`) → `Err`.

**Validation gate:**
- `cargo test --workspace` clean.
- A consumer adding the attribute to a state struct compiles
  successfully (smoke test in `tests/`).

**Estimated complexity: low.** ~½ day.

---

### P0-M2 — Manifest types + TypeRef canonical-string

**Goal:** define the on-disk JSON shape and a stable string
representation of `TypeRef` that round-trips through it.

**Files touched:**
- `src/diagnostics/mod.rs` — new module; declare submodules.
- `src/diagnostics/manifest.rs` — new file; `StateManifest`,
  `EventsManifest`, `RustSourceLoc` types + serde derives + JSON
  read/write helpers.
- `src/parser/typed_bindings.rs` — extend `TypeRef` with
  `to_canonical_string` and `from_canonical_string`. (Method
  rendering already exists implicitly in error formatting; this
  centralizes it.)
- `Cargo.toml` — add `serde = { version = "1", features = ["derive"] }`
  and `serde_json = "1"` to the `ogham` crate. (Already in the
  transitive graph via `tower-lsp`; making it direct is cheap.)
  **Not added to `ogham-derive`** — the macro side writes JSON by
  hand to keep its compile time minimal.

**New shape:**

```rust
// src/diagnostics/manifest.rs

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Manifest {
    State(StateManifest),
    Events(EventsManifest),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateManifest {
    pub binding: String,           // FQ Rust path, e.g. "untold_lore::ui::ChestUiState"
    pub ogh_module: String,        // crate-relative path, e.g. "data/ui/chest.ogh"
    pub rust_source: RustSourceLoc,
    pub host_state: ManifestRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventsManifest {
    pub binding: String,
    pub ogh_module: String,
    pub rust_source: RustSourceLoc,
    pub events: BTreeMap<String, ManifestEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestRecord {
    pub fields: BTreeMap<String, ManifestField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestField {
    /// TypeRef in canonical-string form; e.g. "array<Item>",
    /// "map<string, int>", "int?". Matches the exact surface
    /// syntax used in `.ogh` source.
    pub ty: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestEvent {
    /// Each arg as canonical-string TypeRef.
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RustSourceLoc {
    pub file: String,
    pub line: u32,
    pub column: u32,
}

impl Manifest {
    pub fn ogh_module(&self) -> &str { /* ... */ }
    pub fn binding(&self) -> &str    { /* ... */ }
    pub fn read(path: &Path) -> std::io::Result<Self>;
    pub fn write(&self, path: &Path) -> std::io::Result<()>;
}
```

`TypeRef` extension:

```rust
impl TypeRef {
    /// Render to the canonical string form used in manifests and
    /// diagnostic messages. Stable across releases — adding a variant
    /// requires extending both this and `from_canonical_string`.
    pub fn to_canonical_string(&self) -> String;

    pub fn from_canonical_string(s: &str) -> Result<Self, TypeRefParseError>;
}
```

**Tests** (`tests/diagnostics_manifest.rs`):
- Round-trip: every `TypeRef` constructor → canonical string →
  parse → equality.
- Manifest JSON round-trip: `StateManifest` and `EventsManifest`
  fixtures → write → read → equality.
- Malformed JSON → typed error (not panic).
- Property test: `proptest`-style generator over `TypeRef` →
  `to_canonical_string` → `from_canonical_string` → identity.

**Validation gate:**
- `cargo build --workspace` clean.
- All new tests pass; existing tests untouched.

**Estimated complexity: low.** ~1 day. The TypeRef parser is the
only mildly tricky piece — recursive-descent over a small grammar.

---

### P0-M3 — Proc-macro emit

**Goal:** when `#[ogham(binding_module = "...")]` is set on an
`OghamState` or `OghamMsg` derive, the macro writes the corresponding
manifest JSON to `<target>/ogham/<binding-id>.json` at expansion
time.

**Files touched:**
- `crates/ogham-derive/src/lib.rs` — call new emit helper from
  `expand_ogham_state` and `expand_ogham_msg` after building the
  field/event metadata.
- `crates/ogham-derive/src/manifest_emit.rs` (new) — the emit
  helper. Hand-written JSON serialization; no serde dep.

**Target dir resolution:**

```rust
fn target_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("CARGO_TARGET_DIR") {
        return PathBuf::from(custom);
    }
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR set by cargo");
    PathBuf::from(manifest).join("target")
}

fn manifest_path(binding_id: &str) -> PathBuf {
    target_dir().join("ogham").join(format!("{binding_id}.json"))
}
```

**Binding-id construction:**

```
<kind>-<sanitized-binding-module>-<type-name>
```

where `kind` is `state` or `events`, and `sanitized-binding-module`
replaces `/`, `\`, and `.` with `_`. Example:

```
state-data_engine_ui_chest_ui_ogh-ChestUiState.json
events-data_engine_ui_chest_ui_ogh-ChestUiMsg.json
```

The full Rust binding path goes in the manifest body, so the
filename only needs to be unique-per-binding within a crate.

**JSON writer:**

The macro builds the JSON string via `format!()` calls, since
serde_json adds compile time and isn't load-bearing for hand-shaped
output. Roughly:

```rust
fn write_state_manifest(
    binding: &str,
    ogh_module: &str,
    rust_src: &RustSourceLoc,
    fields: &[(String, String)],          // (name, canonical-typeref)
) -> String {
    let fields_json = fields.iter()
        .map(|(n, t)| format!(r#""{}":{{"ty":"{}"}}"#, json_escape(n), json_escape(t)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"kind":"state","binding":"{}","ogh_module":"{}",\
"rust_source":{{"file":"{}","line":{},"column":{}}},\
"host_state":{{"fields":{{{}}}}}}}"#,
        json_escape(binding),
        json_escape(ogh_module),
        json_escape(&rust_src.file),
        rust_src.line, rust_src.column,
        fields_json,
    )
}
```

Round-trippable against the serde-derived reader by construction —
the round-trip test in P0-M4 catches any drift.

**Skip mechanism:**

```rust
if std::env::var_os("OGHAM_SKIP_MANIFEST_EMIT").is_some() { return; }
```

For sandboxed CI / read-only filesystems.

**Files written by the proc-macro must be:**
- created idempotently (overwrite ok; no append).
- written via tempfile-rename for atomicity (avoid the LSP reading
  a half-written file during a watch event). Standard pattern:
  write to `<path>.tmp`, fsync, rename to `<path>`.

**Tests** (`crates/ogham-derive/tests/manifest_emit.rs`):
- Macro fixture with `binding_module` set → invoke compilation in
  a tempdir → assert manifest file appears at expected path with
  expected content (parsed via the P0-M2 reader).
- Without `binding_module` → no manifest written.
- With `OGHAM_SKIP_MANIFEST_EMIT=1` → no manifest written.
- Two derives in the same file with different `binding_module`
  values → two distinct manifests.

**Validation gate:**
- `cargo build --workspace` clean.
- An end-to-end smoke test in `tests/typed_ogham_e2e.rs` (existing
  file) gains an assertion that manifest files materialize when
  the test crate uses `binding_module`.

**Estimated complexity: medium.** ~1-2 days. The risk is in the
macro-side filesystem write — needs the tempfile-rename pattern and
the env-var skip, neither of which are Rust-stdlib-trivial. Allow a
half-day buffer for sandbox-edge cases.

---

### P0-M4 — Diagnostic backend

**Goal:** lift `check_schemas_match` (`src/typed.rs:246`) into
`ogham::diagnostics::check_against_manifest`, operating on data
rather than `S: OghamState, M: OghamMsg` generics. Keep
`check_schemas_match` as a thin wrapper that derives schemas from
the generic types and calls the new function.

**Files touched:**
- `src/diagnostics/check.rs` (new) — the new function.
- `src/diagnostics/diagnostic.rs` (new) — the crate-level
  `Diagnostic` type. Crate-internal; the LSP converts to
  `tower_lsp::Diagnostic` at its boundary.
- `src/typed.rs` — `check_schemas_match` becomes a 5-line wrapper.

**New shape:**

```rust
// src/diagnostics/diagnostic.rs

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,                // e.g. "ogham:binding"
    pub message: String,
    pub primary: Span,               // location in the .ogh file
    pub related: Vec<RelatedSpan>,   // typically the Rust source loc
    pub binding_id: Option<String>,  // for multi-binding output
}

#[derive(Debug, Clone, Copy)]
pub enum Severity { Error, Warning, Info }

#[derive(Debug, Clone)]
pub struct RelatedSpan {
    pub file: PathBuf,
    pub line: u32,
    pub column: u32,
    pub message: String,
}
```

```rust
// src/diagnostics/check.rs

pub fn check_against_manifest(
    parsed: &ModuleSchema,
    state: Option<&StateManifest>,
    events: Option<&EventsManifest>,
    binding_id: &str,
) -> Vec<Diagnostic>;
```

The body is the existing `check_schemas_match` logic, with diff
strings replaced by `Diagnostic` constructions that carry the .ogh
span (from `parsed.host_state.decl_span` or the field's `decl_span`)
and a `RelatedSpan` pointing at the manifest's `rust_source`.

**Refactor:**

```rust
// src/typed.rs

fn check_schemas_match<S: OghamState, M: OghamMsg>(parsed: &ModuleSchema)
    -> Result<(), RuntimeError>
{
    let state_manifest = StateManifest::synthesize::<S>(parsed.module_path());
    let events_manifest = EventsManifest::synthesize::<M>(parsed.module_path());
    let diags = ogham::diagnostics::check_against_manifest(
        parsed,
        Some(&state_manifest),
        Some(&events_manifest),
        std::any::type_name::<S>(),
    );
    if diags.is_empty() {
        Ok(())
    } else {
        Err(RuntimeError::SchemaMismatch(render_diagnostics(&diags)))
    }
}
```

`StateManifest::synthesize::<S>` is a constructor that builds a
manifest from a generic `S: OghamState` — same data the macro emits
to disk, just produced at runtime from the trait methods. Lets the
runtime reuse the same backend.

**Tests** (`tests/diagnostics_check.rs`):
- Every existing diff variant from `check_schemas_match` covered
  here too: missing field, extra field, type mismatch on field,
  missing event, extra event, type mismatch on event arg.
- Multi-binding: two state manifests, one matches and one
  disagrees → one binding's diagnostics are empty, the other's
  carry a `binding_id`.
- Empty manifests + non-empty parsed schema → error per missing
  field/event.

**Validation gate:**
- `cargo build --workspace` clean.
- The existing TypedOgham tests in `tests/typed_ogham.rs` still
  pass — they exercise `check_schemas_match` end-to-end and would
  catch any regression in the refactor.
- New backend tests pass.

**Estimated complexity: low.** ~1 day. Mostly mechanical refactor
of an existing function plus type-shuffling.

---

### P0-M5 — `ogham check` CLI

**Goal:** ship the `ogham check` subcommand that runs the diagnostic
backend against a `.ogh` file and prints results in `cargo`-shaped
text. Exits non-zero on ERROR diagnostics.

**Files touched:**
- `Cargo.toml` — new `[[bin]]` entry: `name = "ogham"`,
  `path = "src/cli/main.rs"`.
- `src/cli/main.rs` (new) — entry point, subcommand dispatch.
- `src/cli/check.rs` (new) — the `check` subcommand body.
- `src/cli/render.rs` (new) — text rendering of `Diagnostic` ->
  `cargo`-style output.

**CLI shape:**

```
ogham check <path-to-ogh-file>
ogham check --all                        # walk cwd for .ogh files
ogham check <path> --workspace <dir>     # override workspace root
ogham check <path> --no-staleness-check  # suppress staleness warnings
```

**Workspace discovery:**

1. Resolve workspace root: `--workspace` arg if given; otherwise
   walk up from the .ogh file's directory looking for a `Cargo.toml`
   with `[workspace]`, then for any `Cargo.toml` (single-crate
   case).
2. Enumerate workspace members from the discovered Cargo.toml
   (using `cargo_metadata` crate — adds one dep, but it's the
   right tool).
3. For each member, glob `<member>/target/ogham/*.json` and load
   each manifest.
4. Group manifests by absolute `ogh_module` path.

**Match + run:**

For the requested `.ogh` file (or each file under `--all`):
1. Parse → `ModuleSchema`.
2. Look up manifests by absolute path.
3. For each `(state, events)` pair (or singleton) sharing a
   `binding_id`, call `check_against_manifest`.
4. Add staleness-check diagnostics (P1-M3 logic, lifted into the
   shared backend in this merge — used by both CLI and LSP).
5. Render to text; print.

**Text rendering** mirrors `cargo --message-format=short`:

```
data/engine/ui/chest_ui.ogh:14:5: error[ogham:binding]: host_state field
  `selecte` declared in .ogh but missing from Rust struct
  `untold_lore::ui::chest::ChestUiState`
  --> src/ui/chest_ui.rs:42:10
  help: did you mean `selected`?
```

ERROR / WARNING / INFO color via `nu-ansi-term` or `colored`
(either fine; pick whichever ogham already uses if any). Falls back
to plain text when stdout isn't a TTY.

**Exit codes:**
- 0 — no ERROR diagnostics (warnings + infos OK).
- 1 — at least one ERROR diagnostic.
- 2 — usage error (bad path, missing file, etc.).

**Tests** (`tests/cli_check.rs`):
- Fixture: a small `tests/fixtures/cli_check_workspace/` tree with
  one `.ogh`, one Rust crate, one pre-baked `target/ogham/*.json`
  manifest pair.
- Run `ogham check fixture.ogh` → expect clean exit, no output.
- Mutate the fixture manifest to disagree → expect exit 1, expect
  text output containing the binding ID and the offending field.
- `--all` walks cwd correctly.
- Staleness fixture: manifest mtime older than .rs mtime → expect
  WARNING in output.

**Validation gate:**
- `cargo build --workspace --bin ogham` clean.
- All CLI tests pass.
- Manual smoke: in untold_lore, `ogham check
  data/engine/ui/chest_ui.ogh` runs without crashing (full
  validation comes in P0-M6).

**Estimated complexity: medium.** ~2-3 days. Bulk of the work is
workspace discovery + cargo-metadata wrangling + text rendering.

---

### P0-M6 — Untold Lore validation

**Goal:** prove the round-trip works against a real consumer.
Lives in the `untold_lore` repo, not in `ogham`.

**Steps (consumer-side):**

1. Annotate the chest_ui binding pair:
   ```rust
   #[derive(OghamState)]
   #[ogham(binding_module = "data/engine/ui/chest_ui.ogh")]
   pub struct ChestUiState { /* ... */ }

   #[derive(OghamMsg)]
   #[ogham(binding_module = "data/engine/ui/chest_ui.ogh")]
   pub enum ChestUiMsg { /* ... */ }
   ```
2. Run `cargo build` in untold_lore.
3. Verify `target/ogham/state-data_engine_ui_chest_ui_ogh-ChestUiState.json`
   and `events-…-ChestUiMsg.json` exist with the expected content.
4. Run `ogham check data/engine/ui/chest_ui.ogh` from the untold
   lore root — expect clean exit, no output.
5. Synthetic drift test: rename a field in `ChestUiState`. Run
   `cargo build`. Run `ogham check`. Expect ERROR diagnostic
   pointing at the .ogh field that no longer matches.
6. Wire `ogham check --all` into untold lore's validation pipeline:
   either as a `cargo test` step (run from a small Rust integration
   test that shells out) or as a pre-commit hook.

**Validation gate** for declaring Phase 0 done:
- The drift in step 5 is caught by `ogham check` *before*
  launching the binary.
- The pre-commit / CI integration in step 6 fails red on the same
  drift.

This isn't a code change in `ogham`; it's the smoke test that
proves Phase 0 delivered the agent-ergonomics promise. Document the
result on `MEMORY.md`'s schema-diagnostics entry once it lands.

**Estimated complexity: low.** ~½ day, mostly mechanical.

---

## Phase 1 — LSP integration

### P1-M1 — Manifest discovery + caching

**Goal:** `ogham-lsp` learns to find and cache binding manifests at
startup and refresh them when they change on disk.

**Files touched:**
- `src/lsp/manifests.rs` (new) — `ManifestRegistry` with
  `discover()`, `get_for_module(path)`, `refresh_one(path)`.
- `src/lsp/server.rs` — `OghamLanguageServer` gains a
  `manifests: Mutex<ManifestRegistry>` field; `initialize()`
  populates it.
- File-watch wiring — extend the existing `notify` crate usage
  (already used for hot-reload) to also watch each
  `target/ogham/` directory found at discovery.

**`ManifestRegistry` shape:**

```rust
pub struct ManifestRegistry {
    /// Keyed by absolute path of the .ogh module.
    by_module: HashMap<PathBuf, Vec<Manifest>>,
    /// Watched directories (for cleanup on shutdown).
    watched_dirs: Vec<PathBuf>,
}

impl ManifestRegistry {
    pub fn discover(&mut self, workspace_folders: &[Url]);
    pub fn refresh_one(&mut self, manifest_path: &Path);
    pub fn get_for_module(&self, module_abs_path: &Path) -> &[Manifest];
}
```

**Discovery walk:**
1. For each workspace folder URL, convert to filesystem path.
2. Walk the directory tree; for each `Cargo.toml`, resolve via
   `cargo_metadata` and find the target dir.
3. Glob `<target>/ogham/*.json`; load each via the P0-M2 reader.
4. Insert into `by_module` keyed on absolute path of `ogh_module`
   (joined relative to the manifest's owning crate root).

**Tests** (`tests/lsp_manifest_registry.rs`):
- Discover finds a single manifest in a single-crate workspace.
- Discover finds manifests across two workspace members.
- File-watch event triggers `refresh_one`; registry reflects the
  new content.
- Manifest file deletion removes it from the registry.

**Estimated complexity: low.** ~1-2 days.

---

### P1-M2 — Stage 5 binding diagnostics

**Goal:** wire the diagnostic backend through `collect_diagnostics`
in `ogham-lsp`. Schema mismatches show up in the editor's problems
pane.

**Files touched:**
- `src/lsp/server.rs` — `collect_diagnostics` gains a fifth stage
  that runs after the existing four when manifests exist for the
  open file.
- `src/lsp/diagnostic_convert.rs` (new) — convert
  `ogham::diagnostics::Diagnostic` to `tower_lsp::Diagnostic`,
  including `related_information` for Rust source spans.

**Stage 5 in `collect_diagnostics`:**

```rust
// 5. Binding-mismatch diagnostics.
let module_path = uri_to_path(&doc.uri);
let manifests = registry.get_for_module(&module_path);
if !manifests.is_empty() {
    let by_binding = group_by_binding_id(manifests);
    for (binding_id, (state, events)) in by_binding {
        let backend_diags = ogham::diagnostics::check_against_manifest(
            &doc.schema, state, events, &binding_id,
        );
        for d in backend_diags {
            diagnostics.push(diagnostic_convert::to_tower(d));
        }
    }
}
```

`group_by_binding_id` pairs state and events manifests that share a
binding-module + crate context (the manifest filename's binding-id
is identity here).

**Tests** (`tests/lsp_binding_diagnostics.rs`):
- Open a `.ogh` file with a known mismatching manifest fixture in
  the test workspace → assert diagnostic appears with expected
  `code = "ogham:binding"`, expected message, expected related-
  information location.
- Match: no diagnostic emitted.
- Multi-binding mismatch: diagnostic per binding with `binding_id`
  in the message.

**Estimated complexity: low.** ~1 day.

---

### P1-M3 — Stale-manifest WARNING

**Goal:** when a binding manifest's `rust_source.file` mtime is
newer than the manifest file's mtime, emit a single WARNING-severity
diagnostic at line 1 of the `.ogh` file.

**Files touched:**
- `src/diagnostics/check.rs` — staleness check (already added in
  P0-M5 if we lifted it then; otherwise here).
- `src/lsp/server.rs` — stage 5 hook calls staleness check.

**Logic:**

```rust
fn check_staleness(manifest_path: &Path, manifest: &Manifest) -> Option<Diagnostic> {
    let manifest_mtime = std::fs::metadata(manifest_path).ok()?.modified().ok()?;
    let rust_path = Path::new(&manifest.rust_source().file);
    let rust_mtime = std::fs::metadata(rust_path).ok()?.modified().ok()?;
    if rust_mtime > manifest_mtime {
        Some(Diagnostic { severity: Warning, code: "ogham:stale-manifest".into(), /* ... */ })
    } else {
        None
    }
}
```

The mismatch diagnostics still publish — staleness doesn't suppress
them. The warning is a side-channel cue.

**Tests:**
- Touching the .rs file after the manifest produces the warning.
- Manifest newer than .rs → no warning.
- Missing .rs file → no panic; warning is suppressed (or rendered
  with a hint to rebuild).

**Estimated complexity: low.** ~½ day.

---

### P1-M4 — "No manifest found" INFO hint

**Goal:** if a `.ogh` module declares `host_state {}` but no
manifest matches it, emit a single INFO diagnostic on line 1
suggesting the user build the consumer crate.

**Files touched:**
- `src/lsp/server.rs` — stage 5 emits the hint when
  `manifests.is_empty() && doc.schema.host_state.is_some()`.

**Message:**

```
info[ogham:no-binding]: no Rust binding manifest found for this module
  help: this is fine for loose-mode modules, but if you expect typed
  bindings, run `cargo build` in the consumer crate to generate one
```

Suppressible via the P1-M5 settings.

**Tests:**
- Strict module + empty registry → INFO diagnostic.
- Loose module (no host_state) + empty registry → no diagnostic.
- Strict module + matching manifest → no INFO diagnostic.

**Estimated complexity: low.** ~½ day.

---

### P1-M5 — Settings + per-document opt-out

**Goal:** users (and agents) can disable binding diagnostics either
globally per LSP session or per-file via a magic comment.

**Files touched:**
- `src/lsp/server.rs` — read `initializationOptions` in
  `initialize()`; honor the flag at stage 5.
- `src/lsp/document.rs` — scan source for the pragma at line 1;
  cache the result on `Document`.

**Initialization options:**

```jsonc
{
  "oghamBinding": {
    "checkEnabled": true,           // default
    "showStalenessWarnings": true,  // default
    "showNoManifestHint": true      // default
  }
}
```

**Per-document pragma:**

```ogh
// @ogham:binding-check off

host_state { … }
```

Only honored when found within the first 10 lines (avoids
mid-file pragmas being invisible at distance).

**Tests:**
- `checkEnabled = false` suppresses all stage-5 output.
- Pragma suppresses for the specific document only.
- Other documents in the same workspace remain checked.

**Estimated complexity: low.** ~1 day.

---

### P1-M6 — Hover affordance

**Goal:** hovering a `host_state` field name (or event name in
`events {}`) renders, alongside the existing `TypeRef` info,
"bound to `<RustType>::<field>` at `<file>:<line>`" pulled from the
manifest registry.

**Files touched:**
- `src/lsp/hover.rs` — extend `hover_at` to optionally consult the
  manifest registry. Currently the function takes `&Document` and
  position; it'll need access to the registry too. Easiest: pass
  `&ManifestRegistry` as a parameter and update the call site in
  `server.rs`.

**Hover content extension:**

For a host_state field at cursor, the existing markdown looks like:

```markdown
**field**: `selected`
**type**: `Int`
```

After P1-M6:

```markdown
**field**: `selected`
**type**: `Int`

**Bound to**:
- `untold_lore::ui::chest::ChestUiState::selected` ([src/ui/chest_ui.rs:45](file:///.../src/ui/chest_ui.rs#L45))
```

When multiple bindings target the module, list all of them.

**For event names** in the `events {}` block, similar treatment:

```markdown
**event**: `TakeItem(Int)`

**Bound to**:
- `untold_lore::ui::chest::ChestUiMsg::TakeItem` ([src/ui/chest_ui.rs:78](…))
```

**Tests** (`tests/lsp_binding_hover.rs`):
- Hover on a host_state field with a matching manifest → markdown
  contains the bound Rust path and a clickable file URL.
- Hover on a host_state field with no manifest → unchanged
  (existing TypeRef-only hover).
- Hover on an event name → bound message variant appears.
- Multi-binding → all variants listed.

**Validation gate** for Phase 1 done:
- Open a `.ogh` file in any LSP-aware editor.
- Edit a host_state field name to a typo → ERROR diagnostic appears
  in the problems pane within one keystroke debounce.
- Hover a valid field → hover shows bound Rust type and a clickable
  source location.

**Estimated complexity: low.** ~1 day.

---

## After Phase 1

The whole stack lands at twelve merges. Beyond that, two natural
follow-ups (not in this plan, but called out so they're not lost):

- **Cross-module record validation against `OghamRecord` derives.**
  The schema resolver already validates `import` chains within the
  `.ogh` ecosystem; checking that an imported record matches the
  Rust `OghamRecord` it pairs with is a clean extension. Reuses the
  manifest format with a `record` kind.
- **`ogham init-binding`** — generate a `.ogh` skeleton from a Rust
  state struct, or vice versa. Mostly authoring affordance; punt
  until pain demands it.

Document those when they land or when picked up.
