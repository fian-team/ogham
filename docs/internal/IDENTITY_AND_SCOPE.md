# Ogham — Identity & Scope (workshop draft)

> **Status: workshop draft. NOT a contract.** This captures a multi-session
> design reckoning about *what Ogham is for* and *what it should keep, cut, and
> defer*. It will be workshopped over several sessions and, once settled, will
> inform new tenets in [`INTENT.md`](INTENT.md) and the cut/keep sequencing.
> Until then, nothing here overrides existing contracts. Where this doc and the
> live tenets disagree, that disagreement is the *agenda*, not a directive.
>
> Decision legend used below:
> **[DECIDED]** — agreed this session, stable enough to build toward.
> **[LEANING]** — consensus direction, not yet pinned.
> **[OPEN]** — genuine fork, needs a future session.
>
> First drafted: 2026-06-07. Revised 2026-06-07 (the generic-editing reckoning:
> §3 de-universalized; §4 reframed around reading/writing Rust structs from one
> `editable` derive, with no second crate and no serde in the UI path). Revised
> 2026-06-08 (seam fully specified: nested node tree with uniform `children`, the
> `Reader` mirror of `apply`, the `$variant` discriminant, ambient reads, and the
> cut/build/migrate execution sequence — see §5). Revised 2026-06-08b (codebase
> reckoning, blockers 1–3: `runtime/schema.rs` / `parser/typed_bindings.rs` are
> split-not-delete — the `.ogh`-source schema stays; `$variant` blast radius
> verified — runtime flip is automatic, 23 *test* edit-paths need a schema-aware
> audit (11 in `editable` all-union, 12 in `content-core` of which 3 are
> record-field `.kind` traps that must not flip), no production/content change;
> `Reader` scoped to ~2 days with Design C —
> promotions inline at the field site, no read-walk cycle guard; execution sequence
> made repo-explicit across `ogham` / `lorekeeper` / `small_mercies`; write wire
> format pinned to six named events — four already proven in the frozen panes, +2
> for maps — with the `PaneAction`→`FieldOp` decode as a write-side survivor).
> Revised 2026-06-08c (pre-implementation verification pass — every `file:line`
> claim re-checked against all three repos: confirmed sound except four drifts now
> corrected — `text_input_widget` does **not** leak skia; the `compile_increment`
> bug is an *unconditional* `SetState` the local path guards, at `1249-1262`/1257;
> the `$variant` audit is **23** test paths not ~18, of which 3 `content-core`
> record-field `.kind` sites must not flip; the `OghamField` trait + a separate
> `FromOghamValue` belong in the schema.rs cut list. No design change — counts and
> pointers only. Cleared for implementation.)

---

## 1. The reckoning

Ogham has been quietly trying to be **two frameworks in one binary**:

- **A "JavaScript + React replacement" / standalone app platform.** This
  identity funds: local `useState`-style state, the `OghamState`/`OghamMsg`
  typed-binding layer (the React+TS analogue), `mutation()` request/response,
  the standalone `client` viewer with `Ctrl+O`, portals/modals, and the LSP. It
  presupposes *Ogham is the application* and the host is a thin shell.

- **A "UI framework for Rust applications."** This identity is host-state-in /
  events-out: the host owns all state and logic, and Ogham is a *projection
  surface* over it. Here the Rust domain model is the source of truth and the
  `.ogh` renders + emits.

**The framework ships both and has committed to neither.** That indecision is
the root cause of most of the weak spots found in the audit: unused surface that
still has to be maintained, two rival typed-state systems, and a dual state
model that confuses authors.

### What the consumers actually voted for

Revealed preference across the three real consumers (small_mercies ~7.7k `.ogh`
LOC, untold_lore ~19k, lorekeeper 146 — a fossil) is decisive:

| Signal | Finding |
|---|---|
| Typed bindings (`derive(OghamState/OghamMsg)`) | **1 site total** (UL `chest_ui.rs`); 0 in SM, 0 in LK |
| `ogham check` / strict validation in any build/CI | **none** |
| Local `state` declarations | **1 in SM, ~10 in UL** (several serving dead drag/hover surface) |
| Lifecycle hooks (`on_mount`/`on_unmount`/`effect`/`cleanup`) | **0** in real `.ogh` (only the test suite) |
| Portals | **0 SM / 1 UL**; SM overlays are plain stacked flex + frosted glass; 0 Rust portal calls |
| `svg` widget / `mutation()` / drag (`draggable`/`on_drag`) | **0** anywhere |
| The schema-driven `editable` derive | **spreading across all authored content** up to the `Content` root; an active refactor |

Meanwhile the *render + language core* is heavily used: `for` (~21 SM / 29 UL
files), `match` (27 / 40), imports (27 / 41), spread-as-conditional-children
(~50 in SM), function-components, closures-as-event-handlers (112 `on_*` in SM),
flex-with-wrap (5 / 8), `text_input` (29+ in UL).

The verdict writes itself: **when the developers hit the exact need the typed
React-style layer was meant to serve — typed, introspectable host state — they
built a *second, better* system (`editable`) outside Ogham entirely**, because
Ogham's version was scoped to the wrong problem (view-binding validation) and
lived at the wrong layer (coupled to the renderer, so a domain type couldn't
derive it without dragging in the UI).

### Decision

**[DECIDED] Commit Ogham to the "UI framework for Rust applications" identity.
Shed the standalone-browser / JS-replacement ambition as a *deliberate
deferral*, not a thing to keep half-building toward.**

The dream of replacing the JS ecosystem is a legitimate *north star* but a
terrible *navigator*. The current browser surface delivers all the costs of the
ambition (maintained dead code, two state models, a rival typed layer) and none
of the thing (it is not three percent of a browser). The way to eventually
displace JS is the way JS won: dominate one embedded niche so completely it
becomes the obvious substrate, then grow outward under real demand. Premature
breadth is how challengers lose.

**The preserved option is the four-pass pipeline** (scanner → parser → compiler
→ VM). That spine already pays rent (it runs `for`/`match`/components) and keeps
the door open to richer in-language logic later *without a rewrite*. `mutation`, the
unused dynamism tail, `OghamState`, the standalone-app posture are **not** option
value — they are depreciating inventory that rots while it waits (proof: the
`compile_increment`-on-upvalue bug at `compiler.rs:1249-1262` — a feature unused
long enough to be silently wrong: the upvalue branch emits an **unconditional**
`SetState` (line 1257) that the local branch guards behind `is_local_state`
(line 1240), so `++`/`--` on a captured non-state upvalue writes a spurious entry
into the component-state map. We *keep* `++`/`--` per §2 because they're
cheap and expected, but that bug is the tax of having shipped unexercised
surface, and fixing it is part of the price of keeping them).

> If the JS-killer is ever a real goal, it deserves its own roadmap and timeline
> — not a standing tax on the game projects. Conflated, both lose.

---

## 2. Keep / Cut / Defer

### Keep — the Rust-app-UI core

- The four-pass pipeline and the VM. Language features in real use: `for`,
  `match`, spread-as-conditional-children, function-components, imports,
  closures-as-event-handlers, `range` *inside* `for`, string `+`/index.
- Render engine: flex-with-wrap, reconcile-by-key (INTENT §3/§5),
  presence/animation, `text`, `text_input`, `image`, the Skia backend,
  hot-reload / file-watcher.
- `client` **as a dev previewer only** — a tool, not a platform claim.
- **Local state.** See §3 for the principle that keeps it from rotting into a
  back-door for host-defined state.
- **Grid** — [DECIDED] keep. It's a flex specialization (INTENT §4), nearly
  free, and UL uses it (5 files).
- **Portal** — [DECIDED] keep. Unnecessary ~95% of the time, but the genuine
  escape hatch for modals/overlays that must cleanly supersede other elements
  across subtree boundaries — the case in-tree flex layering *can't* do cleanly.
  Low cost to retain; keep UL's existing use, no migration.
- **`++` / `--`** — [DECIDED] keep. Cheap to retain and expected to exist in a
  language. *Caveat:* fix the latent increment-on-upvalue bug
  (`compiler.rs:1249-1262`: the upvalue branch emits an unconditional `SetState`
  at 1257 that the local branch guards behind `is_local_state` at 1240) as part of
  keeping them — don't ship the rot.
- **LSP + structural diagnostics** — [DECIDED] keep. Hover, goto-def, semantic
  tokens, scanner/parser diagnostics genuinely reduce friction for *agents
  hand-authoring `.ogh`*, which remains a substantial fraction of player-facing
  UI. Note it **re-sources, not dies**: it stops validating against `OghamState`
  manifests and validates `.ogh` field references against `editable` schema
  blobs instead (see §4).
- **`Surface` / `RenderContext` seam** — [DECIDED] keep, but **demote the
  rationale**. It is no longer "backend portability" (Skia is the only impl and
  GPU/tile-baking lives *outside* Ogham, so there is no second backend coming).
  It survives as a **paint-isolation + test seam**: it keeps `skia_safe` out of
  `widget/` (layout, hit-test, animation stay backend-pure and unit-testable)
  and prevents the skia-leak drift INTENT §6 warns about. Rewrite §6's *why*
  accordingly. (Current `widget/` skia leaks: `text_widget` for text measurement,
  `image` for decode, `svg_widget` + the `draw_svg_dom` trait method, and the
  `FontCollection` import in `widget/mod.rs` — `text_input_widget` does **not**
  leak skia. The `svg_widget`/`draw_svg_dom` pair goes when `svg` is cut in step 1,
  leaving `text_widget` + `image` + `FontCollection` as the standing leaks.)

### Cut — safe immediately (zero consumer entanglement)

- The rival typed layer: `OghamState`/`OghamMsg`, `ogham check`, the diagnostics
  manifest-matching (`diagnostics/{check,manifest}.rs`, `cli/check.rs`, `typed.rs`),
  and the typed half of `ogham-derive` (the `OghamState`/`OghamMsg` derives +
  `manifest_emit`). Superseded by `editable`; 1 site.
  - **Caveat — `runtime/schema.rs` and `parser/typed_bindings.rs` are split, not
    deleted.** Both mix the cut Rust-derive layer with the *kept* `.ogh`-source
    schema. Cut only the Rust-derive traits: `OghamField` (417), `OghamRecord` (430),
    `OghamState` (445), `OghamMsg` (458), `HostStateSinkErased` (474) — the 417–482
    cluster — plus `FromOghamValue` (605), which sits separately after the primitive
    impls (so it's two excisions, not one contiguous block). **Keep** `ModuleSchema`/
    `RecordSchema`/`FieldSchema`/`EventSig`/`from_module`/`load_schema*` and the
    `parser/typed_bindings.rs` parser for the `.ogh` `host_state {}` / `events {}` /
    `record {}` syntax + the `TypeRef`/`PrimType`/`KeyType`/`SchemaLiteral`
    vocabulary — they are load-bearing for compiler strict-mode identifier
    resolution + event validation, the **view layer's Tenet 6 host-state
    requirements** (`view/mod.rs` `requirements_from_schema`, central to the kept
    "host-state-in" identity), and LSP hover. A grep listing `runtime/schema.rs` for
    wholesale deletion in step 1 will break code the kept subsystems compile against.
- Lifecycle/effects: `effect`/`cleanup`/`on_mount`/`on_unmount`,
  `RegisterEffect`/`EffectSlot`, the mount/unmount drain queues,
  `cancel_unmount_for_prefix`. 0 uses.
- `mutation()` request/response seam. 0 uses.
- `svg` widget. 0 uses — but it is more than one file: cutting it removes
  `svg_widget.rs`, the `builder.rs:50` `"svg"` registration, the `draw_svg_dom`
  method on the `RenderContext` trait (`widget/mod.rs:1252`) + its `skia.rs` impl,
  and the `"svg"` `skia-safe` Cargo feature.
- Dead scaffolding: the `Environment` tree-walker leftover, `Placement`/`Gating`
  (inert), the `Dim` backdrop policy (TODO stub), `bezier_curve_to`/`close_path`.

> Note: `range` as a *standalone* expression is dead, but `range`-in-`for` is
> load-bearing — keep the parse path, it feeds for-loop bounds.

### Defer — behind the seam, demand-gated (NOT built now)

- **Fine-grained reactivity engine** (subtree re-execution keyed on which struct
  changed). See §3, rung 2. The seam is cheap and we take it; the engine is the
  exact "build VM machinery for a scale we don't have" we're swearing off.
- **Scoped / declared component reads** (the isolation benefit). See §3 note on
  implication #3. A separate, bigger language decision, in tension with
  moddability.
- **The standalone-browser ambition** itself. Re-addable later via the preserved
  pipeline; not built toward in the interim.

---

## 3. The state model

**[DECIDED — revised 2026-06-07] Host state that needs *generic editing* is
schema-described (via `editable`); everything else the host wants on screen is
plain Rust injected as `Value`; local state stays free-form and ephemeral.**

> This *supersedes* an earlier same-day draft that read "**all** host state is
> schema-described." Universalizing the schema was the overcomplication — it made
> the §4 seam look like a framework-defining crisis instead of a bounded tool.
> The honest position: schema/`editable` is an **opt-in tool for the
> generic-editing case** (§4), not a tax on every screen.

Three mechanically-distinct disciplines result:

- **Schema-described host state** — the content that gets *generically* edited
  (SM's `Scene` / `Character` / `Encounter` / …): typed, introspectable, driven
  by `editable`. The §4 seam exists for exactly this.
- **Plain host state** — everything else the host just wants rendered: injected
  as a `Value` directly (read in, events out, §2); no schema, no `editable`.
- **Local state** — free, ephemeral, view-owned. *Never* schema'd.

### The dividing line (drift indicator)

The boundary is **who names the state into existence**:

- State the **host defines** (current character, resources, save-relevant
  selection) → host state. Putting *this* in a local cell is the bug — saves and
  deep-links silently lose it.
- State the **view invents** (a mod's custom tabs, hover, open/closed, an
  in-progress input buffer) → local state, *because the host literally cannot
  name it*.

This is the present-tense (not future-proofing) justification for keeping local
state: if a mod can restructure the UI — fold two modes into five custom tabs —
then "which tab is active" is state the host *cannot* own, because it never
defined those tabs. Local state is the mechanism that makes the decoupling
possible. The erosion to lint against is **host-defined semantic state leaking
into local cells**.

### Honest accounting of the gains (for the schema-described subset)

These gains apply to the *schema-described* content (the generic-editing subset),
not to host state at large. We adopt the discipline *with eyes open*:

**Real, present-tense gains:**
1. **LSP/typo checking where it pays.** Generically-edited content gets its
   `.ogh` field references validated against the schema (the LSP re-sourcing, §4).
   Plain host state stays ad-hoc — fine, it carries no editor.
2. **A named change-unit seam** (rung 1, below).

> The earlier "all host state is a struct" draft also claimed **uniformity**
> ("one mental model, no two-tier boundary"). That is *deliberately given up*:
> opt-in schema means there *is* a two-tier boundary (schema'd vs. plain). Worth
> it — universal schema cost more than the uniformity bought.

**Deferred / speculative gains (do NOT build for these now):**
3. **Rerender granularity (rung 2).** Subtree reactivity keyed on struct change.
   *Valuable and dangerous* — flagged by the proposer as "not strictly
   necessary." Split it:
   - **Rung 1 — take now, nearly free:** a struct is a named change-unit. The
     host already does per-key change detection (`inject_host_state_if_changed`
     in SM `client.rs`); struct-level dirty-checking is a small step up and is
     present-tense useful.
   - **Rung 2 — defer:** widgets subscribing to a struct and skipping
     re-execution requires the VM to track *which subtree read which struct*
     (the signals/observable-store problem; path-based hook identity §9 is its
     prerequisite). Build only when a real app janks on global rerenders.
   Framing: struct-partitioned state is *preserved option value* for reactivity.
   Take the seam; defer the engine.
4. **Component isolation — real but NOT automatic.** Ogham's read model is
   *ambient* (plain identifiers fall through to the global host bag, §2), so
   structs-in-host-state isolate nothing by themselves. The benefit only
   materializes if components must *declare* which structs they see (props, not
   ambient global) — a separate, bigger decision that *trades against
   moddability* (ambient reads make a modder's "just show this here" easy;
   declared reads force threading). Keep it separable; don't bundle.
5. **Clearer typing — weakest.** The type system enforces *existence*, not
   *cohesion*; nothing prevents a junk-drawer `ScreenState`. A mild discipline
   nudge, not a structural guarantee. Don't lean on it.

### Consequence

For the schema-described subset, the host emits *typed `editable` values* and the
read walk (§4) turns them into the `Value` the renderer consumes — so that read
is **load-bearing for the generic-editing path** (not for all host state; plain
state is still injected as `Value` directly). It is the one piece worth designing
carefully (§4).

---

## 4. The `editable` ⟷ Ogham seam — reading & writing Rust structs

### The goal, stated explicitly  [DECIDED]

**A Rust struct that derives `editable` should round-trip through Ogham's
existing host-state-in / events-out pattern with no per-type hand-written glue.**
For a content type like SM's `Scene`:

- **Read:** `&Scene` projects into a `Value` tree of *field rows* that `.ogh`
  renders as an editor. Each row carries a label, the current value, a `kind`
  telling `.ogh` which widget to draw, and the dotted `path` an edit carries
  back. (SM's `field_entries` in `editor/src/client.rs` already hand-writes this
  shape: `{ key, value, field (=path), kind, options, editable }`.)
- **Write:** a field edit leaves `.ogh` as an event carrying `(path, op)`; the
  host applies it via `editable::apply()`. No `SetHostState` opcode (§2 holds).

That is the *entire* job — not a new pattern, just host-state-in / events-out
with "the state" a projected struct and "the events" field edits.

### Scope: generic editing only  [DECIDED]

The schema machinery (`editable`, `Kind`, path-`apply`, the read walk) exists for
**one** thing: handling content **generically** — rendering/editing it without a
hand-written UI per type. That is the pervasive real case (SM's inspector over
`Scene` / `Character` / `Encounter` / …). Two would-be justifications are **out
of scope**:

- **Mod-defined shapes.** Mods rely solely on **hand-authored Ogham state**, not
  Rust structs — a mod's content shape has no compile-time struct to derive from,
  so it never enters this seam. (Retires the old §3 `Kind::Map`-for-mod-resources
  framing.)
- **"All host state is schema-described."** Superseded (§3). Most host state is
  plain Rust injected as `Value`; schema is **opt-in** for the generic-editing
  case. Universalizing it was the overcomplication that made this seam look like a
  framework crisis instead of a bounded tool.

> **Amendment (2026-07-13, regency):** the read walk has a second consumer
> beyond generic editing — a host projecting a **typed host-state struct**
> (`#[derive(Editable)]` chrome state → `Value` at mutation time, diffed
> per field before injection). This stays inside the seam's rules: the
> `Value`-building visitor lives **host-side** (the composition-point rule
> below), the `.ogh` `host_state {}` block remains the single contract
> description (validated at load via `ModuleSchema::validate_host_state`),
> and Ogham stays derive-free — `addd9da` stands; this is *not* a revival
> of `TypedOgham`/`OghamState`. It relaxes only the "one thing" framing
> above: the walk serves generic *projection*, of which generic editing is
> the richest case.

### One derive, both directions — no second crate needed  [DECIDED]

Read and write come from **the same `editable` derive**. `apply` already walks the
real types to *write*; the derive gains the symmetric walk to *read*. There is
**no second derive** and **no `OghamRead` / `OghamWrite` capability crate** — the
prior draft's "thin Ogham-tied traits over the leaf" are superseded.

The code that turns a read into Ogham's `Value` (the row tree) names *both*
`editable` and Ogham, so it can't live in `editable` (purity). But it does **not
need a dedicated crate**: the **host already depends on both** and is the
composition point (the layering invariant), so the `Value`-building visitor can
live in the host. Promote it to a *thin, derive-less* shared helper **only** if a
second host (SM + UL) wants to reuse it — a packaging choice, not a layer.

> This supersedes the earlier "the translator is a second engine crate" lean:
> once read is part of `editable`'s own derive, the remaining glue is small enough
> to be host code.

### Read is the mirror of `apply` — not serde  [DECIDED]

`editable` has no instance-read *today* by design — schema (type-level) + `apply`
(write) only. The read must come from somewhere, and the tempting shortcut —
pull values via **serde** — is **rejected**.

**Why serde is wrong here:** serde is a *second, independently-configured
description of the same struct*, tuned for **persistence**, and not guaranteed to
agree with the `editable` schema / `apply` convention. It already disagrees in the
SM corpus:

- `Effect` (externally-tagged) → `{"Ruleset": {…}}` / `{"GiveItem": {…}}`.
- `Action` (`#[serde(tag = "kind")]`) → `{"kind": "apply_condition", …}`.
- `editable::apply` addresses **every** union through the `kind` discriminant.

So a serde read would feed `.ogh` an `Effect` shaped `{"Ruleset": {…}}` while a
variant-switch write goes out as `effects.0.kind = "give_item"` into `apply`'s
`kind`-path — read and write disagreeing on a union's shape, **in shipping
content**. That is the two-descriptions-drift disease that killed `OghamState`;
serde re-opens it on the read side. (Secondary cost: serde's choices are
*save-format* choices — `#[serde(default)]`, `skip`, tagging — so an editor built
on serde couples to the save format.)

**The fix:** read is emitted by the **same derive** as `apply`, so the two share
the `kind` convention **by construction** and cannot drift. serde keeps its real
job (disk / saves) and stays out of the UI path.

**Keeping `editable` pure:** the read is a **visitor / streaming traversal**, not
a returned tree — `editable` must not depend on Ogham's `Value` *and* keeps its
"no intermediate value model" rule. `editable` gains a small **pure** `Reader`
trait (scalar / list-begin/end / union-variant callbacks); the derived read
drives it; the host's `Value`-building visitor implements it. One walker, no serde
in the UI path, purity intact, read/write drift-proof.

### The layering invariant (unchanged, now sharper)

`editable` depends on Ogham for nothing — it gains only a *pure* `Reader` trait
and stays a serde-only leaf. Ogham stays domain-agnostic. The host composes the
two by supplying the `Value`-building visitor. Domain types still derive **only**
`editable`.

### The `Value` ↔ `Kind` mapping: a nested node vocabulary  [DECIDED — shape LEANING]

The projection is *what node does each `Kind` produce?* — and the read emits a
**nested tree** that mirrors the struct exactly, **not** a flattened row list.

> **[DECIDED] Nested, true to the structure — never flatten.** A flat-with-depth
> row list (as `backstory_rows_value` hand-builds today) is *itself a second,
> derived representation* layered over the real shape — precisely the kind of
> rival description that drifts, which this whole doc kills elsewhere (serde, §4;
> `OghamState`, §1). Flattening also bakes in assumptions that break when a new
> schema nests differently. The read mirrors the data structure 1:1; any
> flattening for display is a *rendering* choice made later in `.ogh`, never baked
> into the projection.

> **[DECIDED] `Ref` candidates ride a sibling channel.** A `ref` node carries only
> `{ kind: "ref", table, value: <id> }`. The candidate lists live **once** in a
> separate host-state map keyed by table (`refs.<table>`), not inline per node —
> one copy, looked up by the picker.

The node shape (a recursive `Value::Map`). Every node carries
`{ key (label), path, kind, editable }`; **every container nests its child nodes
under a single uniform `children: [node…]`** (so `field_node` is one
`for node.children` loop), with kind-specific metadata alongside:

| `Kind` | `kind` | extra node fields | write op |
|---|---|---|---|
| `Str`/`Int`/`Float` | `text`/`number` | `value` (string) | `Set` |
| `Bool` | `bool` | `value` | `Set` |
| `Text` | `textarea` | `value` | `Set` |
| `Enum(v)` | `enum` | `value`, `options` | `Set` |
| `Formula` | `formula` | `value` (+ host validity) | `Set` |
| `Ref(t)` | `ref` | `table`, `value` (id) — candidates via `refs.<t>` | `Set` |
| `Record` | `group` | `children` (named) | — |
| `Tuple` | `group` | `children` (positional `.0`/`.1`) | — |
| `List(T)` | `list` | `children` (each `path…​.N`) | `AddListItem` / `RemoveListItem` / `MoveListItem` |
| `Optional(T)` | `option` | `children` (0 or 1) | `AddListItem` (set) / `RemoveListItem(0)` (clear) |
| `Map(T)` | `map` | `children` (each carries `map_key`) | `AddMapEntry{key}` / `RemoveMapEntry{key}` |
| `Union(v)` | `union` | `variant`, `variants`, `children` (active payload) | `Set` on `…​.$variant` (re-defaults payload) |

**Container ops map 1:1 onto `FieldOp`** — that correspondence is what retires the
`__add` / `__objdel` sentinel-key hacks: a `list` node exposes real add/remove/move
events instead of smuggling them through magic keys. Keep the node vocabulary and
`FieldOp` in lockstep — they're one list seen from two ends.

#### The write wire format: six named events  [DECIDED]

The `.ogh`→host edit channel is **named, typed events** (not one generic
`apply_edit(path, op_map)` — a tagged-union-in-`Value` would be the rival encoding
this doc kills everywhere else). **Four already exist and are proven in the frozen
panes** (scenes, companion_reactions; `editor/src/client.rs:840-935` dispatch →
`schema_form.rs:109-120` `apply_action` decode to `FieldOp`, with the frozen panes
also hand-decoding in their own `handle()`); the nested model adds **two** for
maps:

| node action | `.ogh` event | args | `FieldOp` |
|---|---|---|---|
| set any scalar leaf | `set_field(path, value)` | `String, String` | `Set(value)` |
| switch a union variant | `set_field(path + ".$variant", variant)` | `String, String` | `Set` on discriminant |
| list/option add | `add_list_item(path)` | `String` | `AddListItem` |
| list remove / option clear | `remove_list_item(path, index)` | `String, Integer` | `RemoveListItem(index)` (option uses `0`) |
| list reorder | `move_list_item(path, index, delta)` | `String, Integer, Integer` | `MoveListItem{index, delta}` |
| **map add** *(new)* | `add_map_entry(path, key)` | `String, String` | `AddMapEntry{key}` |
| **map remove** *(new)* | `remove_map_entry(path, key)` | `String, String` | `RemoveMapEntry{key}` |

`$variant` switch and `option` set/clear need **no new event** — they reuse
`set_field` / `add_list_item` / `remove_list_item`. The marker-path hacks
(`.__move.` / `.__remove.` / `__objdel`) exist only because the old `characters`
pane renders a *flat* string-only channel that can't pass an `Integer` index, so it
smuggles one into the path; the nested `field_node` passes typed indices natively,
so the markers evaporate with the flat model — no replacement encoding needed.

> **Write-side survivor (cleanup ordering).** The host-side decode — the
> `PaneAction`→`FieldOp` match (`schema_form.rs:109-120`) — lives *inside* the file
> the migration deletes. It is the write-side analogue of the surviving
> `Value`-building visitor: **extract it before deleting `schema_form.rs`** (step 5),
> or the event→`FieldOp` decode goes with it. (`content-core`'s `Edit`/`apply_edit`
> envelope is the *agent* path, table+id+op; it is unaffected — both paths bottom
> out at `editable::edit`.)

**The `.ogh` render consequence:** a **recursive `field_node` component** that
`match`es `node.kind` and recurses **uniformly into `node.children`** via `for`.
Each child is **keyed by its `path`** — the dotted path is a naturally stable
identity, so reconcile-by-key (INTENT §3/§5) and path-based hook identity (INTENT
§9) make the recursive tree animate and preserve state correctly.

> **[DECIDED 2026-06-08c — Ogham gains function self-recursion; `field_node` is
> interim-tiered until it lands.]** Building step 3 surfaced that Ogham's kept core
> *deliberately* lacks function self-recursion (the explicit `recursive_function_not_supported`
> test, `tests/language.rs:542`; `effects.ogh` already works around it with manually
> unrolled depth tiers). So a *truly* recursive `field_node` is impossible **today**,
> and the shipped step-3 component is unrolled into **9 depth tiers** (the established
> `effects.ogh` pattern). That is a stopgap, not the design: it merely *raises* the
> `MAX_NESTED_DEPTH` display cap (from the old 3 to 9) instead of *eliminating* it as
> this doc intends — and the content schema permits **unbounded** nesting (`Action.on_success:
> Vec<Action>` is self-recursive), so deep instances still bottom out at the tier floor
> (data round-trips at any depth; only live *editing* past depth 9 is bounded).
>
> **Decision: add function self-recursion to the Ogham core.** The enabling change
> looks small — `compile_function` (`src/runtime/compiler.rs:1297`) already reserves
> **slot 0 for the callee** (the executing closure on the stack); it currently binds
> that slot to an empty name (`String::new()`, ~line 1309). Binding it to the
> function's own `name` makes a self-reference resolve to slot 0 → recursion. This
> reverses the deliberate "not supported" decision, so it must be done carefully:
> cover closures that capture themselves, nested function-components, and the upvalue
> path; replace `recursive_function_not_supported` with positive recursion tests.
> Once it lands, `field_node` collapses to the clean recursive component above, the
> 9-tier unroll (and ideally `effects.ogh`'s tiers) disappears, and the depth cap is
> genuinely gone. **Status: DECIDED, not yet implemented** — a prerequisite for
> finalizing step 4's `field_node`.

### The read walk: a `Reader` mirror of `apply`  [DECIDED — names LEANING]

The derived read is the **structural mirror of the generated `apply`** — same
derive, same field idents, same `#[serde(rename)]` logic — so read paths and write
paths agree *by construction*. Where `apply` consumes a path top-down and routes
into the live value to write, the read walks the live value top-down and emits its
structure:

| derive case | `apply` (write) | read (mirror) |
|---|---|---|
| named struct | route segment → `field.apply` | `begin_group; field(name); …; end_group` |
| tuple struct/variant | route index → `self.i.apply` | `begin_group; index(i); …; end_group` |
| union | discriminant-switch / route variant | `begin_union(variant, variants); …; end_union` |
| `Vec<T>` | index → element | `begin_list(len); index(i); …; end_list` |
| `Option<T>` | unwrap | `begin_option(present); …; end_option` |
| scalar leaf | `Set` parses | `scalar(kind, value)` |

Three properties pin it:

- **Self-describing leaves — no `schema()` at render time.** The single leaf
  callback is `scalar(&Kind, &str)`: the `Kind` carries ref-table and enum-options,
  the string is the instance value. One walk yields structure + kinds + values;
  `schema()` survives only for the LSP manifest. **Caveat — promotions live at the
  field site, not the leaf.** A `String` leaf's own `schema()` is always
  `Kind::Str`; the `#[editable(ref/text/formula)]` upgrade is known only to
  `field_schema_tokens` *where the field is declared*, not to the leaf. So a plain
  `String::read` cannot self-describe as `Ref`. **[DECIDED] Design C:** plain fields
  delegate `self.f.read(v)` (the leaf emits `scalar(&Kind::Str, …)` cheaply from its
  own `schema()`); a promoted field instead emits its `scalar` *inline at the field
  site* with the promoted `Kind`, wrapped through any `Option`/`Vec` nesting by
  reusing the existing `wrap_semantic` (`editable-derive/lib.rs:139`). This keeps
  the common path allocation-free and puts promotion where the derive already has
  the metadata. (Rejected: threading a `Kind` argument through every `read` — nested
  struct fields would build and discard a full `Record`/`Union` `Kind` per render.)
- **The instance value's string form.** `scalar`'s `&str` is `self.to_string()` for
  numeric/`bool` scalars, identity for `String`, and the **derive-computed
  serde-renamed variant name** for a unit enum (not `Display`). Pin float
  round-trip (`f32::to_string()` → `s.parse::<f32>()`) with a parity test.
- **No cycle guard in the read walk.** Unlike `schema_of`'s `EXPANDING` thread-local
  (which exists because *type-level* expansion of a recursive type is infinite),
  `read` walks a *finite instance* and terminates on the data. `Kind::Named` arises
  only inside a `Record`/`Union` (→ `begin_group`/`begin_union`), never inside a
  `scalar`, so a `Reader` never sees it. No depth/cycle handling is needed at render.
- **The visitor owns the path.** The derive fires `field(name)` / `index(i)` to
  name the next child and stays path-agnostic; the host visitor accumulates the
  dotted path. That visitor is a small SAX→tree stack machine — and it is the
  **surviving core of the replaced `schema_form.rs`** (the *walk* moves into the
  derive; this visitor + the `refs` channel is all that stays host-side).
- **`Reader` lives in `editable`, depending only on `editable::Kind`** — purity
  intact; it never names an Ogham type.

Callback set (shape fixed, names at implementation): `scalar(&Kind,&str)`,
`begin_group`/`end_group`, `begin_list(len)`/`end_list`,
`begin_option(present)`/`end_option`, `begin_map`/`end_map`,
`begin_union(variant,variants)`/`end_union`, `field(name)` / `index(i)` to name
the next child, and `map_key(&str)` to name a map entry (keys may be non-`String`,
e.g. `BTreeMap<u64,[u8;3]>`; `read` surfaces `k.to_string()`, the write path
recovers it via `FromStr` exactly as `apply` already does).

**Positional vs. named labels (the `"0"` rule).** Structure is *always* faithful —
a tuple/newtype variant's fields are real children at `.0`, `.1`, …, never
flattened. (Flattening is a structural edit, banned by the nesting rule, and it
doesn't generalize: `Foo(u32, u32)` has nothing to hoist.) The ugly `"0"` is a
*label* problem, fixed in `field_node`, not the derive: a node marks whether its
label is positional, and `field_node` **hides the label of a *sole* positional
field** (so `Effect::Ruleset(Action)` renders its `Action` inline under the
`ruleset` variant) while showing `0`/`1` for genuine multi-field tuples. Want
names instead of `0`/`1`? Use a struct variant — that's the nudge.

### Three structural warts the `Scene` walk surfaced

1. **[DECIDED] Newtype/tuple-variant `"0"`** — label-only fix above; structure
   stays faithful.
2. **[DECIDED] `Optional` is its own `option` kind** (set/clear), not a cap-1
   `list`. Eight `Scene` fields are `Option`; "+ Add / clear" reads far better
   than list chrome.
3. **[DECIDED] The union discriminant moves to the sigil `$variant`.** Today
   `editable`'s discriminant is `kind`, and the derive *rejects at compile time*
   any union-variant field named `kind` (`editable-derive/lib.rs:360`) — safe but
   restrictive: it reserves a legal, reasonable field name (a *record* field like
   `Transition.kind` is fine, since the special-case only fires on unions). A
   `$`-sigil can never be a Rust field, so the restriction disappears and the path
   grammar gains a clean invariant — `$`-prefixed segments are *control* (the
   variant selector), everything else is *data*. It also de-conflates from serde's
   on-disk `#[serde(tag = "kind")]`, which stays `kind` (**no content migration**).
   Change `DISCRIMINANT` in both crates (`editable/lib.rs:53`,
   `editable-derive/lib.rs:38`) and delete the now-vacuous field-name rejection;
   `parse_path` / `schema()` need no change (runtime edit-paths only).

   **Blast radius (verified across ogham / lorekeeper / small_mercies).** The flip
   is *automatic at runtime* — the derived `apply` routes the discriminant via
   `::editable::DISCRIMINANT` (`editable-derive/lib.rs:439`), so every union's
   variant-switch follows the const. **No content migration** holds: serde's
   on-disk `#[serde(tag = "kind")]` is untouched and no `.ogh` file references the
   discriminant (both verified). **But** every *string-literal* edit-path whose
   segment is exactly `kind` must be hand-audited.

   **[DONE 2026-06-08c — flipped & migrated; the pre-flip estimate was wrong, this
   records what was actually true.]** The operative rule is sharper than "parent is
   a union → flip": **flip a `kind` segment iff it reaches `editable::apply`.** Two
   things the pre-flip count (a guessed "23, two crates") missed:
   - **Scope is four crates, ~40 sites — not two crates, 23.** The audit also covers
     `lorekeeper/ruleset` (5 union paths in `effects.rs` tests — `Action`/`Duration`;
     omitted by the plan but in the path-dep blast radius) and the whole
     `small_mercies/editor` crate (`content-core` had **13** sites, not 12 —
     `schema.rs:1475` was uncounted). Flipped, all editable-routed: 11 in
     `editable`, 5 in `ruleset`, 13 in `content-core`, and the `schema_form.rs`
     editable-apply test paths. Record-field traps left as `kind`:
     `water_source.0.kind` (`WaterSource.kind: String`), `stats.0.kind`
     (`StatDef.kind: StatKind`), `beats.0.transition.kind`
     (`Transition.kind: TransitionKind`).
   - **The editor panes do NOT reach `editable::apply`** — and so DON'T flip. The
     frozen panes (`companion_reactions`/`scenes`/`backstory`/`abilities`/…) route
     edits through *hand-rolled serde patchers* (`effects::set_field`, keyed on the
     literal `field == "kind"`), a parallel write path that is serde's tag, not the
     editable discriminant. ~40 pane `.kind` paths correctly stay `kind`. These die
     with the flat model in step 5; until then they coexist untouched.
   - **One genuine production consequence** (the plan said "zero"): `schema_form.rs`
     used `DISCRIMINANT` both to build an editable apply path (`child(path, …)`, line
     356 — correctly → `$variant`) AND to read a variant tag out of a *serde* map
     (`map.get`/`contains_key`, lines 439/460). The flip de-conflates these: 439/460
     now use the literal serde tag `"kind"`. This kept the doomed `schema_form.rs`
     green through steps 2–4.

   A blind `s/.kind/.$variant/` would have corrupted all three classes (record
   traps, serde-patcher panes, the serde-read in `schema_form`). Every edit was
   judged against the schema and the write path, not sed'd; all four affected crates
   are green (`editable` 35, `ruleset` 64, `content-core` 185, `editor` 182).

### Worked example: `Scene` (the buildable target)

The derived read of one `Scene` (`small_mercies/content-core/src/schema.rs`),
abbreviated to the instructive nodes:

```
Scene                                   group  []
├ id                  text     [id]
├ backdrop  Opt<ref sprite>    option   [backdrop]            (set/clear → ref:sprite)
├ body      #[text]            textarea [body]
├ beats     Vec<Beat>          list     [beats]               +add/remove/move
│  └ ⟨N⟩ Beat                  group    [beats.N]
│     ├ advance  Advance       union    [beats.N.advance]     auto|click|either
│     │   └ (auto) f32         number   [beats.N.advance.0]   sole positional → unlabeled
│     ├ transition Transition  group    [beats.N.transition]
│     │   ├ kind  TransitionKind enum   [beats.N.transition.kind]   (record field — fine; discriminant is $variant)
│     │   └ duration_secs f32  number   [beats.N.transition.duration_secs]
│     └ on_enter Vec<Effect>   list     [beats.N.on_enter]
│        └ ⟨M⟩ Effect          union    [beats.N.on_enter.M]  15 variants
│           ├ (ruleset) Action union    [beats.N.on_enter.M.0]  sole positional → inline
│           └ (give_item) → ref:item [..M.item], number [..M.quantity]
├ next      Opt<ref scene>     option   [next]
├ choices   Vec<Choice>        list     [choices]
│  └ ⟨N⟩ Choice → label(text), gate(option·formula), check(option·ref check),
│        time_cost(number), outcomes(group → success/failure(group),
│        critical/partial(option·group Outcome)), moral_tags(list·text)
├ ends_run  bool               bool     [ends_run]
└ epilogue  Vec<EpilogueFragment> list  [epilogue]
```

Every leaf write is `Set(path, value)`; every container exposes its `FieldOp`
directly. No sentinels, no serde, no flattening.

### What the derived read retires (cleanup — do not leave these behind)

The nested derived read is *also a deletion mandate*. A lot of recently-moved
machinery is the flat/serde model we rejected and must go — otherwise we keep two
rival inspectors, the disease this doc exists to kill:

- **`editor/src/widgets/schema_form.rs` (~814 LOC) — [DECIDED] REPLACE, not
  evolve.** It is a half-built generic inspector on every choice we rejected:
  **flat** (`§list` markers, `inspector_lines`), **serde-read**
  (`T: Editable + Serialize`, walks `serde_json::Value` — the drift bomb, live),
  **Ref-as-plain-text**, and a `MAX_NESTED_DEPTH = 3` "(edit in JSON)" fallback.
  Its *walk* moves into the `editable` derive (the `Reader`); what survives
  host-side is the small `Value`-building visitor + the `refs.<table>` channel.
- **`FieldKind`** (`pane/mod.rs:129`, 3 variants) → subsumed by the node `kind`
  vocabulary. Delete.
- **`InspectorField` + builders** `read_only`/`editable`/`number`/`toggle`
  (`pane/mod.rs:142`) → the derived node tree. Delete.
- **Hand-written `*_value` projections in `client.rs`** — `inspector_value`,
  `backstory_rows_value`, `entries_value`, `field_entries`, `inspector_fields`
  (the "300+ `Value::String` boxings"). Delete.
- **The `§list` marker hack** (`list_marker`; list chrome encoded as a `Toggle`)
  → real `list` nodes.
- **Sentinel keys `__add` / `__objdel` / `__add_kind`** (`characters.rs:647/685`,
  `items.rs:197`) and their `.ogh` arms (`characters.ogh:93-96`,
  `add_control_row` / `obj_del_row`) → explicit container ops.
- **`MAX_NESTED_DEPTH` + the "edit in JSON" leaf** → goes with nesting. No cycle
  guard replaces it: the read walks a finite instance and terminates on the data
  (see the read-walk section); `Kind::Named` never reaches the renderer. *(Interim:
  until Ogham gains function self-recursion (decided, above), `field_node` is
  9-tier-unrolled, so the cap is raised 3→9 rather than removed; it vanishes for
  real once recursion lands.)*
- **Per-pane hand-written inspector projections** (`scenes.rs` / `characters.rs` /
  `abilities.rs` / …) → the migration `schema_form` started, completed onto the
  nested derived read.
- **The flat `field_row` match** (`common.ogh:183`; the inner `match line.kind` at
  line 190, 3 arms, no default) → the
  recursive `field_node`.

**Explicitly NOT retired (stays plain host state, never schema'd):** the map-pane
chrome — `panes_value`, `map_modes_value`, `terrain_layers_value`, `layers_value`,
filters, selection. Bespoke UI, injected as `Value` directly — §3's opt-in line in
action.

**Smaller decisions (settled):** `value` is a **string** in every node (symmetric
with `FieldOp::Set`, which parses); **labels** default to the field name with an
`#[editable(label = "…")]` override; **`editable`** is per-field via
`#[editable(readonly)]`, and Flow B (player UI) is whole-pane read-only.

---

## 5. Decisions log & remaining open work

**Resolved (workshop sessions 2026-06-07/08):**

- **[RESOLVED] Nested vs. flat projection** (§4) — **nested**, true to the data
  structure; flattening is a rival representation that drifts and breaks on new
  schemas. `Ref` candidates ride a sibling `refs.<table>` channel.
- **[RESOLVED + BUILT] Node vocabulary** (§4) — kinds, ops, the `option` kind, the
  positional-label (`"0"`) rule, and **uniform `children`** (every container nests
  under one key; `field_node` is a single `for node.children` loop) are decided,
  worked end to end on `Scene`, and **implemented in step 3** (`inspector/node_tree.rs`
  visitor + `field_node.ogh`, parity-tested against a live `Scene`). The `.ogh`
  `field_node` is interim-tiered until the recursion enablement (step 3.5).
- **[RESOLVED] Write wire format** (§4) — **six named, typed events** (not a generic
  op-map). Four exist and are proven in the frozen panes; the nested model adds only
  `add_map_entry` / `remove_map_entry`; `$variant` and `option` reuse `set_field` /
  `add_list_item` / `remove_list_item`. The marker-path / sentinel hacks retire with
  the flat channel. The `PaneAction`→`FieldOp` decode is the write-side survivor —
  extract before deleting `schema_form.rs`.
- **[RESOLVED + BUILT] The read walk / `Reader`** (§4, step 2b) — a structured
  begin/end visitor that mirrors `apply` (same derive); leaves emit
  `scalar(&Kind,&str)` (no `schema()` at render); the visitor owns the path; lives
  in `editable`. Shipped: pure `Reader` + `read` leaf impls + `read` codegen, 7
  parity tests (`editable` 35→42). **Design C [DECIDED + BUILT]:** `#[editable(...)]`
  promotions emitted inline at the field site via `wrap_semantic_read`. No read-walk
  cycle guard (finite instance). `map_key(&str)` in the callback set. One
  implementation correction: `Option` emits `index(0)` before its present inner so
  read paths equal `apply`'s `Seg::Index(0)`.
- **[RESOLVED] Union discriminant → `$variant` sigil** (§4 wart 3) — frees the
  field name `kind`, gives the path grammar a control/data split, de-conflates from
  serde's disk tag; no content migration.
- **[RESOLVED] Reads stay ambient** (§3 #4) — plain identifiers fall through to the
  host bag. Scoped/declared reads cost threading and fight moddability (a core
  value), and the isolation benefit only matters at a scale we don't have. Revisit
  only on concrete isolation pain, as an opt-in that leaves ambient undisturbed.
- **[RESOLVED] Ogham-facing trait facade** — dropped. Read lives in `editable`'s
  own derive; the host builds the `Value`; no `OghamRead`/`OghamWrite`, no
  capability crate.
- **[RESOLVED] Local-state slimming** (INTENT §9) — *consequential to the lifecycle
  cut*, not a redesign: when lifecycle/effects go, delete `EffectSlot` /
  `RegisterEffect` / the drain queues / `cancel_unmount_for_prefix`. The `state`
  cell and its path-based identity are **untouched** (load-bearing for
  component-state independence and reorder survival).

**Execution sequence** (cut/build/migrate order — keeps two inspectors from ever
coexisting). This spans **three repos** — `ogham`, `lorekeeper` (the `editable` /
`editable-derive` leaf crates), and `small_mercies` (the editor host) — so each
step is tagged with where it lands and what gates it.

> **Progress (2026-06-08c):** Steps **1, 2a, 2b, 3 are DONE and committed** across
> all three repos, each reviewed and green (`ogham` 522 tests; `editable` 42,
> `ruleset` 64; `content-core` 185, `editor` 198). **Remaining:** the Ogham
> function-recursion enablement (new step **3.5**, decided §4), then **4** (migrate
> panes) and **5** (delete the flat machinery). Two notable as-built deviations are
> recorded where they occurred: the `$variant` blast radius was ~3× the estimate and
> spanned four crates (§4 wart 3); `field_node` is interim-tiered pending recursion
> (§4, the render-consequence note).
>
> **Progress (2026-06-08d):** Steps **3.5, 4, and 5 are now DONE and committed** —
> the migration is complete. Each step was built, independently reviewed, and
> committed (`ogham cbb72c8`; `small_mercies b3253e7`/`579b51d`/`d91d452`/`e82eacd`/
> `17e1337`). Final state: `ogham` 527 tests; `content-core` 185, `editor` 190.
> **3.5** — function self-recursion landed (the one-line slot-0 bind needed a
> companion change: the `let name = fn …` declaration path now threads the binding
> name through, since function literals don't carry it; anonymous `fn` keep the
> unreferenceable `"fn"` slot). `field_node` collapsed from 9 tiers to one true
> recursive component (1581→378 lines), depth cap gone. **4** — all **11**
> inspector-bearing panes migrated onto the nested read; `editable_default()`-shape
> shifts and the `Option`/`$variant` two-step gestures are the visible consequences.
> **5** — ~6k lines of flat/serde machinery deleted (`schema_form.rs`,
> `widgets/effects.rs`, per-pane flat projections + write handlers). As-built
> deviations: (a) the doc's retire-list named `InspectorField`/`FieldKind`/
> `field_row`/`inspector_value` for deletion, but they **survive** — the **Map**
> pane's Places/Regions inspector (already an §4 carve-out), the **items** add-kind
> chooser, **conversations** Topics-nav + inline rename, the **backstory** canvas
> chrome, and **assets** lint rows are legitimate remaining flat consumers; (b) a
> host-side **prune** (`inspector/prune.rs`) realizes the "editor surfaces a per-tool
> filter, not `#[editable(skip)]`" tenet for `characters`/`items`/`backstory` (e.g.
> `characters` drops `dialogue` + runtime state while the full type still
> round-trips on write); (c) the map-add UX uses a host-stamped `next_key` (the
> runtime `TextInput` has no submit event). **Open follow-up:** the editable op set
> has no **`RenameMapEntry`**, so new map entries are stuck at a `key_<n>` placeholder
> (id-keyed maps like `sheet.stats` can't be author-named through the seam) — add it
> to `editable` before relying on map authoring in anger. Conversations' gate
> downgraded from the structured atom buffer to a raw-formula leaf (no data loss).

1. **[`ogham`] — safe-immediate cuts** ✅ **[DONE]** (independent, 0–1 sites; grep-verify no
   hidden consumer first): the `OghamState`/`OghamMsg` traits + manifest matching +
   `ogham check` + the typed half of `ogham-derive` (**split, not delete** — keep
   the `.ogh`-source `ModuleSchema` + `parser/typed_bindings.rs` parser per §2's
   caveat); lifecycle/effects + machinery (the slimming above); `mutation`; `svg`;
   dead scaffolding. Fold in the `compile_increment` upvalue-bug fix. *Gate: none.*
2. **[`lorekeeper`] — the `editable` seam, leaf side.** ✅ **[DONE — 2a & 2b]** (a) Flip `$variant` in both
   crates + delete the field-name rejection + migrate the editable-routed test
   edit-paths in the *same commit* — flip a `kind` segment iff it reaches
   `editable::apply` (~29 across `editable`/`ruleset`/`content-core`/`schema_form`);
   the record-field traps and the editor's serde-patcher panes stay `kind`; de-conflate
   `schema_form`'s serde-read (§4 wart 3). **[DONE 2026-06-08c.]** (b) Add the pure `Reader` trait + `Editable::read` + the
   ~6 hand leaf impls (`editable`), and the `read` codegen in `editable-derive`
   (Design C promotion at the field site) — parity-tested against the hand impls.
   *Gate: (b) independent of step 1; ~2 days.*
3. **[`small_mercies`] — the host seam.** ✅ **[DONE]** The `Value`-building visitor (the SAX→tree
   stack machine surviving from `schema_form.rs`, now `inspector/node_tree.rs`) +
   `field_node` (interim 9-tier; see below) + the `refs.<table>` channel
   (`inspector/refs.rs`; 13 catalogued tables wired, 5 asset tables TODO'd) + the
   `option` kind rendering + the two new write events (`add_map_entry` /
   `remove_map_entry`). The `PaneAction`→`FieldOp` decode extracted into
   `editor/src/edit_apply.rs`. *Gate: step 2b — met.*
3.5. **[`ogham`] — function self-recursion** ✅ **[DONE]** Bind the
   callee slot to the function name in `compile_function` (§4 render-consequence
   note) **+ thread the binding name through the `let name = fn …` declaration path**
   (function literals don't carry their name); replaced `recursive_function_not_supported`
   with positive tests; covered self-capturing closures + nested components. `ogham`
   527 tests. *Gate: none — was a prerequisite for finalizing step 4's `field_node`.*
4. **[`small_mercies`] — migrate** the panes onto the derived read. ✅ **[DONE]** All
   11 inspector-bearing panes migrated (scenes, abilities, ruleset, characters,
   tuning, assets, winnability, companion_reactions, items, conversations, backstory);
   `field_node` de-tiered to true recursion; `characters`/`items`/`backstory` use a
   host-side prune (`inspector/prune.rs`) for delegated/runtime fields. Carve-outs
   left flat by design: Map inspector, conversations Topics-nav, backstory canvas.
   *Gate: step 3 + step 3.5 — met.*
5. **[`small_mercies`] — then delete** the flat/serde editor machinery. ✅ **[DONE]**
   Deleted `schema_form.rs`, `widgets/effects.rs`, the `§list`/`__add`/`__objdel`/
   `__add_kind` sentinels, `MAX_NESTED_DEPTH`, per-pane flat projections + write
   handlers (~6k lines). **`FieldKind`/`InspectorField`/`field_row`/`inspector_value`
   survive** — they remain load-bearing for the Map inspector + the items chooser +
   the conversations/backstory nav carve-outs (an as-built correction to the
   retire-list, which assumed Map also migrated). *Gate: the last pane migrated — met.*

Step 1 is independent and goes first; the editor deletion (5) is gated on the whole
build chain (2→3→4). **Portal is keep — there is no portal migration**; the earlier
draft's mention of one was stale.

**Still open / not active:**

- **[PARKED] Scoped/declared reads** (§3 #4) — superseded by the ambient decision;
  listed only so a future isolation pain has a home.
- **Eventually:** translate the [DECIDED] items into `INTENT.md` tenets
  (identity/scope; `Surface`-as-isolation rewrite of §6; host/local state
  discipline; the generic-editing seam — one `editable` derive, read+write, no
  serde, `$variant` discriminant, ambient reads). With every seam question now
  settled, this is the natural next action — the workshop has reached its end.
