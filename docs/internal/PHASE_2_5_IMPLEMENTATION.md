# Phase 2.5 — Engine surface alignment with UL

> **Status: Implementation plan.** Companion to
> `UL_ADOPTION_READINESS.md` (the gap analysis) and
> `LIFECYCLE_AND_PORTAL.md` (the Phase 2 design that this
> phase extends). Five merges + docs graduation; estimated
> ~1,300 LOC implementation + ~50 tests, ~10 person-days.
>
> **Goal**: close the Phase 2 ↔ UL `UI_RUNTIME.md` gap so UL
> can begin its UI adoption work on a stable engine surface.
> Drag events (largest single chunk) are explicitly **deferred
> to Phase 3** to keep this phase tight.
>
> **Non-goal**: any UL-side migration work. That's UL's
> Pass 2 work, gated on this phase shipping.

---

## At a glance

| Merge | Title | LOC | Tests | Risk | Hardest part |
|---|---|---|---|---|---|
| P25-M0 | Portal layer system + viewport-absolute coords | ~700 | ~15 | High | Reworking the per-frame portal_layer into named/priority layers without breaking existing tests |
| P25-M1 | Cursor coordination signal | ~150 | ~6 | Low | Defining the per-portal/per-widget cursor declaration semantics |
| P25-M2 | Focus management script API + key suppression contract | ~400 | ~12 | Medium | Crosses ogham/lorekeeper boundary; key-suppression contract design |
| P25-M3 | Drain-time unmount refinement + hot-reload reset | ~250 | ~8 | Medium | The mid-exit-cancel edge case — finally proper drain-time semantics |
| P25-M4 | Timer primitive (audit + ship if missing) | 0–200 | ~5 | Low | Probably a non-event; pre-audit may show it exists |
| P25-M5 | Docs graduation + UL adoption-ready validation | ~50 docs | — | Low | Capturing decisions; writing the "Phase 2.5 ships" checklist |

**Total: ~1,500 LOC + ~46 tests**, similar density to Phase 2's
M3 alone. Smaller phase overall — this is API-shape work, not a
new subsystem.

### Sequencing rationale

P25-M0 is the foundation — every other primitive builds on the
new layer infrastructure. Cursor coord (M1) is a tiny follow-on
that consumes layer info. Focus mgmt (M2) is largely independent
but its key-suppression contract crosses into lorekeeper; budget
time for that.

Drain-time refinement (M3) is touchy because it's the first
real change to the lifecycle dispatch logic since M1 (Phase 2)
shipped. We've documented the "path-disappear" semantics as a
deviation; this is where we honor the design's actual contract.

Timer primitive (M4) is pre-gated on an audit — `panel_transition`
in UL's `main.ogh:10–15` implies scheduled interpolation already
exists somewhere. May be a no-op merge if the audit shows the
primitive's already in place.

M5 is doc graduation — same pattern as Phase 2's M5: status
banners, "what shipped" trailer, post-Phase-2.5 backlog
recording.

### Validation gate at each merge

Same per-merge gate as Phase 2:
- `cargo build --workspace` clean
- `cargo test --workspace` green (with all P2 tests still
  passing — guards against regression)
- UL builds clean (`cd ../untold_lore && cargo build`)
- New tests added in this merge pass
- No regressions in existing tests

If a gate fails, the work-in-progress stays uncommitted until
resolved. Single-branch, "rip and tear" between merges.

---

## P25-M0 — Portal layer system + viewport-absolute coords

### Goal

Replace Phase 2's single `UI.portal_layer: Vec<PortalEntry>`
with a named-and-priority-ordered layer system per UL's
`UI_RUNTIME.md` §1. Five layers, per-layer backdrop policies,
hit-test honoring layer priority. Folds in the M3-deferred
viewport-absolute coordinate fix for Pass B.

### Deliverables

- New enum `PortalLayer { Main, OverlayModal, Popover, Tooltip,
  Toast, CursorAttached }` in `src/widget/portal_widget.rs` or
  a new `src/widget/portal_layer.rs`.
- Layer registry as a runtime-known constant — NOT extensible
  from userspace. Per UL's spec: "A new pattern that needs a
  new layer requires a runtime change — that's a feature, not
  a bug."
- Per-layer backdrop policy: `BackdropPolicy { None, Dim, Block }`.
  Defaults: `OverlayModal → Block`, others → `None`.
- `Portal` widget gains `layer: PortalLayer` property
  (defaults to `OverlayModal` for backward compatibility with
  Phase 2's API).
- `UI.portal_layer: Vec<PortalEntry>` replaced with
  `UI.portal_layers: HashMap<PortalLayer, Vec<PortalEntry>>`
  OR a sorted `Vec<(PortalLayer, Vec<PortalEntry>)>` ordered
  by priority. Pick the latter for cache-friendliness; cardinality
  is small.
- Skia `draw` walks the main pass, then iterates layers
  low-priority-to-high (so `cursor-attached` paints on top of
  `tooltip` etc.). Within a layer, mount-order LIFO for paint.
- Hit-test walks layers high-priority-to-low; within a layer
  reverse iteration; falls through to base tree only if the
  topmost layer's policy isn't `Block`.
- **Viewport-absolute coordinate fix**: track cumulative
  translates through `draw_widget_recursive` so
  `PortalEntry.parent_rect` carries viewport-absolute coords,
  not parent-relative. Pass B no longer accumulates incorrect
  translates.
- Builder rejects unknown layer names with a clear diagnostic.

### Implementation steps

1. **Define `PortalLayer` enum** with explicit priority
   ordering. Implement `Ord` so collections sort naturally.
2. **Add `BackdropPolicy` enum** + a `policy_for(layer:
   PortalLayer) -> BackdropPolicy` const-fn that hardcodes the
   layer-default mapping.
3. **Extend `PortalInfo`** with `layer: PortalLayer`. Default
   to `OverlayModal` so widgets that haven't been updated still
   land somewhere reasonable.
4. **Update `PortalWidget`**: parse `layer` property in builder
   (`create_portal_widget`); store on the widget; surface via
   `as_portal()`.
5. **Refactor `UI.portal_layer`** to
   `portal_layers: Vec<(PortalLayer, Vec<PortalEntry>)>`,
   pre-allocated with all layers in priority order at
   `UI::new`. Per-frame: clear each layer's vec; populate
   during main render pass.
6. **Skia draw**: rewrite the Pass B loop to iterate layers in
   priority order. Apply backdrop policy per-layer (Dim/Block
   are render-side concerns; Block additionally blocks
   pointer events to lower layers).
7. **Hit-test rewrite**: `UI::handle_click_event` walks layers
   high-priority-to-low. The first layer with a matching
   non-portal-node hit returns. If that layer's policy is
   `Block`, fall-through is suppressed; otherwise walk lower
   layers, then base tree.
8. **Viewport-absolute coords**: thread an `accumulated_translate:
   (f32, f32)` through `draw_widget_recursive`. When pushing
   to portal_layer, capture
   `parent_rect = layout_rect + accumulated_translate`. Pass B
   then translates by parent_rect.{x,y} from viewport origin
   (no accumulation needed).
9. **Update `has_input_blocking_portal`**: walk the
   `OverlayModal` layer for any focus_trap=true entry. (The
   relationship between `block` policy and focus_trap is
   answered in [Open question 1](#p25-m0-open-questions).)
10. **Builder diagnostic**: unknown layer names → reject with
    `BridgeError::InvalidPropertyType` + a list of valid
    layers in the message.

### Test matrix

`tests/portal.rs` extensions:

1. `portal_with_no_layer_property_defaults_to_overlay_modal`
2. `portal_layer_property_parses_each_named_layer`
3. `portal_layer_property_rejects_unknown_layer_name`
4. `cursor_attached_layer_paints_above_tooltip_layer`
5. `block_policy_layer_swallows_clicks_to_lower_layers`
6. `none_policy_layer_lets_clicks_fall_through`
7. `portal_at_nested_position_paints_at_viewport_absolute_coords`
8. `multiple_portals_in_same_layer_stack_lifo`
9. `multiple_portals_across_layers_paint_low_to_high`

`tests/focus_trap.rs` (regressions):

10. `has_input_blocking_portal_only_walks_overlay_modal_layer`
11. `focus_trap_in_popover_layer_does_not_block_input`

`tests/portal_coords.rs` (new file for the viewport fix):

12. `pass_b_translate_includes_parent_chain_translates`
13. `nested_portal_inside_translated_parent_renders_at_correct_viewport_position`
14. `portal_at_root_unchanged_from_phase_2_behavior`

### Cross-merge dependencies

- M0 must land before M1 (cursor coord consumes layer info)
  and M2 (focus mgmt's key-suppression isn't dep-blocked but
  the focus-trap-vs-block-policy question is decided here).
- Independent of M3, M4.

### P25-M0 open questions

These need a decision before M0 starts.

1. **Does `block` layer policy subsume `focus_trap: true`?**
   Per `UI_RUNTIME.md` §1, `OverlayModal` defaults to `block`
   pointer-event policy. Per Phase 2, `focus_trap: true`
   isolates focus separately. UL's spec keeps both as
   independent: layer policy gates clicks; focus_trap gates
   focus moves. **Recommend keeping them independent** — they
   address different concerns. A modal might trap focus but
   allow clicks (rare); a tooltip might block clicks but not
   trap focus (also rare, but legal).

2. **Per-layer backdrop policy: hardcoded or configurable?**
   `UI_RUNTIME.md` says "Each layer can declare a *backdrop
   policy*" — implying configurable. But also "layers are a
   fixed set defined by the runtime (not extensible from
   userspace)." **Recommend hardcoding** — it's a small set
   and lets the runtime guarantee behavior. Userspace can
   still render its own backdrop into the same layer if it
   wants finer control (per the spec).

3. **Coordinate-fix scope: just Pass B, or also nested-portal
   recursion?** Currently Pass B has its own paint_portal_entry
   recursion that handles nested portals via a throwaway
   second-level layer. After M0's layer system, nested portals
   land in their actual destination layer, so the throwaway
   path goes away. Means the recursion is simpler — verify
   this in implementation.

### What M0 does NOT include

- Drag-related layer interactions (Phase 3).
- Per-portal cursor declaration (M1's job).
- Focus_trap script API (M2's job).
- Any change to lifecycle dispatch (M3's job).

### Validation gate

- All M0 tests pass.
- All Phase 2 tests still pass (no regressions).
- UL builds clean.
- Manual smoke: a `Portal { layer: "tooltip", ... }` paints
  above a `Portal { layer: "overlay-modal", ... }` per layer
  priority.

---

## P25-M1 — Cursor coordination signal

### Goal

Land `Runtime::wants_cursor_free()` and per-portal/per-widget
cursor declarations. UL's `update.rs:1561+` cursor-lock
composition gets one cleaner signal to consume.

### Deliverables

- `CursorPreference { Free, Inherit }` enum in
  `src/widget/portal_widget.rs`.
- `Portal` widget gains `cursor: CursorPreference` property,
  defaulting to `Free` for `OverlayModal` and `Popover`,
  `Inherit` for others.
- Widget trait gains `cursor_preference(&self) ->
  Option<CursorPreference>` (default `None`). Focused
  `TextInputWidget` overrides to return `Some(Free)`.
- `Runtime::wants_cursor_free() -> bool` walks active portals'
  layers and the focused widget's cursor preference. Returns
  true if anything declares Free.
- `Ogham::wants_cursor_free()` public forwarder (mirrors
  `has_input_blocking_portal`).

### Implementation steps

1. **Define `CursorPreference`** enum with `Free` and `Inherit`
   variants. `Inherit` means "don't influence."
2. **Extend `PortalInfo`** with `cursor: CursorPreference`
   (defaulting per layer in M0's per-layer defaults map).
3. **Builder reads `cursor` property** with the layer-default
   fallback.
4. **`Widget::cursor_preference`** trait method, default `None`.
5. **`TextInputWidget` override**: if focused, return
   `Some(Free)`. Done by reading the focus state held on
   `EventContext` or via a per-widget focused flag.
6. **`Runtime::wants_cursor_free()`**: walk
   `ui.portal_layers`, returning true on any portal with
   `cursor: Free`. Then walk the currently-focused widget
   (via `ui.get_focused()`) and check its
   `cursor_preference()`.
7. **`Ogham::wants_cursor_free()`** public method.

### Test matrix

`tests/cursor_coord.rs` (new):

1. `wants_cursor_free_false_when_nothing_active`
2. `wants_cursor_free_true_when_overlay_modal_open`
3. `wants_cursor_free_false_when_only_tooltip_open` (tooltip
   defaults to Inherit)
4. `wants_cursor_free_true_when_textinput_focused`
5. `portal_explicit_cursor_inherit_overrides_layer_default`
6. `wants_cursor_free_combines_portal_and_focus_signals`

### Cross-merge dependencies

- Depends on M0 (consumes layer info).
- Independent of M2, M3, M4.

### What M1 does NOT include

- Game-side cursor-lock composition (UL's
  `update.rs:1561+` rewrite). That's UL's Pass 2 work.
- Per-key cursor opt-in for non-focused widgets (out of scope
  for v1).

### Validation gate

- All M0 + M1 tests pass.
- UL builds clean.
- Smoke: `Ogham::wants_cursor_free()` returns the expected
  value for each canonical scenario.

---

## P25-M2 — Focus management script API + key suppression contract

### Goal

Expose focus management to `.ogh` scripts. Land the
key-suppression contract (runtime consumes
`Key::Character(_)` for focused text widgets before the
game-side input pump). Crosses into lorekeeper for the
input-pump side.

### Deliverables

**Ogham side:**
- New built-in: `focused_widget()` — returns a widget-ref
  value or `Void` if nothing focused.
- New built-in: `focus(widget_ref)` — programmatic focus
  request. Routes through `try_set_focus` (rejects if
  trapped).
- `Runtime::consumes_character_key() -> bool` — true if a
  focused widget claims `Key::Character(_)` events.
  Currently only `TextInputWidget` does.
- `Runtime::input_unfiltered() -> bool` — opt-out flag for
  editors that need raw input. Settable via Ogham config or
  per-call query.

**Lorekeeper side:**
- Input pump consults `runtime.consumes_character_key()`
  before populating `pressed()` / `held()` queries with
  Character events.
- New `input.pressed_unfiltered(...)` query returns
  unfiltered input regardless of focus.

**Suppression scope (per UL spec):**
- `Key::Character(_)` ONLY when a focused widget claims it.
- `Key::Named(NamedKey::Escape | F1..F12 | Arrow* | Tab)` and
  modifiers (Ctrl/Alt/Meta/Shift) NEVER suppressed by focus
  alone.

### Implementation steps

1. **Built-in registry extension** for `focused_widget()` and
   `focus(widget_ref)`. The widget-ref value type needs a
   serializable identity — probably an opaque integer ID
   issued by `WidgetTree`.
2. **Widget ID surface**: add `Widget::id() -> WidgetId` (a
   `u64` newtype, allocated by UI on widget construction).
   Existing widgets get IDs at builder time.
3. **`focused_widget()` returns** the focused widget's ID
   wrapped in a `Value::WidgetRef` (new variant). Or — to
   avoid a new variant — return as `Value::Integer` with a
   convention. Decide before implementation.
4. **`focus(widget_ref)`**: looks up widget by ID, calls
   `try_set_focus`. Returns `bool` for accept/reject.
5. **`Runtime::consumes_character_key()`**: walks focused
   widget; checks if it's a TextInputWidget (or any future
   widget that claims keys). Probably via a new trait method
   `claims_character_keys(&self) -> bool` (default false).
6. **TextInputWidget override**: returns true.
7. **Lorekeeper input-pump change**: in the Character-event
   handling path, query `ogham.runtime.consumes_character_key()`
   and skip `pressed()` population if true.
8. **`pressed_unfiltered(...)`**: parallel query that always
   returns the raw event.

### Test matrix

`tests/focus_script_api.rs` (new):

1. `focused_widget_returns_void_when_nothing_focused`
2. `focused_widget_returns_id_after_set_focus`
3. `focus_built_in_accepts_widget_ref_and_sets_focused`
4. `focus_rejected_when_target_outside_trapped_subtree`
5. `consumes_character_key_true_when_text_input_focused`
6. `consumes_character_key_false_otherwise`

`tests/key_suppression.rs` (lorekeeper-side, new):

7. `character_key_suppressed_when_text_input_focused`
8. `escape_key_passes_through_when_text_input_focused`
9. `arrow_keys_pass_through_when_text_input_focused`
10. `pressed_unfiltered_returns_keys_regardless_of_focus`
11. `modifier_keys_pass_through`
12. `f_keys_pass_through`

### Cross-merge dependencies

- Depends on M0 (focus_trap interaction with layers).
- Crosses into lorekeeper repo. Coordinate timing.
- Independent of M3, M4 functionally.

### P25-M2 open questions

1. **Widget ref representation in Ogham values.** New
   `Value::WidgetRef(u64)` variant vs reusing
   `Value::Integer`? Recommend a new variant for type safety
   — the runtime already gates other types via Value variants.
2. **WidgetId allocation.** Per-`UI` counter, reset on UI
   reconstruction? Or process-wide atomic? Recommend per-`UI`
   — IDs are only meaningful within a UI tree.
3. **`focus(widget_ref)` returning bool vs panicking on
   reject.** Recommend bool — caller can decide whether to
   retry.

### What M2 does NOT include

- Tab navigation through focusable widgets (separate, larger
  feature). Phase 3 candidate.
- Click-outside-loses-focus rule (could go here, but it
  intersects with M0's layer policy work — defer to M3 if
  it doesn't fit cleanly).
- Game-side `text_focused` cleanup in UL (UL's Pass 2 work).

### Validation gate

- All M0 + M1 + M2 tests pass.
- UL builds clean (lorekeeper changes don't break the path-dep
  build).
- Smoke: focused TextInput consumes 'n' keypress; same key
  with no focus reaches game-side input.

---

## P25-M3 — Drain-time unmount refinement + hot-reload reset

### Goal

Honor the design's "drain-time unmount" contract that Phase 2
M1 deferred to "path-disappear" semantics. Also wire
`UI::clear_lifecycle_state` into the hot-reload path.

### Deliverables

- `tick_animations` chain threaded with a way to communicate
  drain events back to `Runtime` / `StateManager`.
  Implementation choice: side-channel Vec on UI, drained by
  Runtime at frame boundary. (Avoids `&mut Runtime`
  threading through the widget tree.)
- `UI.drained_path_prefixes: Vec<String>` populated by
  `drain_exited_children` in `flex_widget.rs:431`.
- `UI.cancelled_unmount_prefixes: Vec<String>` populated by
  `cancel_exit` in `flex_widget.rs:1476`.
- `Runtime::process_drain_queues(&mut UI)` consumes both
  vecs: drained → flush_for_path_prefix; cancelled →
  cancel_unmount_for_prefix.
- `queue_disappeared_unmounts` in `Runtime::rerender` /
  `execute_module` is replaced — instead of computing
  path-disappear diff, defer to the drain queue. The diff
  becomes a candidate set; actual flush waits on drain.
- `Ogham::reload_file` and `Ogham::recompile_from_source`
  call `ui.clear_lifecycle_state()` before swapping in the
  new UI.

### Implementation steps

1. **Add `drained_path_prefixes`/`cancelled_unmount_prefixes`
   to UI**.
2. **Hook `drain_exited_children`**: push the drained widget's
   `owned_path_prefix` to UI's drain queue. Need to thread
   `&mut UI` access — actually let me think. drain_exited_children
   is on FlexWidget, called from tick_animations which is on
   the Widget trait, called from UI::tick_animations. UI has
   `&mut self`. The recursion happens inside the widget tree.
   Cleanest: make drain_exited_children return a Vec of drained
   prefixes, bubble up through TickResult, UI accumulates.
3. **Extend `TickResult`** with `drained_prefixes:
   Vec<String>` and `cancelled_prefixes: Vec<String>`. Bubble
   up through tick_animations.
4. **UI::tick_animations** appends bubbled prefixes to its own
   queues.
5. **Runtime::process_drain_queues**: consumes UI queues, calls
   the StateManager helpers.
6. **Wire into the host frame loop**: process_drain_queues runs
   *between* tick_animations and the next render. New
   `Ogham::tick(dt)` method (or extend update) that runs:
   tick_animations → process_drain_queues → reconcile if
   needed.
7. **Replace queue_disappeared_unmounts**: candidate set now
   stored on StateManager; actual flush waits on drain
   notification.
8. **Wire `clear_lifecycle_state` into reload paths** —
   `Ogham::reload_file` and `recompile_from_source` clear
   the old UI's lifecycle state before swapping in the new
   one. The new UI starts clean.

### Test matrix

`tests/drain_time_unmount.rs` (new):

1. `unmount_fires_after_exit_animation_completes_not_at_path_disappear`
2. `cancel_exit_during_animation_cancels_pending_unmount`
3. `cancel_exit_then_re_exit_fires_unmount_on_eventual_drain`
4. `widget_with_no_exit_animation_unmounts_immediately_on_path_disappear`
5. `effect_cleanup_runs_at_drain_time_too`
6. `multiple_drain_events_in_one_frame_all_processed`

`tests/hot_reload_lifecycle.rs` (new):

7. `reload_file_clears_focus_stack_from_old_ui`
8. `recompile_from_source_clears_portal_layer_from_old_ui`

### Cross-merge dependencies

- Touches lifecycle dispatch — same code that M1/M2 (Phase 2)
  shipped. Verify P2 tests still pass.
- Independent of M0, M1, M2 functionally.

### P25-M3 open questions

1. **TickResult shape impact.** Adding `Vec<String>` to a
   per-frame result that bubbles through every widget is
   unfortunate for hot-path allocation. Alternative: pass a
   `&mut DrainAccumulator` as a tick context. Cleaner but
   touches every widget that overrides `tick_animations`.
   Decide based on widget count (small) vs allocation cost.
2. **What about `is_exit_complete` widgets that drop without
   a drain event?** Those are widgets reconciled out
   without ever entering exit (no exit_style declared). They
   currently drop immediately. Should they fire unmount?
   Recommend yes — the design treats "not exiting" and
   "exited" identically for unmount purposes.
3. **`set_compiled_module` reset interaction.** Phase 2's
   `set_module` resets `lifecycle_active` but not the focus
   stack / portal_layer / pending queues. With this merge,
   `clear_lifecycle_state` should be called from
   `set_module` too. But `set_module` is on Runtime, doesn't
   have UI access. Actual hot-reload happens at the Ogham
   facade — call there. Confirm there's no other call site
   to set_module that needs the reset.

### What M3 does NOT include

- Cancel-mid-exit + re-mount-as-NEW-path (treated as a fresh
  mount). Already works correctly — included as test #3 to
  verify.
- The general question of "what counts as a path's identity
  across hot reload" — still an open question for Phase 3+.

### Validation gate

- All M0 + M1 + M2 + M3 tests pass.
- UL builds clean.
- Smoke: a Portal-housed dialog with `exit:` style fades out,
  then unmount fires (visible via `event("test_log", "...")`
  in the .ogh).

---

## P25-M4 — Timer primitive (audit + ship if missing)

### Goal

Audit existing scheduling infrastructure (`panel_transition`
in UL's `main.ogh:10–15` implies it exists). If missing,
ship `set_timeout` / `set_interval` / `clear_*` as runtime
built-ins with auto-cancel on widget unmount.

### Deliverables

**If timer primitive doesn't exist:**
- `set_timeout(delay_ms, callback)` returning a handle.
- `set_interval(period_ms, callback)` returning a handle.
- `clear_timeout(handle)` / `clear_interval(handle)`.
- Auto-cancel on the registering widget's unmount via
  Phase 2's lifecycle infrastructure.

**If timer primitive exists:**
- Document what's there in the `LIFECYCLE_AND_PORTAL.md` or a
  new doc.
- Ensure auto-cancel on unmount works.

### Implementation steps

1. **Audit existing scheduling**. Likely candidates: animation
   ticks (`tick_animations`), `panel_transition` infrastructure,
   spring scheduling. Check how `state` cells transition over
   time; that's the closest analog.
2. **If missing — design API**: handle type
   (`Value::TimerHandle(u64)`?), cancel semantics, what scope
   is allowed (event handler? hook body? top-level?).
3. **If missing — implement**: timer registry on Runtime.
   Each timer has a `(deadline_ms, callback_closure,
   owning_path)`. Per-frame tick advances the registry; when
   deadline passes, queue callback for next render's
   post-layout drain.
4. **Auto-cancel on unmount**: when a path's hooks flush
   (in P25-M3's drain machinery), drop any timers whose
   `owning_path` matches the prefix.

### Test matrix

If timer primitive is added (5 tests):

1. `set_timeout_fires_callback_after_delay`
2. `clear_timeout_prevents_fire`
3. `set_interval_fires_callback_repeatedly`
4. `timer_auto_cancels_on_widget_unmount`
5. `timer_handle_round_trips_through_value_type`

If audit shows timer exists, just regression-test
auto-cancel-on-unmount with M3's drain wiring.

### Cross-merge dependencies

- Depends on M3 (auto-cancel uses drain machinery).
- Independent of M0, M1, M2 functionally.

### P25-M4 open questions

1. **Where do callbacks fire — at a tick, at frame start, at
   post-layout drain?** Recommend post-layout drain to
   integrate with Phase 2's lifecycle cadence. Means timers
   fire at most once per frame regardless of delay
   granularity.
2. **`set_interval` semantics — fire-then-wait vs
   wait-then-fire?** React's `useInterval` is wait-then-fire.
   Match that.

### What M4 does NOT include

- High-precision timing (per-tick callbacks). Only frame-aligned.
- Cross-frame timer accumulation for missed deadlines (a 100ms
  timer with a 500ms frame gap fires once, not 5 times).

### Validation gate

- All M0 + M1 + M2 + M3 + M4 tests pass.
- UL builds clean.
- Smoke: a `set_timeout(1000, fn () { log "fired" })` fires
  once after ~1 second; cancelled timers don't fire.

---

## P25-M5 — Docs graduation + UL adoption-ready validation

### Goal

Capture all Phase 2.5 decisions in the appropriate docs.
Update Phase 2 docs that are now superseded. Update the UL
adoption readiness doc to reflect the closed gap. Confirm
UL is unblocked for its Pass 2 migration work.

### Deliverables

- `LIFECYCLE_AND_PORTAL.md` updated: status banner mentions
  Phase 2.5; layer system, cursor coordination, focus script
  API, drain-time semantics all spec'd correctly.
- `LIFECYCLE_AND_PORTAL_IMPLEMENTATION.md` "What shipped"
  trailer extended with P25-M0 through P25-M4.
- `UL_ADOPTION_READINESS.md` status updated: §2 gaps closed
  (or remaining); §4 per-UI verdicts reconfirmed; §7
  recommended sequencing reduces to "Pass 2 = UL-side work."
- New entry in `ROADMAP.md`-equivalent (if it exists) or in
  the project memory.
- `CHANGELOG`-equivalent entry if the project has one.

### Implementation steps

1. **Update design doc.** `LIFECYCLE_AND_PORTAL.md` Portal
   section now describes the layer system, not the
   single-layer M3 shape.
2. **Update implementation history.** Per-merge "What shipped"
   entries for each P25-M0..M4.
3. **Update readiness doc.** Mark each §2 gap as closed.
   Re-verify §4 per-UI verdicts against the new surface.
4. **UL-side smoke** (this is checking, not changing UL): run
   `cargo build` on UL; manually verify a small `.ogh`
   exercising the new layers + cursor + focus + timer
   compiles and runs.
5. **Memory update.** Add Phase 2.5 status memo.

### Test matrix

Doc-only merge; no new tests.

### Cross-merge dependencies

- Depends on all of M0–M4.

### Validation gate

- All Phase 2 + Phase 2.5 tests pass.
- UL builds clean.
- Doc round-trip: someone reading `LIFECYCLE_AND_PORTAL.md`
  cold understands the layer system without consulting old
  trailer entries.

---

## Cross-cutting concerns

### Spec drift between Phase 2 design and Phase 2.5 reality

Phase 2's `LIFECYCLE_AND_PORTAL.md` specs Portal's API as
`open + focus_trap + children`. Phase 2.5 adds `layer` and
`cursor`, plus changes the per-frame portal_layer
infrastructure. The design doc needs a substantial revision
in M5, not just a status-banner update.

**Recommend** writing the revisions inline as each merge
ships, not waiting until M5. Stale design docs corrupt
future agents' mental model fast.

### Backward compatibility

Phase 2's API was `Portal { open, focus_trap, children }`.
Phase 2.5 adds `layer` (with default `OverlayModal`). Existing
`.ogh` code that doesn't specify a layer continues to work
— layer defaults to `OverlayModal`, which is the moral
equivalent of Phase 2's behavior (modal in front).

The `cursor` property similarly defaults sensibly per layer.
No migration burden for Phase 2 code.

### Test infrastructure

Phase 2's test files (`portal.rs`, `focus_trap.rs`,
`lifecycle_hooks.rs`, `effects.rs`) cover Phase 2 behavior.
Phase 2.5 adds new test files; existing ones may need
updates if Phase 2 tests assumed single-layer behavior.

Audit during M0: any test that pokes `ui.portal_layer`
directly will break. Update to walk the new
`portal_layers` map.

### Cross-repo coordination (lorekeeper)

P25-M2's key-suppression contract changes lorekeeper's input
pump. Coordinate the change with lorekeeper's owner; both
repos need to land the change atomically (or with a feature
flag).

If lorekeeper lands first: ogham's
`runtime.consumes_character_key()` returns false until M2
implements it; lorekeeper's check is a no-op; safe.

If ogham lands first: ogham starts returning true for
focused TextInputs; lorekeeper still pumps Character events
to game; UL's editors continue to work as before; M2's
benefit unrealized but no regression.

Either order is safe — pick whichever fits the team's
schedule.

### Phase 2's documented limitations

Phase 2.5 closes:
- §3.1 path-disappear → drain-time (M3)
- §3.3 viewport-absolute coords (M0)
- §3.4 hot-reload reset wiring (M3)

Phase 2.5 does NOT close:
- §3.2 mount-after-layout. Could land in M0 alongside the
  layer rework but was previously deferred for "Portal
  positioning needs post-layout sizes" reason. Layer system
  doesn't change that. Mark as Phase 3 candidate.

---

## Risk register

Consolidated risks with mitigation per merge.

| # | Risk | Probability | Severity | Mitigation | Owner merge |
|---|---|---|---|---|---|
| 1 | Layer rework breaks Phase 2 portal tests | High | Medium | Phase 2 tests audited and updated as part of M0; backward-compat default layer `OverlayModal` matches Phase 2 single-layer behavior | M0 |
| 2 | Viewport-absolute coords ripple — translate accumulation has subtle bugs | Medium | High | Dedicated test file (`portal_coords.rs`) with parent-chain scenarios; manual smoke with a deeply-nested portal in test .ogh | M0 |
| 3 | Block policy interaction with hit-test propagation has edge cases | Medium | Medium | Tests #5 and #6 in M0 cover the basic block/none distinction; fuzz with click positions outside any portal but with portals open | M0 |
| 4 | Cursor coordination semantic confusion (when does Free vs Inherit win?) | Low | Low | Doc cleanly: any-Free-wins; explicit-property-overrides-layer-default | M1 |
| 5 | Focus script API: WidgetId stability across reconcile | Medium | Medium | Reuse the existing reconcile-key infrastructure; widget IDs stable when key stays the same; new IDs on key change | M2 |
| 6 | Key suppression contract requires lorekeeper coordination | Medium | Medium | See "Cross-repo coordination" above; both orders are safe | M2 |
| 7 | TickResult bubbling allocates per-frame Vec<String> | Medium | Low | Profile during M3; switch to `&mut DrainAccumulator` if hot | M3 |
| 8 | Drain-time semantic regression — unmount fires too late | Medium | Medium | Tests #1 + #4 in M3 explicitly cover the no-exit case; manual smoke with both exit-styled and bare widgets | M3 |
| 9 | Timer primitive already exists in unanticipated shape — audit reveals refactor needed | Low | Low | M4 audit step blocks implementation; if existing primitive is awkward, raise design issue rather than ship-and-fix | M4 |
| 10 | Phase 2 design doc drift during M0–M3 leaves agents confused | High | Medium | Update design doc inline as each merge ships, not waiting until M5 | all |

---

## Summary timeline

Estimated calendar time at ~1–2 person-days per moderate merge:

| Merge | Estimate | Cumulative |
|---|---|---|
| P25-M0 — Portal layers + viewport coords | 3 days | 3 |
| P25-M1 — Cursor coordination | 1 day | 4 |
| P25-M2 — Focus script API + key suppression | 2 days | 6 |
| P25-M3 — Drain-time + hot-reload reset | 2 days | 8 |
| P25-M4 — Timer primitive (audit + ship) | 1–2 days | 9–10 |
| P25-M5 — Docs graduation | 0.5 days | 9.5–10.5 |

**Total: ~10 person-days**, similar density to Phase 2's 12.

Per the single-branch workflow, each merge commits straight to
`main` after passing its gate. Push freely between merges; no
PRs for internal Phase 2.5 work.

---

## Decision points before starting

1. **Phase 2.5 vs Phase 2-and-a-half-iterations.** The "2.5"
   framing suggests a coherent batch. Ship this as a real
   phase with its own status discipline (live design contract
   → live contract on completion), or as a series of "audit
   follow-ups"? **Recommend phase-with-discipline** —
   matches Phase 2's pattern; the docs gain status banners.
2. **Drag events in Phase 3 — when?** The user stated drag is
   out of 2.5 to keep boundaries clear. Phase 3 sequencing
   isn't on the agenda yet, but: drag is the largest
   self-contained chunk and gates UL's inventory drag-drop.
   Before P25 ships, decide whether Phase 3 starts immediately
   after, runs in parallel with UL Pass 2, or waits.
3. **Portal `layer` as `String` vs typed enum.** UL's spec
   uses string layer names (`"modal"`, `"tooltip"`).
   Type-safety pull says enum. Author-friendliness pull says
   string. **Recommend enum internally + string property in
   `.ogh` source** — parser maps string to enum, rejects
   unknown layers with the diagnostic message.
4. **WidgetId allocation strategy** — see P25-M2 open question
   2. Recommend per-`UI` counter; document.
5. **`Value::WidgetRef` — new variant?** See P25-M2 open
   question 1. Recommend yes for type safety.
6. **Test file naming.** Phase 2 used `tests/<feature>.rs`.
   Phase 2.5 follows: `tests/cursor_coord.rs`,
   `tests/focus_script_api.rs`, `tests/key_suppression.rs`,
   `tests/drain_time_unmount.rs`, `tests/hot_reload_lifecycle.rs`,
   `tests/portal_coords.rs`. Approve.
7. **Drag deferred to Phase 3** — does Phase 2.5's M5 doc
   graduation note this as the "what's left" pointer?
   **Recommend yes** — closes the loop on the Phase 2.5 ↔
   UL_ADOPTION_READINESS scope clarity.

If any of these need re-litigation, do it before P25-M0.

---

## What "Phase 2.5 ships" looks like

When P25-M5's gate passes:

- `Portal { layer: "tooltip", ... }` works; tooltips paint
  above modals; click on a tooltip doesn't dismiss the modal
  beneath it (per non-block policy).
- `Runtime::wants_cursor_free()` returns the right answer
  for: empty UI (false), open modal (true), open tooltip
  (false), focused TextInput (true).
- `.ogh` source can call `focused_widget()` and `focus(ref)`
  built-ins. Focus rejects out-of-trap moves.
- A focused TextInput consumes 'n' keypress; the same key
  with no focus reaches game-side input.
- A Portal-housed dialog with `exit:` style fades out, THEN
  unmount fires (drain-time semantics, not path-disappear).
- Hot-reload clears focus stack and portal layers; new
  module starts clean.
- `set_timeout` works (or is documented as already-existing
  with the auto-cancel-on-unmount semantic).
- `LIFECYCLE_AND_PORTAL.md` describes the layer system as
  the canonical Portal API.
- `UL_ADOPTION_READINESS.md` shows §2 gaps closed; UL is
  cleared to start Pass 2.
- All Phase 2 tests still pass (no regressions).
- All Phase 2.5 tests pass (~46 new).
- UL builds clean.

That's Phase 2.5 done. UL Pass 2 — OverlayStack migration,
Settings save-on-close, escape menu Portal, inventory
tooltip — can begin.
