# Ogham — Untold Lore Adoption Readiness

> **Status: Planning doc.** What needs to be true before UL can
> begin adopting Phase 2's lifecycle hooks and Portal widget.
> Derived from a survey of UL's forward-looking UI documentation
> (`UI_RUNTIME.md`, `UI_SHELL.md`, `SETTINGS.md`, `ROADMAP.md`)
> against what Phase 2 shipped on 2026-05-05.
>
> Companion to `LIFECYCLE_AND_PORTAL.md` (the design),
> `LIFECYCLE_AND_PORTAL_UL_AUDIT.md` (the per-UI migration
> verdicts), and `LIFECYCLE_AND_PORTAL_IMPLEMENTATION.md` (the
> per-merge implementation history). Read those first for
> context.

---

## TL;DR

Phase 2 shipped a **minimal portal + lifecycle**; UL's
`UI_RUNTIME.md` describes a **richer engine surface** UL needs
(named portal layers with priority + backdrop policies,
script-exposed focus management, drag events with hit-testing,
cursor-coordination signal). The Phase 2 ↔ UL-target gap is
substantial.

The adoption blocker isn't UL hesitation — it's that Phase 2's
shipped surface doesn't yet meet UL's documented minimum API.
Adopting today would force UL to build on primitives that
need to grow significantly. Concrete examples:

- Phase 2 ships a single `portal_layer` with mount-order
  stacking. UL wants 5 named layers with priority ordering and
  per-layer backdrop policies (`overlay-modal` defaults to
  `block`, `tooltip`/`popover` default to `none`).
- Phase 2's `try_set_focus` is internal. UL wants script-
  exposed `focus()` action and `runtime.focused_widget()`.
- Phase 2 has no key-suppression contract for focused
  TextInputs. UL wants the runtime to consume
  `Key::Character(_)` for focused text widgets *before* the
  game-side input pump sees them.
- Phase 2 has no drag, no contextmenu event, no
  cursor-coordination signal. UL needs all three.

Adoption sequencing should be: **close the API gap first,
then migrate UIs.** Migrating against today's surface and then
re-migrating once layers/focus/drag land would be wasted work.

---

## Status snapshot

| Area | State |
|---|---|
| UL build against current ogham `main` | ✓ Clean (verified 2026-05-05) |
| Phase 2 ogham primitives | ✓ Shipped (M0–M5 + audit) |
| UL `UI_RUNTIME.md` minimum API surface | ✗ Significant gaps |
| UL `OverlayStack` migration ready | ✗ Gates on portal layers + focus management |
| UL Settings save-on-close ready | ✗ Gates on instance-swap restructuring (see §4) |
| UL inventory tooltip ready | ⚠ Possible today via single-layer portal; would re-migrate when layers land |

---

## 1. Build sanity (immediate)

Verify on a clean checkout that UL builds against current ogham
`main`. The schema-diagnostics workstream's WIP can leave the
ogham crate in a transiently-broken state (uncommitted
`src/diagnostics/` references; missing `mod check` / `mod render`
were observed earlier this session).

- [ ] **Confirm UL `cargo build` is green** on a fresh
      `cargo clean && cargo build`.
- [ ] **Coordinate with the schema-diagnostics workstream** so
      their WIP commits land cohesively (no `pub mod X;`
      references without the corresponding files). This is
      their workstream's responsibility, not Phase 2's, but it
      blocks UL adoption when broken.
- [ ] **Establish a CI signal** that catches "ogham main breaks
      UL." Either a workspace member, a UL-side smoke test in
      CI, or a pre-commit hook on ogham that runs
      `cd ../untold_lore && cargo check`.

---

## 2. Close the Phase 2 ↔ UL API gap

`UI_RUNTIME.md` enumerates five runtime primitives UL needs.
Phase 2 shipped a starting point on portals and focus, none of
the rest. Each gap below is engine work *before* UL adoption
can begin.

### 2.1 Portal layer system (gap: significant)

**UL wants** (`UI_RUNTIME.md` §1):
- Named layers: `main` (0), `overlay-modal` (100), `popover`
  (200), `tooltip` (300), `toast` (400), `cursor-attached`
  (500). Priority ordering across layers; declaration order
  within a layer.
- Per-layer backdrop policy: `none` / `dim` / `block`.
  `overlay-modal` defaults `block`; others `none`.
- API: `Portal { layer: "modal", children, on_dismiss? }`.
- Pointer routing: click lands on topmost layer with a hit;
  lower layers receive nothing if topmost is `block`.

**Phase 2 ships**:
- One unnamed portal_layer, mount-order stacking.
- No backdrop policy (consumer composes a backdrop child).
- API: `Portal { open, focus_trap, children }`.
- Hit-test searches portal_layer first (LIFO) then falls
  through unconditionally (no block policy).

**Work needed**:
- [ ] Add `layer: String` property to Portal widget.
- [ ] Define the layer registry in `src/widget/mod.rs` as a
      runtime-known set (5 layers per UL's spec). Layer
      registration is *not* extensible from userspace — that's
      a feature, not a bug per `UI_RUNTIME.md`.
- [ ] Per-layer backdrop policy enum + per-frame composition.
      Skia draw + hit-test honor the policy.
- [ ] Replace single `portal_layer: Vec<PortalEntry>` with
      `portal_layers: HashMap<LayerName, Vec<PortalEntry>>` (or
      a sorted Vec ordered by priority).
- [ ] Migrate the M3 → M4 hit-test order to walk by layer
      priority (higher first), then within-layer LIFO.
- [ ] Update `LIFECYCLE_AND_PORTAL.md` to spec the layer system
      (currently spec'd as single-layer); this is a real spec
      delta, not an addition.
- [ ] Decide: does the existing `focus_trap: bool` stay, or
      does `overlay-modal` layer's `block` policy subsume it?
      `UI_RUNTIME.md` keeps focus_trap as opt-in *on top of*
      layer policy. Match that.

**Estimated effort**: ~600 LOC + ~12 tests. Mid-sized.
Conceptually a follow-up to M3.

### 2.2 Focus management script API (gap: medium)

**UL wants** (`UI_RUNTIME.md` §2):
- `runtime.focused_widget() -> Option<WidgetId>` script-callable.
- `focus()` action available on widget refs (not just internal
  `try_set_focus`).
- Focus traps via Portal property (matches Phase 2 ✓).
- Key suppression contract: runtime consumes `Key::Character(_)`
  for focused text widgets before game-side input pump.
- `input.pressed_unfiltered(...)` opt-in for editors.
- Click-outside loses focus (with layer opt-out for modals).

**Phase 2 ships**:
- Internal `UI::try_set_focus`, `UI::has_input_blocking_portal`.
- Focus stack with sync-from-portal-layer.
- `Ogham::has_input_blocking_portal()` public.
- No script-exposed focus surface.
- No key suppression contract.

**Work needed**:
- [ ] Expose `focused_widget()` as a built-in to `.ogh` scripts.
      Add to runtime built-ins alongside `event(...)` etc.
- [ ] Expose `focus(widget_ref)` as a built-in or method on
      widget refs.
- [ ] Add the key suppression contract on the
      lorekeeper/input side: a contract method
      `runtime.consumes_character_key() -> bool` that
      input-pump checks before populating `pressed()`. Define
      the suppression set (Character only, NOT Escape /
      F-keys / arrows / Tab / modifiers).
- [ ] `input.pressed_unfiltered(...)` on the input side for
      editor opt-in.
- [ ] Click-outside-loses-focus rule. Layers with `block`
      policy opt out (focused widget inside an `overlay-modal`
      portal doesn't lose focus when clicking the dimmed
      backdrop).

**Estimated effort**: ~400 LOC + ~10 tests. Crosses the
ogham/lorekeeper boundary (input-pump side is in lorekeeper).

### 2.3 Drag events (gap: total)

**UL wants** (`UI_RUNTIME.md` §3):
- `drag_start(payload)` on originator after dead-zone (4px
  default).
- `drag_move(payload, position)` on widgets entered.
- `drag_end(payload, drop_widget?)` on drop target or
  originator.
- `accepts_drop(payload) -> bool` widget attribute.
- `drag_preview` widget attribute (renders into
  `cursor-attached` layer).
- `contextmenu(position)` event distinct from `click`.

**Phase 2 ships**: nothing.

**Today's substrate (per `UI_RUNTIME.md`)**: dead-zone state
machine already lives in `ogham/src/client/input.rs:25–198`.
What's missing is the event-emission layer.

**Work needed**:
- [ ] `drag_start` / `drag_move` / `drag_end` event types in
      the widget event system.
- [ ] Drop-target hit-testing (which widget under cursor at
      release accepts the payload).
- [ ] `accepts_drop` widget attribute / predicate.
- [ ] `drag_preview` widget attribute → renders into
      `cursor-attached` layer (gates on §2.1 layer system).
- [ ] `contextmenu` event distinct from `click`.

**Estimated effort**: ~700 LOC + ~15 tests. Largest
single-feature item. Gates the inventory drag-drop migration
(`ITEMS_UX.md`).

### 2.4 Cursor coordination signal (gap: small but sequenced)

**UL wants** (`UI_RUNTIME.md` §4):
- `runtime.wants_cursor_free() -> bool`.
- Per-portal `cursor: "free" | "inherit"` declaration.
- Per-widget cursor declaration (focused TextInput → cursor-free
  implicitly).

**Phase 2 ships**: `Ogham::has_input_blocking_portal()` — the
moral equivalent for "is something modal open" but not the
broader cursor-free signal.

**Work needed**:
- [ ] `Runtime::wants_cursor_free()` — true if any focused
      widget or any active portal in `overlay-modal` /
      `popover` declares cursor-free.
- [ ] Per-portal `cursor` property (defaults to `free` for
      `overlay-modal`).
- [ ] Per-widget cursor declaration on focusable widgets.

**Estimated effort**: ~150 LOC + ~5 tests. Small. Gates UL's
`update.rs:1561` cursor-lock simplification.

### 2.5 Timer primitive (gap: probably done)

**UL wants** (`UI_RUNTIME.md` §5):
- `set_timeout(delay_ms, callback)` returning a handle.
- `set_interval(period_ms, callback)`.
- `clear_timeout(handle)` / `clear_interval(handle)`.
- Auto-cancel on widget unmount.

**Phase 2 ships**: not directly. `effect (deps) { cleanup }`
can approximate a self-cleaning subscription, but not a
fire-once-after-delay.

**Work needed**:
- [ ] **Audit first**: `panel_transition` in `main.ogh:10–15`
      implies scheduled interpolation already exists somewhere.
      Find what's there before adding new API.
- [ ] If a timer primitive doesn't exist: add `set_timeout` /
      `set_interval` / `clear_*` as runtime built-ins. The
      auto-cancel on unmount is the new piece — pairs well
      with Phase 2's path-keyed lifecycle.

**Estimated effort**: 0–200 LOC depending on audit outcome.

---

## 3. Phase 2 surface tightening

Independent of UL's UI_RUNTIME ask, Phase 2 has documented
limitations that should be addressed before UL relies on the
primitives.

### 3.1 Path-disappear → drain-time unmount semantics

Currently `on_unmount` fires when a path stops being visited
(M1 deviation). UL's overlay-instance-swap pattern doesn't
fire `on_unmount` at all — runtime drop has no diff.

- [ ] **Decide**: refine to true drain-time semantics, OR
      explicitly support "instance close fires unmount" (a
      new mechanism for graceful drop).
- [ ] Either path needs design work; not a one-line fix.
      Implement when the first canonical UL migration
      (Settings save) runs into the problem.

### 3.2 Mount-after-layout

Currently mount fires inside `rerender()` before layout. UL
hasn't called this out specifically, but Portal positioning
patterns (the inventory tooltip use case) read post-layout
sizes from the trigger.

- [ ] Refine when Portal positioning needs it. Could land
      alongside the layer system in §2.1.

### 3.3 Pass B viewport-absolute coordinates

Documented limitation in `paint_portal_entry`. Root-level
portals work; nested portals render at wrong viewport position.

- [ ] Track cumulative translates during Pass A; capture
      viewport-absolute coords in `PortalEntry.parent_rect`.
- [ ] Lands naturally as part of §2.1 (layer system rewrite).

### 3.4 `set_compiled_module` reset on hot reload

`UI::clear_lifecycle_state` exists but isn't wired to the
hot-reload path in `Ogham::reload_file` /
`recompile_from_source`.

- [ ] Wire `clear_lifecycle_state` into the reload path so
      stale focus restoration doesn't survive into a torn-down
      tree.

---

## 4. Per-UI migration readiness

The `LIFECYCLE_AND_PORTAL_UL_AUDIT.md` per-UI verdicts assumed
Phase 2's surface as shipped. With the §2 gap closure
re-sequencing, the dependency graph for each migration shifts.

### 4.1 Settings save-on-close

**Audit verdict**: High value. M5 canonical migration.

**Blocker**: UL's overlay-instance-swap pattern. When Settings
closes, the entire Ogham instance for the overlay is dropped —
no path-disappear, no `on_unmount` fires. The pattern UL is
moving toward is `OverlayStack` (per `UI_SHELL.md` §B2), where
Settings is a Portal pushed into the `overlay-modal` layer of
a single long-lived Ogham instance. Once that's true,
`on_unmount` fires when the portal closes.

- [ ] **Cannot adopt today** without restructuring overlay
      handling. Wait for §5 (OverlayStack migration).
- [ ] When OverlayStack lands, this migration becomes the
      ~50 LOC change the audit estimated.

### 4.2 Escape menu Portal migration

**Audit verdict**: Highest leverage. M5 canonical migration.

**Blocker**: same as 4.1 (overlay-instance-swap). Plus: needs
the `overlay-modal` layer with `block` backdrop policy from
§2.1 to work as the audit envisioned.

- [ ] **Cannot adopt today** without §2.1 (layer system) AND
      §5 (OverlayStack migration).
- [ ] When both land, this migration validates focus-trap +
      backdrop-as-child + nested confirm + overlay-active
      derivation in one go.

### 4.3 Inventory tooltip (worked non-modal example)

**Audit verdict**: Worked example.

**Blocker**: lighter than 4.1/4.2. The `inventory_hud.ogh` is
a non-gating world panel (per `UI_SHELL.md` §B3b) — it's NOT
in the overlay stack and stays mounted across overlays. So
adding a tooltip to inventory cells is feasible *today* with
Phase 2's single portal_layer. The catch: it would need to
re-migrate once §2.1 lands (move from unnamed layer to
explicit `tooltip` layer).

- [ ] **Could adopt today** as a smoke test, accepting that
      it'll re-migrate when layers land.
- [ ] **OR wait** for §2.1 and migrate once. Cleaner.

### 4.4 Other per-UI migrations (post-M5 backlog)

The audit listed ~12 person-days across 12 medium-priority
migrations. Most are gated on the same primitives:

- Character Select / Talents / Tip Log / Crafting / DM
  Inventory `on_mount` for static seeding: feasible today.
- All `effect (host_state.X)` patterns: feasible today.
- All Portal-using migrations (Settings keybind capture,
  Social create-faction form, Inventory shop overlay, DM
  context menus, etc.): wait for §2.1.
- Editor save-on-close (Map / Blueprint / Ruleset /
  LifeStages): wait for §5 (OverlayStack — these are
  full-screen GameStates, but the pattern overlap is similar).

---

## 5. Shell-level migration: OverlayState → OverlayStack

`UI_SHELL.md` §B2 + §I document UL's plan to migrate from
flat `OverlayState` enum to a userspace `OverlayStack` state
machine that pushes runtime portals.

**Why this matters for adoption**: Settings save-on-close
(§4.1) and Escape menu Portal (§4.2) both depend on this
migration. Without it, UL's overlay-instance-swap pattern
makes lifecycle hooks awkward to use.

- [ ] **This is UL-side work**, not ogham work, but the
      ordering matters for adoption planning.
- [ ] Gates on §2.1 (portal layer system) per UL's own doc:
      "Without portals, the userspace stack has no escape
      mechanism for rendering and the migration recreates
      today's flat-overlay problem under a different name."
- [ ] Estimated effort (UL-side): non-trivial — touches
      `client/mod.rs`, `update.rs`, `actions.rs`, plus every
      overlay UI's open/close path.
- [ ] Provides natural M5-equivalent validation for the
      ogham layer system.

---

## 6. Schema-diagnostics workstream coordination

Separate parallel workstream that touched ogham's `lib.rs`
and added `src/diagnostics/`. Has caused transient UL build
breakage in this session.

- [ ] **Coordinate** so their commits land atomically (every
      `pub mod X;` in `lib.rs` has its corresponding file).
- [ ] **Their derive macros (`OghamState`, `OghamMsg`) use
      `::ogham::*` paths that don't resolve when the macro
      is invoked inside the `ogham` crate itself**. The
      workstream's `src/diagnostics/manifest.rs` test module
      hits this. Their workstream to fix; flagging here so
      it's tracked.
- [ ] Once their work lands, UL gains the cross-side
      `.ogh` ↔ Rust drift detection. Independent of Phase 2.

---

## 7. Recommended sequencing

A pragmatic order, balancing engine work vs. UL adoption:

### Pass 1 — close the API gap (engine-side)

1. **§2.1 portal layer system** (~600 LOC, mid-size). Largest
   item; everything else builds on it.
2. **§2.4 cursor coordination signal** (~150 LOC, small).
   Cleanest place to land while §2.1's structure is fresh in
   mind.
3. **§2.2 focus management script API** (~400 LOC). Crosses
   ogham/lorekeeper boundary; budget time for cross-repo
   coordination.
4. **§3.1 path-disappear → drain-time unmount semantics**
   design + implementation. Surface the design tradeoff:
   pure drain-time vs. instance-close-fires-unmount.
5. **§3.3 viewport-absolute coords** (small; folds into §2.1).
6. **§3.4 wire `clear_lifecycle_state` into hot-reload** (10
   LOC, do alongside §3.1).
7. **§2.5 timer primitive audit + ship if missing** (small).
8. **§2.3 drag events** (~700 LOC, largest). Independent of
   Pass 1 functionally — could move earlier or later. Do last
   because it's its own self-contained chunk.

Phase 2's docs need updating throughout to match new layer/
focus/cursor specs. The triplet (design / audit / impl plan)
remains the right shape.

### Pass 2 — UL adoption (UL-side)

1. **§5 OverlayStack migration**. Largest UL change. Gates §4.1
   and §4.2.
2. **§4.1 Settings save-on-close**. Validates `on_unmount` +
   event dispatch in the new shell.
3. **§4.2 Escape menu Portal**. Validates focus_trap + backdrop
   + nested portals + `has_input_blocking_portal`.
4. **§4.3 Inventory tooltip**. Validates the `tooltip` layer
   and non-modal Portal pattern.
5. **§4.4 backlog**. Each UI on its own pace.

### Sanity gates throughout

- [ ] After every Pass 1 step: `cd ../untold_lore && cargo build`
      green.
- [ ] After Pass 1: a fresh holistic audit of Phase 2 + the
      added primitives. Triplet-doc graduation.
- [ ] After each Pass 2 migration: UL launches; the migrated
      UI behaves correctly in playtest.

---

## 8. Open decisions before starting

Items that need a call before Pass 1 begins:

1. **Phase 2.5 vs Phase 3?** The §2 work is substantial —
   feels too large for "audit follow-up" but smaller than a
   fresh Phase. Suggest: Phase 2.5 (single triplet doc covering
   layers + cursor + focus script API + drain-time refinement),
   estimated ~10 person-days.
2. **Drag events in Phase 2.5 or Phase 3?** Drag is the largest
   self-contained chunk. Including in Phase 2.5 doubles its
   scope; deferring to Phase 3 leaves UL's inventory drag-drop
   blocked. Either choice is defensible.
3. **OverlayStack migration order vs Pass 1.** UL could start
   the OverlayStack work in parallel with Pass 1's §2.1 (since
   they're in different repos). Just need clear contracts on
   what the layer system's API surface will be before UL starts
   building against it.
4. **Schema-diagnostics workstream coordination cadence.** Need
   a recurring sync OR a CI gate that catches cross-workstream
   breaks.
5. **Documentation discipline.** UL's `UI_RUNTIME.md` says "once
   Ogham implements a primitive, the canonical reference moves
   to `ogham/docs/`." Plan that as part of Pass 1 — each
   shipped primitive lands a section in the appropriate
   ogham doc and `UI_RUNTIME.md` shrinks accordingly.

---

## What this doc does NOT do

- **Doesn't design Phase 2.5/3.** Each gap above needs a real
  design pass. This doc just identifies the gaps.
- **Doesn't migrate any UI.** That's Pass 2 work.
- **Doesn't cover non-UI workstreams.** Schema-diagnostics is
  flagged for coordination only.
- **Doesn't bind sequencing as committed.** §7 is a
  recommendation; the team's other priorities may reshuffle.
