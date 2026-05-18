# Ogham — Untold Lore Adoption Readiness

> **Status: Ogham-side ready; UL adoption (Pass 2) outstanding.**
> All Ogham primitives UL needs (Phase 2 lifecycle + Portal, Phase
> 2.5 layer system + cursor / key signals + hot-reload reset, Phase
> 3 drag events + contextmenu + drain-time unmount) shipped
> 2026-05-05. AGENTS.md and the subsystem docs reflect the full
> shipped surface. The remaining work is consumer-side: UL Pass 2,
> a ~12 person-day per-UI migration to the new APIs, including the
> lorekeeper input-pump rewire (`dispatch_drag_*` /
> `dispatch_contextmenu`) and the cursor / key-signal coordination
> calls. See "Status snapshot" below for the full breakdown.
>
> Originally derived from a survey of UL's forward-looking UI
> documentation (`UI_RUNTIME.md`, `UI_SHELL.md`, `SETTINGS.md`,
> `ROADMAP.md`) against what Phase 2 shipped on 2026-05-05.
>
> Companion to `LIFECYCLE_AND_PORTAL.md` (the design),
> `LIFECYCLE_AND_PORTAL_UL_AUDIT.md` (the per-UI migration
> verdicts), and `LIFECYCLE_AND_PORTAL_IMPLEMENTATION.md` (the
> per-merge implementation history). Read those first for
> context.

---

## TL;DR

**Phase 2.5 + Phase 3 + docs-pass all shipped 2026-05-05** —
closed the entire §2 API gap. Layer system, cursor coordination
signal, key suppression contract, hot-reload reset, drag
events (start/move/end + accepts_drop + drag_preview),
contextmenu, drain-time unmount semantics — all in. `AGENTS.md`
revised to fold in Phase 1 (typed bindings), Phase 2 (lifecycle
hooks + Portal), Phase 2.5 (layers + cursor + key signals), and
Phase 3 (drag + drain-time). `SKILL.md` gotchas refreshed.
LIFECYCLE_AND_PORTAL.md still describes the Phase 2 single-layer
Portal but its banner redirects to the per-phase implementation
docs for current truth. Still remaining for UL adoption:

- **UL Pass 2 — adoption** — 12 person-days of per-UI work
  per the audit's verdicts. Includes the lorekeeper-side
  input-pump rewire to call `Ogham::dispatch_drag_*` /
  `dispatch_contextmenu` and the cursor / key-signal
  coordination calls.

Resolved sequencing: **Phase 2.5 + Phase 3 + docs-pass
(✓ shipped) → UL Pass 2.**

---

## Status snapshot

| Area | State |
|---|---|
| UL build against current ogham `main` | ✓ Clean (verified 2026-05-05) |
| Phase 2 ogham primitives | ✓ Shipped (M0–M5 + audit) |
| Phase 2.5 ogham primitives | ✓ Shipped 2026-05-05 (M0–M5; timer deferred per scope) |
| Phase 3 ogham primitives | ✓ Shipped 2026-05-05 (M0–M4; drag events + drain-time unmount) |
| UL `UI_RUNTIME.md` minimum API surface | ✓ Complete |
| UL `OverlayStack` migration ready | ✓ Gated only on docs revision now |
| UL Settings save-on-close ready | ⚠ Still gated on UL-side instance-swap restructuring (see §4) |
| UL inventory tooltip ready | ✓ Real `tooltip` layer exists; awaits docs revision |
| UL inventory drag-drop ready | ✓ Phase 3 drag events shipped; awaits docs revision + UL-side wiring |

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

### 2.3 Drag events ✓ CLOSED (Phase 3 shipped 2026-05-05)

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

**Phase 3 ships** (see `PHASE_3_IMPLEMENTATION.md`
"What shipped"):
- `drag_start` / `drag_move` / `drag_end` event listeners on
  any widget; payload threaded through `Event.payload`.
- `accepts_drop: fn (payload: bool): bool` widget property
  → drop-target hit-test walks portal layers (high→low) then
  base tree, picks the deepest accepting widget.
- `drag_preview: <widget>` property → UI tracks
  `active_drag_preview`; Skia renderer pushes a synthetic
  `CursorAttached` PortalEntry at the cursor each frame.
- `contextmenu` listener fires via
  `UI::dispatch_contextmenu(point)` on the deepest widget.
- Per-widget `drag_payload` and `drag_dead_zone` properties
  for source declaration and threshold override.
- Public API on both `UI` and `Ogham`:
  `dispatch_drag_start/move/end`, `dispatch_contextmenu`,
  `hit_test_drop_target`. Hosts (lorekeeper input pump) call
  these from their dead-zone state machine.

**What still needs UL-side work** (Pass 2):
- Lorekeeper input pump translates the existing dead-zone
  state machine in `ogham/src/client/input.rs` into the new
  `dispatch_drag_*` calls. ETA ~half a day; mechanical.
- UL inventory's drag-drop migration to `accepts_drop` +
  `drag_payload` + `drag_preview`. ETA ~1-2 days
  (per-widget refactor, not architectural).

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
in the overlay stack and stays mounted across overlays.

- [ ] **Wait for Phase 2.5 §2.1** — per the resolved
      sequencing (no UL adoption before docs revision pass),
      the early-adopt-and-re-migrate option is no longer on
      the table. Tooltip migrates against the real `tooltip`
      layer once Phase 2.5 ships and docs are revised.

### 4.5 Inventory drag-drop (Phase 3 consumer)

Per `ITEMS_UX.md` and the resolved sequencing in §7, inventory
drag-drop migrates against the real Phase 3 drag primitives.
Not part of the M5 audit's verdict list; surfaced here because
the Phase 3 → UL Pass 2 sequencing makes it a first-class
adoption target.

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

Resolved sequencing as of planning. Engine work runs as two
phases (2.5 and 3) followed by a docs revision pass; UL
adoption is the final pass.

### Phase 2.5 — close the in-scope API gap (engine-side)

Per `PHASE_2_5_IMPLEMENTATION.md`. Five merges + docs:

1. **P25-M0** — Portal layer system + viewport-absolute
   coords (~700 LOC, foundation).
2. **P25-M1** — Cursor coordination signal (~150 LOC).
3. **P25-M2** — Focus management script API + key
   suppression contract (~400 LOC, crosses lorekeeper).
4. **P25-M3** — Drain-time unmount refinement + hot-reload
   reset (~250 LOC).
5. **P25-M4** — Timer primitive (audit + ship if missing).
6. **P25-M5** — Docs graduation.

Estimated: ~10 person-days, ~1,500 LOC, ~46 new tests.

### Phase 3 — Drag events (engine-side, follows 2.5 immediately)

Largest self-contained chunk in `UI_RUNTIME.md` §3. Split
out from Phase 2.5 to keep that phase tight; lands before
UL adoption per the resolved sequencing in
`PHASE_2_5_IMPLEMENTATION.md` decision #2.

Will need its own implementation plan doc when Phase 2.5
M5 graduates. Estimated ~700 LOC + ~15 tests, ~3 person-days.

### Docs + Ogham-skill revision pass (between Phase 3 and UL adoption)

Once both engine phases ship, the reference documentation
needs a substantive revision pass:

- `ogham/AGENTS.md` (530 lines, canonical language reference)
  — needs Portal-layers, lifecycle hooks, focus API, cursor
  coord, timer, drag.
- `untold_lore/.agents/skills/ogham/SKILL.md` (62 lines, the
  pointer skill in UL's repo) — gotchas list (lines 43–57)
  needs a refresh; today it doesn't mention any Phase 2
  primitive (lifecycle, Portal) let alone the 2.5/3
  additions.
- `ogham/README.md` — short marketing teaser; review for
  staleness.
- `ogham/docs/internal/` triplet docs — possibly tidy up the
  audit doc which was scoped to Phase 2's primitives only.
- `INTENT.md`, `LANGUAGE.md`, `LSP.md` — review for
  staleness.

Estimated: ~2 person-days. Not part of any phase's M5 — it's
its own focused pass once the surface is stable.

### Pass 2 — UL adoption (UL-side, after engine + docs land)

1. **§5 OverlayStack migration**. Largest UL change. Gates §4.1
   and §4.2.
2. **§4.1 Settings save-on-close**. Validates `on_unmount` +
   event dispatch in the new shell.
3. **§4.2 Escape menu Portal**. Validates focus_trap + backdrop
   + nested portals + `has_input_blocking_portal`.
4. **§4.3 Inventory tooltip**. Validates the `tooltip` layer
   and non-modal Portal pattern.
5. **Inventory drag-drop** (per `ITEMS_UX.md`). Validates the
   Phase 3 drag primitives.
6. **§4.4 backlog**. Each UI on its own pace.

### Sanity gates throughout

- [ ] After every Phase 2.5 / Phase 3 merge:
      `cd ../untold_lore && cargo build` green.
- [ ] After Phase 2.5: triplet-doc graduation (Phase 2.5 M5).
- [ ] After Phase 3: triplet-doc graduation (Phase 3 M5).
- [ ] After docs revision pass: UL agents reading
      `SKILL.md` cold can navigate to the new primitives
      without consulting old session memory.
- [ ] After each Pass 2 migration: UL launches; the migrated
      UI behaves correctly in playtest.

---

## 8. Resolved planning decisions

1. **Phase 2.5 vs informal iterations.** ✓ Phase-with-discipline,
   spec'd in `PHASE_2_5_IMPLEMENTATION.md`.
2. **Drag events sequencing.** ✓ Phase 3, runs after Phase 2.5,
   *before* UL adoption. UL's inventory drag-drop should be
   built against the real primitive, not worked around.
3. **OverlayStack migration order.** ⚠ UL-side; can start once
   Phase 2.5 M0 ships (the layer system contract becomes
   stable then). Coordinate with UL team.
4. **Schema-diagnostics workstream coordination.** Open. Needs
   either a recurring sync or a CI gate. Flag for the team.
5. **Documentation discipline.** ✓ Resolved via the explicit
   "docs + Ogham-skill revision pass" between Phase 3 and UL
   adoption (per `PHASE_2_5_IMPLEMENTATION.md` §"What follows
   Phase 2.5"). Each phase's M5 also updates relevant
   subsystem docs inline.

---

## What this doc does NOT do

- **Doesn't design Phase 2.5/3.** Each gap above needs a real
  design pass. This doc just identifies the gaps.
- **Doesn't migrate any UI.** That's Pass 2 work.
- **Doesn't cover non-UI workstreams.** Schema-diagnostics is
  flagged for coordination only.
- **Doesn't bind sequencing as committed.** §7 is a
  recommendation; the team's other priorities may reshuffle.
