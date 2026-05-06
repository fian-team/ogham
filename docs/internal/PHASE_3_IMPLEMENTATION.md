# Phase 3 — Drag events + drain-time unmount refinement

> **Status: Shipped 2026-05-05.** All five merges (P3-M0..M4)
> are complete; see "What shipped" at the bottom of this doc.
> Companion to
> `UL_ADOPTION_READINESS.md` §2.3 (drag events as the
> remaining Phase 2.5 ↔ UL gap), `PHASE_2_5_IMPLEMENTATION.md`
> "What's next" (drain-time deferred from M3), and
> `LIFECYCLE_AND_PORTAL.md` (Phase 2 design Phase 3 extends).
>
> **Goal**: ship drag events (`drag_start`/`drag_move`/`drag_end`,
> `accepts_drop`, `drag_preview`, `contextmenu`) per UL's
> `UI_RUNTIME.md` §3, AND fold in the deferred drain-time
> unmount semantics from P25-M3 (shared "thread state through
> the widget tree" architectural problem). After Phase 3, the
> remaining gates before UL adoption are the docs+skill
> revision pass and UL Pass 2 itself.
>
> **Estimated**: ~1,000 LOC implementation + ~25 tests across
> 5 merges. ~5 person-days. The "2-3 person-days" estimate in
> earlier plans was for drag alone; folding in drain-time
> brings it to a real medium-sized phase.

---

## At a glance

| Merge | Title | LOC | Tests | Risk | Hardest part |
|---|---|---|---|---|---|
| P3-M0 | DragContext threading + tick context refactor | ~250 | ~6 | High | Replacing `tick_animations(&mut self, dt) -> TickResult` chain with a context-passing pattern that supports drag + drain wiring without a Vec<String> on every TickResult |
| P3-M1 | Drag events: drag_start / drag_move / drag_end | ~350 | ~10 | Medium | Drop-target hit-testing across portal layers + payload typing |
| P3-M2 | drag_preview + contextmenu | ~200 | ~5 | Low | drag_preview rendering into cursor-attached layer (M0 layer system makes this straightforward) |
| P3-M3 | Drain-time unmount semantics | ~150 | ~6 | Medium | Wiring the deferred work onto the new context infrastructure |
| P3-M4 | Docs graduation + Phase 3 wrap | ~50 docs | — | Low | Capturing decisions; "what's next" pointer to docs+skill revision |

**Total: ~1,000 LOC + ~27 tests**, ~5 person-days. Single-branch,
"rip and tear" between merges.

### Sequencing rationale

P3-M0 is the architectural foundation — both drag and
drain-time unmount need to thread mutable state through the
widget tree's per-frame walks. Doing it once in M0 amortizes
across both features and avoids a TickResult<Vec<String>>
that would touch every widget that overrides `tick_animations`.

M1 ships the core drag event surface (start/move/end +
accepts_drop + drop hit-test). M2 layers drag_preview
rendering on top of M0's layer system + adds the small
contextmenu event. M3 finally honors the design's drain-time
unmount contract. M4 graduates docs.

### Validation gate at each merge

Per the established convention:
- `cargo build --workspace` clean
- `cargo test --workspace` green (Phase 2 + 2.5 regressions
  caught by suites)
- UL builds clean (`cd ../untold_lore && cargo build`)
- New tests pass

---

## P3-M0 — DragContext + tick context refactor

### Goal

Establish the architectural pattern that lets drag events
(M1) and drain-time unmount (M3) share infrastructure: a
mutable `TickContext` threaded through `tick_animations`
and a parallel mutable context through event dispatch
(extending `EventContext`).

This is the merge that pays the architectural cost up
front so M1 + M3 can use the same plumbing.

### Deliverables

- New `TickContext` struct passed through `tick_animations`:
  ```rust
  pub struct TickContext {
      pub dt: f32,
      /// Phase 3: prefixes drained this tick. UI consumes
      /// after the recursion returns.
      pub drained_path_prefixes: Vec<String>,
      /// Phase 3: prefixes whose exit was cancelled this tick.
      pub cancelled_unmount_prefixes: Vec<String>,
  }
  ```
- `Widget::tick_animations` signature changes to
  `tick_animations(&mut self, ctx: &mut TickContext) -> TickResult`.
  Default implementation: `TickResult::NONE` (unchanged
  semantics; just signature).
- `FlexWidget::drain_exited_children` pushes the dropped
  widget's `owned_path_prefix` to `ctx.drained_path_prefixes`.
- `FlexWidget::cancel_exit` pushes to
  `ctx.cancelled_unmount_prefixes`.
- `UI::tick_animations(dt)` constructs a `TickContext`,
  walks the tree, then drains the populated vecs into UI's
  per-frame state for Runtime to consume on next frame.
- New `UI.pending_drained_prefixes: Vec<String>` and
  `UI.pending_cancelled_prefixes: Vec<String>` fields,
  populated post-tick.
- Same context-passing pattern for `EventContext` is
  PRE-WIRED but not yet consumed (M1 will use
  `EventContext.drag_state` for drag dispatch; M0 just
  adds the field as `Option<DragState>` defaulting to None).

### Implementation steps

1. **Define `TickContext`** in `src/widget/event.rs` (or
   alongside `EventContext`).
2. **Update `Widget::tick_animations` trait method** to
   take `&mut TickContext` instead of `f32`. Default impl:
   `let _ = ctx; TickResult::NONE`.
3. **Update every widget's `tick_animations` impl** to
   accept the new signature. Most just rename the param;
   FlexWidget threads `ctx` into its child recursion.
4. **`FlexWidget::drain_exited_children`** signature
   changes to take `&mut TickContext`. When dropping a
   widget with non-empty `owned_path_prefix`, push to
   `ctx.drained_path_prefixes`.
5. **`FlexWidget::cancel_exit`** is called from
   `reconcile_children` (no tick context available there).
   Add a separate `cancelled_unmount_prefixes` Vec to UI;
   reconcile_children gets a different threading mechanism
   — actually, since it's already in the
   `UpdateResult` bubble, extend `UpdateResult` with a
   `cancelled_unmount_prefixes: Vec<String>` field.
6. **`UI::tick_animations(dt)`** constructs `TickContext`,
   walks tree, copies populated vecs into UI's pending fields.
7. **`UI::reconcile`** captures cancelled prefixes from
   the bubbled UpdateResult into UI's pending fields.
8. **`EventContext`** gains an `Option<DragState>` field
   (DragState defined as a placeholder struct; M1 fills
   in the actual drag tracking).

### Test matrix

`tests/tick_context.rs` (new):

1. `tick_context_drained_prefix_propagates_from_drain_exited_children`
2. `tick_context_empty_when_no_drain_happened`
3. `cancelled_prefix_propagates_from_cancel_exit_via_update_result`
4. `existing_widgets_still_implement_tick_animations` (smoke)
5. `tick_context_dt_passes_through` (sanity that dt isn't lost)
6. `ui_pending_drained_populated_after_tick` (integration)

### Cross-merge dependencies

- M0 must land before M1 (drag dispatch uses
  `EventContext.drag_state`) and M3 (drain-time consumes
  `pending_drained_prefixes`).
- Independent of M2 functionally.

### P3-M0 open questions

1. **Where does `cancel_exit` push?** It's called from
   `reconcile_children`, which is in the event/reconcile
   path, not the tick path. Options:
   - (a) Extend `UpdateResult` with cancelled prefixes.
   - (b) Add a thread-local for the duration of reconcile.
   - (c) Pass `&mut Vec` through `reconcile_children`.

   Recommend **(a)** — UpdateResult already bubbles up
   through reconcile; adding a Vec preserves the pattern.
   `UpdateResult` loses `Copy` but it's already a struct
   with `bool` fields, not really Copy-relied-upon.
2. **Does M0 also rename `tick_animations` parameter for
   stylistic consistency?** Currently `dt: f32`; new shape
   is `ctx: &mut TickContext`. Yes — better to rename now
   than rip up later.
3. **Is `EventContext.drag_state` exposed publicly?** Yes
   — widgets need to read drag-in-progress state for hover
   highlighting (`accepts_drop` glow per UI_RUNTIME §3).

### What M0 does NOT include

- Actual drag event dispatch (M1).
- drag_preview rendering (M2).
- Drain-time unmount consumption logic (M3).
- contextmenu event (M2).

### Validation gate

- All M0 tests pass.
- All Phase 2 + 2.5 tests still pass (no regressions in the
  ~80 existing tests).
- UL builds clean.
- Smoke: a widget with `exit:` style + `owned_path_prefix`
  drains; UI's `pending_drained_prefixes` contains the
  prefix after the tick.

---

## P3-M1 — Drag events: drag_start / drag_move / drag_end + accepts_drop

### Goal

Land the core drag event surface. Authors write `drag_start`,
`drag_move`, `drag_end` event handlers on widgets;
`accepts_drop(payload)` predicate determines drop targets.
Payload typing flows through.

### Deliverables

- `DragState` struct in `src/widget/event.rs`:
  ```rust
  pub struct DragState {
      pub origin_widget: WidgetRef,
      pub payload: Value,
      pub start_position: Point,
      pub current_position: Point,
      /// True once the cursor has moved past dead-zone (4px
      /// default; per-widget override).
      pub past_dead_zone: bool,
  }
  ```
- New widget event names: `drag_start`, `drag_move`,
  `drag_end`. Standard event-listener infrastructure
  (`register_event_listener` etc.) handles registration in
  the builder.
- Widget trait: `accepts_drop(&self, payload: &Value) -> bool`
  (default false).
- Builder reads `drag_payload: Value` widget attribute (the
  payload to start dragging on this widget) — required for
  drag_start to fire.
- Drop-target hit-testing during drag: walks portal_layers
  high-priority-to-low (M0 layer system), then base tree;
  finds deepest widget whose `accepts_drop(payload)` returns
  true. That widget receives `drag_end`.
- Lorekeeper-side input pump translates the existing
  dead-zone state machine (`mouse_button_released_after_drag`)
  into `drag_start`/`drag_move`/`drag_end` event dispatches.
  The pump constructs `DragState` and passes via
  `EventContext.drag_state`.

### Implementation steps

1. **Define `DragState`** in event.rs.
2. **`EventContext.drag_state: Option<DragState>`** populated
   by the dispatch path.
3. **Widget trait `accepts_drop`** method (default false).
4. **Builder**: standard event listeners for drag_start /
   drag_move / drag_end. New `drag_payload: Value` property
   stored on FlexWidget; drag_start uses it.
5. **Widget trait `drag_payload`** accessor (default None).
6. **Hit-test for drop targets**: new `UI::hit_test_drop_target`
   walks portal_layers + base tree returning the first
   widget whose `accepts_drop(payload)` returns true.
7. **Drag dispatch path**: when the lorekeeper input pump
   detects drag start, it calls `UI::dispatch_drag_start`;
   on subsequent mouse_move events while dragging, calls
   `UI::dispatch_drag_move`; on release, `UI::dispatch_drag_end`
   which uses hit_test_drop_target.
8. **Coordinate with lorekeeper-side input pump**: same
   pattern as P25-M2's key suppression — ogham exposes the
   API, lorekeeper consumes. Cross-repo timing is safe in
   either order.

### Test matrix

`tests/drag_events.rs` (new):

1. `drag_start_fires_on_originator_after_dead_zone`
2. `drag_move_fires_on_widgets_under_cursor_during_drag`
3. `drag_end_fires_on_drop_target_when_accepts_drop_true`
4. `drag_end_fires_on_originator_when_no_drop_target_accepts`
5. `accepts_drop_false_widget_doesnt_receive_drag_end`
6. `dead_zone_threshold_per_widget_override`
7. `drag_payload_round_trips_through_event`
8. `drag_state_in_event_context_during_drag`
9. `drop_target_hit_test_walks_portal_layers_first`
10. `drag_originating_in_overlay_modal_finds_targets_in_overlay_modal_only` (block-policy interaction)

### Cross-merge dependencies

- Depends on M0 (`EventContext.drag_state`).
- Touches lorekeeper input pump (cross-repo coordination).

### P3-M1 open questions

1. **Payload typing.** `Value` is dynamically typed; UL
   wants type-checked drops ("only inventory items in
   inventory cells"). Options:
   - (a) Pass `Value` through; consumer checks payload
     shape in `accepts_drop` predicate.
   - (b) Add a `payload_type: String` discriminator field.
   - (c) Defer to userspace convention (consumer puts a
     `kind: "inventory_item"` field in the payload Map).

   Recommend **(a) + (c)** — runtime stays untyped; consumer
   convention handles the discrimination. Matches how
   `event(...)` payloads work today.
2. **`accepts_drop` as predicate or static type filter?**
   UI_RUNTIME §3 says "predicate (or a static type filter)".
   Recommend **predicate only** — simpler; static type
   filter is a future optimization.
3. **What about touch / pen drag?** Out of scope for v1;
   mouse-only.

### What M1 does NOT include

- drag_preview rendering (M2).
- contextmenu event (M2).
- Auto-cancellation on Escape (could go here but adds
  complexity; defer to a follow-up unless UL needs it).

### Validation gate

- All M0 + M1 tests pass.
- UL builds clean.
- Smoke: a test `.ogh` with a drag-source widget and a
  drop-target widget can complete a full drag-drop cycle
  via simulated mouse events.

---

## P3-M2 — drag_preview + contextmenu

### Goal

Render drag previews into the `cursor-attached` portal
layer; add the `contextmenu` event distinct from `click`.

### Deliverables

- Widget trait: `drag_preview() -> Option<WidgetRef>` (default
  None). When a drag is in flight from this widget, the
  returned subtree renders attached to the cursor.
- Builder: `drag_preview` property accepts a widget (or
  array; first widget wins).
- During drag, the renderer pushes the preview into the
  `cursor-attached` portal layer with `viewport_rect`
  positioned at the cursor.
- `contextmenu` event: fires on the deepest widget under the
  cursor on right-click. Suppresses the default click
  handling for the right button.
- Lorekeeper input pump translates right-click events to
  `UI::dispatch_contextmenu`.

### Implementation steps

1. **Widget trait `drag_preview`** accessor.
2. **Builder**: parse drag_preview property; store on
   FlexWidget.
3. **Renderer**: when DragState is in flight, look up the
   originator's drag_preview; if Some, push a synthetic
   PortalEntry into `cursor-attached` layer with
   viewport_rect at cursor position.
4. **`contextmenu` event dispatch**: new `UI::dispatch_contextmenu`
   walks portal_layers + base tree finding the deepest
   widget at the cursor; fires `contextmenu` event with
   position payload.
5. **Builder**: register `contextmenu` listener like
   other mouse events.

### Test matrix

`tests/drag_preview.rs` (new):

1. `drag_preview_widget_renders_in_cursor_attached_layer`
2. `drag_preview_position_tracks_cursor`
3. `widget_without_drag_preview_dragging_doesnt_push_to_layer`

`tests/contextmenu.rs` (new):

4. `contextmenu_fires_on_deepest_widget_at_cursor`
5. `contextmenu_suppresses_right_click_default_handling`

### Cross-merge dependencies

- Depends on M0 (DragState exists), M1 (drag dispatch
  populates it).
- Independent of M3.

### What M2 does NOT include

- drag_preview animation/transition during drag (would be
  via standard `transition:` style — not Phase 3 specific).
- Right-click drag (very rare; mouse-only contextmenu).

### Validation gate

- All M0–M2 tests pass.
- UL builds clean.
- Smoke: drag from a source with drag_preview defined; the
  preview renders attached to the cursor as it moves.

---

## P3-M3 — Drain-time unmount semantics

### Goal

Honor the Phase 2 design's drain-time unmount contract that
P25-M3 deferred. Now that M0 has the TickContext infrastructure,
the wiring is straightforward.

### Deliverables

- `Runtime::process_drain_queues(ui: &mut UI)` consumes
  `ui.pending_drained_prefixes` and `ui.pending_cancelled_prefixes`,
  calling `flush_for_path_prefix` and `cancel_unmount_for_prefix`
  on the StateManager.
- `Runtime::rerender` no longer calls `queue_disappeared_unmounts`
  via path-disappear semantics; instead, it processes the
  drain queues populated by the previous frame's tick.
- Path-disappear semantics retained as a fallback for paths
  that disappear without a corresponding drain event (e.g.
  widgets that drop immediately without entering exit
  animation). Document the dual policy.
- `Ogham::tick(dt)` new method that runs the canonical
  per-frame sequence: tick_animations → process_drain_queues
  → check_for_changes → reconcile if needed.

### Implementation steps

1. **`Runtime::process_drain_queues`** consumes UI vecs
   into StateManager helpers.
2. **`UI::tick_animations` post-walk**: copy
   `ctx.drained_path_prefixes` into `ui.pending_drained_prefixes`.
3. **`Runtime::rerender`**: replace the
   `queue_disappeared_unmounts` call with
   `process_drain_queues`. Path-disappear fallback runs
   for paths that didn't generate a drain event (M1 behavior
   preserved as a safety net).
4. **`Ogham::tick(dt)`**: optional convenience method
   wrapping the per-frame sequence. Hosts can call it OR
   continue calling individual methods.
5. **Update test infra**: `tests/drain_time_unmount.rs`
   (the test file proposed in P25-M3 plan) — finally
   ships.

### Test matrix

`tests/drain_time_unmount.rs` (new):

1. `unmount_fires_after_exit_animation_completes`
2. `cancel_exit_during_animation_cancels_pending_unmount`
3. `cancel_then_re_exit_fires_unmount_on_eventual_drain`
4. `widget_with_no_exit_animation_unmounts_immediately`
   (path-disappear fallback)
5. `effect_cleanup_runs_at_drain_time_with_unmount`
6. `multiple_drain_events_in_one_frame_all_processed`

### Cross-merge dependencies

- Depends on M0 (TickContext + UI pending vecs).
- Independent of M1, M2 functionally.

### P3-M3 open questions

1. **Dual path-disappear + drain-time policy — which wins?**
   Recommend: drain-time supersedes for widgets that
   register a drain (any FlexWidget with an exit_style or
   owned children); path-disappear fallback for widgets
   that drop without notification. Document the dual policy
   clearly so authors aren't confused.
2. **Does cancel-then-re-exit fire unmount on the second
   exit's drain?** Yes — the cancel cleared the pending
   unmount; the re-exit creates a fresh one. Test #3
   covers this.

### What M3 does NOT include

- Mount-after-layout refinement (separate Phase 2 known
  limitation; out of Phase 3 scope unless trivially folded).

### Validation gate

- All M0–M3 tests pass.
- UL builds clean.
- Smoke: a Portal-housed dialog with `exit:` style fades
  out THEN unmount fires (not at exit-begin). Verify via
  `event("test_log", "...")` in the .ogh.

---

## P3-M4 — Docs graduation + Phase 3 wrap

### Goal

Capture all Phase 3 decisions. Update Phase 2 + 2.5 docs
where they're now superseded. Update the readiness doc to
reflect "drag closed; only docs revision pass + UL Pass 2
remain."

### Deliverables

- `LIFECYCLE_AND_PORTAL.md` mentions Phase 3 in status
  banner (full body revision waits for the docs+skill pass).
- `PHASE_3_IMPLEMENTATION.md` "What shipped" trailer.
- `UL_ADOPTION_READINESS.md` §2.3 marked closed; status
  snapshot updated.
- Memory updated.
- "What's next" pointer naming the docs+skill revision pass
  as the only remaining gate.

### Implementation steps

1. Update status banners.
2. Add per-merge trailer.
3. Update readiness doc.
4. Memory update.

### Validation gate

- All Phase 2 + 2.5 + 3 tests pass.
- UL builds clean.
- Doc round-trip: someone reading `PHASE_3_IMPLEMENTATION.md`
  cold understands the drag event surface + drain-time
  semantics without consulting the merge commits.

---

## Cross-cutting concerns

### TickContext signature change ripples to every Widget impl

P3-M0 changes `tick_animations(&mut self, dt: f32) ->
TickResult` to `tick_animations(&mut self, ctx: &mut
TickContext) -> TickResult`. Every widget that overrides
this method needs the signature updated.

Audit during M0: widgets that override:
- `FlexWidget` (the meaty one)
- `PresenceWidget` (delegates to inner Flex)
- `PortalWidget` (delegates to inner Flex)
- Possibly others — grep at M0 start

For widgets that just delegate (Presence/Portal), the
update is mechanical. For widgets that don't override,
the default impl handles it.

### Lorekeeper coordination for drag dispatch

P3-M1 needs the lorekeeper input pump to call
`UI::dispatch_drag_*` methods. Same coordination shape as
P25-M2's key suppression:
- Ogham lands first → API exists, lorekeeper still uses
  raw mouse events; drag events never fire; existing UL
  click behavior unchanged.
- Lorekeeper lands first → calls API methods that don't
  exist yet; build break. Land ogham first.

Recommend: ogham M1 ships, lorekeeper integration follows
in a separate workstream.

### Drag-during-portal-close edge cases

If a portal closes mid-drag (e.g. `open` flips false), the
drag's originator might still be in the tree (in another
layer or the base tree). Drag should continue cleanly.

If the originator itself is unmounted mid-drag (e.g. its
parent fn stops being visited), drag_end fires on the
originator immediately with no drop target. Define this in
M1 and test it.

### Backwards compatibility

P3-M0's TickContext signature change is a **breaking change
to the Widget trait**. Existing widget impls in the
codebase get updated; external consumers (none today
besides UL, which doesn't impl Widget directly) are
unaffected.

`accepts_drop` and `drag_payload` and `drag_preview` are
all default-None additions — non-breaking.

`Value::DragPayload` is NOT added — drag payloads use
existing Value variants per the M1 open question
resolution (consumer convention).

---

## Risk register

| # | Risk | Probability | Severity | Mitigation | Owner merge |
|---|---|---|---|---|---|
| 1 | TickContext refactor breaks every widget's tick_animations impl | High | Medium | Mechanical update during M0; widget audit at start. Default impl unchanged semantics so widgets that didn't override are safe | M0 |
| 2 | UpdateResult-with-Vec loses Copy bound, ripples through reconcile callers | Medium | Low | UpdateResult was already a struct with bool fields; Copy reliance was minimal. Audit at M0 | M0 |
| 3 | Drop-target hit-test cost during drag (every mouse_move walks layers) | Low | Low | Hit-test is already done for hover; drag adds an accepts_drop check. Negligible | M1 |
| 4 | drag_preview rendering interacts unexpectedly with cursor-attached layer's other contents | Low | Low | cursor-attached layer is rare in current use (no other consumers); drag_preview is the canonical use | M2 |
| 5 | Drain-time unmount fires too late (UI feels laggy) | Low | Low | Drain happens at exit-animation completion (typical 200-400ms). Same latency as Phase 2 design intended. UL audit confirmed acceptable | M3 |
| 6 | contextmenu event collides with widgets that use right-click for other things | Low | Low | Default behavior preserved for widgets without `contextmenu` listeners | M2 |
| 7 | Cross-repo coordination delay (lorekeeper input pump for drag) | Medium | Low | Both orders safe; ogham can land first and wait for lorekeeper integration | M1 |

---

## Summary timeline

Estimated calendar time at ~1 person-day per moderate merge,
~2 for the architectural one:

| Merge | Estimate | Cumulative |
|---|---|---|
| P3-M0 — TickContext refactor | 2 days | 2 |
| P3-M1 — Drag events core | 1.5 days | 3.5 |
| P3-M2 — drag_preview + contextmenu | 1 day | 4.5 |
| P3-M3 — Drain-time unmount | 1 day | 5.5 |
| P3-M4 — Docs graduation | 0.5 days | 6 |

**Total: ~5-6 person-days**, slightly above the original
~3-day estimate for drag-only because folding in drain-time
adds the M0 architectural foundation work.

Per the single-branch workflow, each merge commits straight
to `main` after passing its gate.

---

## Decision points before starting

Items that need a call before P3-M0 begins. Most have
recommended answers from the open-question sections above;
captured here for the resolution-trail.

1. **TickContext rename of `dt` parameter.** ✓ Yes — better
   to do during the signature change than rip up later.
2. **`UpdateResult` extension vs alternate cancel-prefix
   plumbing.** Recommend extending UpdateResult with
   `cancelled_unmount_prefixes: Vec<String>`. Approve.
3. **`EventContext.drag_state` public visibility.** Recommend
   `pub` — widgets need read access for hover highlighting.
4. **Payload typing in M1.** ✓ Untyped Value; consumer
   convention. Approve.
5. **`accepts_drop` predicate vs static type filter.** ✓
   Predicate only for v1.
6. **Touch/pen drag.** ✓ Out of scope; mouse-only.
7. **Auto-cancel-on-Escape.** Defer unless UL needs.
8. **Drain-time path-disappear dual policy.** ✓ Drain-time
   supersedes; path-disappear fallback for widgets that
   drop without notification. Document clearly.
9. **`Value::DragPayload` new variant.** No — use existing
   variants.
10. **Test file naming.** As proposed:
    `tests/tick_context.rs`, `tests/drag_events.rs`,
    `tests/drag_preview.rs`, `tests/contextmenu.rs`,
    `tests/drain_time_unmount.rs`. Approve.

If any of these need re-litigation, do it at P3-M0 kickoff,
not in the middle of a merge.

---

## What "Phase 3 ships" looks like

When P3-M4's gate passes:

- `Portal { layer: "cursor-attached", ... }` works with
  drag_preview rendering attached to the cursor.
- `drag_start`, `drag_move`, `drag_end` events fire per
  the dead-zone state machine that already exists in
  `input.rs`.
- `accepts_drop` widget attribute predicates drop targets;
  `drag_end` fires on the matching widget or the
  originator (for cancel).
- `contextmenu` event fires on right-click; suppresses
  default click handling.
- A Portal-housed dialog with `exit:` style fades out THEN
  fires `on_unmount` (true drain-time semantics, not
  path-disappear).
- All Phase 2 + 2.5 + 3 tests pass.
- ~25-27 new Phase 3 tests pass.
- UL builds clean.
- `LIFECYCLE_AND_PORTAL.md` status banner mentions Phase 3.
- `UL_ADOPTION_READINESS.md` §2.3 closed; only the
  docs+skill revision pass remains before UL Pass 2 begins.

That's Phase 3 done. UL adoption can begin after the
docs+skill revision pass.

---

## What shipped (2026-05-05)

### P3-M0 — TickContext + UpdateResult extension

- `widget::event::TickContext { dt, drained_path_prefixes,
  cancelled_unmount_prefixes }` replaces the bare `dt: f32`
  arg on `Widget::tick_animations`. UI builds the context per
  frame, harvests both Vecs into `pending_drained_prefixes`
  and `pending_cancelled_unmount_prefixes`, exposes them via
  `take_*` for the runtime to consume.
- `widget::event::DragState { origin_widget, payload,
  start_position, current_position, past_dead_zone }` and
  `EventContext.drag_state: Option<DragState>` so M1+M2 can
  thread drag info through dispatch without re-touching
  every event call site.
- `UpdateResult` grows `cancelled_unmount_prefixes:
  Vec<String>`; constants converted to functions
  (`replace()`, `unchanged()`, `layout_changed()`) since
  Vec isn't const-eligible.
- `FlexWidget::reconcile_children` records cancelled exits'
  owned_path_prefixes; `drain_exited_children` records
  drained prefixes into the `TickContext`.
- Two new tests (`drain_records_owned_path_prefix_in_tick_context`,
  `cancel_exit_records_owned_path_prefix_in_update_result`)
  alongside ~13 widget impls migrated to the new
  `tick_animations(&mut TickContext)` signature.

### P3-M1 — Drag events core

- New widget trait methods: `drag_payload(&self) ->
  Option<&Value>`, `drag_dead_zone(&self) -> Option<f32>`,
  `accepts_drop(&self, payload: &Value) -> bool`,
  `fire_event_listener(&self, event: &Event) -> bool`.
- `FlexWidget` stores `drag_payload`, `drag_dead_zone`,
  `accepts_drop_predicate: Option<Box<dyn Fn>>`. Builder
  reads `drag_payload`, `drag_dead_zone`, `accepts_drop`
  properties + `drag_start`/`drag_move`/`drag_end` listeners.
- `Event.payload: Option<Value>` carries the drag payload
  through dispatch; `Event::drag(name, point, payload)`
  constructor.
- UI dispatch surface:
  `dispatch_drag_start(origin, payload, point) -> DragState`,
  `dispatch_drag_move(state, point) -> Option<WidgetRef>`,
  `dispatch_drag_end(state, point) -> Option<WidgetRef>`,
  `hit_test_drag_target(point)`, `hit_test_drop_target(payload, point)`.
  Hit-test walks portal layers high→low then base tree;
  honors Block backdrop policy.
- All four exposed on `Ogham::dispatch_*` for hosts.
- 12 tests in `tests/drag_events.rs`.

### P3-M2 — drag_preview + contextmenu

- `Widget::drag_preview() -> Option<WidgetRef>`; FlexWidget
  stores it; builder reads `drag_preview` (single widget).
- `UI` tracks `active_drag_preview: Option<DragPreviewState
  { preview, cursor }>` set by `dispatch_drag_start`,
  updated by `dispatch_drag_move`, cleared by
  `dispatch_drag_end`. Skia renderer pushes a synthetic
  `CursorAttached` PortalEntry each draw with viewport_rect
  at the cursor.
- `UI::dispatch_contextmenu(point) -> bool` fires
  `contextmenu` listener on the deepest widget at the point
  (uses M1's `hit_test_drag_target` walker). Builder
  registers `contextmenu` like other mouse events.
- `Ogham::dispatch_contextmenu` exposed for hosts.
- 6 tests in `tests/drag_preview.rs` + 3 in
  `tests/contextmenu.rs`.

### P3-M3 — Drain-time unmount semantics

- `Runtime::process_drain_queues(&mut UI)` consumes UI's
  `pending_cancelled_unmount_prefixes` (cancels first) and
  `pending_drained_prefixes` (then flushes). `Ogham::update`
  calls it after `ui.reconcile()` and runs
  `pre_layout_drain` again so the freshly-promoted hooks
  fire in the same frame.
- `queue_disappeared_unmounts` now stages to
  `candidate_unmounts` instead of immediately flushing;
  paths that reappear in `active_state_paths` have their
  candidate cleared so a re-mount mid-disappear doesn't
  fire a stale unmount.
- New `Runtime::flush_remaining_unmount_candidates()`
  fallback for hosts (or tests) that exercise `Runtime`
  without the widget tree's drain machinery.
- `UpdateResult.drained_path_prefixes: Vec<String>` for
  immediately-dropped widgets (no exit animation);
  reconcile_children pushes their owned_path_prefix at
  both drop sites (type-mismatch, orphaned-old).
- `UI::reconcile` harvests both new vecs into UI's pending
  vecs.
- Existing path-disappear tests in `lifecycle_hooks.rs` +
  `effects.rs::cleanup_runs_when_path_unmounts` updated to
  call the explicit flush + re-drain (since they exercise
  `Runtime` directly without a widget tree).
- 5 tests in `tests/drain_time_unmount.rs`.

### P3-M4 — Docs graduation

- This trailer.
- Status banner updated.
- `LIFECYCLE_AND_PORTAL.md` mentions Phase 3.
- `UL_ADOPTION_READINESS.md` §2.3 marked closed.
- Memory updated.
- "What's next": docs + Ogham-skill revision pass, then UL
  Pass 2 begins.
