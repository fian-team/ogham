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
> `editable` derive, with no second crate and no serde in the UI path).

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
`compile_increment`-on-upvalue bug at `compiler.rs:1255-1257` — a feature unused
long enough to be silently wrong. We *keep* `++`/`--` per §2 because they're
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
  (`compiler.rs:1255-1257` emits `SetState` before `SetUpvalue`) as part of
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
  accordingly. (With `svg` cut, only `text_widget` still leaks skia, for text
  measurement.)

### Cut — safe immediately (zero consumer entanglement)

- The rival typed layer: `OghamState`/`OghamMsg`, `runtime/schema.rs`,
  `parser/typed_bindings.rs`, `ogham check`, and the typed half of
  `ogham-derive`. Superseded by `editable`; 1 site.
- Lifecycle/effects: `effect`/`cleanup`/`on_mount`/`on_unmount`,
  `RegisterEffect`/`EffectSlot`, the mount/unmount drain queues,
  `cancel_unmount_for_prefix`. 0 uses.
- `mutation()` request/response seam. 0 uses.
- `svg` widget. 0 uses.
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

The node shape (a recursive `Value::Map`; **field names LEANING**, structure
decided). Every node carries `{ key (label), path, kind, editable }`, plus:

| `Kind` | `kind` | extra node fields | write op |
|---|---|---|---|
| `Str`/`Int`/`Float` | `text`/`number` | `value` (string) | `Set` |
| `Bool` | `bool` | `value` | `Set` |
| `Text` | `textarea` | `value` | `Set` |
| `Enum(v)` | `enum` | `value`, `options` | `Set` |
| `Formula` | `formula` | `value` (+ host validity) | `Set` |
| `Ref(t)` | `ref` | `table`, `value` (id) — candidates via `refs.<t>` | `Set` |
| `Record`/`Tuple` | `group` | `children: [node…]` | — |
| `List(T)` | `list` | `items: [node…]` (each `path…​.N`) | `AddListItem` / `RemoveListItem` / `MoveListItem` |
| `Optional(T)` | `list` (cap 1) | `items` (0 or 1) | `AddListItem` / `RemoveListItem(0)` |
| `Map(T)` | `map` | `entries: [{ key, node }…]` | `AddMapEntry{key}` / `RemoveMapEntry{key}` |
| `Union(v)` | `union` | `variant`, `variants` (names), `payload: [node…]` | `Set` on `…​.kind` (re-defaults payload) |

**Container ops map 1:1 onto `FieldOp`** — that correspondence is what retires the
`__add` / `__objdel` sentinel-key hacks: a `list` node exposes real add/remove/move
events instead of smuggling them through magic keys. Keep the node vocabulary and
`FieldOp` in lockstep — they're one list seen from two ends.

**Two consequences fall out of "nested":**
- The `Reader` trait is a **structured (begin/end) visitor**, not a flat row
  emitter: `scalar` / `begin_record…end_record` / `begin_list…item…end_list` /
  `begin_union(variant)…` callbacks. (Resolves the direction of the §5 "Reader
  shape" item; only the exact callback names remain open.)
- `.ogh` renders with a **recursive `field_node` component** that `match`es
  `node.kind` and recurses into `children`/`items`/`payload`/`entries` via `for`.
  Each child is **keyed by its `path`** — the dotted path is a naturally stable
  identity, so reconcile-by-key (INTENT §3/§5) and path-based hook identity
  (INTENT §9) make the recursive tree animate and preserve state correctly.

**Smaller decisions (settled):** `value` is a **string** in every node (symmetric
with `FieldOp::Set`, which parses); **labels** default to the field name with an
`#[editable(label = "…")]` override; **`editable`** is per-field via
`#[editable(readonly)]`, and Flow B (player UI) is whole-pane read-only.

---

## 5. Open questions for future sessions

- **[RESOLVED 2026-06-07] Nested vs. flat projection** (§4) — **nested**, true to
  the data structure; flattening is a rival representation that drifts and breaks
  on new schemas. `Ref` candidates ride a sibling `refs.<table>` channel.
- **[LEANING] The `Value` ↔ `Kind` node vocabulary** (§4) — structure decided
  (nested node per `Kind`, ops 1:1 with `FieldOp`, retiring the sentinel hacks);
  what remains is finalizing node *field names* (`children`/`items`/`payload`/
  `entries`) and the `.ogh` `field_node` component.
- **[OPEN, direction set] The read visitor's exact shape** (§4) — a **structured
  (begin/end) `Reader`** is decided (nested demands it); the open part is the
  exact callback names and the host's `Value`-building visitor. Concrete enough to
  hand to whoever writes the derive.
- **[OPEN] Scoped/declared reads vs. ambient reads** (§3 #4) — the component-
  isolation benefit collides with the moddability value. Separable from the
  state-model decision; decide on its own.
- **[RESOLVED 2026-06-07] Ogham-facing trait facade over `editable`** — dropped.
  Read lives in `editable`'s own derive (visitor-based) and the host builds the
  `Value`; no `OghamRead` / `OghamWrite` traits and no capability crate. Revisit
  only if a shared host helper later wants a named trait.
- **[OPEN] Cut sequencing** — order the safe-immediate cuts vs. the
  decision-gated ones (portal migration); confirm nothing in the "safe" list has
  a hidden consumer.
- **[LEANING] Local-state machinery slimming** — with lifecycle hooks cut, the
  path-based hook identity (§9) serves only `state`. Keep the cell; consider
  radically simplifying the identity machinery behind it.
- **Eventually:** translate the [DECIDED] items into `INTENT.md` tenets
  (identity/scope tenet; `Surface`-as-isolation rewrite of §6; the host/local
  state-discipline tenet; the generic-editing seam — one `editable` derive,
  read+write, no serde in the UI path). **Not yet** — that's the end of the
  workshop, not the middle.
