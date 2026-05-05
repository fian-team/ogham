# Phase 2 — Lifecycle Hooks + Portal Widget

> **Status: Aligned action plan.** Design decisions locked
> through walkthrough; ready to graduate into the same triplet
> shape we used for typed bindings (design / UL audit / impl
> plan) when implementation starts.

---

## 0. Why these are one project

The original audit listed these as items #5 and #6 of the
priority list. They were ranked adjacent because they unlock
adjacent UX (modals, tooltips, dropdowns) — but the deeper
reason to ship them together is mechanical:

- **Portal's mount/unmount IS the lifecycle event you want
  callbacks on.** When a tooltip portal opens, `on_mount` is
  what positions/focuses it. When a modal portal closes,
  `on_unmount` is what restores focus to the trigger and
  flushes any pending state. Shipping Portal without lifecycle
  forces every consumer to re-implement these in Rust.

- **Lifecycle without Portal is still useful** (typed UIs can
  do cleanup; the Settings UI's "save on close" pattern moves
  from `actions.rs` into the `.ogh` itself), so it ships first
  and stands on its own. Portal then composes onto it.

- **Both share the same identity question.** "When does this
  thing logically appear/disappear?" — today's path-based
  state model (`StateManager.component_state` keyed by
  call-stack path) answers it for state. We need to extend the
  *same* answer to lifecycle hooks and to Portal contents so
  authors don't learn three identity rules.

**Sequencing**: lifecycle first (M0–M2, foundation), then
Portal (M3–M4, builds on it), then a UL validation pass (M5).

---

## 1. What the agents found that drove this plan

Two pieces of grounding from the exploration pass:

**Ogham today**
- Single Skia canvas, strict tree, depth-first render, no
  z-index. Per-widget clip via `push_clip_rect` /
  `pop_clip_rect` in `flex_widget.rs:1604–1622`.
- State lives in `StateManager.component_state` keyed by
  call-stack path + variable name — *not* by widget instance.
  `state x = 0` survives any reconcile that produces the same
  call-stack path; it disappears when the path stops being
  visited.
- Mount entry: `apply_entry_transition()` at
  `flex_widget.rs:156`, called from the builder at
  `builder.rs:651`. Unmount: `drain_exited_children()` at
  `flex_widget.rs:431`, plus immediate-drop and ghost-replace
  paths.
- No focus stack today — single `UI.focused` ref
  (`mod.rs:82`), cleared on widget drop.

**Untold Lore today**
- 17 wrapper structs, **zero `Drop` impls**, **one** explicit
  unmount path (Settings's `CloseSettings` action calls
  `client_settings.save()` — the rest leak silently because
  there's nothing to clean up yet).
- Escape menu's overlay is a full-screen `Flex` with
  `background_color: colors.backdrop` (alpha-160 black) at
  `escape_menu.ogh:100`. Input gating is a `bool overlay_active`
  in `update.rs:347` checked at `update.rs:1469` — not an
  Ogham concept at all.
- Zero tooltips. Zero dropdowns (settings uses inline segmented
  buttons because there's no popover primitive). Zero
  context menus (DM HUD comment at `dm_hud.ogh:16–20`
  explicitly says "Ogham doesn't have a positioned-popover
  primitive").
- Modal-on-modal (escape menu's disconnect confirm) is done by
  swapping subtrees on a `confirm_disconnect: bool` flag.
  Works, but locks the dialog inside the parent panel's
  bounds.

**Translation**
- The lifecycle-hook payoff in UL today is small (one
  `client_settings.save()`) but the ceiling is high — every
  UI's setup-in-Rust pattern (event handler chains, host_state
  seeding) is something authors would pull into `.ogh` if
  there were a place for it.
- The Portal payoff is concretely large: tooltips (currently
  not done), dropdowns (currently faked with segmented
  buttons), context menus (currently not done), and *the
  escape menu* itself (which would shed its hand-rolled
  overlay strategy).

---

## 2. Design — Lifecycle hooks

Headline proposal:

```ogh
let settings_panel = fn () {
  state form = { master_volume: master_volume, fov: fov };

  on_mount {
    // Runs once when this call-stack path first becomes live.
    // Has access to the same scope as the function body.
    log "settings opened";
  };

  on_unmount {
    // Runs once when this call-stack path stops being visited.
    // Has access to the *last-rendered* scope, not the
    // first — so it can read final state values.
    event("save_settings", form);
  };

  effect (form.master_volume) {
    // Re-runs whenever the named dep value changes between
    // renders. Mount = first run; unmount = cleanup.
    event("preview_master_volume", form.master_volume);
  };

  Flex { ... }
};
```

### 2.1 Identity model — recommended: path-based, same as state

`on_mount` fires the first time a given call-stack path is
visited; `on_unmount` fires the first frame that path is no
longer visited. This matches `state x` semantics exactly
(both share the same `StateManager` keying), so authors learn
one rule.

**Trade-off vs. instance-based:** path-based "moves" are
indistinguishable from "same identity" — a `settings_panel`
that moves from `column[2]` to `column[3]` between renders
would *not* fire mount/unmount (it's the same path; columns
are ordered). Authors who want explicit identity get it via
`key:`, same as for animations.

### 2.2 When hooks fire

Recommended ordering, after each VM render of a frame:

1. VM produces the new descriptor tree.
2. Reconcile diffs old vs new tree.
3. **`on_unmount` callbacks fire** for paths in old-tree but
   not new-tree. Run in reverse-tree-order (deepest first).
4. Layout runs.
5. **`on_mount` callbacks fire** for paths in new-tree but not
   old-tree. Run in tree-order (parents first).
6. Paint.

`on_mount` fires *after* layout so callbacks can read
post-layout sizes (useful for portal positioning). `on_unmount`
fires *before* layout so the unmounting subtree's last layout
is still valid when the callback runs.

**Drain-time, not ghost-begin**: when a widget begins exiting
(ghost state, exit animation in flight), its `on_unmount` is
*pending* but does not fire. It fires only when
`drain_exited_children` actually removes the widget from the
tree. Ghost cancellation (`cancel_exit`, when a re-mount
arrives mid-exit) cancels the pending unmount — it never
fires. This avoids a fragile "is it gone yet" check at the
cost of `on_unmount` being delayed by the exit-animation
duration.

### 2.3 Cleanup blocks

```ogh
effect (player.health) {
  let timer = start_timer(500);
  cleanup {
    cancel_timer(timer);
  };
};
```

A `cleanup { ... }` block inside an effect runs:
- Before the effect re-runs (when a dep changes).
- When the effect's owning path unmounts.

Modeled directly on React's `useEffect` return-function. Far
better than a separate `on_unmount` for paired setup/teardown.

For `on_mount`, no cleanup — pair with `on_unmount` if you
need teardown. (Effects subsume the common case.)

### 2.4 Async / IO

**Recommendation: synchronous only for v1.** No `await`, no
"return a Promise." Every callback runs to completion on the
main thread before the next frame.

If a callback needs to do async work, it dispatches an event
to Rust (`event("load_data", id)`); Rust does the I/O and
pushes results back via host_state. This is exactly today's
pattern, just initiated from `.ogh` instead of from Rust.

Cost: no first-class "load this image once on mount" inside
`.ogh`. Mitigation: this is what typed events are for —
`event("load_image", url)` is now type-checked by Phase 1.

### 2.5 Where the callback bodies execute

**Same VM, same scope, no special context.** The body of
`on_mount` / `on_unmount` / `effect` is just a block of
statements compiled inline. They have access to the
function's local state and parameters, can call `event(...)`,
and can read host state.

Widgets produced as expressions inside callback bodies are
discarded — same treatment as any other unused expression
value. Authors who write `on_mount { Tooltip { ... } }`
expecting it to mount the tooltip get nothing, but that's no
different from any other "value evaluated and not used"
situation in the language; no special-case rejection.

Cost: no easy way to share helper logic across mount blocks
(no `compose-able effects`). Mitigation: a `fn` can be
declared at module scope and called from the mount body.

---

## 3. Design — Portal

**Portal does one thing: lift its children's paint and
hit-test out of the parent's clip/order, rendering them into
the viewport at the parent's slot.** That's the entire
mechanism. Everything that *was* bundled into Portal in the
first draft (anchoring, backdrop, dismiss-on-outside,
dismiss-on-escape) becomes ordinary composition with the
existing primitives.

Headline shape:

```ogh
Portal {
  open: state.open,
  focus_trap: true,    // optional
  children: [ ... ],
}
```

Three properties total; the rest is children.

### 3.1 What the children look like in practice

Common patterns build out of regular widgets *inside* the
portal:

```ogh
// Tooltip — no backdrop, positioned below trigger via transform
Portal {
  open: state.hover,
  children: [
    Flex {
      style: { transform: { translate_y: 32 }, ... },
      children: [ Text { value: "Save your changes" } ],
    },
  ],
}

// Modal — backdrop + dismiss-on-outside via children
// (Escape-to-dismiss is a separate event handler in the
// consumer, not shown.)
Portal {
  open: state.modal_open,
  focus_trap: true,
  children: [
    // First child = full-viewport dim layer; click anywhere
    // dismisses. Author is in full control.
    Flex {
      style: { w: "100%", h: "100%", background_color: colors.backdrop },
      on_click: fn () { state.modal_open = false; },
    },
    // Second child = the actual dialog, positioned via Flex
    Flex {
      style: { ... center via Flex ... },
      children: [ dialog_body() ],
    },
  ],
}
```

Escape-to-dismiss is a normal global key handler in the
consumer — not a Portal property. UL already has key handling
infrastructure (`update.rs:1469`); a portal consumer adds an
event handler that toggles `state.open` on Escape, same way
they would for any other dismissable UI.

For repeated patterns (modals, tooltips, dropdowns), authors
write a `fn` once in their own `components.ogh`-style file
that wraps Portal with the boilerplate they want. We will
ship one or two such wrappers as part of the `examples/`
directory in M5, but they are not part of the language.

### 3.2 Layer system — two-pass paint, single canvas

Render pipeline becomes:

1. **Pass A**: walk the tree, paint normally. When a `Portal`
   node is encountered with `open: true`, *do not paint its
   children*; instead, push a `(WidgetRef, parent_rect)` pair
   onto a per-frame `portal_layer: Vec<...>`.
2. **Pass B**: paint everything in `portal_layer` after Pass
   A completes. Each portal paints with its own clip rect
   (the viewport, not the parent's). Within a portal, layout
   starts at `parent_rect` — i.e., a portal child with no
   transform appears exactly where the portal node would have
   appeared if it hadn't been a portal.

This is a small change relative to multi-surface compositing:
no new Skia surface, no z-index sort, no per-widget z. Just a
"this widget defers its own paint" branch in the recursive
walker.

Hit-testing mirrors paint: walk portal_layer first
(top-most-portal first), fall through to the base tree only if
no portal claims the click. A portal's children whose layout
covers the full viewport (a backdrop child) will swallow
clicks naturally — no Portal-level `dismiss_on_outside`
needed.

**Trade-off vs. real z-index**: portals can't interleave with
non-portal siblings ("render this widget on top of *just* its
parent's siblings, but below other portals"). For tooltips /
modals / dropdowns / context menus this isn't needed. If we
later need true z-index, the portal layer becomes the first
layer of a multi-layer stack — additive, not breaking.

### 3.3 Multiple portals stack, last-opened wins focus

A portal opened while another is already open paints on top.
For focus and Escape-style dismissal, the most-recently-mounted
portal "wins" — though since dismissal is consumer code, this
is really only a focus-trap concern (covered in §3.4).

### 3.4 Focus trap

`focus_trap: true` is what makes "modal" work as a focus-
isolation primitive. When a portal with `focus_trap` is on
the layer, the focus-management subsystem refuses to move
focus out of its subtree.

Today there's no focus stack — `UI.focused` is a single ref.
We add a small extension: a stack of "focus restoration
points" pushed on portal-with-focus-trap mount, popped on
unmount, with focus restored to the previous holder.

This is the lifecycle/portal interlock — the focus push/pop
is naturally driven by `on_mount` / `on_unmount` on the
portal itself, sharing the M0 plumbing.

Tooltips and other non-modal popovers leave `focus_trap`
unset (default false) and don't disturb focus.

### 3.5 Entry / exit animations on portal contents

`open: false` causes the portal's children subtree to be
treated as removed — the existing `begin_exit` / drain
machinery in `flex_widget.rs` runs on the children's
declared `exit:` styles, just like any other reconciled
removal. No special "fade out the portal" knob; authors
declare exit transitions on the children.

### 3.6 What happens to `overlay_active` in UL

Today UL has a Rust-level `overlay_active: bool` that gates
whether keyboard input reaches the game world. New runtime
API:

```rust
ogham.has_input_blocking_portal() -> bool
```

…returns `true` if any active portal has `focus_trap: true`.
UL's `overlay_active` becomes one line, derived from this
rather than tracked by hand.

(Note: with backdrop logic moved into children, the runtime
can no longer detect "this portal has a backdrop" from the
Portal itself — which is fine, because `focus_trap` is the
true signal for "this portal is modal." A backdrop-only
portal that doesn't trap focus is a tooltip-style overlay,
not a modal, and shouldn't gate game input.)

---

## 4. Migration story (UL, sketched)

| UL today                                       | After Phase 2                                       |
|------------------------------------------------|-----------------------------------------------------|
| `CloseSettings` action calls `.save()`         | `on_unmount` in `settings.ogh` fires `save_settings` event; Rust handler calls `.save()` |
| Escape menu = full-screen Flex w/ hardcoded backdrop | Escape menu = `Portal { focus_trap: true }` whose first child is the backdrop Flex (same as today's hardcoded backdrop, just inside the portal) |
| Confirm-disconnect = inline subtree swap on `bool` | Confirm-disconnect = nested `Portal` whose children center-align a styled dialog Flex |
| DM HUD context menu = "future Skia"            | `Portal { open: state.context_menu_open }` whose child Flex uses `transform: { translate_x: click.x, translate_y: click.y }` |
| Inventory item details inline in grid          | `Portal` over the hovered cell; child uses `transform: { translate_y: cell.h }` to appear below |
| `overlay_active: bool` tracked manually        | `overlay_active = ogham.has_input_blocking_portal()` |
| Settings dropdowns faked w/ segmented buttons  | True dropdowns: `Portal` whose child Flex translates below the trigger |
| 17 UIs each register handlers in `new()`       | Hand-pickable: setup that's purely UI moves into `on_mount`; setup that needs Rust resources stays |

Rough payoff: the escape menu shrinks (no `overlay_active`
plumbing — derived from `has_input_blocking_portal()`); two
new UI categories (tooltips, context menus) become trivially
expressible; one piece of Rust scaffolding per UI
(`new()`-time host_state seeding) becomes optional. Manual
backdrop / dismissal stays in the consumer's `.ogh`, which
matches Ogham's "each primitive does one thing" style.

---

## 5. Implementation plan — five merges

Each independently shippable; the branch convention from
typed bindings carries over. Minimal Portal collapses what
was previously M3+M4+M5 into M3+M4.

### M0 — Lifecycle plumbing (foundation)
- New `LifecycleEvent::{Mount, Unmount}` and a per-render
  diff that identifies appearing/disappearing call-stack
  paths.
- New `StateManager` API: `paths_visited_this_render() ->
  HashSet<CallStackPath>` plus a "compare against previous
  render" pass that yields mount/unmount events.
- Hook into `drain_exited_children` so `on_unmount` fires
  at drain-time, not ghost-begin. Cancellation
  (`cancel_exit`) cancels a pending unmount.
- No author surface yet — wire is internal-only and tested
  by direct unit tests.
- Validation: existing test suite passes; a unit test
  asserts mount/unmount events fire in the right order for
  a synthetic tree, and that ghost cancellation cancels
  pending unmounts.

### M1 — `on_mount` / `on_unmount` in `.ogh`
- Scanner: keywords `on_mount`, `on_unmount`.
- Parser: block-form expressions inside `fn` bodies.
- Compiler: emits a `RegisterMountHook(path, body_offset)` /
  `RegisterUnmountHook(path, body_offset)` opcode at
  module-load time; the runtime's per-render diff walks them.
- Runtime: at lifecycle-event time, executes the
  registered body in the same VM with the function's last
  scope.
- Validation: ~15 tests covering ordering, identity-by-path,
  `key:` overrides, ghost cancellation correctness, scope
  capture semantics.

### M2 — `effect` with deps + `cleanup`
- `effect (dep_a, dep_b) { ... cleanup { ... } }` syntax.
- Compiler tracks per-effect previous-dep-values in
  `StateManager` alongside `state` cells.
- Runtime compares current vs previous deps each render;
  fires effect (after running prior `cleanup` if any) on
  mismatch. Compile-time check: deps must be primitive or
  record values (no function refs / opaque values).
- Validation: ~10 tests covering dep equality, cleanup
  ordering, cleanup on unmount, mount-fires-effect-once.

### M3 — Portal: deferred-paint primitive
- New `PortalWidget` type with `open: bool` and `children`.
  Behaves like a no-op `Flex` for layout in Pass A.
- Render pipeline: `draw_widget_recursive` learns to push
  the portal onto the per-frame `portal_layer` instead of
  recursing into its children.
- Second pass after main render walks `portal_layer`,
  paints each one with the viewport as its clip rect, layout
  origin = parent_rect captured at portal-discovery time.
- Hit-testing: portal_layer searched first (LIFO), base
  tree searched only if no portal claims.
- `open: false` removes children, runs entry/exit
  animations as for any reconciled removal.
- Validation: a portal renders in front of its siblings
  even when its parent has `overflow: Hidden`. Multiple
  portals stack. A child Flex with full-viewport sizing
  swallows clicks (the backdrop pattern works).

### M4 — Portal: focus_trap + has_input_blocking_portal
- Add `FocusStack` to `UI`: stack of focus-restoration
  points pushed on portal mount when `focus_trap: true`,
  popped on unmount. Reuses M0/M1 lifecycle plumbing.
- New runtime API: `Ogham::has_input_blocking_portal()`
  returns `true` if any active portal has `focus_trap: true`.
  For UL's `overlay_active` derivation.
- Validation: focus is trapped inside a `focus_trap` portal;
  un-trapped portals (tooltips) leave focus alone; closing
  a focus-trap portal restores focus to the prior holder.

### M5 — UL validation pass + docs + worked examples
- Migrate Escape menu to Portal (highest-leverage real
  consumer; exercises focus_trap + the backdrop-as-child
  pattern + Escape handling in consumer code).
- Migrate Settings's `.save()` from `CloseSettings` action
  into an `on_unmount` block in `settings.ogh`.
- Add one tooltip (DM HUD or inventory) as a worked example.
- Add a `Modal()` and `Tooltip()` `fn` to `examples/` as a
  reference library — not part of the language.
- Promote `LIFECYCLE_AND_PORTAL.md` from "aligned action
  plan" to "live contract — Phase 2 shipped." Spin out
  `LIFECYCLE.md` updates and a new `PORTAL.md` with the
  same depth as `LIFECYCLE.md` today.

---

## 6. Resolved decisions (record from walkthrough)

| #   | Decision                                                                 | Rationale |
|-----|--------------------------------------------------------------------------|-----------|
| 1   | Identity = path-based, same as `state` cells                              | One identity rule across state, hooks, and portal contents |
| 2   | Hook timing: unmount before layout, mount after layout                    | Mount can read post-layout rects; unmount sees its last-valid layout |
| 3   | Cleanup lives only inside `effect`, not `on_mount`                        | Two ways to express teardown is confusing; can add to `on_mount` later if needed |
| 4   | Sync only in v1; async dispatches via `event(...)`                        | Async opens scope we're not ready for; matches today's pattern |
| 5   | `on_unmount` fires at drain-time, after exit animation completes          | Avoids "is it gone yet" ambiguity; cancellation cancels pending unmount |
| 6   | Callback bodies allow widget expressions; values discarded                | No special-case rejection; consistent with how Ogham treats other unused values |
| 7   | Effect deps explicit; React-17-style                                      | Auto-tracking is Shift C / signal territory; explicit deps are well-understood |
| 8   | Effect deps must be primitive or record values; compile-time error otherwise | Function refs / opaque values can't be compared; reject at module-load |
| 9   | Portal API: `open` + `focus_trap` + `children` only                       | Each primitive does one thing; backdrop / dismiss / anchor compose from existing widgets |
| 10  | Backdrop, dismiss-on-outside, anchor positioning live in user code        | Composable from Flex + style + on_click; no Portal-level policy |
| 11  | Escape-to-dismiss lives in user code, not on Portal                       | Same as #10; consumer adds an event handler |
| 12  | Multiple portals stack; last-opened wins focus-trap                        | Modal-on-modal works without special handling |
| 13  | Portal contents get entry/exit animations transparently                    | `open: false` is a normal reconcile removal on the children |
| 14  | `Modal` and `Tooltip` ship as `examples/` library `fn`s, not language     | Promote later if patterns crystallize |
| 15  | `on_visible` / `on_hidden` deferred                                        | Different semantic (visibility, not lifecycle); can add later if demand |

---

## 7. What this defers

- **Scenes / routing (Shift B)** — lifecycle hooks are the
  prerequisite, but the routing layer itself (lazy-mounted
  scenes, scene-scoped state, scene-level transitions) is
  Phase 3+. The 17 Ogham instances in UL stay 17 instances
  through Phase 2.
- **Signals / fine-grained reactivity (Shift C)** —
  effects in M2 use coarse path-based identity, not
  per-cell subscriptions. A signal-based effect would be
  cheaper but is much more invasive.
- **True z-index** — portals can render in front of the
  base tree but can't interleave with arbitrary siblings.
  Sufficient for tooltips / modals / dropdowns; insufficient
  for "render this card in front of just one of its peers."
  Additive change later if needed.
- **Animation completion callbacks (#2 from the
  priority list)** — separate, smaller piece of work.
  Worth doing in parallel; not bundled here because it
  doesn't share infrastructure with lifecycle/portal.
- **Slot-based composition (#4)** — also separate. Could be
  bundled with Phase 2 if there's appetite (it would let
  the user-defined `Modal`/`Tooltip` wrappers shipped in M5
  take styled slot children), but it expands the scope by
  about 50% and isn't a hard prereq.

---

## 8. Proposed branch + cadence

Mirroring Phase 1: single long-lived feature branch
`phase2-lifecycle-portal`, one commit per merge, validation
gate at each merge boundary, "rip and tear" if all checks
green. Five merges (collapsed from six after minimal-Portal
simplification), similar density to Phase 1 (~2,500–3,500
LOC in implementation + tests).

UL validation gate at M5 is the production-readiness check
(same role as the chest_ui migration was for Phase 1).
