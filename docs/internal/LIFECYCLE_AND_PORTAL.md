# Ogham — Lifecycle Hooks + Portal Widget (Phase 2)

> **Status: Live design contract — implementation pending.**
>
> This document specifies Phase 2 of the post-audit roadmap: a
> path-based callback lifecycle (`on_mount`, `on_unmount`,
> `effect` with `cleanup`) and a minimal `Portal` widget. It
> supersedes none of the existing live contracts; it adds two
> opt-in subsystems that compose with today's primitives.
>
> See [`INTENT.md`](INTENT.md) §3 for the path-based identity
> story this design extends from `state` to lifecycle.
> See [`ANIMATION_LIFECYCLE.md`](ANIMATION_LIFECYCLE.md) for the
> *animation* lifecycle (entry/exit/Presence) — a separate
> subsystem named "lifecycle" but operating on a different
> axis. See [`RUNTIME.md`](RUNTIME.md), [`VM.md`](VM.md), and
> [`LANGUAGE.md`](LANGUAGE.md) for the front-end and runtime
> machinery this design grafts onto. See
> [`WIDGET_TREE.md`](WIDGET_TREE.md) for the reconciliation
> rules Portal extends.
>
> The companion docs are
> [`LIFECYCLE_AND_PORTAL_UL_AUDIT.md`](LIFECYCLE_AND_PORTAL_UL_AUDIT.md)
> (the Untold Lore validation audit) and
> [`LIFECYCLE_AND_PORTAL_IMPLEMENTATION.md`](LIFECYCLE_AND_PORTAL_IMPLEMENTATION.md)
> (the per-merge implementation history, populated as M0–M5
> ship).

---

## Motivation

The original holistic audit ranked items #5 (lifecycle hooks)
and #6 (Portal) adjacent because they unlock adjacent UX —
modals, tooltips, dropdowns, context menus. The deeper reason
to ship them as one phase is mechanical:

- **Portal's mount/unmount IS the lifecycle event you want
  callbacks on.** When a tooltip portal opens, `on_mount` is
  what positions or focuses it. When a modal portal closes,
  `on_unmount` is what restores focus to the trigger and
  flushes any pending state. Shipping Portal without lifecycle
  forces every consumer to re-implement these in Rust.

- **Lifecycle without Portal is still useful** — typed UIs
  can do cleanup; the Settings UI's "save on close" pattern
  moves from `actions.rs` into the `.ogh` itself; per-UI
  setup that today lives in `new()` constructors becomes
  declarative. So lifecycle ships first and stands on its
  own. Portal then composes onto it.

- **Both share the same identity question.** "When does this
  thing logically appear or disappear?" — today's path-based
  state model (`StateManager.component_state` keyed by
  call-stack path) answers it for state. Phase 2 extends the
  *same* answer to lifecycle hooks and to Portal contents so
  authors learn one rule, not three.

Sequencing within Phase 2: lifecycle first (M0–M2,
foundation), then Portal (M3–M4, builds on the lifecycle
plumbing), then a UL validation pass (M5).

### Grounding from the audit

Two pieces of state-of-the-world from the exploration pass
inform every decision below.

**Ogham today**

- Single Skia canvas, strict tree, depth-first render, no
  z-index. Per-widget clip via `push_clip_rect` /
  `pop_clip_rect` in `flex_widget.rs:1604–1622`.
- State lives in `StateManager.component_state` keyed by
  call-stack path + variable name — *not* by widget instance.
  `state x = 0` survives any reconcile that produces the same
  call-stack path; it disappears when the path stops being
  visited (`cleanup_unmounted_state` at `mod.rs:136`).
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
- Zero tooltips. Zero dropdowns (settings uses inline
  segmented buttons because there's no popover primitive).
  Zero context menus (DM HUD comment at `dm_hud.ogh:16–20`
  explicitly says "Ogham doesn't have a positioned-popover
  primitive").
- Modal-on-modal (escape menu's disconnect confirm) is done
  by swapping subtrees on a `confirm_disconnect: bool` flag.
  Works, but locks the dialog inside the parent panel's
  bounds.

**Translation.** The lifecycle-hook payoff in UL today is
small in raw lines (one `client_settings.save()`) but the
ceiling is high — every UI's setup-in-Rust pattern (event
handler chains, host_state seeding) is something authors
would pull into `.ogh` if there were a place for it. The
Portal payoff is concretely large: tooltips (currently not
done), dropdowns (currently faked), context menus (currently
not done), and *the escape menu* itself (which would shed
its hand-rolled overlay strategy).

---

## Conceptual overview

A one-page mental model.

**Lifecycle hooks** are bodies of code attached to a
function definition that the runtime fires at structured
moments in the function's path-lifetime:

```ogh
let panel = fn () {
  on_mount   { /* fires when this path first becomes active   */ };
  on_unmount { /* fires when this path stops being active     */ };

  effect (some_dep) {
    /* fires after each render where some_dep changed value */
    cleanup { /* fires before next effect run or at unmount */ };
  };

  Flex { ... }   // the function's actual return value
};
```

The identity rule is the **same** as for `state`: the unit
of identity is the *call-stack path*, not a widget instance.
A function moved between sibling slots keeps its path-based
identity — and so keeps its mount, its `state`, and its
pending unmount. The escape hatch for cases where authors
want explicit identity is `key:`, identical to the existing
animation-lifecycle escape hatch.

**Portal** is a single new widget whose only job is to lift
its children's paint and hit-test out of the parent's clip
and order, rendering them into the viewport at the parent's
slot:

```ogh
Portal {
  open: state.open,
  focus_trap: true,    // optional
  children: [ ... ],
}
```

Three properties total. Backdrop, dismiss-on-outside, anchor
positioning, escape-to-dismiss — all *consumer-side*
composition using existing widgets. The portal does not
embed any of these as policy.

**The two compose** through their shared path-identity: a
portal mounts and unmounts in the same vocabulary as any
other path-keyed thing, so `on_mount` on a portal-using
function fires when the portal's call-stack path first
appears, not when the portal's `open: true` flips. (Authors
who want "fire when `open` flips" use `effect (open)
{ ... }`.)

---

## The contract surfaces

Phase 2 adds material to four surfaces. Each is bounded; no
existing surface is changed in a breaking way.

| Surface | Existing | Phase 2 adds |
|---|---|---|
| `.ogh` grammar | `let`, `state`, `fn`, `if`, `match`, `for`, widget literals, etc. | `on_mount` block, `on_unmount` block, `effect (deps) { ... }`, `cleanup { ... }` |
| Widget set | `Flex`, `Text`, `Image`, `Grid`, `Presence`, `Context`, custom widgets | `Portal` (built-in) |
| Runtime API | `Runtime::set_host_state`, `register_event_handler`, `inject_host_state`, etc. | `Runtime::has_input_blocking_portal() -> bool` |
| LSP | Phase 1 typed-bindings diagnostics, hover, semantic tokens | Lifecycle keyword highlighting, hook + Portal hover, conditional-hook warnings, malformed-deps errors |

What's **not** added: no new opcodes for the user-facing
surface (the new opcodes are internal compilation targets);
no changes to `state`, `host_state`, typed bindings, or any
existing widget; no new value types; no module loader
changes.

---

## Lifecycle hooks: grammar

Three new statement forms inside `fn` bodies.

### `on_mount`

```ogh
on_mount {
  // statements
};
```

The body is a brace-delimited block of statements that runs
in the same VM as the surrounding function. It has access
to all locals and parameters of the function via closure
capture. The body's value is discarded — `on_mount` is a
side-effect statement, not an expression.

A function may declare multiple `on_mount` blocks; they fire
in source order. Two blocks with side effects that need to
run in a specific order should both live in the same
`on_mount` body for clarity, but the multi-block form is
legal.

### `on_unmount`

```ogh
on_unmount {
  // statements
};
```

Same grammar as `on_mount`. The body has access to the
function's *last-rendered* scope (i.e., the most recent
render's locals and parameters). See the
[firing-timing section](#hook-firing-timing) for what
"last-rendered" means in detail.

### `effect`

```ogh
effect (dep_a, dep_b) {
  // statements
  cleanup {
    // statements
  };
};
```

An `effect` declares a body that re-runs whenever any of
the named dependencies changes value between renders. The
dep list is required (use `effect ()` for run-on-mount only)
and may contain any number of expressions. Each dep is
evaluated each render and compared via structural equality
to the previous render's value.

A `cleanup { ... }` block inside the effect body runs:

- Before the effect body re-runs (when a dep changes), and
- When the effect's owning path unmounts.

The cleanup block has access to the scope captured at the
time the *most recent* effect body executed — i.e., it can
see whatever the body computed.

### Multiple statements per function

A function may freely mix all three:

```ogh
let timer_panel = fn () {
  state elapsed = 0;

  on_mount   { event("timer_started"); };
  on_unmount { event("timer_stopped"); };

  effect (elapsed) {
    let id = event("schedule_tick", 100);
    cleanup { event("cancel_tick", id); };
  };

  Flex { children: [ Text { value: format(elapsed) } ] }
};
```

There is no maximum count and no specific ordering rule
between `on_mount` / `on_unmount` / `effect` declarations
within a function — each fires according to its own
schedule.

---

## Identity model

`on_mount` fires the first time a given call-stack path is
visited; `on_unmount` fires when that path stops being
visited (or, more precisely, when its owning widget drains
— see [hook firing timing](#hook-firing-timing)). This
matches `state x` semantics exactly — both share the same
`StateManager` keying — so authors learn one rule.

### Why path-based

A path-based model means: a `settings_panel` that moves
from `column[2]` to `column[3]` between renders is *the
same path* (columns are ordered); mount and unmount do not
fire. State is preserved. This is identical to today's
state behavior.

The trade-off is that "moves" are indistinguishable from
"same identity." Authors who want explicit identity
(common when iterating over a keyed list) get it via the
existing `key:` syntax, the same escape hatch used for
`state` and animation lifecycle.

### React's "no conditional hooks" rule does not apply

A natural question is whether Phase 2 should adopt React's
rule that hooks must be called unconditionally at the top
of a function body. The short answer is **no, the structural
constraint that drives that rule doesn't exist for us**.

React's `useState` etc. work via call-order indexing: the
first hook is index 0, the second is index 1, and the state
cells are stored in an array indexed by call order. A
conditional hook causes all subsequent indices to shift,
returning the wrong cell. The "no conditional hooks" rule is
a structural constraint that keeps the array indexing valid.

Ogham's `state` is keyed by `(path, varname)` — name-based,
not order-based. The lifecycle proposal extends this:
hooks are keyed by `(path, hook_id)` where `hook_id` is a
compile-time disambiguator analogous to a state cell's name.
A conditional hook just doesn't get re-registered next
frame; the map handles it cleanly.

That said, conditional hooks expose a *semantic* foot-gun:

```ogh
if is_admin {
  on_mount { fetch_admin_dashboard() };
}
```

If `is_admin` is true on the first render, the hook
registers and fires (the path is newly mounted). If
`is_admin` is false on the first render and flips true on
render 5, the hook registers on render 5 — but the path was
already mounted on render 1, so mount **does not fire**.
What the author probably wanted is `effect (is_admin)`.

Phase 2's policy: **conditional hooks are legal at runtime;
the LSP emits a warning** when it sees `on_mount` /
`on_unmount` / `effect` inside an `if` / `match` / loop
body. The warning text points the author to `effect`. This
preserves power-user expressiveness for cases where the
runtime semantic is intentional, while catching the common
confusion at write time.

### Disambiguation by `hook_id`

A function may declare multiple hooks of the same kind:

```ogh
let panel = fn () {
  on_mount { setup_a(); };
  on_mount { setup_b(); };
};
```

Each is assigned a compile-time `hook_id` (1, 2, …) by the
compiler walking the function body in source order.
`hook_id` is stable — adding a third `on_mount` between
the two existing ones shifts the third one's ID, but the
first two keep their IDs. (Authors who care about
identity-stability across edits should structure their
hooks to be append-only or use a single block.)

---

## Hook firing timing

Lifecycle events are dispatched at structured points
within each render cycle. The full ordering, after each
VM render of a frame:

1. **Begin frame.** Rotate `active_state_paths` →
   `previous_active_paths`; clear the new active set.
2. **Run module bytecode.** Hook opcodes register/refresh
   closures; function-entry inserts the current path into
   `active_state_paths` (subject to the
   [compile-time gate](#compiled-artifact-additions)).
3. **Reconcile.** The descriptor tree from step 2 is
   diffed against the live widget tree.
4. **Pre-layout: drain pending unmounts and effect
   cleanups.** Run `pending_unmounts` deepest-first, then
   `pending_effect_cleanups`.
5. **Layout** runs.
6. **Post-layout: drain pending mounts and effect fires.**
   Run `pending_mounts` parents-first, then
   `pending_effect_fires`.
7. **Paint.**
8. **Cleanup.** `cleanup_unmounted_state` runs as today.

`on_mount` fires *after* layout so callbacks can read
post-layout sizes — useful for portal positioning and
scroll-to-element patterns. `on_unmount` fires *before*
layout so the unmounting subtree's last layout is still
valid when the callback runs.

### Drain-time unmount, not reconcile-time

When a widget begins exiting (ghost state, exit animation in
flight), its `on_unmount` is *pending* but does not fire. It
fires only when `drain_exited_children` actually removes the
widget from the tree. This is the same window that today's
state cleanup uses.

Ghost cancellation (`cancel_exit`, when a re-mount arrives
mid-exit) cancels the pending unmount — it never fires.
This avoids a fragile "is it gone yet" check at the cost of
`on_unmount` being delayed by the exit-animation duration.

The mechanism: each widget records its `owned_path_prefix`
at construction. When a widget drains, the runtime walks
`unmount_hooks` and `effects` for any entries whose path
starts with that prefix and queues them for the *next*
frame's pre-layout slot. When a widget cancels exit, the
same prefix walk re-arms by re-inserting the affected
paths into the next active set's seed.

### Effect cleanup ordering

For an effect whose deps changed:

1. The pending cleanup (from the previous body run, if any)
   runs in step 4 above.
2. The effect body runs in step 6 above.
3. If the body registers a new cleanup, it becomes the new
   pending cleanup for the next dep change or unmount.

For an unmounting path: the cleanup runs in step 4 of the
frame the unmount drains, ahead of the unmount hook itself.
Cleanup-then-unmount is the consistent ordering.

### Worked timing diagram

```
Render N          Render N+1 (path P unmounts)    Render N+2
─────────         ──────────────────────────       ─────────
1. begin          1. begin                         1. begin
2. bytecode       2. bytecode (P not visited)      2. bytecode
   - P visited       - P → candidate_unmounts         (drain
   - mount           (held; P still in widget         flushes P)
     queued          tree as ghost)
3. reconcile      3. reconcile (P → ghost)        3. reconcile
4. pre-layout     4. pre-layout                    4. pre-layout
   (none)            (none — P not drained yet)       - P drained
                                                      → unmount
                                                        fires
5. layout         5. layout                        5. layout
6. post-layout    6. post-layout                   6. post-layout
   - mount           (none)                           (none)
     fires
7. paint          7. paint (ghost still painting)  7. paint
8. cleanup        8. cleanup (P state preserved    8. cleanup (P
                     while ghost lives)               state purged)
```

---

## Effects with deps and cleanup

### Dep semantics

Each dep is an expression evaluated each render. After the
expression yields a value, it is compared to the previous
render's value via `Value::eq` (the same equality used by
`Eq` opcode in the VM). If any dep value changed, the
effect is scheduled to fire after layout.

First-render special case: every dep has "no previous
value," so the effect always fires once on the path's
first visit. This matches React's `useEffect` first-mount
behavior.

Empty dep list (`effect () { ... }`) means "deps that
never change" — equivalent to "fire once on mount, run
cleanup on unmount." Useful for pure setup/teardown that
isn't tied to a specific value. Functionally equivalent to
`on_mount + on_unmount` but folds them into one block with
shared scope.

### Compile-time dep type check

Dep expressions must evaluate to **primitive or record
values** — anything for which structural `Value::eq` is
meaningful. Function references (`Value::Function`,
`Value::Closure`), mutation handles (`Value::Mutation`),
and opaque host-state values that don't define equality
are rejected at module-load time with the diagnostic:

```
error: effect deps must be primitive or record values;
       'on_click' is a function
```

The check happens in the strict-mode resolver pass (the
same pipeline that handles typed-bindings name resolution),
so the LSP surfaces it immediately. Runtime type errors
are not possible because the check fires before bytecode is
emitted.

### Cleanup as an explicit block

`cleanup { ... }` is a block-form statement that may appear
inside an `effect` body (and only there). The compiler
emits a `RegisterEffectCleanup` opcode that pops the
just-built cleanup closure and attaches it to the
currently-executing effect's slot.

The alternative — making the effect body's "return value"
be a cleanup function (React-style) — was rejected for
two reasons:

- Block-return-values aren't otherwise semantically
  meaningful in Ogham. Adding a special case for effect
  bodies would be inconsistent.
- Visual scanning is easier with an explicit `cleanup { }`
  delimiter than with `return fn () { }` at the end of a
  block.

A `cleanup` outside an `effect` body is a compile-time
error.

### Cleanup invocation contract

A cleanup body is called with the scope of the effect body
that registered it. If an effect runs three times across
five renders, each invocation registers its own cleanup
that supersedes the previous. The order is always
*previous-cleanup → new-body* on a dep change, never
overlapping.

If the effect's cleanup itself errors, the error is logged
and the new effect body still runs — see
[error policy](#error-policy).

---

## Async and IO

**v1 is synchronous only.** No `await`, no "return a
Promise," no first-class async hook bodies. Every hook
body runs to completion on the main thread before the next
frame.

If a hook body needs async work, it dispatches an event to
Rust:

```ogh
on_mount {
  event("load_inventory_for", character_id);
};
```

Rust does the I/O and pushes the result back via
`set_host_state`. This is exactly today's pattern, just
initiated from `.ogh` instead of from `actions.rs`.

Cost: no first-class "load this image once on mount"
inside `.ogh`. Mitigation: this is what typed events are
for — `event("load_image", url)` is now type-checked by
Phase 1, so the round-trip to Rust is no longer the
ergonomic regression it would have been pre-Phase-1.

Rationale for sync-only: async opens a scope (cancellation
on unmount, ordering across multiple in-flight asyncs,
error propagation) that's a Phase 3+ topic on its own.
Shipping sync-only now does not preclude an async layer
later — adding `async on_mount` is additive.

---

## Body scope and capture

**Same VM, same scope, no special context.** A hook body
is compiled exactly like any other closure: its bytecode
lives in a sub-`FunctionProto`, its captured upvalues are
described by `UpvalueDescriptor`, and the runtime invokes
it via the existing `call_bytecode_closure` API
(`runtime/mod.rs:574`).

Concretely, the body has access to:

- **Function locals and parameters** of the surrounding
  `fn`, captured as upvalues at the moment the
  `on_mount` / `on_unmount` / `effect` opcode executes.
- **State cells** at the same path, via normal `state`
  reads.
- **Host state** via normal host-state reads.
- **`event(name, args)`** dispatch.
- **Other functions** in scope (called normally).
- **The same `call_stack`** as the surrounding function —
  the closure carries its `captured_path` and the runtime
  restores it before invoking the body.

What this means for each hook kind:

- **`on_mount`**: captures the scope from the render that
  newly mounted the path. Fires once.
- **`on_unmount`**: captures the scope from the *most
  recent* render — re-registration each frame overwrites
  the closure. By the time it fires (drain-time), the
  upvalues reflect what the function last saw before the
  path stopped being visited.
- **`effect`**: captures the scope from the render where
  the effect last registered. Re-registration every frame
  is intentional — keeps the body's view of state fresh
  even when deps haven't changed.
- **`cleanup`**: captures the scope from the effect-body
  invocation that registered it. Stable across the
  effect's quiet periods.

### Widgets evaluated in a body are discarded

```ogh
on_mount {
  Tooltip { ... }    // evaluated, then dropped
};
```

The hook body's return value is discarded. A widget
expression in a hook body builds a descriptor that goes
nowhere — same treatment as any unused expression value.
There's no special-case rejection; it's quietly useless,
and the
[LSP](#lsp-integration) does not emit a diagnostic for it.

Authors who write this are confused about Ogham's model
(widgets must be in the function's *return* tree to render);
the design doc and tutorials should make this clear, but
the runtime does not enforce it.

### State writes from hook bodies

Allowed via normal `SetState` opcodes, which flag
`needs_rerender`. Two timing cases:

- **From `on_mount` (post-layout):** writes flag rerender,
  which fires the next frame. One extra render — same as
  React's `useEffect` re-render semantics.
- **From `on_unmount` (drain-time, pre-layout):** writes
  go to a path that's about to be cleaned up by
  `cleanup_unmounted_state` after the render. **The write
  is silently discarded** — the path's state cells are
  purged in the same frame, before any other code can
  read them. There is no error and no warning at runtime.

  This is the single most common migration foot-gun. The
  natural shape for save-on-close logic in `on_unmount`
  is **dispatch an event**, not write state:

  ```ogh
  // CORRECT: Rust handler does the I/O.
  on_unmount {
    event("save_settings", form);
  };

  // WRONG: write goes to a path about to be cleaned up.
  on_unmount {
    state.committed_form = form;  // discarded
  };
  ```

  The LSP hover for the `on_unmount` keyword surfaces this
  rule; the design intentionally does not warn at the call
  site (state writes are otherwise legal in any context,
  and a per-call-site check would cost too much). Author
  discipline plus the hover hint are the contract.

### Recursion and re-entrance

A hook body can call any function in scope. The called
function pushes onto `call_stack`, runs, pops back. If
that function itself has hooks, they get registered and
queued normally — but a hook fired during the post-layout
step that registers more mount hooks would queue them for
*next* frame's firing (the diff is computed once per
render at the start). This is a contained quirk; it is
unlikely to bite real consumers because hook bodies should
mostly be calling host events, not tree-building functions.

---

## Portal: API surface

```ogh
Portal {
  open: state.open,
  focus_trap: true,
  children: [ ... ],
}
```

Three properties. That's the entire API.

| Property | Type | Default | Meaning |
|---|---|---|---|
| `open` | `bool` | `false` | When `true`, children are mounted into the portal layer. When `false`, children are reconciled out (entry/exit animations apply normally). |
| `focus_trap` | `bool` | `false` | When `true`, focus cannot leave this portal's subtree while it is open. Push/pop on the focus stack is bound to this portal's mount/unmount. |
| `children` | `array<widget>` | `[]` | The widgets to render in the portal layer. Layout starts at the parent's slot rect; transforms apply normally. |

### What's deliberately not in the API

Everything that *was* bundled into Portal in the first
draft becomes ordinary composition with the existing
primitives:

- **Backdrop** → first child in `children` is a
  full-viewport `Flex` with `background_color: backdrop`.
- **Dismiss-on-outside** → `on_click` on the backdrop child
  toggles the consumer's `open` state.
- **Anchor positioning** → the second child uses
  `transform: { translate_x, translate_y }` to position
  itself relative to the portal's slot.
- **Escape-to-dismiss** → consumer adds an event handler
  that toggles `open` on Escape, same as for any other
  dismissable UI.

This is the
[composition section](#composition-patterns) in detail.

### Why three properties

The walkthrough collapsed several initially-considered
properties:

| Originally proposed | Resolution |
|---|---|
| `backdrop: bool` | Authors compose with a full-viewport Flex child — gives full control over color, opacity, dismiss behavior, animation. |
| `dismiss_on_outside: bool` | An `on_click` on the backdrop child does this exactly. |
| `dismiss_on_escape: bool` | A consumer-level key handler does this — and is needed for non-Portal dismissable UIs anyway. |
| `anchor: WidgetRef` | Anchor positioning is one transform call. Wiring it through portal would require widget refs as first-class values, which is its own large project. |
| `z_index: int` | Multiple portals stack last-opened-on-top; explicit z is YAGNI for our concrete use cases (tooltip / modal / dropdown / context menu). |

What remains (`open`, `focus_trap`, `children`) is the
minimum surface that doesn't decompose further.

---

## Two-pass paint and hit-testing

The render pipeline becomes:

1. **Pass A — main tree.** Walk the descriptor tree,
   paint normally. When a `Portal` node is encountered
   with `open: true`, **do not paint its children**;
   instead push a `(WidgetRef, parent_rect)` pair onto a
   per-frame `portal_layer: Vec<...>`. The portal node
   itself paints as a no-op (it's a layout zero, not a
   visual marker).
2. **Pass B — portal layer.** After Pass A completes,
   walk `portal_layer` in mount order. Each entry paints
   with its own clip rect (the viewport, not the parent's)
   and its own layout origin (`parent_rect.top_left`). A
   portal child with no transform appears exactly where
   the portal node would have appeared if it hadn't been
   a portal.

This is intentionally a small change relative to
multi-surface compositing. No new Skia surface, no z-index
sort, no per-widget z value. Just a "this widget defers
its own paint" branch in the recursive walker.

### Hit-testing

Mirrors paint: walk `portal_layer` first
(top-most-portal first), fall through to the base tree
only if no portal claims the click. Within a portal,
hit-testing is normal recursive depth-first.

A portal's children whose layout covers the full viewport
(a backdrop child) will swallow clicks naturally — no
Portal-level `dismiss_on_outside` flag needed.

### What this does NOT support

- **True z-index** — portals can render in front of the
  base tree but cannot interleave with arbitrary siblings
  ("render this widget on top of *just* its parent's
  siblings, but below other portals"). For our concrete
  use cases (tooltips / modals / dropdowns / context
  menus) this isn't needed. If we later need true z-index,
  the portal layer becomes the first layer of a multi-
  layer stack — additive, not breaking.
- **Cross-portal layout dependencies** — Portal A's
  layout cannot depend on Portal B's measurements. If you
  need that, both should live in the base tree or share a
  parent that wraps both.
- **Portal-specific transitions** — `open: false` runs the
  children's exit animations, not a portal-level
  "fade-out." Authors declare what they want on the
  children.

---

## Focus stack and `focus_trap`

`focus_trap: true` is what makes "modal" work as a focus-
isolation primitive. When a portal with `focus_trap` is
on the layer, the focus-management subsystem refuses to
move focus out of its subtree.

### The focus stack

Today there's no focus stack — `UI.focused` is a single
ref (`mod.rs:82`). Phase 2 adds:

```rust
pub struct UI {
    pub focused: Option<WidgetRef>,
    pub focus_stack: Vec<FocusRestoration>,
    // ...
}

pub struct FocusRestoration {
    /// Which portal owns this restoration point.
    portal_path: String,
    /// Where focus should return when this portal unmounts.
    previous_focus: Option<WidgetRef>,
}
```

When a `focus_trap: true` portal mounts:

1. Capture `UI.focused` as `previous_focus`.
2. Push a `FocusRestoration` onto `focus_stack`.
3. Optionally move focus to the portal's first focusable
   child (auto-focus behavior; toggleable via a
   convention property in consumer code, not Portal API).

When the portal unmounts (drain-time):

1. Pop the matching `FocusRestoration` from `focus_stack`.
2. Restore `UI.focused` from `previous_focus` (if the
   widget still exists).

While `focus_stack` is non-empty, focus-changing operations
(tab, arrow keys, programmatic `set_focused`) check that
the new target is within the topmost focus-trap portal's
subtree. If not, the operation is rejected.

### Stacking and last-opened-wins

A portal opened while another `focus_trap` portal is
already open paints on top and wins focus trapping. The
focus stack handles this naturally: each new
`focus_trap: true` portal pushes another `FocusRestoration`,
and trap-checks always consult the *top* of the stack.

A non-focus-trap portal (a tooltip) opened above a
focus-trap portal (a modal) does not push onto the focus
stack — focus stays trapped within the modal even though
the tooltip paints on top.

### Plumbing into lifecycle

The focus push/pop is naturally driven by the portal's
own `on_mount` / `on_unmount` — same machinery as any
other lifecycle hook. The Portal widget internally
registers these hooks as part of its widget construction;
they are not visible to the consumer's `.ogh` code.

---

## Portal animations and reconciliation

`open: false` causes the portal's children subtree to be
treated as **removed** — the existing `begin_exit` /
drain machinery in `flex_widget.rs` runs on the children's
declared `exit:` styles, just like any other reconciled
removal.

```ogh
Portal {
  open: state.open,
  children: [
    Flex {
      style: { /* ... */ },
      initial: { opacity: 0 },
      exit: { opacity: 0 },
      children: [ dialog_body() ],
    },
  ],
}
```

Setting `open: false` triggers the exit animation on the
inner Flex; once the spring settles, `drain_exited_children`
removes the widgets from the portal_layer.

There is no separate "fade out the portal" knob. Authors
declare animations on the children, which gives them
exact control over staggering, easing, and per-element
behavior.

### Reconciliation across `open` toggles

When `open` flips `false → true → false → true` quickly,
each transition runs through the standard reconciliation:

- `false → true`: children mount (entry animations run if
  declared).
- `true → false`: children begin exit; while exit is in
  flight, the children are ghosts in the portal_layer.
- `false → true` mid-exit: ghosts cancel exit and re-mount
  in place (preserving any partial entry state via the
  same mechanism `Presence` uses).

This works because `open` participates in normal
reconciliation — Portal's `children` is just a property,
and the runtime's existing diff handles it.

---

## Composition patterns

The patterns we expect consumers to build out of Portal +
existing widgets. None of these are language features;
all are user-side compositions. Phase 2 ships at least the
first two as `examples/` library `fn`s for reference.

### Tooltip

No backdrop, positioned relative to the trigger via
transform.

```ogh
Portal {
  open: state.hover,
  children: [
    Flex {
      style: {
        transform: { translate_y: 32 },
        background_color: colors.tooltip_bg,
        padding: 8,
        radius: 4,
      },
      children: [ Text { value: "Save your changes" } ],
    },
  ],
}
```

Focus is not trapped (default). The trigger keeps
keyboard focus.

### Modal

Backdrop + center-aligned dialog + dismiss-on-outside.

```ogh
Portal {
  open: state.modal_open,
  focus_trap: true,
  children: [
    // First child: backdrop. Click anywhere dismisses.
    Flex {
      style: { w: "100%", h: "100%", background_color: colors.backdrop },
      on_click: fn () { state.modal_open = false; },
    },
    // Second child: the dialog, centered via Flex.
    Flex {
      style: { w: "100%", h: "100%", justify: "center", align: "center" },
      children: [
        Flex {
          style: { w: 400, padding: 24, background_color: colors.surface },
          children: [ dialog_body() ],
        },
      ],
    },
  ],
}
```

Escape-to-dismiss is a consumer-level key handler (UL
already has this infrastructure at `update.rs:1469`).

### Dropdown

Trigger + portal that opens below the trigger via
transform. Backdrop dismisses on outside-click.

```ogh
let dropdown = fn (label, items, open) {
  Flex {
    children: [
      Button {
        label: label,
        on_click: fn () { open = !open; },
      },
      Portal {
        open: open,
        children: [
          Flex {
            style: {
              w: "100%", h: "100%",
              // Catch outside clicks.
            },
            on_click: fn () { open = false; },
          },
          Flex {
            style: {
              transform: { translate_y: 32 },
              background_color: colors.surface,
              radius: 4,
            },
            children: items,
          },
        ],
      },
    ],
  }
};
```

### Context menu

Trigger via right-click, position by the click's
coordinates.

```ogh
Portal {
  open: state.menu.open,
  children: [
    Flex {
      style: {
        transform: {
          translate_x: state.menu.x,
          translate_y: state.menu.y,
        },
        background_color: colors.surface,
      },
      children: menu_items_for(state.menu.target),
    },
  ],
}
```

### What ships in `examples/`

M5 ships `Modal()`, `Tooltip()`, and `Dropdown()` `fn`
wrappers in `examples/portals/components.ogh` so consumers
can adopt them without re-deriving the boilerplate. They
are **library code, not language**. (`Dropdown()` was
added based on the UL audit — Settings is an immediate
consumer; the implementation is ~30 LOC of `.ogh` over
the bare Portal primitive.)

---

## Runtime API additions

Phase 2 adds one Rust-facing API on `Runtime`:

```rust
impl Runtime {
    /// Returns true if any active portal in the tree has
    /// `focus_trap: true`. Used by hosts to derive their
    /// own input-gating booleans.
    pub fn has_input_blocking_portal(&self) -> bool { ... }
}
```

That's it. The rest of Phase 2's runtime work is internal
plumbing.

### Why this API and not more

Two natural alternatives were considered:

- **`active_portals()`** returning a list of all open
  portals: not needed by any concrete UL use case; would
  expose internal `WidgetRef`s; defer until a use case
  appears.
- **`portal_count()`** returning just an integer: weaker
  than `has_input_blocking_portal()` and doesn't answer
  the actual question consumers have ("should I gate
  input?"). Skip.

`has_input_blocking_portal` is the smallest API that
answers the actual UL use case (collapsing
`overlay_active`).

### Renaming the existing `set_host_state`

Out of scope. Phase 2 does not touch any existing public
API.

---

## Compiled artifact additions

This section names exactly what changes in the compiled
output of an `.ogh` module under Phase 2.

### New opcodes

```rust
// Pop closure, register it for path-newly-mounted firing.
RegisterMountHook(u16),     // hook_id

// Pop closure, register/overwrite (path, hook_id) -> closure.
// Re-registered every frame; fires at drain-time on unmount.
RegisterUnmountHook(u16),

// Pop dep_count values + closure. Compares deps to previous
// render's; if different (or first run), schedules cleanup-
// then-fire.
RegisterEffect { hook_id: u16, dep_count: u8 },

// Pop closure, attach as pending-cleanup for the currently-
// executing effect. Compile-time error if outside effect.
RegisterEffectCleanup,
```

Four opcodes total. The `hook_id` is a compile-time-assigned
disambiguator (1, 2, … in source order within a function).

### `StateManager` field additions

```rust
pub(crate) struct StateManager {
    // existing fields (unchanged)
    pub(crate) component_state: HashMap<String, Value>,
    pub(crate) call_stack: Vec<String>,
    pub(crate) active_state_paths: HashSet<String>,
    pub(crate) has_branched: bool,
    pub(crate) call_counters: HashMap<String, usize>,

    // NEW — diff state across renders
    pub(crate) previous_active_paths: HashSet<String>,

    // NEW — persistent registries for unmount + effect
    // (mount has no persistent registry; see below)
    pub(crate) unmount_hooks: HashMap<(String, u16), Rc<VMClosure>>,
    pub(crate) effects: HashMap<(String, u16), EffectSlot>,

    // NEW — per-frame queues
    pub(crate) pending_mounts: Vec<(String, u16, Rc<VMClosure>)>,
    pub(crate) pending_unmounts: Vec<(String, u16)>,
    pub(crate) pending_effect_fires: Vec<(String, u16)>,
    pub(crate) pending_effect_cleanups: Vec<(String, u16)>,

    // NEW — drain-time tracking
    pub(crate) candidate_unmounts: HashMap<String, ()>,
}

pub(crate) struct EffectSlot {
    pub previous_deps: Option<Vec<Value>>,
    pub pending_cleanup: Option<Rc<VMClosure>>,
    pub closure: Rc<VMClosure>,
}
```

### Mount has no persistent map

`on_mount`'s opcode handler:

```rust
OpCode::RegisterMountHook(hook_id) => {
    let closure = self.pop_closure()?;
    let path = runtime.state.get_call_stack_path();
    if !runtime.state.previous_active_paths.contains(&path) {
        // Path is newly mounted this frame.
        runtime.state.pending_mounts
            .push((path, hook_id, closure));
    }
    // Else: path already mounted; closure is dropped.
}
```

This eliminates a "did I fire this already?" bookkeeping
concern. `pending_mounts` is consumed at the post-layout
step and cleared.

### Compile-time `lifecycle_active` gate

The compiler walks the bytecode after compilation and sets
a module-level flag:

```rust
pub struct CompiledModule {
    pub proto: FunctionProto,
    pub lifecycle_active: bool,
    // ... existing fields
}
```

`lifecycle_active = true` if any of the four new opcodes
appears anywhere in the module or its imports. This flag
is exposed on the `Runtime` and gates the function-entry
path-marking:

```rust
// In Call opcode handler, after pushing path component:
if runtime.lifecycle_active {
    let path = runtime.state.get_call_stack_path();
    runtime.state.active_state_paths.insert(path);
}
```

Modules without lifecycle hooks pay one branch-predicted
boolean check per call and zero allocations. Modules with
hooks pay the cost on every call.

### Widget owns path prefix

The widget tree's existing `Widget` trait gains one new
method:

```rust
trait Widget {
    // ... existing methods
    fn owned_path_prefix(&self) -> &str;
}
```

Default implementation returns `""` (empty — owns no
specific paths). Widgets constructed by an `fn` call
record the function's path prefix; on drain, the runtime
walks `unmount_hooks` and `effects` for entries whose
key-path starts with that prefix and queues them for the
next frame's pre-layout slot.

### What does NOT change in the compiled artifact

- `state` opcodes (`DeclareState`, `GetState`, `SetState`).
- Closure / upvalue / function-frame machinery.
- All existing opcodes.
- The `FunctionProto` shape or layout.
- The constant pool.

---

## VM execution model

How the VM actually invokes hook bodies.

### Hooks reuse existing closure machinery

Hook bodies compile to sub-`FunctionProto`s, identical in
shape to any other closure. The compiler emits, for each
`on_mount { body }`:

```
[compile body as a sub-FunctionProto with no params]
Closure(proto_idx)         // creates closure, captures upvalues
RegisterMountHook(hook_id) // pops closure, registers it
```

Same for `on_unmount` and `effect`. The closure carries
its `captured_path` and upvalues through the existing
`Closure` opcode handler — no special path.

### Dispatch at lifecycle-event time

The runtime fires hooks via `call_bytecode_closure`
(`runtime/mod.rs:574`), which:

1. Spins a fresh `VM`.
2. Restores the closure's `captured_path` as
   `runtime.state.call_stack`.
3. Pushes the closure's upvalues onto the new frame.
4. Executes the closure's chunk.
5. Returns.

State reads/writes inside the body resolve against the
right path automatically because `call_stack` is set
correctly. No new dispatch primitive is needed.

### Mount, unmount, effect — different storage shapes

| Kind | Storage | Why |
|---|---|---|
| `on_mount` | None persistent — inline check at opcode time | Mount fires once per path-lifetime; queue and discard |
| `on_unmount` | `HashMap<(path, hook_id), Closure>`, overwritten each render | Has to outlive the function call to fire on a future drain |
| `effect` | `HashMap<(path, hook_id), EffectSlot>` with `previous_deps`, `pending_cleanup`, `closure` | Needs `previous_deps` to compare; persists across renders |

The mount simplification is intentional. `on_unmount` and
`effect` need persistent registries because they fire in
the *future* relative to their registration. `on_mount`
fires immediately or never, so a queue suffices.

### Dispatch order within a frame

```
Pre-layout step (after reconcile, before layout):
  1. Drain pending_unmounts (deepest-first).
     For each (path, hook_id):
       - Look up closure in unmount_hooks
       - Call it
       - Remove from unmount_hooks
  2. Drain pending_effect_cleanups.
     For each (path, hook_id):
       - Look up cleanup in effects[key].pending_cleanup
       - Call it
       - Set pending_cleanup = None

Layout runs.

Post-layout step:
  3. Drain pending_mounts (parents-first).
     For each (path, hook_id, closure):
       - Call closure
  4. Drain pending_effect_fires.
     For each (path, hook_id):
       - Look up closure in effects[key].closure
       - Call it
       - The closure may register a new pending_cleanup
         via RegisterEffectCleanup
```

Order within "deepest-first" / "parents-first" is determined
by sorting `pending_unmounts` / `pending_mounts` by path
length descending / ascending, respectively. This gives
predictable nesting behavior.

### Error policy

If a hook body errors (a `RuntimeError` propagates out of
`call_bytecode_closure`):

- Log to stderr / runtime error sink.
- **Continue with the next hook in the queue.** Do not
  propagate up to render.

Reasoning:

- Symmetric with existing `event(...)` semantics: missing
  handler returns `Void` rather than erroring.
- One bad hook should not break the frame.
- Compilation errors (malformed deps, etc.) get caught at
  module-load by the LSP / strict-mode resolver — they
  never reach runtime.

The cost is silent failures. M5 adds a per-frame
diagnostic counter exposed via a runtime API
(`Runtime::lifecycle_error_count() -> usize`) that hosts
can surface in debug overlays. The error itself is
stringified into a per-frame log buffer, also queryable.

### Re-entrance

A hook body that triggers a synchronous re-render is a
non-issue: state writes don't synchronously re-render;
they flag `needs_rerender` for next frame. So no
re-entrance risk during the post-layout dispatch step.

A hook body that calls a function that itself registers
new mount hooks: the new hooks are registered against the
*current* frame's `active_state_paths`, but `pending_mounts`
is consumed once per frame at step 3 above, so the new
hooks queue for *next* frame's firing. Documented quirk;
unlikely in practice.

---

## Renderer changes

### `portal_layer` per-frame stack

The render pipeline gains one struct field on the renderer:

```rust
pub struct Renderer {
    // ... existing
    portal_layer: Vec<PortalEntry>,
}

pub struct PortalEntry {
    widget: WidgetRef,
    parent_rect: Rect,
    focus_traps: bool,
}
```

`portal_layer` is cleared at the start of each frame. The
existing recursive painter (`draw_widget_recursive` in the
renderer module) gains a branch:

```rust
fn draw_widget_recursive(&mut self, widget: &WidgetRef, ...) {
    if let Some(portal) = widget.as_portal() {
        if portal.is_open() {
            self.portal_layer.push(PortalEntry {
                widget: widget.clone(),
                parent_rect: current_layout_rect(),
                focus_traps: portal.focus_trap(),
            });
        }
        // Portal node itself paints nothing.
        return;
    }
    // ... existing recursion
}
```

After the main pass:

```rust
fn paint_frame(&mut self, ...) {
    self.portal_layer.clear();
    self.draw_widget_recursive(root, ...);
    // Pass B
    for entry in &self.portal_layer {
        self.paint_portal_layer(entry);
    }
}
```

`paint_portal_layer` paints each portal's children with
the viewport as the clip rect and the portal's
`parent_rect.top_left` as the layout origin.

### Hit-test path

Symmetric:

```rust
fn hit_test(&self, point: Point) -> Option<WidgetRef> {
    // Try portals first, top-most-portal first (LIFO).
    for entry in self.portal_layer.iter().rev() {
        if let Some(hit) = hit_test_portal(entry, point) {
            return Some(hit);
        }
    }
    // Fall through to base tree.
    hit_test_recursive(root, point)
}
```

### Focus stack on `UI`

```rust
pub struct UI {
    pub focused: Option<WidgetRef>,
    pub focus_stack: Vec<FocusRestoration>,
    // ... existing
}
```

Focus-changing operations check
`focus_stack.last()`'s portal subtree before allowing the
move. See [focus stack](#focus-stack-and-focus_trap) for
detail.

### What does NOT change in the renderer

- Skia surface count: still one.
- Per-widget z value: not added.
- Existing `push_clip_rect` / `pop_clip_rect` calls.
- Animation tick loop.
- Layout pass (other than that portal nodes lay out as a
  zero-size box).

---

## LSP integration

Phase 2 extends Phase 1's LSP surface with lifecycle-
specific diagnostics, hover, and semantic-token coverage.

### Diagnostics

Five new diagnostic flavors. Three are errors, two are
warnings (the warning channel itself is new — Phase 1
emits errors only).

| # | Severity | Trigger | Message |
|---|---|---|---|
| 1 | ERROR | `cleanup { ... }` outside an `effect` body | `cleanup can only appear inside an effect block` |
| 2 | ERROR | `effect` dep expression of function or opaque type | `effect deps must be primitive or record values; '<name>' is a function` |
| 3 | ERROR | Portal `children` property is not an array of widgets | `Portal expects 'children' (array of widgets); got <type>` |
| 4 | WARNING | `on_mount` / `on_unmount` inside `if` / `match` / loop | `on_mount inside a conditional fires only if its path is also newly-mounted that frame; for "run when this flag changes" use 'effect (flag) { ... }'` |
| 5 | WARNING | `effect` inside `if` / `match` / loop | `effect inside a conditional won't be tracked when the condition is false; consider moving the effect to top-level and using 'if' inside the body` |

Implementation note: Phase 1's `SyntaxError` gains a
`severity: DiagnosticLevel` field (default `Error`) so the
warning channel piggybacks on existing infrastructure with
minimal churn. The LSP server's `collect_diagnostics`
maps `DiagnosticLevel::Warning → DiagnosticSeverity::WARNING`.

### Hover additions

Three new `HoverInfo` variants:

```rust
pub enum HoverInfo {
    // ... existing variants

    LifecycleHook { kind: HookKind },  // Mount or Unmount
    Effect { dep_count: usize },
    Portal,
}
```

Plus extending the existing `Keyword` variant to include
`cleanup`, with hover text explaining its bracket
("runs before the effect re-fires or when the path
unmounts").

Sample hover for `on_mount`:

```
on_mount { ... }

Fires once when this function's call-stack path first
becomes active. Body runs after layout. Has access to
the function's locals via closure capture.
```

Short and operational. The design doc is the reference;
hover is for orientation.

### Semantic tokens

Four new keyword token types added to the keyword arm of
`semantic_tokens.rs:65`:

- `OnMount`, `OnUnmount`, `Effect`, `Cleanup` → `KEYWORD`.

`Portal` is a built-in widget identifier registered in
`WidgetRegistry::with_defaults()`; it gets the existing
identifier-styling treatment, no special LSP handling.

### Goto-definition and document symbols

No changes for Phase 2:

- Lifecycle hooks don't introduce new bindable names —
  `on_mount` etc. are keywords, not identifiers.
- `Portal` is a built-in widget, no def to jump to.
- Document outline could surface hooks as child symbols
  of their function; deferred to Phase 3+ if outline-view
  feedback shows demand.

### Compile-time gate exposed to LSP

The `lifecycle_active` flag computed by the compiler is
exposed on the LSP-side `Document` so it can:

- Skip warning-walking for hookless modules (cheap noise
  reduction).
- Render `state` cell hovers with a "preserved across
  re-mounts" note when relevant.

---

## Migration story

UL gains, by layer:

| UL today | After Phase 2 |
|---|---|
| `CloseSettings` action calls `client_settings.save()` | `on_unmount` in `settings.ogh` fires `save_settings` event; Rust handler calls `.save()` |
| Escape menu = full-screen Flex w/ hardcoded backdrop | Escape menu = `Portal { focus_trap: true }` whose first child is the backdrop Flex |
| Confirm-disconnect = inline subtree swap on `bool` | Confirm-disconnect = nested `Portal` whose children center-align a styled dialog Flex |
| DM HUD context menu = "future Skia" | `Portal { open: state.context_menu_open }` whose child Flex uses `transform: { translate_x: click.x, translate_y: click.y }` |
| Inventory item details inline in grid | `Portal` over the hovered cell; child uses `transform: { translate_y: cell.h }` to appear below |
| `overlay_active: bool` tracked manually | `overlay_active = ogham.has_input_blocking_portal()` |
| Settings dropdowns faked w/ segmented buttons | True dropdowns: `Portal` whose child Flex translates below the trigger |
| 17 UIs each register handlers in `new()` | Hand-pickable: setup that's purely UI moves into `on_mount`; setup that needs Rust resources stays |

Rough payoff: the escape menu shrinks (no
`overlay_active` plumbing — derived from
`has_input_blocking_portal()`); two new UI categories
(tooltips, context menus) become trivially expressible;
one piece of Rust scaffolding per UI (`new()`-time
host_state seeding) becomes optional.

The detailed per-UI walkthrough lives in
[`LIFECYCLE_AND_PORTAL_UL_AUDIT.md`](LIFECYCLE_AND_PORTAL_UL_AUDIT.md).

---

## Worked example: escape menu end-to-end

The highest-leverage real consumer. Shows focus_trap +
backdrop-as-child + Escape-handling in consumer code +
nested portal for confirm-disconnect.

### Today

`escape_menu.ogh:100`:

```ogh
let escape_menu = fn () {
  if !show_overlay { return Flex {}; }

  Flex {
    style: {
      position: "absolute",
      w: "100%", h: "100%",
      background_color: colors.backdrop,  // alpha-160 black
      justify: "center", align: "center",
    },
    on_click: fn () { hide(); },
    children: [
      Flex {
        style: {
          w: 400, padding: 32,
          background_color: colors.surface,
        },
        children: [
          if confirm_disconnect {
            disconnect_dialog()
          } else {
            menu_buttons()
          },
        ],
      },
    ],
  }
};
```

Plus, on the Rust side at `update.rs:347`:

```rust
overlay_active: bool,  // tracked manually, set from update logic
```

…and at `update.rs:1469`, gating game input:

```rust
if !self.overlay_active {
    self.handle_world_input(input);
}
```

### After Phase 2

`escape_menu.ogh`:

```ogh
let escape_menu = fn () {
  state confirm_disconnect = false;

  Portal {
    open: host_state.show_escape_menu,
    focus_trap: true,
    children: [
      // Backdrop child — full viewport, click dismisses.
      Flex {
        style: {
          w: "100%", h: "100%",
          background_color: colors.backdrop,
        },
        on_click: fn () { event("hide_escape_menu"); },
      },

      // Centered dialog.
      Flex {
        style: {
          w: "100%", h: "100%",
          justify: "center", align: "center",
        },
        children: [
          Flex {
            style: { w: 400, padding: 32, background_color: colors.surface },
            initial: { opacity: 0, transform: { scale: 0.96 } },
            exit:    { opacity: 0, transform: { scale: 0.96 } },
            children: [
              if confirm_disconnect {
                // Nested portal — dialog-on-dialog.
                Portal {
                  open: true,
                  focus_trap: true,
                  children: [
                    Flex {
                      style: { /* backdrop */ },
                      on_click: fn () { confirm_disconnect = false; },
                    },
                    Flex {
                      style: { /* centered confirm */ },
                      children: [ disconnect_confirm_buttons() ],
                    },
                  ],
                }
              } else {
                menu_buttons(confirm_disconnect)
              },
            ],
          },
        ],
      },
    ],
  }
};
```

On the Rust side, the overlay-active boolean collapses to
one derivation:

```rust
fn update(&mut self, dt: f32) {
    let overlay_active = self.ogham.has_input_blocking_portal();
    if !overlay_active {
        self.handle_world_input(input);
    }
    // ...
}
```

The Escape-to-dismiss handler, which today lives in
`update.rs:1480`, stays in Rust — it dispatches the
`hide_escape_menu` event when the user presses Escape and
the menu is open. Phase 2 does not require moving keyboard
handling into `.ogh`.

### What changed mechanically

- `escape_menu.ogh` shrinks slightly (the conditional
  early-return at top is replaced by `open:`).
- The hardcoded `position: absolute` plus `w/h: 100%`
  pattern is gone — Portal handles the viewport mounting.
- `overlay_active` is no longer tracked; one line of Rust
  derives it.
- The nested `confirm_disconnect` dialog is no longer
  bounded by its parent's panel — it gets its own portal
  layer, can extend past the escape menu's bounds.
- Focus is automatically trapped within the menu and within
  the confirm dialog when present.
- Entry/exit animations on the dialog Flex are now declared
  on the right widget (the dialog itself, not a wrapper),
  thanks to portal-level reconciliation.

### What's the same

- The visual result is identical (modulo the entry/exit
  animations being more correct).
- The Rust event handlers (`hide_escape_menu`,
  `disconnect`, etc.) are unchanged.
- The keyboard handling stays in Rust.

---

## Resolved design decisions

The 15 decisions locked through walkthrough alignment.
This is the canonical record; sections above derive from
these.

| # | Decision | Rationale |
|---|---|---|
| 1 | Identity = path-based, same as `state` cells | One identity rule across state, hooks, and portal contents |
| 2 | Hook timing: unmount before layout, mount after layout | Mount can read post-layout rects; unmount sees its last-valid layout |
| 3 | Cleanup lives only inside `effect`, not `on_mount` | Two ways to express teardown is confusing; can add to `on_mount` later if needed |
| 4 | Sync only in v1; async dispatches via `event(...)` | Async opens scope we're not ready for; matches today's pattern |
| 5 | `on_unmount` fires at drain-time, after exit animation completes | Avoids "is it gone yet" ambiguity; cancellation cancels pending unmount |
| 6 | Callback bodies allow widget expressions; values discarded | No special-case rejection; consistent with how Ogham treats other unused values |
| 7 | Effect deps explicit; React-17-style | Auto-tracking is Shift C / signal territory; explicit deps are well-understood |
| 8 | Effect deps must be primitive or record values; compile-time error otherwise | Function refs / opaque values can't be compared; reject at module-load |
| 9 | Portal API: `open` + `focus_trap` + `children` only | Each primitive does one thing; backdrop / dismiss / anchor compose from existing widgets |
| 10 | Backdrop, dismiss-on-outside, anchor positioning live in user code | Composable from Flex + style + on_click; no Portal-level policy |
| 11 | Escape-to-dismiss lives in user code, not on Portal | Same as #10; consumer adds an event handler |
| 12 | Multiple portals stack; last-opened wins focus-trap | Modal-on-modal works without special handling |
| 13 | Portal contents get entry/exit animations transparently | `open: false` is a normal reconcile removal on the children |
| 14 | `Modal`, `Tooltip`, and `Dropdown` ship as `examples/` library `fn`s, not language | Promote later if patterns crystallize. `Dropdown` added per UL audit (Settings consumer). |
| 15 | `on_visible` / `on_hidden` deferred | Different semantic (visibility, not lifecycle); can add later if demand |

Plus three decisions reached during the implementation-doc
walkthrough:

| # | Decision | Rationale |
|---|---|---|
| 16 | Conditional hooks legal at runtime; LSP warns on them | Underlying React constraint doesn't apply to path-keyed identity; warn to catch the semantic foot-gun |
| 17 | Mount has no persistent registry — inline check at opcode time | Mount fires once per path-lifetime; queue suffices |
| 18 | Hook body errors logged-and-continued, not propagated | Symmetric with `event(...)` semantics; one bad hook shouldn't break the frame |

---

## Out of scope (explicitly deferred)

- **Scenes / routing (Shift B)** — lifecycle hooks are
  the prerequisite, but the routing layer itself
  (lazy-mounted scenes, scene-scoped state, scene-level
  transitions) is Phase 3+. The 17 Ogham instances in UL
  stay 17 instances through Phase 2.
- **Signals / fine-grained reactivity (Shift C)** —
  effects in Phase 2 use coarse path-based identity, not
  per-cell subscriptions. A signal-based effect would be
  cheaper but is much more invasive.
- **True z-index** — portals can render in front of the
  base tree but cannot interleave with arbitrary siblings.
  Sufficient for tooltips / modals / dropdowns;
  insufficient for "render this card in front of just one
  of its peers." Additive change later if needed.
- **Animation completion callbacks** (#2 from the priority
  list) — separate, smaller piece of work. Worth doing in
  parallel; not bundled here because it doesn't share
  infrastructure with lifecycle/portal.
- **Slot-based composition** (#4) — also separate. Could
  be bundled if there's appetite (it would let the
  user-defined `Modal`/`Tooltip` wrappers take styled slot
  children), but it expands scope by ~50% and isn't a hard
  prereq.
- **Async hook bodies** — `await` in hooks. Defer to a
  future async pass; sync-only in v1.
- **`on_visible` / `on_hidden`** — different semantic
  (visibility-via-scroll-or-occlusion, not lifecycle); can
  add later if demand.
- **Lifecycle hooks at module top-level** — hooks must
  appear inside an `fn` body. Module-top-level hooks
  would need a separate "module lifecycle" concept that
  isn't motivated by current use cases.
- **Hook composition / "useX" helpers** — no first-class
  way to write a function that returns hooks (`fn useTimer()
  { state ...; on_mount ...; }` doesn't compose because
  hooks are statements, not expressions). React's hooks
  composition is enabled by their call-order indexing,
  which we explicitly don't have. Workaround: wrap the
  shared logic in a function and call it from inside the
  hook body. Acceptable for v1.

---

## Open implementation questions

Items still loose at design-doc time. Each will be
resolved during the corresponding merge or kicked to a
follow-up explicitly.

1. **Diagnostic surface for hook body errors.** §18 says
   log-and-continue, with a `Runtime::lifecycle_error_count()`
   API. What does the per-frame error log buffer look
   like — `Vec<String>`, `Vec<RuntimeError>`, ring
   buffer? Decide during M1.
2. **`on_unmount` re-registration cost.** Re-registering
   the closure every frame allocates one `Rc<VMClosure>`
   per hook per render. For a UL frame with ~50 hooks,
   that's ~50 small allocations. Probably fine; revisit
   if profiling shows it.
3. **Effect dep capture timing.** Deps are evaluated at
   `RegisterEffect` opcode time. If a dep expression has
   side effects (e.g. calls a function that mutates state),
   the side effect runs every render even when the deps
   compare equal. Document as: "dep expressions should be
   pure reads."
4. **Multiple portals at the same path.** If a function
   declares two `Portal { ... }` widgets in its return
   tree, both register at the same path with different
   slot positions. Should focus stacking respect declaration
   order or mount order? Mount order is the proposal;
   confirm during M3.
5. **Portal inside a `Presence` container.** `Presence`
   sequences generations; a portal inside a generation
   that's exiting needs to either stay open through the
   exit or close. The §13 "open: false runs exit
   animations on children" handles the close case; the
   stay-open case is whatever the parent decides via
   `open:`. Should be consistent; add to M3 test plan.
6. **`focus_stack` cleanup on runtime reset.** Hot-reload
   resets the runtime; what happens to the focus stack?
   Probably: clear it, restore `UI.focused = None`. Add
   to M4 test.
7. **Drain-time unmount and module reload.** When the
   `.ogh` file changes mid-frame and the module reloads,
   pending unmounts are tied to the OLD module's
   compiled artifact. Likely they should fire-and-flush
   before the reload swaps in, but the timing is
   delicate. Add to M5 hot-reload test plan.
8. **Hover hint for state cells preserved across remounts.**
   §20 mentions this. Concrete UX is open: do we say
   "state preserved" only when path identity is stable
   across animation cycles, or always? Defer to M3 LSP
   work; default to always-show for safety.
9. **Owned-path-prefix cost on widget tree.** Every
   widget gains an `owned_path_prefix: String` field,
   even widgets that don't host any state/hooks. For a
   tree of ~5,000 widgets this is ~5,000 small string
   allocations. Could be `Option<String>` or `Cow<'static,
   str>` to make the empty case free. Decide during M0.
10. **Mount fires before paint, not before layout.** §7
    has mount firing post-layout but pre-paint. If a
    mount body calls `request_rerender` synchronously,
    paint of the current frame still proceeds with the
    pre-rerender layout. Document explicitly.

---

## Phase 2 deliverables

| Layer | Phase 2 ships |
|---|---|
| Grammar | `on_mount`, `on_unmount`, `effect`, `cleanup` keywords; statement forms; dep list syntax |
| Compiler | Four new opcodes; `lifecycle_active` flag; effect dep type-check |
| Runtime | New `StateManager` fields; lifecycle dispatch; focus stack |
| Widget tree | `owned_path_prefix` per widget; drain-time hook flush |
| Renderer | `portal_layer` two-pass paint; portal hit-test; `Portal` widget |
| Built-ins | `Portal` widget registered in `WidgetRegistry::with_defaults()` |
| Runtime API | `Runtime::has_input_blocking_portal() -> bool`, `lifecycle_error_count() -> usize`, `lifecycle_error_log() -> &[String]` |
| LSP | 5 new diagnostics; 3 new hover variants + `cleanup` keyword hover; 4 new keyword tokens |
| Tests | ~50 new tests across the three layers (lifecycle plumbing, hook semantics, portal paint/hit, focus stack, UL escape menu integration) |
| `examples/` | `Modal()`, `Tooltip()`, and `Dropdown()` library `fn`s in `examples/portals/components.ogh` |
| Docs | `LIFECYCLE_AND_PORTAL.md` (this file, promoted to "Live contract" at M5); `LIFECYCLE_AND_PORTAL_UL_AUDIT.md`; `LIFECYCLE_AND_PORTAL_IMPLEMENTATION.md`; cross-doc index updates |
| UL migration | Settings save-on-close (M5); escape menu → Portal (M5); one tooltip as worked example (M5) |

The detailed per-merge breakdown lives in
[`LIFECYCLE_AND_PORTAL_IMPLEMENTATION.md`](LIFECYCLE_AND_PORTAL_IMPLEMENTATION.md).

### What "Phase 2 ships" looks like

- `on_mount` / `on_unmount` / `effect` / `cleanup` are
  keywords; the LSP highlights, hovers, and warns
  appropriately; the strict-mode resolver type-checks
  effect deps.
- `Portal` is a built-in widget; tooltips and modals are
  expressible in `.ogh`; the escape menu in UL has been
  migrated as the validation gate.
- `Runtime::has_input_blocking_portal()` is documented
  and used by UL to derive its overlay-active boolean.
- `examples/portals/components.ogh` ships with `Modal()`,
  `Tooltip()`, and `Dropdown()` reference wrappers.
- All Phase 1 functionality continues to work unchanged.
- Hot reload preserves lifecycle state across reloads (or
  flushes deterministically — see open question #7).

---

## What this design does NOT solve, restated

- **Animation completion callbacks** — separate item;
  doable in parallel.
- **Slot-based composition** — separate item; bundling
  with Phase 2 is possible but not currently in scope.
- **True z-index for non-portal widgets** — only portals
  escape the strict-tree paint order.
- **Per-component lazy loading / scenes** — needs a
  routing layer.
- **First-class async / cancellation** — sync-only in v1.
- **Hook composition** — no first-class way to package
  hooks into reusable units; wrap shared logic in
  ordinary functions instead.
- **Per-cell reactivity / signals** — coarse path-based
  identity throughout.
- **`on_visible` / `on_hidden`** — visibility is a
  separate axis from path-lifetime.

These remain valid Phase 3+ topics. Phase 2's design
intentionally leaves room for each — none of them require
breaking changes to the surfaces shipped here.

---

## Migration cookbook

Common patterns translating UL Rust scaffolding to `.ogh`
hooks. Pick the row that matches your situation.

### "Set up host state when this UI opens"

**Today (Rust, in some `new_<UI>()` function):**

```rust
ogham.set_host_state("inventory_items", load_items());
ogham.set_host_state("inventory_filter", "all");
```

**After Phase 2 (`.ogh`):**

```ogh
on_mount {
  event("load_inventory_items");
  // host_state.inventory_filter has its own default.
};
```

### "Save state when this UI closes"

**Today:**

```rust
// In CloseSettings action handler:
self.client_settings.save();
```

**After Phase 2:**

```ogh
on_unmount {
  event("save_settings", form);
};
```

(Rust handler `save_settings` calls `.save()`.)

### "Cancel a timer when this UI closes"

**Today:** typically not done; UIs leak timers because
nothing forces cleanup.

**After Phase 2:**

```ogh
effect () {
  let id = event("start_timer", 1000);
  cleanup { event("cancel_timer", id); };
};
```

The empty dep list runs the body once on mount; the
`cleanup` runs on unmount.

### "React to a host_state value changing"

**Today:** manual `OnUpdate` or polling in Rust.

**After Phase 2:**

```ogh
effect (host_state.player_health) {
  if host_state.player_health < 20 {
    event("play_low_health_sfx");
  }
};
```

### "Move a tooltip from 'not implemented' to 'shipped'"

**Today:** doesn't exist.

**After Phase 2:**

```ogh
let with_tooltip = fn (text, child) {
  state hover = false;
  Flex {
    on_pointer_enter: fn () { hover = true; },
    on_pointer_leave: fn () { hover = false; },
    children: [
      child,
      Portal {
        open: hover,
        children: [
          Flex {
            style: { transform: { translate_y: 32 }, /* tooltip styling */ },
            children: [ Text { value: text } ],
          },
        ],
      },
    ],
  }
};
```

### "Convert an inline modal to a real Portal"

**Today (UL escape menu disconnect-confirm):**

```ogh
if confirm_disconnect {
  // inline subtree; bounded by parent panel
  disconnect_dialog()
}
```

**After Phase 2:**

```ogh
if confirm_disconnect {
  Portal {
    open: true,
    focus_trap: true,
    children: [
      Flex { /* backdrop */ on_click: fn () { confirm_disconnect = false; } },
      Flex { /* centered dialog */ children: [ disconnect_dialog() ] },
    ],
  }
}
```

### "Replace `overlay_active: bool` with derived state"

**Today (Rust, manual tracking):**

```rust
self.overlay_active = matches!(self.current_ui,
    UI::EscapeMenu | UI::Settings | UI::Inventory);
```

**After Phase 2:**

```rust
let overlay_active = self.ogham.has_input_blocking_portal();
```

### "Hook into a `Presence` generation transition"

Use the existing `Presence` container — Phase 2 doesn't
add anything new for this. Pair `Presence` with hooks if
you want side effects per generation:

```ogh
Presence {
  children: [
    settings_panel(state.tab),  // hooks fire per-generation
  ],
}
```

Each `settings_panel` invocation has its own path;
generation switches mount/unmount the appropriate paths.

---

That's the design. The implementation contract for each
piece — opcode emission rules, exact field types, test
matrices, per-merge dependencies — lives in
[`LIFECYCLE_AND_PORTAL_IMPLEMENTATION.md`](LIFECYCLE_AND_PORTAL_IMPLEMENTATION.md).
The UL-specific impact analysis lives in
[`LIFECYCLE_AND_PORTAL_UL_AUDIT.md`](LIFECYCLE_AND_PORTAL_UL_AUDIT.md).

This document graduates from "Live design contract" to
"Live contract" at M5, when the implementation has shipped
and the UL escape-menu validation gate has passed.
