# Typed Bindings — Untold Lore Validation Audit

> **Status: Working notes (in-progress).**
>
> This doc validates [`TYPED_BINDINGS.md`](TYPED_BINDINGS.md) against
> Untold Lore's 20 production Ogham UIs. It deliberately mixes
> Ogham-design findings with Untold-Lore-specific migration notes;
> a cleanup pass to separate the two will happen once Phase 1
> implementation is underway. The Untold-Lore-specific bits will
> probably move to a doc in that repo; the Ogham-design bits will
> fold into TYPED_BINDINGS.md or a sibling doc.
>
> **Methodology:** four parallel extraction passes covering 20
> UIs (17 in `src/ui/` plus map_editor_ui at the crate root, the
> menu inside `client/mod.rs`, and the escape menu inside
> `client/actions.rs`). For each UI: read the Rust wrapper and
> the matching `.ogh`; infer what `host_state {}`, `events {}`,
> and supporting `record` declarations would look like under
> typed bindings. Quantify boilerplate. Flag patterns that would
> not migrate cleanly.

---

## TL;DR

- **No fundamental design holes.** All 20 UIs are representable
  in the Phase 1 type universe as drafted. Every observed nested
  shape resolves to named records of depth ≤ 3. No heterogeneous
  arrays were found anywhere in production.
- **One real strict-mode migration blocker found:**
  `place_inspector.ogh::pill_btn` calls `event(evt, key)` with
  `evt` bound to a function parameter — this is a computed event
  name, forbidden in strict mode. Solvable with an inline
  refactor on the `.ogh` side; no Ogham design change needed.
- **One scope-pressure finding:** `map_editor_ui` has 147
  host_state fields, 108 event handlers, and uses pipe-delimited
  encoded event payloads (`"place|42|name_changed|..."`). The
  schema would be migratable but ugly; the encoded-payload
  pattern is a smell that suggests `map_editor` should be the
  *last* UI migrated, after the rest of the codebase has shaken
  out the typed-bindings ergonomics.
- **Three "future sum-type" patterns confirmed** (deferred, as
  expected): string-encoded enums (`kind: "input"|"output"|...`),
  string-encoded state machines (`menu_mode`, `target_mode`),
  and string-encoded "selected variant" fields. All compile fine
  as `string` in Phase 1 with no schema-level enum check; LSP
  hover surfaces the type but not the value set.
- **Three patterns the design handles cleanly across the board:**
  per-frame full state rebuilds (the typed `set_state` diff is a
  real perf win), `Arc<Mutex<FormState>>` shadow state (vanishes
  on the typed path), and multi-argument events
  (`accept_faction_request(string, int)`, etc.).
- **Cross-module record sharing matters more than expected.** At
  least 4 record types (`Item`/`InvItem`, `Skill`, `Player`,
  `Place`) are duplicated across 2–4 UIs each. The
  `import [Foo] from "./shared.ogh"` story is load-bearing for
  the migration to be coherent rather than a rats' nest of near-
  identical record decls.
- **Numeric-as-string draft pattern is universal** in form-heavy
  UIs (settings, sea_config, ruleset_editor, life_stages,
  map_editor, character_creation). The Phase 1 type universe
  expresses it as `string`; this works but invites a future
  `draft<float>` or `<field>_error: string?` convention.

---

## Cross-cutting findings

Numbered for citation in design-doc updates.

### F1. Computed event names exist in one production file (strict-mode blocker)

**Location:** `data/engine/ui/place_inspector.ogh` lines 24–42, the
`pill_btn` helper.

```ogh
let pill_btn = fn (label: string, is_active: bool, evt: string, key: string) {
  Flex {
    mouse_down: fn () { event(evt, key); },
    ...
  }
};
```

Strict-mode rule (TYPED_BINDINGS.md §"Event-call checking"):
*"`name` must be a string literal that matches a declared event.
Computed event names (`event(some_var, ...)`) are an error in
strict mode."*

**Migration options for Untold Lore:**
1. **Inline expansion** — replace the four `pill_btn(...)` call
   sites with explicit `Flex { mouse_down: fn () { event("set_arrival_mode", "ship_arrival"); }, ... }`
   blocks. Adds ~40 lines; loses one function abstraction.
2. **Per-event helpers** — define `arrival_pill_btn(label, is_active, key)` and
   `shop_pill_btn(label, is_active, key)` that hardcode the event
   name. Keeps abstraction; small line cost.
3. **Refactor to a single dispatch event** —
   `set_pill(string, string)` taking `(group, key)`, with the
   game side switching on group. Trades one event for two
   string args; loses some Phase-1 type-safety value.

**Recommendation:** option 2. Keeps the closure shape, costs
~10 lines, no Ogham changes.

**Ogham design implication:** none. The rule is sound; the
production cost is small. **No action needed in TYPED_BINDINGS.md.**

### F2. Encoded multi-field event payloads (large UI scope concern)

**Location:** `src/map_editor_ui.rs` — events like
`set_property` take pipe-delimited strings:
`"place|42|name_changed|text_input|<value>"`. The Rust handler
splits the string and dispatches to the appropriate field
update. ~30 of the editor's 108 events use this pattern.

**Why it exists:** the map editor's "entity details panel" is a
generic component that renders different fields per
selected-entity-kind. To avoid creating
`set_place_name`, `set_place_description`, `set_resource_amount`,
… (~150 events), the UI funnels everything through one event
and parses the destination from the payload.

**Migration paths:**
1. **Accept the explosion** — declare the ~150 typed events.
   Largest-line-count change in the codebase but most precise.
2. **Multi-arg event** —
   `set_property(string, string, string, string)` for
   `(kind, id, key, value)`. Phase-1-compatible. Same parsing
   on Rust side; type system gains nothing over today.
3. **Defer migration of `map_editor_ui`** — keep it on the loose
   path until the rest of the codebase has migrated and (a) we
   know whether the explosion is tolerable in production, or (b)
   a future Ogham feature (Scenes, custom-widget property
   schemas in Phase 4) makes the right factoring obvious.

**Recommendation:** option 3. Defer `map_editor_ui` until last.
Carry on migrating the other 19 UIs; revisit map_editor's
factoring informed by what we learn.

**Ogham design implication:** none required for Phase 1, but
worth flagging for Phase 4 (custom widget property schemas) and
Shift B (Scenes) — both might offer a cleaner factoring than
multi-arg dispatch.

### F3. Comma-separated string payloads in event args (small smell)

**Location:** `src/ui/sea_config_ui.rs` — RGB color edits dispatch
events like `set_deep_color("0.10,0.25,0.50")` rather than three
separate float events or one `(float, float, float)` event.

**Migration paths:**
1. **Multi-arg event** — `set_deep_color(float, float, float)`.
   Phase-1 supported. Cleanest.
2. **Record arg** — `set_deep_color(RgbColor)` where
   `record RgbColor { r: float, g: float, b: float }`. Phase-1
   supported (records in event args). Cleanest if `RgbColor`
   appears elsewhere.
3. **Keep the comma-separated string** — `set_deep_color(string)`
   and parse Rust-side. Works but loses the type win.

**Recommendation:** option 1 or 2 during migration.

**Ogham design implication:** none. Already supported.

### F4. String-encoded enums (deferred sum-type case, tracked)

Counted instances:
- `console`: `kind: "input" | "output" | "error"`
- `place_inspector`: `arrival_mode`, `shop_type`, `selected_kind`
- `settings`: `window_mode`, `default_camera`
- `dm_hud`: `kind`, `burden_zone`, `hunger_label`, `thirst_label`
- `character_creation`: `target_mode` (state machine)
- `character_select`: `kind` (per Adventure)
- `inventory_hud`: `burden_zone`, `shop_type`
- `ruleset_editor`: `mode` (per Ability), `active_tab`
- `map_editor`: `active_tab`, `editor_tool`, `brush_mode`,
  `canvas_mode`, `active_blend_mode`, `selected_constant_mode`,
  `selected_math_op`, `feature_create_type`, `selected_feature_type`,
  `selected_feature_arrival_mode`, `selected_place_origin`, …
  (15+ instances)
- `main menu`: `menu_mode` (the big one — drives 7+ screens)
- `escape_menu`: implicitly `confirm_disconnect: bool` is the
  only flag; no string enum

**Phase 1 behavior:** all of these declare as `string`. The
schema validates the field is present and that it's a string;
it does not validate the value set. The LSP hover shows
`string`. Runtime crashes on invalid values are still possible
but no worse than today.

**Future work:** when `enum`/sum types land, every one of these
would be revisited. Each migration is mechanical (declare an
enum, replace the `string` field with the enum type, replace
match arms in `.ogh` with the enum's variant tags).

**Ogham design implication:** none for Phase 1. Worth noting in
TYPED_BINDINGS.md's "Out of scope" list as the most-felt
deferral. Done already.

### F5. Numeric-as-string draft pattern (universal in form UIs)

Forms preserve in-progress text by storing user input as `String`
on the Rust side and displaying it back through host_state as
`string`. Affected fields:
- `settings_ui`: 5 numeric fields (volumes, sensitivity, fov)
- `sea_config_ui`: 18 numeric fields (water properties, RGB
  channels)
- `ruleset_editor_ui`: ~10 numeric fields (xp, growth rate,
  costs, levels)
- `life_stages_editor_ui`: ~6 numeric fields per choice (currency,
  item counts, skill levels)
- `map_editor_ui`: ~30 numeric fields (constants, blur, sea level,
  brush radii, etc.)
- `character_creation_ui`: 1 implicit (trait point cost)
- `place_inspector_ui`: 1 (`place_discovery_radius`)

**Why:** `"0."` is a legitimate in-progress value during user
typing. Parsing to `f32` mid-edit either snaps the visible text
back to `"0"` or to `NaN`; both are user-hostile.

**Phase 1 behavior:** declare these as `string`. Validation
errors flow through companion fields (e.g. `base_xp: string` +
`base_xp_invalid: bool`).

**Future work options** (out of scope for Phase 1):
- A `draft<float>` type that round-trips the string but offers
  Rust-side accessors for "parsed value or None" and "raw
  string."
- A pair convention: `field: string` paired with `field_error:
  string?` always declared together; possibly enforced.
- Just leave it as is — the convention works, costs no Ogham
  feature.

**Ogham design implication:** none required. Worth noting in the
migration guide ("for form fields you want raw text preserved,
declare as `string`, even when the underlying value is numeric").

### F6. Per-frame full state rebuild — typed `set_state` diff is a real win

Every UI audited rebuilds its full host_state map on every
frame, regardless of whether anything changed. Some use
`inject_host_state_if_changed` per field (DmHudUI is the
exemplar; partial usage in a few others). Most just call
`inject_host_state` unconditionally and rely on the runtime to
diff per-key.

`TypedOgham::set_state` diffs the whole struct internally before
calling `inject_host_state_if_changed` per field. Two wins:
- **No-op frames trigger zero rerenders** (already true today
  in the partial-adopters; will be universal after migration).
- **One call per frame** instead of N. Cleaner update loops.

**Quantitative:** the per-frame `inject_state` / `push_state`
helpers across 20 UIs make a combined ~250 `inject_host_state`
calls per frame across the whole client. Post-migration: 20
`set_state` calls per frame, each diffing internally.

**Ogham design implication:** validates the design as drafted.
No change.

### F7. `Arc<Mutex<FormState>>` shadow state vanishes on typed path

Confirmed in: `console_ui.rs` (input buffer),
`settings_ui.rs` (15-field form state), `sea_config_ui.rs`
(20-field form state), `social_ui.rs` (5-field UI-local state),
`character_creation_ui.rs` (`Arc<Mutex<CreationState>>`),
`character_select_ui.rs` (`Arc<Mutex<Option<CharacterId>>>`),
`map_editor_ui.rs` (large `Arc<Mutex<EditorFormState>>`).

The pattern exists today only because event handlers must be
`Fn(&[Value]) -> Result<Value, String> + Send + Sync + 'static`
and therefore can't hold `&mut Self`. The shadow gives them
*something* mutable to write to.

`TypedOgham`'s MPSC-based `poll_msg`/`drain_msgs` model
eliminates this entirely: the consumer drains messages in their
normal update loop with full mutable access to game state.

**Quantitative:** ~7 of the 20 UIs use this pattern. Migration
removes a `Arc<Mutex<...>>` clone from every event handler
closure (typically 5–30 closures per UI), shrinking each UI by
20–60% in line count.

**Ogham design implication:** validates the design's core
motivation. No change.

### F8. Heterogeneous arrays — none found

Phase 1 cannot represent a `Vec` of mixed-shape items
(`record A | record B`). The audit deliberately looked for these.

**Findings:** zero instances. Every observed `Vec<Value>` field
is uniform. The closest case is `character_select.adventures`
where each `Adventure` has a `kind: string` discriminator that
drives different button rendering — but the *struct* is uniform;
only its presentation varies.

**Ogham design implication:** Phase 1's omission of sum types is
safe. No production code is blocked.

### F9. Nesting depth stays ≤ 3 across all UIs

The deepest observed structure is in `character_creation_ui`:
`host_state → array<CommittedRow> → CommittedRow.lines:
array<string>`. Depth 3.

Most UIs are depth ≤ 2 (`host_state → array<Record>` where
Record fields are scalars or `array<string>`).

**Ogham design implication:** the Phase 1 type universe handles
production complexity. No pressure to add structural-sharing or
generic record forms.

### F10. Records likely shared across modules (forces import-records story)

Cross-module candidates:

| Record | Used in |
|---|---|
| `InvItem` / `Item` / `Cell` | `inventory_hud`, `dm_inventory`, `chest`, `crafting`, `character_creation` (review) |
| `Skill` / `SkillEntry` / `SkillRequirement` | `inventory_hud`, `talents`, `character_creation`, `ruleset_editor` |
| `Player` / `PlayerInfo` | `inventory_hud`, `character_select`, `social`, `dm_hud` |
| `Place` / `PlaceKind` / `PlaceEntry` | `place_inspector`, `map_editor` |
| `MemberInfo` / `JoinRequestInfo` | `social` (faction & party share) |
| `Adventure` | `character_select`, `main menu` |
| `Ruleset` / `Campaign` | `main menu`, `ruleset_editor` |
| `Trait` / `TraitOption` | `character_creation`, future `talents` cross-ref |
| `RgbColor` (proposed) | `sea_config`, `map_editor` (constant nodes), future visual editors |

Without cross-module record imports, each UI would re-declare
its own `record InvItem { name, x, y, w, h, ... }` with the same
shape (and risk drift). With them, a `data/engine/ui/shared.ogh`
or per-domain modules
(`data/engine/ui/inventory_types.ogh`,
`data/engine/ui/social_types.ogh`) become natural.

**Ogham design implication:** the
`import [Item] from "./shared.ogh"` story specified in
TYPED_BINDINGS.md §"`ModuleSchema` tenets" is load-bearing, not
a nice-to-have. Confirms the design choice. Open implementation
question O3 (alias syntax for import conflicts) is real — `Item`
will collide with itself when imported from two specialized
modules.

### F11. Multi-argument events are common

Counted: 13 of the 20 UIs have at least one event with ≥ 2
arguments. Examples:
- `social`: `accept_faction_request(string, int)`,
  `deny_faction_request(string, int)`,
  `kick_party_member(int)`
- `inventory_hud`: `inv_cell_click(int, int)`,
  `shop_buy(string, string)`,
  `shop_sell_item(string, int, int)`
- `dm_inventory`: `dm_inv_take(int, int)`
- `place_inspector`: most events take a single string but
  several take key+value pairs
- `character_creation`: `jump_to_stage(int, string)`
- `ruleset_editor`: `skill_id_changed(string, string)` etc. —
  many `(idx, value)` shapes
- `life_stages_editor`: 13+ events of shape `(kind, id, ...,
  value)`
- `map_editor`: many

**Ogham design implication:** Phase 1's `events { name(T1, T2,
...) }` syntax handles all observed cases. Confirmed as drafted.

### F12. `Self`-references not used; `map<int, T>` not used

No tree-shaped data structures appear in any UI's host_state.
All maps observed are `map<string, T>` (e.g.
`settings.keybinds: map<string, string>`).

**Ogham design implication:** confirms `Self` syntax is
nice-to-have and not load-bearing for Phase 1. Confirms
deferring `map<int, T>` (open question O1 in TYPED_BINDINGS.md)
is safe.

### F13. Optional fields are usually represented as empty strings or zeros today

Examples:
- `place_inspector_ui.place_discovery_radius: string` — `""`
  means "use kind default"
- `character_creation_ui.prefilled_backstory: string` — `""`
  means "no prefilled backstory"
- `character_select_ui.selected_character_id: string` — `""`
  means "no selection"
- `place_inspector_ui.selected_kind: string` — `""` means
  "nothing selected"
- `dm_hud.selected_entity` — an empty map for "no selection"
- `ruleset_editor.selected: AbilityDetail` — empty map for
  "no selection"

**Migration choice:** keep these as `string` / record (no
behavior change), or switch to `T?` / `Record?` for cleaner
semantics. Either works in Phase 1; the latter forces
`match selected { Some(s) => …, None => … }` where today's code
uses `match (selected_id != "") { … }`.

**Recommendation:** migrate the easy ones to `T?` opportunistically
during their UI's typed-bindings migration; don't force it.

**Ogham design implication:** none.

### F14. Module-scoped `state` declarations work fine alongside typed host_state

`inventory_hud.ogh` lines 13–15:

```ogh
state hovered_inv_x = -1;
state hovered_inv_y = -1;
state hovered_shop_type = "";
```

These persist across rerenders, are mutated from event handlers
in `.ogh`, and never touch host_state. The typed boundary doesn't
care about them; they remain loose. Confirmed compatible across
all UIs that use this pattern (inventory_hud, character_creation,
several editors).

**Ogham design implication:** the typed boundary deliberately
covers only the Rust↔Ogham seam, not in-language state. Confirms
design scope as drafted.

### F15. Map editor's 147-field schema is the upper bound — also a scope-pressure signal

The map editor is by an order of magnitude the largest UI:
- 147 host_state fields
- 108 events
- 2906 lines of Rust wrapper
- 5 sidebar `.ogh` files (largest is 1680 lines)

Migration is mechanical but creates a 147-field `EditorState`
struct and a 108-variant `EditorMsg` enum on the Rust side, both
of which are auditable but not pleasant.

**Recommendation:** migrate `map_editor_ui` *last* and use the
migration as the forcing function for Shift B (Scenes) — the
natural factoring is one Scene per sidebar, each with its own
~30-field schema.

**Ogham design implication:** Phase 1 is not blocked by this;
but Shift B's priority is reinforced.

---

## Ogham design implications, distilled

What this audit changes about the Phase 1 design:

| Finding | Action on TYPED_BINDINGS.md |
|---|---|
| F1 (computed event names blocker) | Add a "migration cookbook" appendix entry on refactoring computed-name closures. No grammar change. |
| F2 (encoded payloads in map_editor) | Add a note under "Migration story" that very-large UIs may benefit from deferring until after Phase 4 / Shift B. |
| F3 (comma-string color payloads) | None; already supported via multi-arg events or record args. |
| F4 (string-encoded enums) | Already in "Out of scope" — perhaps elevate to a top-of-mind future-work bullet. |
| F5 (numeric-as-string drafts) | Add a "common patterns" appendix entry confirming `string` is the right Phase 1 representation for raw form input. |
| F6 (per-frame state rebuild) | None; validates `set_state` design. |
| F7 (Arc<Mutex<FormState>> shadow) | None; validates the MPSC event channel design. |
| F8 (no heterogeneous arrays) | None; confirms sum-type deferral is safe. |
| F9 (nesting depth ≤ 3) | None; confirms type universe scope. |
| F10 (cross-module record sharing) | Confirms `import [Foo] from ...` is load-bearing. Open question O3 (alias syntax for import conflicts) confirmed real. |
| F11 (multi-arg events common) | None; already supported. |
| F12 (no Self, no map<int>) | Confirms Self is nice-to-have; confirms map<int> deferral safe. |
| F13 (optional-as-empty-string today) | Add migration-cookbook entry: `T?` is encouraged but not required. |
| F14 (module-scoped `state` orthogonal) | None; already covered in scope language. |
| F15 (map_editor at 147 fields) | Reinforces Shift B (Scenes) prioritization for post-Phase-1 work. |

**Net assessment of TYPED_BINDINGS.md:** no breaking changes
needed. Three appendix-style additions (migration cookbook
entries for F1, F5, F13). The doc's Phase 1 deliverables list is
correct and complete for what production code requires.

---

## Untold Lore migration impact

### Estimated line-count reduction by UI

Computed by counting `with_event_handler` chains, per-field
`set_host_state` / `inject_host_state` calls, `Arc<Mutex>` shadow
state setup, and accessor boilerplate (`ogham()` / `ogham_mut()`
/ `get_ui_mut()`) eliminated by the typed path.

| UI | Today (lines) | After (est.) | Reduction |
|---|---|---|---|
| `chest_ui.rs` | 53 | ~25 | 53% |
| `tip_log_ui.rs` | 70 | ~30 | 57% |
| `crafting_ui.rs` | 117 | ~50 | 57% |
| `console_ui.rs` | 117 | ~45 | 62% |
| `blueprint_editor_ui.rs` | 168 | ~75 | 55% |
| `talents_ui.rs` | 195 | ~90 | 54% |
| `dm_inventory_ui.rs` | 239 | ~110 | 54% |
| `dm_hud_ui.rs` | 301 | ~140 | 53% |
| `character_select_ui.rs` | 347 | ~165 | 52% |
| `place_inspector_ui.rs` | 327 | ~160 | 51% |
| `settings_ui.rs` | 341 | ~140 | 59% |
| `sea_config_ui.rs` | 202 | ~85 | 58% |
| `inventory_hud.rs` | 523 | ~260 | 50% |
| `social_ui.rs` | 578 | ~260 | 55% |
| `character_creation_ui.rs` | 783 | ~360 | 54% |
| `life_stages_editor_ui.rs` | 1005 | ~500 | 50% |
| `ruleset_editor_ui.rs` | 536 | ~260 | 51% |
| `map_editor_ui.rs` | 2906 | ~1900 | 35% |
| escape menu (in `actions.rs`) | ~70 | ~35 | 50% |
| main menu (in `client/mod.rs`) | ~1500 (slice) | ~750 | 50% |
| **Total** | **~10,378** | **~5,440** | **~48%** |

Roughly half of the Untold-Lore Ogham boilerplate disappears.
Excluding map_editor, the average reduction is ~54%.

### Required `.ogh` refactors (migration blockers)

1. **`place_inspector.ogh::pill_btn`** — refactor per F1 above.
   Estimated: 30 minutes of work.
2. **All files with `host_state {}`-eligible fields** — declare
   the schema. Mechanical work; LSP diagnostics will guide.
3. **Sea config color events** (per F3) — optional but
   recommended; rewrite RGB events to `(float, float, float)`
   or to use a `RgbColor` record.

No other strict-mode-incompatible patterns surfaced.

### Required Rust-side scaffolding (one-time)

1. New crate `ogham-derive` (or in-tree macro) for the three
   derives.
2. A small `Untold Lore-side trait `UIController`** can replace
   the per-UI `ogham()` / `ogham_mut()` / `get_ui_mut()` accessors
   by delegating through `TypedOgham::inner()`. Already feasible
   without the typed path — just easier to motivate now.
3. The `client/ui.rs` 69-line dispatch (`active_ogham`,
   `active_ogham_mut`, `get_ui_mut`) is unaffected by typed
   bindings; it remains the same. (Shift B is its eventual fix.)

---

## Suggested migration sequence

Smallest/cleanest first to flush bugs out of the macros and the
typed handle before tackling the gnarly cases.

| Order | UI | Why this slot |
|---|---|---|
| 1 | `chest_ui` | Empty-state smoke test (no host_state, 2 events). Forces the macro to handle the trivial case. |
| 2 | `tip_log_ui` | Smallest non-empty schema (2 fields, 1 event, 1 record). |
| 3 | `crafting_ui` | First real `OghamRecord` (CraftingRecipe), small surface. |
| 4 | `blueprint_editor_ui` | Scalar-only state (13 fields), no records — tests wide-flat state. |
| 5 | `talents_ui` | First nested-record (Talent + SkillRequirement), but tiny event surface. **The "gold standard" UI per the audit** — clean one-way data flow. |
| 6 | `dm_inventory_ui` | Tests `array<array<Cell>>` (nested arrays) and multi-arg event (`dm_inv_take(int, int)`). |
| 7 | `dm_hud_ui` | Tests `T?` Optional records (selected_entity becomes `EntityInspector?`). |
| 8 | `console_ui` | First `Arc<Mutex<String>>` shadow elimination — a clean before/after. |
| 9 | `escape_menu` | Trivial; tests the special-case construction site at `actions.rs`. |
| 10 | `character_select_ui` | Multi-screen state via flags (10 events, 9 fields, 3 records). |
| 11 | `place_inspector_ui` | **Requires `pill_btn` refactor first.** Otherwise straightforward. |
| 12 | `tip_log` cross-check | (Already done in slot 2 — re-verify after macro maturation.) |
| 13 | `settings_ui` | First large form (15 fields, 13 events, no records). Tests the "form state shadow disappears" claim at scale. |
| 14 | `sea_config_ui` | RGB-event refactor (F3) opportunity. |
| 15 | `social_ui` | First large nested-records UI (5 record types, multi-arg events). |
| 16 | `inventory_hud` | Largest single-screen UI (~30 fields, 4 records). Tests cross-module record imports if migrated alongside chest/dm_inventory. |
| 17 | `character_creation_ui` | Deepest nesting (depth 3), wizard state machine. |
| 18 | `life_stages_editor_ui` | Heavily nested grants, dynamic toggles. |
| 19 | `ruleset_editor_ui` | 26 fields, 8 records, 40+ events. |
| 20 | `main menu` | 94 fields, 55 events, 2085 `.ogh` lines. State-machine-by-string is felt here. |
| 21 | `map_editor_ui` | **Defer until after Shift B work begins.** 147 fields, 108 events, encoded payloads — wait for Scenes to factor. |

Migrating items 1–10 first lets the macro story stabilize before
the high-line-count UIs in 13–20. Item 21 is intentionally last
and may stay loose indefinitely.

---

## Open questions raised by the audit

1. **Is there a "draft<float>" type in our future?** F5 shows
   the pattern is universal in form UIs. Phase 1 just declares
   `string`; this works. But as we accumulate experience, a
   first-class `draft<T>` (raw text + parsed value + validation
   error) might be worth a small language addition. Not a Phase
   1 question.

2. **How aggressive should the migration cookbook be about
   Optional adoption?** F13 shows ~10 fields where `T?` would be
   semantically cleaner than the empty-string sentinel. Should
   the migration guide encourage this opportunistically, or
   prescribe it? Recommend: encourage, don't prescribe.
   Migration is cheaper if shape stays the same.

3. **Do we need `inject_host_state_if_changed` semantics
   exposed on the typed path?** Today's `set_state` does a
   field-level diff internally. If a caller wants to skip even
   the diff cost (e.g. they know nothing changed), the answer is
   "just don't call set_state." That seems fine.

4. **Can we run schema-extraction tooling against the existing
   `*_ui.rs` to bootstrap the schemas?** A small offline script
   that reads `set_host_state(...)` calls and emits a starter
   `host_state {}` block would shave hours off the migration.
   Not Phase 1 work but a candidate for after.

5. **Where do shared records actually live?** F10 confirms the
   need; the question is the file path. Candidates:
   - `data/engine/ui/types/inventory.ogh`
   - `data/engine/ui/types/social.ogh`
   - `data/engine/ui/types/world.ogh`
   - or one big `data/engine/ui/types.ogh`

   Untold-Lore-side decision; Ogham doesn't care.

6. **Does map_editor's encoded-payload pattern justify a
   first-class "tagged event" in Ogham?** The cleanest answer is
   probably "no, let Scenes solve it" — a per-entity-kind Scene
   gets per-kind events naturally. Ties to Shift B.

---

## Per-UI detail

Compact reference. Each UI gets the inferred schema, the
matching Rust derive sketch, and any UI-specific notes worth
keeping. Records that appear in multiple UIs are flagged with
**[shared candidate]**.

---

### `chest_ui` (53 lines + 36 .ogh)

```ogh
events {
  chest_pick_up(),
  chest_cancel(),
};
```

```rust
#[derive(OghamState, Default)] struct ChestState {}
#[derive(OghamMsg)] enum ChestMsg { ChestPickUp, ChestCancel }
```

Empty state, two scalar events. Migration smoke test.

---

### `tip_log_ui` (70 + 88)

```ogh
record TipEntry { title: string, body: string };
host_state { tips: array<TipEntry>, tip_count: int };
events { close_tip_log() };
```

`tip_count` is redundant given `tips.length()`; harmless.

---

### `crafting_ui` (117 + 100)

```ogh
record CraftingRecipe { id: string, name: string, inputs_text: string, can_craft: bool };
host_state {
  station_name: string,
  recipes: array<CraftingRecipe>,
  is_pending: bool,
};
events {
  craft_recipe(string),
  close_crafting(),
};
```

`is_pending` was a clock-driven boolean recomputed each frame
on the Rust side; clean separation from the .ogh.

---

### `console_ui` (117 + 105)

```ogh
record ConsoleEntry { text: string, kind: string };  // kind: future enum
host_state {
  history: array<ConsoleEntry>,
  history_count: int,
  input_value: string,
};
events {
  console_input_changed(string),
  console_close(),
};
```

The `Arc<Mutex<String>>` input shadow on the Rust side
disappears entirely.

---

### `blueprint_editor_ui` (168 + 303)

```ogh
host_state {
  blueprint_id: string, display_name: string,
  tool: string,                 // "select" | "draw_wall"
  structure_count: int,
  selected_kind: string,        // "" or "structure"
  selected_structure_id: string,
  selected_vertex_count: int,
  draft_active: bool, draft_vertex_count: int,
  can_undo: bool, can_redo: bool,
  dirty: bool, pending_discard: bool,
  error: string,
};
events {
  save_and_exit(), discard(),
  set_tool(string),
  display_name_changed(string),
  commit_draft(), cancel_draft(),
  delete_selected(),
  undo(), redo(),
};
```

13 scalars, no records. Cleanest "wide flat state" test case.

---

### `talents_ui` (195 + 154) — **gold standard**

```ogh
record SkillRequirement { text: string, met: bool };
record TalentCard {
  id: string, name: string, description: string,
  requirements: array<SkillRequirement>,
  meets_requirements: bool, owned: bool,
  can_learn: bool, blocked_reason: string,
};
host_state {
  char_level: int,
  talent_points_available: int,
  respec_points_available: int,
  talents: array<TalentCard>,
};
events {
  close_talents(),
  learn_talent(string),
  respec_talent(string),
};
```

No form state, all derived data computed Rust-side, clean
event surface. The audit's exemplar of how typed bindings
should feel.

---

### `dm_inventory_ui` (239 + 161)

```ogh
record Cell {
  cx: int, cy: int,
  kind: string,                 // "empty" | "origin" | "covered"
  name: string, w: int, h: int, // "only meaningful when kind == origin"
};
host_state {
  target_name: string,
  target_kind: string,
  inv_width: int, inv_height: int,
  rows: array<array<Cell>>,
  grant_buffer: string,
};
events {
  dm_inv_close(),
  dm_inv_take(int, int),
  dm_inv_grant_input(string),
  dm_inv_grant_submit(),
};
```

Tests `array<array<Cell>>` and a 2-arg int event. **Cell record
is similar to but not identical to `inventory_hud`'s `InvItem`** —
keep separate.

---

### `dm_hud_ui` (301 + 245)

```ogh
record EntityInspector {
  name: string,
  kind: string,                 // "Player Character" | "NPC" | ...
  position_text: string,
  detail_lines: array<string>,
  can_possess: bool,
  is_possessing: bool,
  can_open_inventory: bool,
};
host_state {
  paused: bool,
  selection_count: int,
  selected_entity: EntityInspector?,    // was empty-map sentinel
};
events {
  dm_toggle_pause(),
  dm_open_inventory(),
  dm_possess(),
  dm_release(),
  dm_deselect(),
};
```

`selected_entity` is a clean F13 candidate to migrate to `T?`.

---

### `character_select_ui` (347 + 495)

```ogh
record Character { id: string, name: string, backstory_title: string };
record Map { id: string, name: string, live: bool, player_count: string };
record Adventure { id: string, name: string, kind: string };
host_state {
  account_display_name: string,
  characters: array<Character>,
  confirm_disconnect: bool,
  server_error: string,
  maps: array<Map>,
  adventures: array<Adventure>,
  campaign_name: string,
  selected_character_id: string,
  is_pending: bool,
  can_run_as_dm: bool,
};
events {
  select_character_row(string), delete_character(string),
  create_character(),
  prompt_disconnect(), cancel_disconnect(), disconnect(),
  travel_to_map(string),
  join_adventure(string), host_adventure_from_hub(string),
  run_as_dm(),
};
```

`Character`, `Map`, `Adventure` are **shared candidates** with
`main menu`.

---

### `place_inspector_ui` (327 + 193) — **requires pill_btn refactor**

```ogh
record PlaceKind { id: string, display_name: string };
host_state {
  place_id: string, place_name: string,
  place_kind: string, place_kind_display: string,
  place_description: string, place_notes: string,
  place_discovery_radius: string,           // F5 numeric draft
  place_has_spawn: bool, place_has_shop: bool, place_has_skiff_dock: bool,
  place_arrival_mode: string,               // future enum
  place_shop_type: string,                  // future enum
  place_kinds: array<PlaceKind>,
};
events {
  inspector_save(), inspector_close(),
  inspector_set_name(string),
  inspector_set_kind(string),
  inspector_set_description(string),
  inspector_set_notes(string),
  inspector_set_discovery_radius(string),
  inspector_toggle_attachment(string),
  inspector_set_arrival_mode(string),
  inspector_set_shop_type(string),
};
```

`PlaceKind` is a **shared candidate** with `map_editor`.

---

### `settings_ui` (341 + 294 + 190 components)

```ogh
host_state {
  master_volume: string, music_volume: string,
  sfx_volume: string, ui_volume: string,        // F5 numeric drafts
  mouse_sensitivity: string,
  invert_y: bool,
  window_mode: string, resolution: string, fov: string,
  resolution_choices: array<string>,
  default_camera: string,
  show_fps: bool, show_tips: bool, show_zone_banners: bool,
  keybinds: map<string, string>,
  rebinding: string,
};
events {
  set_master_volume(string), set_music_volume(string),
  set_sfx_volume(string), set_ui_volume(string),
  set_mouse_sensitivity(string),
  toggle_invert_y(),
  set_window_mode(string), set_resolution(string),
  set_fov(string), set_default_camera(string),
  toggle_show_fps(), toggle_show_tips(), toggle_show_zone_banners(),
  rebind_action(string),
  close_settings(),
};
```

15-field form. Removes `Arc<Mutex<SettingsFormState>>` entirely.
First test of the `volume_handler` / `bool_toggle_handler`
factory collapse.

---

### `sea_config_ui` (202 + 176) — **F3 RGB-event refactor opportunity**

Today: 18 individual `_r`/`_g`/`_b` channel string fields,
events take `"r,g,b"` strings. Recommended migration:

```ogh
record RgbColor { r: float, g: float, b: float };
host_state {
  sea_level: string, turbulence: string,
  wave_scale: string, wave_speed: string, choppiness: string,
  deep_color: RgbColor, shallow_color: RgbColor,
  absorb_color: RgbColor, foam_color: RgbColor,
  foam_intensity: string, foam_width: string,
};
events {
  set_sea_level(string), set_turbulence(string),
  set_wave_scale(string), set_wave_speed(string),
  set_choppiness(string),
  set_foam_intensity(string), set_foam_width(string),
  set_deep_color(float, float, float),
  set_shallow_color(float, float, float),
  set_absorption(float, float, float),
  set_foam_color(float, float, float),
  save_sea_config(), close_sea_config(),
};
```

`RgbColor` is a **shared candidate** with `map_editor`'s constant
nodes.

---

### `inventory_hud` (523 + 760)

```ogh
record Player { name: string };
record InvItem {
  name: string, x: int, y: int, w: int, h: int,
  description: string, weight: int, item_type: string,
  size_text: string, sell_price_text: string,
  can_sell: bool, dormant: bool,
};
record ShopItem {
  name: string, item_type: string, price_text: string,
  description: string, weight: int, size_text: string,
  can_afford: bool,
};
record Skill { name: string, level: int, xp: int, xp_needed: int };

host_state {
  show_inventory: bool,
  player_count: int,
  players: array<Player>,
  tool_name: string, has_tool: bool,
  inv_width: int, inv_height: int,
  inv_items: array<InvItem>,
  skills: array<Skill>,
  stamina_visible: bool, stamina_fill_pixels: int,
  stamina_track_pixels: int, stamina_exhausted: bool,
  hunger_visible: bool, hunger_label: string, hunger_critical: bool,
  thirst_visible: bool, thirst_label: string, thirst_critical: bool,
  burden_visible: bool, burden_fill_pixels: int,
  burden_track_pixels: int, burden_zone: string,
  is_shopping: bool,
  shop_name: string, shop_type: string,
  shop_currency: string, currency_label: string,
  shop_buy_items: array<ShopItem>,
  shop_pending: bool,
};
events {
  inv_cell_click(int, int),
  unequip_tool(),
  close_shop(),
  shop_buy(string, string),
  shop_sell_item(string, int, int),
  shop_unequip_tool(),
  view_talents(),
};
```

`Player`, `Skill` are **shared candidates**.
`InvItem` is a **shared candidate** with `chest`/`dm_inventory`
but with diverging fields — keep separate or factor with care.

---

### `social_ui` (578 + 505)

```ogh
record PlayerInfo {
  id: int, name: string, has_party: bool,
  is_party_leader: bool, faction_count: int,
};
record MemberInfo { id: int, name: string };
record JoinRequestInfo {
  id: int, name: string,
  faction_id: string, faction_name: string,
};
record FactionRow {
  id: string, name: string, description: string,
  owner_label: string, member_count: int,
  is_authored: bool, is_pending: bool,
  members: array<MemberInfo>,
};
record PartyInfo { id: int, leader_name: string, member_count: int };

host_state {
  active_tab: string,                       // "faction" | "party"
  player_self_id: int,
  player: PlayerInfo,
  your_factions: array<FactionRow>,
  browse_factions: array<FactionRow>,
  my_faction_requests: array<JoinRequestInfo>,
  expanded_faction_id: string,
  faction_name_input: string,
  show_create_faction_form: bool,
  parties: array<PartyInfo>,
  my_party_requests: array<JoinRequestInfo>,
  selected_party_id: int,
  selected_party_members: array<MemberInfo>,
};
events {
  close_panel(), select_tab(string),
  faction_name_changed(string),
  show_create_faction(), hide_create_faction(),
  create_faction(string),
  toggle_expand_faction(string),
  request_join_faction(string),
  accept_faction_request(string, int),
  deny_faction_request(string, int),
  leave_faction(string),
  create_party(),
  select_party(int),
  request_join_party(int),
  accept_party_request(int),
  deny_party_request(int),
  leave_party(),
  kick_party_member(int),
};
```

Largest "clean fit" UI. Multi-arg events are well-tested here.

---

### `character_creation_ui` (783 + 573) — depth-3 nesting

```ogh
record CommittedRow {
  index: int, stage_id: string, stage_title: string,
  choice_label: string,
  lines: array<string>,                     // depth 3: array<Record>.lines: array<string>
  active: bool, locked: bool,
};
record Choice { id: string, label: string, description: string, selected: bool };
record TraitOption {
  id: string, name: string, description: string,
  cost: int, selected: bool,
};
record SkillGrant { skill_id: string, level: int };
record ItemGrant { item_type: string, count: int };

host_state {
  character_name: string,
  prefilled_identity: bool,
  prefilled_backstory: string,
  target_mode: string,                      // "stage" | "traits" | "review" — future enum
  committed_rows: array<CommittedRow>,
  traits_row_visible: bool, traits_row_active: bool,
  traits_row_names: array<string>,
  traits_row_cost: int,
  active_stage_title: string, active_stage_prompt: string,
  active_stage_choices: array<Choice>,
  active_has_pending: bool,
  pending_traits: array<TraitOption>,
  pending_trait_cost: int, trait_budget: int,
  pending_budget_valid: bool,
  review_backstory: string, review_currency: int,
  review_skills: array<SkillGrant>,
  review_items: array<ItemGrant>,
  review_tool: string, review_traits: array<string>,
  can_accept: bool, server_error: string,
};
events {
  set_name(string),
  select_choice(string),
  continue_stage(),
  toggle_trait(string),
  continue_traits(),
  jump_to_stage(int, string),
  jump_to_traits(),
  go_back_from_review(),
  accept(),
  cancel_character_creation(),
};
```

`SkillGrant`, `ItemGrant` are **shared candidates** with
`life_stages_editor`.

---

### `life_stages_editor_ui` (1005 + 93 + sidebar)

```ogh
record GrantItem { index: int, item_type: string, count: string };
record GrantSkill { index: int, skill_id: string, level: string };
record GrantTrait { index: int, value: string };
record Grants {
  currency: string,
  tool: string,
  items: array<GrantItem>,
  skills: array<GrantSkill>,
  traits: array<GrantTrait>,
};
record ChoiceEntry {
  id: string, label: string, description: string,
  string_fragment: string,
  grants: Grants,
  grants_collapsed: bool,
};
record StageOption { id: string };

host_state {
  selected_stage_id: string,
  selected_stage_title: string,
  selected_stage_prompt: string,
  selected_stage_choices: array<ChoiceEntry>,
  selected_stage_grants: Grants,
  selected_stage_grants_collapsed: bool,
  default_next_stage_options: array<StageOption>,
  selected_stage_default_next_stage: string,
  root_stage_id: string,
  is_root: bool,
  validation_message: string,
  validation_ok: bool,
  sidebar_width: float,
  header_label: string,
};
events {                                    // 26 total — see agent report
  new_stage(), delete_selected_stage(),
  add_choice(), remove_choice(string),
  set_stage_title(string), set_stage_prompt(string),
  set_choice_label(string, string),
  set_choice_description(string, string),
  set_choice_string_fragment(string, string),
  set_default_next_stage(string),
  toggle_grants_section(string, string),    // (kind: "stage"|"choice", id)
  set_grant_currency(string, string, string),
  set_grant_tool(string, string, string),
  add_grant_item(string, string),
  remove_grant_item(string, string, int),
  set_grant_item_type(string, string, int, string),
  set_grant_item_count(string, string, int, string),
  add_grant_skill(string, string),
  remove_grant_skill(string, string, int),
  set_grant_skill_id(string, string, int, string),
  set_grant_skill_level(string, string, int, string),
  add_grant_trait(string, string),
  remove_grant_trait(string, string, int),
  set_grant_trait(string, string, int, string),
  set_root_stage(string),
  exit_life_stages_editor(),
};
```

The `(kind, id, ...)` event shape is the cleanest expression of
F2's tagged-dispatch question; keeps the event count manageable
without resorting to encoded payloads. This is the "good"
version; map_editor is the "bad."

---

### `ruleset_editor_ui` (536 + 1019)

8 records, 26 host_state fields, 40+ events. Full schema in
agent report. Notable points:

- `selected: AbilityDetail` could be `AbilityDetail?` — F13
  candidate.
- Many `(idx, value)` event pairs for editing array entries —
  same pattern as life_stages_editor, scaled smaller.
- `Modifier` record is currently empty (effects UI not yet
  implemented); placeholder.

---

### escape menu (in `client/actions.rs`, ~70 lines + 142 .ogh)

```ogh
host_state { steam_enabled: bool, confirm_disconnect: bool };
events {
  close_escape_menu(),
  open_settings(), view_social(), view_friends(), invite_friends(),
  view_tip_log(),
  prompt_disconnect(), cancel_disconnect(), disconnect(),
};
```

Minimal. Tests construction at a non-`src/ui/` site.

---

### main menu (in `client/mod.rs`, ~1500 lines slice + 2085 .ogh)

94 host_state fields, 55 events. Full schema in agent report.
Notable design-validation points:

- `menu_mode: string` is a 7-state machine — the largest F4 sum-
  type case. Compiles fine as `string` in Phase 1.
- Many fields are state-mode-conditional: `new_campaign_*` only
  matters when `menu_mode == "new_campaign"`. Phase 1 declares
  them all; future sum-type work could attach them to the
  variant.
- `Campaign`, `Adventure`, `Ruleset` records are **shared
  candidates** with `character_select` and `ruleset_editor`.

---

### `map_editor_ui` (2906 + 155 + 5 sidebar files) — **defer migration**

147 host_state fields, 108 events, encoded payload events. See
agent report for partial schema. Migration recommendation:
**defer until after Shift B (Scenes) lands** — the natural
factoring is one Scene per sidebar (left, right, top, biome),
each with its own ~30-field schema. Migrating in the current
shape would produce a 147-field struct and a 108-variant enum,
both auditable but ugly.

---

## Quantitative summary

| Metric | Value |
|---|---|
| UIs audited | 20 |
| Total `with_event_handler` registrations across all UIs | ~410 |
| Total per-frame `inject_host_state*` calls | ~250 (per frame, across all UIs) |
| Total `Arc<Mutex<...>>` shadow-state instances | 7 |
| Total record types inferred (post-dedupe) | ~35 distinct |
| Total cross-UI shared record candidates | ~9 |
| Computed-event-name instances (strict-mode blockers) | 1 (place_inspector::pill_btn) |
| Heterogeneous arrays found | 0 |
| Maximum nesting depth observed | 3 |
| Self-referential record uses | 0 |
| `map<int, T>` uses | 0 |
| Estimated total Rust line reduction | ~48% (~5,000 lines across 20 UIs) |
| Average reduction per UI (excluding map_editor) | ~54% |

---

## Next steps

Once Phase 1 implementation begins:

1. Fold F1 / F5 / F13 migration cookbook entries into
   `TYPED_BINDINGS.md` (or a sibling `TYPED_BINDINGS_MIGRATION.md`).
2. Re-validate this audit's UI counts at implementation time —
   Untold Lore evolves continuously; the per-UI numbers here are
   a 2026-05-04 snapshot.
3. Move the per-UI detail and the migration sequence to a doc
   in the Untold Lore repo; keep the cross-cutting findings and
   design implications in Ogham.
4. Spike the `ogham-derive` macros against `chest_ui` (UI #1 in
   the migration sequence) to validate the empty-state path
   before tackling any other UI.

End of working notes.
