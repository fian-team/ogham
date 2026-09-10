# Ogham — Events

> **Status: Live contract.**
>
> Two layers, both called "events", both worth keeping straight:
>
> 1. **UI events** — mouse / keyboard / scroll input dispatched
>    through the live widget tree. Handlers declared on widgets
>    fire as part of input handling.
> 2. **Host events** — the `event(name, ...)` and
>    `mutation("name").trigger(...)` calls that leave the
>    runtime and reach handlers registered by the embedder.
>
> The two share the word "event" but are entirely separate
> mechanisms with separate types. This doc covers both, in that
> order.

---

## Layer 1 — UI events

### Authority

- Event types: [`src/widget/event.rs`](../../src/widget/event.rs).
- Top-level dispatch: `UI::call_event` in
  [`src/widget/mod.rs`](../../src/widget/mod.rs).
- Per-widget handling: each widget's `handle_event` impl. Flex
  is the canonical reference
  ([`src/widget/flex_widget.rs`](../../src/widget/flex_widget.rs)).
- Hover propagation: `UI::update_hover_recursive` in
  [`src/widget/mod.rs`](../../src/widget/mod.rs).

### `Event` struct

```rust
struct Event {
    name: String,             // "mouse_down", "mouse_up", "scroll", "keydown", "drag_start", …
    point: Option<Point>,     // for pointer events
    keyboard_data: Option<KeyboardData>,
    callback: Option<Box<dyn Fn(&Event)>>,
    value: Option<String>,    // for "on_change" etc.
    scroll_delta: Option<(f32, f32)>,
    payload: Option<Value>,   // Phase 3 — drag payload travels here through
                              // drag_start / drag_move / drag_end dispatch
}
```

Constructors: `Event::new(name)`, `Event::with_point(name, p)`,
`Event::with_keyboard(...)`, `Event::with_value(name, value)`,
`Event::scroll(point, dx, dy)`, `Event::keydown/keypress/keyup`,
`Event::drag(name, point, payload)`.

`Event::shift_point(dx, dy)` returns a clone with the point
shifted; this is how the dispatcher walks the tree without
giving each widget the full ancestor chain. **The `callback`
field is dropped on shift** because it isn't `Clone`-able.

### `EventContext`

A side-channel that travels with a dispatch call, used for
coordination with the UI root:

- `focus_request: Option<WidgetRef>` — set by widgets that want
  to take focus. The root reads this after dispatch and updates
  `UI::focused`.
- `focused_widget: Option<WidgetRef>` — read-only inside
  dispatch; lets widgets check `ctx.is_focused(self_ref)`.
  Compares `Arc::as_ptr` to handle the
  cloned-Arc-different-instance case.
- `listener_fired: bool` — set when a real listener runs.
  Critical for distinguishing "a child consumed the event by
  invoking a real handler" from "a child returned `true` purely
  because `block_interactions` was set". See tenets below.
- `drag_state: Option<DragState>` (Phase 3) — set by
  `UI::dispatch_drag_*` for the duration of a drag dispatch.
  Widgets read it for hover highlighting and `accepts_drop`
  checks against the current cursor.
- `needs_layout: bool` — set when an event handler mutated
  widget state in a way that affects layout (e.g. `TextInput`
  with `width: Shrink` ingesting a character). Most handlers
  just fire host events; the resulting state flows back through
  reconcile, so a same-frame layout pass is unnecessary.
- `captured_widget: Option<WidgetRef>` / `request_capture(widget,
  local_point)` / `release_capture()` (2026-09-07) — pointer
  capture, below. `is_captured(self_ref)` is how a widget learns
  the point it was handed may lie outside its rect.

### `TickContext` (per-frame, not per-event)

Distinct from `EventContext`: `TickContext`
(`src/widget/event.rs`) is the per-tick context threaded
through `Widget::tick_animations`. It carries:

- `dt: f32` — elapsed seconds since the last tick.
- `drained_path_prefixes: Vec<String>` — path prefixes whose
  owning widgets drained this tick (settled exit animation).
  Populated by `FlexWidget::drain_exited_children` and the
  equivalent path on other drainable widgets. Consumed by
  `UI::tick_animations` → `UI.pending_drained_prefixes` →
  next render's `Runtime::process_drain_queues` to fire
  delayed `on_unmount` / effect cleanups.
- `cancelled_unmount_prefixes: Vec<String>` — mirror of the
  above for re-mount-during-exit cancellations.

`TickContext` replaces the bare `dt: f32` parameter that
existed before Phase 3 M0 — the side channels let drag and
drain machinery share infrastructure without growing
`TickResult` into a Vec-bearing struct that loses `Copy`.

### `DragState` (per-frame)

```rust
struct DragState {
    origin_widget: Option<WidgetRef>, // where drag_start fired
    payload: Option<Value>,            // declared by originator
    start_position: Point,             // anchor for dead-zone math
    current_position: Point,           // most recent dispatch
    past_dead_zone: bool,              // true once cursor moved past N px
}
```

Constructed by `UI::dispatch_drag_start`; the host's input pump
threads the same `DragState` through subsequent
`dispatch_drag_move` / `dispatch_drag_end` calls. Lifecycle
ends on `dispatch_drag_end` or when the host drops it
(cancellation).

### Dispatch model

#### Pointer events (with `point`)

`UI::call_event` for pointer events:
1. **`mouse_move`**: special-cased. Walks the tree updating
   `hovered` flags via `update_hover_recursive`. Fires
   `mouse_enter` / `mouse_leave` listeners on widgets whose
   hover state flipped. Returns whether anything changed.
   Marks the UI for **repaint only** (not relayout).
2. **Other pointer events** (clicks, scroll): clears focus,
   builds a fresh `EventContext`, calls `handle_click_event`
   which delegates to `root.handle_event(...)`.

`FlexWidget::handle_event` (the canonical impl) for pointer
events:
1. Bail if `!self.contains_point(point)`.
2. Special-case scroll for `Overflow::Scroll` containers: update
   `scroll_y_target`, return `true`.
3. Build a `local_event` with the point shifted into this
   widget's content space (subtract origin, add scroll offset).
4. Iterate children **in declaration order**; the first child to
   return `true` consumes the event (`break`).
5. Decide whether *this* widget's listener should fire: only if
   `!ctx.listener_fired` *and* this widget has a listener for
   the event name. If it fires, set `ctx.listener_fired = true`.
6. Return `self.block_interactions || child_consumed || my_fired`.

#### Pointer capture (2026-09-07)

A widget that consumes `mouse_down` may ask to own the pointer until
the release: `ctx.request_capture(self_ref, point)`, where `point` is
the event point *as the widget received it* (its parent-relative
space). `UI` stores the widget and `viewport − local`, and from then
until `mouse_up` every `mouse_down` / `mouse_move` / `mouse_up` goes
straight to that widget — shifted by the stored offset, with
`ctx.captured_widget` set — and nowhere else: no hover walk, no
hit-test, no focus clearing. The release is delivered and then ends
the capture; a widget may end it early with `ctx.release_capture()`;
a host whose window lost the pointer calls `UI::release_capture()`.
`call_event` reports every captured event consumed, because the
gesture is the chrome's. A captured widget that reconciles out of
the tree drops the capture.

The offset is taken once, at the press, rather than re-derived by
walking ancestors each move. That is exact for as long as nothing
between the widget and the viewport scrolls or re-lays out, which is
the length of a drag; it is also why capture needed no
measured-position query. `Slider` and `TextInput` (drag-select) are
the two takers. Requests on any event but `mouse_down` are dropped.

Capture is the UI's; the host's drag state machine
(`dispatch_drag_*`) is separate and unchanged — a slider carries no
`drag_payload`, so the host never starts a drag over one.

#### Non-pointer events (no point)

`UI::call_event` for these uses `EventContext::with_focused(...)`
so widgets can self-check focus, and dispatches to the root.

`FlexWidget::handle_event` for non-pointer events:
- Walks children **in reverse order** (`children.iter().rev()`),
  calling `handle_event` on each.
- Doesn't break on the first `true` — *all* children get a chance
  to handle the event.
- If no child handled, fires the widget's own listener if one
  exists.

#### Key chords — `keydown:` on `Flex` (2026-09-07)

```ogh
Flex {
  keydown: {
    ctrl_k:     fn () { event("open_palette"); },
    escape:     fn () { event("close"); },
    arrowdown:  fn () { event("next"); },
    shift_f2:   fn () { event("rename"); },
  },
  children: [ … ],
}
```

A map of chord → handler. Map keys are identifiers, so the joiner is
`_`; `+` is accepted where a string can be written. Modifiers are
`ctrl`, `alt`, `shift`, `meta` (aliases `control`, `cmd`, `super`)
in any order; the key is one of `escape`, `tab`, `enter`, `space`,
`backspace`, `delete`, `home`, `end`, `arrowup` / `arrowdown` /
`arrowleft` / `arrowright`, `a`–`z`, `digit0`–`digit9` (a bare digit
where a string can be written), `f1`–`f12`, with the aliases `esc`,
`return`, `del`, `up` / `down` / `left` / `right`. Chords are
normalised (`widget::keys::normalize`) so `shift_ctrl_K` and
`ctrl+shift+k` are one listener; a spelling that names no key is a
`BridgeError` at build time, not a listener that never fires.

**Dispatch, in `UI::call_event`, for a `keydown`:**

1. The host's intercepts have already run (below).
2. While a widget is focused, Tab moves focus and Escape blurs, at
   the UI level; neither reaches a listener.
3. A chord with `ctrl` / `alt` / `meta` is a **command**: the
   listeners are asked first, walking the **focus chain** — the
   focused widget and each ancestor up to the root, innermost
   first — and the first match consumes the key. With nothing
   focused the chain is the root alone. Only if no listener matches
   does the tree see it (so Ctrl+A still reaches a field's
   select-all).
4. Everything else goes to the **tree first**. What the tree declines
   is then offered to the listeners on the same chain — except a key
   a focused field would *type* (a bare letter, digit or space while
   `consumes_character_key()`), which is the field's whether or not
   it reported the `keydown` handled: the character arrives on the
   `keypress` that follows.

A key code no name covers (a modifier alone, an unmapped named key a
host sends as `0`) resolves to no chord and the listeners are never
consulted. The hosts map Backspace, Tab, Enter, Escape, Space, End,
Home, the arrows, Delete and characters (`lorekeeper/app/src/lib.rs`);
`f1`–`f12` are named here for the JS codes 112–123 but neither host
sends them yet.

**Ordering against the hosts.** Both hosts take some keys before any
tree is asked, and a listener for those never fires while the host
claims them:

- `lorekeeper/driver/src/binding.rs` `intercept_event` offers
  **Escape** to the route path first and only lets it through when
  the path leaves it unclaimed.
- untold_lore's `ul-editor/src/client.rs` `library_undo_shortcut`
  takes **Escape, Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y, Ctrl+S** on the
  library screen, ahead of `call_event` — deliberately, so a focused
  field cannot swallow undo.

Neither host was changed. A document listener for one of those
chords is reached only when the host declines the key.

#### Hover propagation

`UI::update_hover` runs once per `mouse_move`, setting
`hovered = true` on every widget on the path from a root to the
deepest hit, and `hovered = false` on every other widget. Fires
`mouse_enter` / `mouse_leave` listeners for transitions. The
point is shifted into child coordinate space using the same
origin-and-scroll rule as click dispatch. An exiting subtree is
hover-suppressed as a unit (the walk still recurses so cleared
descendants get their `mouse_leave`).

**Hover resolves through the portal layers the way a press does**
(2026-09-07; before this the walk covered the base tree only, so
hover slipped through a dropdown's dismiss backdrop and a modal's
block backdrop to the widgets under them, and anchored popover
content hovered at its declaration site instead of where it
paints). The rule, mirroring `UI::handle_click_event`:

1. Portal layers are walked high→low, within a layer in reverse
   mount order. An entry *claims* the point when a direct child of
   its content contains it (the point shifted by the entry's
   `viewport_rect`; ghosts invisible). The first claimant's content
   is hovered and **everything else** — later entries, lower layers,
   the base tree — is cleared, with `mouse_leave` where needed.
2. A layer nothing claims is settled by its **open** entries'
   effective backdrop policies: `block` or `dismiss` stops hover
   there, so every lower layer and the base tree reads as
   un-hovered; `none` falls through. (A closing portal's ghost entry
   neither claims nor obstructs.)
3. The base tree is walked last. It **stops at a Portal node that
   has a layer entry this frame** — that content was resolved in
   step 1 at its painted position — and clears the content of a
   Portal node without one (closed, or an anchored portal with no
   anchor).
4. The `cursor-attached` layer never claims: its content is pinned
   under the pointer and would hover itself on every move.
5. **Pointer capture skips the walk entirely.** The hover chain stays
   where the press left it until the release: the captured widget
   keeps its hover, nothing else lights up, and no `mouse_leave`
   fires mid-gesture.

`UI::hovered_blocks` answers `true` while step 2 stopped the walk,
and `UI::blocks_point` obstructs on the same gate (an open `block`
or `dismiss` entry), so a host gating world hover on either sees
the same thing a press would. `UI::hovered_cursor` reads the entry
contents ahead of the base tree. `tests/portal_hover.rs` pins all
of it.

### Tenets

- **Exiting widgets are hit-test-invisible.** A widget whose
  `is_exiting()` is true — and its whole subtree — receives no
  pointer events, no hover, no drag targeting, and does not
  occlude (`blocks_point` is false). Enforced in the pointer
  branch of `FlexWidget::handle_event`, in `blocks_point`, and
  in the `update_hover` / `deepest_at` / `deepest_drop_target`
  walks. Keyboard routing is untouched.

  *Why:* a half-faded button must not eat the click aimed at
  the live content beneath it. Load-bearing for Presence pop
  mode, where ghosts overlap the incoming generation
  (`PRESENCE_POP.md` §6); a bug fix for plain reconcile ghosts,
  which could consume clicks mid-fade.

  *Drift indicators:*
  - A new hit-test walk (or widget type's `handle_event`) that
    forgets the `is_exiting()` skip.
  - Ghosts intercepting hover/clicks again "because they're
    still visible".

- **First-hit-in-declaration-order wins for pointer events.**
  The walker iterates `self.children.iter()` and breaks on the
  first child that returns `true`. *Painter order, not
  reverse-painter*. This is the choice the code makes; whether
  it's the right choice is an open question — see below.

  *Why (as written):* unclear. The code matches what users
  declared; layered widgets stacked later (typically rendered
  on top by the painter) end up second-class for hit-testing.

  *Drift indicators:*
  - A child-walk change in `handle_event` that flips to
    `iter().rev()` for pointer events without flipping
    rendering as well.

- **`listener_fired` is a propagation gate, not a "stop"
  flag.** When a descendant fires a real listener, every
  ancestor sees `ctx.listener_fired = true` and skips its own
  listener. But ancestors *still* return `true` when
  `block_interactions` is set, so the event-consumed signal
  still flows correctly.

  *Why:* before `listener_fired` existed, an opaque container
  (Flex with `block_interactions = true`) returning `true`
  looked indistinguishable from a real handler firing. So
  outer ancestors couldn't decide whether they should fire too.
  `listener_fired` is the discriminator: it goes up only when
  a real handler ran.

  *Drift indicators:*
  - Reading `listener_fired` to *prevent* the event from
    propagating upward (it doesn't gate propagation; it gates
    redundant listener calls).
  - A new "blocking" mechanism that mimics
    `block_interactions` but doesn't update `listener_fired`.
  - Resetting `listener_fired` mid-walk inside a single
    top-level event.

- **`block_interactions` defaults to `true` on FlexWidget.**
  An ordinary `Flex { ... }` blocks interactions on the
  background it covers, even if no listener is attached. Set
  `block_interactions: false` to make a layout-only Flex
  transparent to clicks.

  *Why:* the common case for a styled container (a card, a
  panel) is that clicks landing on its background should not
  fall through to whatever is behind it. Default-to-block matches
  expectations for opaque visuals.

  *Drift indicators:*
  - Default flipped to `false`, which would break the
    overwhelmingly common case.
  - A widget type that ignores `block_interactions` while
    claiming to be a "container".

- **`PresenceWidget` and other wrappers delegate event handling
  to their inner Flex** unless they have a reason to intercept.
  Nothing in `Presence::handle_event` adds Presence-specific
  routing — it's a forward to the inner Flex. Generation
  sequencing is invisible to the event system.

  *Drift indicators:*
  - A wrapper widget that re-implements hit-testing when its
    inner could handle it.

- **Hit-testing ignores `transform`.** `contains_point` works
  off the un-transformed layout rect; `transform` is paint-only.
  A widget rotated 45° still hit-tests as its axis-aligned
  bounding box.

  *Why:* documented in `RenderEffects` and by the `transform`
  property in `style.rs`. The implementation cost of
  transform-aware hit-testing is high (inverse-transform every
  point, walk affine ancestors); the value is unclear given
  how rare transforms are in production UIs.

  *Drift indicators:*
  - A `transform`-aware hit-test path that doesn't mirror the
    paint-time transform stack precisely.
  - Documentation that promises transform-aware hit-testing
    without an actual implementation.

- **Hit-testing IS aware of scroll.** A scrollable container
  shifts the event point by `+scroll_offset` before recursing,
  exactly the inverse of what the renderer does
  (`-scroll_offset` translation). The two stay symmetric or
  click hot-zones drift from the visible widget.

  *Drift indicators:*
  - A new render-side translation that doesn't have a
    matching hit-test shift.
  - A scroll-mode flag that updates one side but not the other.

- **Pointer events use forward iteration; non-pointer events
  use reverse iteration.** The two paths are deliberately
  different. Pointer dispatch wants the *first* matching child
  to consume; keyboard / focus dispatch wants *every* child to
  see it (so that the focused descendant, wherever it lives,
  gets the keystroke).

  *Why for non-pointer:* there's no spatial constraint to filter
  by, so the only way to find the focused widget is to ask
  every child. Reverse order is a cosmetic choice — child-most
  first matches the painter order so newer overlays get
  keystrokes first.

  *Drift indicators:*
  - Unifying the two paths into one iteration order.
  - Adding `break` to the non-pointer walk on first
    `event_handled = true` — would suppress keystrokes for
    sibling focused widgets, which doesn't happen today but
    is a real edge case.

### Built-in event names

| Name           | Trigger                                      | Carries                       |
|----------------|----------------------------------------------|-------------------------------|
| `mouse_down`   | Mouse button pressed inside widget           | `point`                       |
| `mouse_up`     | Mouse button released inside widget          | `point`                       |
| `mouse_enter`  | Hover entered widget (fired by hover walker) | `point` not set               |
| `mouse_leave`  | Hover left widget (fired by hover walker)    | `point` not set               |
| `scroll`       | Wheel inside scrollable                      | `point`, `scroll_delta`       |
| `keydown`      | Key pressed (focused widget)                 | `keyboard_data`               |
| `keypress`     | Character typed (focused widget)             | `keyboard_data`               |
| `keyup`        | Key released (focused widget)                | `keyboard_data`               |
| `on_change`    | TextInput value changed; Slider value changed while held (`payload`) | `value` / `payload` |
| `on_submit`    | TextInput: Enter                             | `value`                       |
| `on_blur`      | TextInput lost focus, by any route           | `value`                       |
| `on_commit`    | Slider: the release, or a key step           | `payload` (`Value::Float`)    |
| `dismiss`      | Portal: a press outside it under the `dismiss` policy | `point`              |
| `keydown` (chord map on Flex) | A key the focus chain's listeners match, see *Key chords* | `keyboard_data` |
| `drag_start`   | Cursor crossed dead-zone past `mouse_down` on a widget with `drag_payload:` | `point`, `payload` |
| `drag_move`    | `mouse_move` while a drag is in flight (deepest widget at cursor) | `point`, `payload` |
| `drag_end`     | `mouse_up` while a drag is in flight (resolves to the deepest accepting drop target, or originator) | `point`, `payload` |
| `contextmenu`  | Right-click on the deepest widget at cursor (no auto-bubble) | `point`           |

The widget's listener map is built from descriptor properties
of matching name (e.g. `mouse_down: fn () { ... }`). Listeners
are `Box<dyn Fn(&Event)>`; the standard one-arg version
(`make_event_listener`) ignores the event payload, while
`make_event_listener_with_arg` (used for `on_change`) extracts
`event.value` and passes it as the closure's first argument.

### Drag dispatch (Phase 3)

Drag isn't dispatched through `UI::call_event` — it has its own
seam through `UI::dispatch_drag_*`, called by `Ogham::dispatch_drag_*`
on the host side. The split exists because:

- The dead-zone state machine (deciding when a `mouse_down` plus
  cursor delta becomes a drag rather than a click) lives in the
  host's input pump; the runtime doesn't know how the host
  reasons about the boundary.
- Drag end has to walk *portal layers* high-priority-to-low
  before the base tree to find the right drop target — a
  routing path `call_event` doesn't do.
- Drag preview rendering goes through the
  `cursor-attached` portal layer; the dispatch path needs to
  know about it to set up `UI::active_drag_preview`.

Source-side fields on a draggable `FlexWidget`:

- `drag_payload: Option<Value>` — the value carried through the
  gesture. Travels on `Event.payload`.
- `drag_dead_zone: f32` — px the cursor must cross before
  `drag_start` fires. Default 4.
- `drag_preview: Option<WidgetRef>` — optional widget rendered
  attached to the cursor while the drag is in flight.

Drop-target side:

- `accepts_drop_predicate: Option<...Closure>` — invoked with
  the current `payload` during drop-target hit-test. Returns
  truthy to opt in.

Listeners on the source: `drag_start`, `drag_move`, `drag_end`.
Listeners on the drop target: `drag_end` (fired only after
`accepts_drop` returns truthy). Both share `Event.payload`.

`UI::dispatch_drag_end` walks open portal layers
high-priority-to-low, then the base tree, picking the deepest
widget whose `accepts_drop(payload)` returns truthy. If none
accept, `drag_end` fires on the originator (cancel-style
behaviour).

### `contextmenu`

`Ogham::dispatch_contextmenu(point)` fires on the deepest widget
at the cursor and stops there — *no automatic bubble*. Wrap if
ancestor handling is needed. Use this instead of overloading
`mouse_down` so left-click and right-click route independently.

A host that hands the same `contextmenu`-named event to
`UI::call_event` instead (lorekeeper's `app` does) gets the
ordinary click walk: portal layers first, then the base tree
child-to-parent, the deepest widget with a `contextmenu:` listener
taking it. Two things are true of a right-click on either path
(2026-09-10, for the context menu):

- **The press is recorded** under `PRESS_ANCHOR` before anything
  answers it — every `mouse_down` too — so a `Portal { anchor:
  "press" }` the answer opens seats itself where the hand was
  (`ANCHORED_PORTALS.md` §8).
- **Outside a dismissing popover it dismisses and falls through.**
  A left press outside is swallowed and reported; a right-click
  outside is reported and then reaches what is under it, so a
  right-click on another row while a menu is up closes the menu
  and opens the row's in one gesture. The document sees `dismiss`
  then `contextmenu`, in that order (`LIFECYCLE_AND_PORTAL.md`,
  *The dismiss policy*).

### Tenets — names

- **Event name keys are strings, not enums.** Listener registry
  is `HashMap<String, Vec<Box<dyn Fn(&Event)>>>`. Adding a new
  event name requires no enum changes — host code can dispatch
  arbitrary names if it wants to (though there's no current
  authoring shape that benefits).

  *Why:* keeps the language layer flexible. Hosts can fire
  custom events through to listeners (e.g. game events that
  appear as widget callbacks).

  *Drift indicators:*
  - A typed `EventName` enum that requires recompilation to
    add new events.
  - A `match` on event names somewhere that doesn't have a
    catch-all arm (would silently drop unknown events).

---

## Layer 2 — Host events (outbound)

This is the channel `.ogh` code uses to ask the host to do
things the runtime can't do itself. See
[INTENT §2](INTENT.md#2-host-state-flows-in-events-flow-out)
for the asymmetry that makes this load-bearing.

### Two shapes

**Fire-and-forget — `event(name, ...args)`:**

```ogh
Flex {
  on_click: fn () {
    event("save_clicked", document_id);
  },
  ...
}
```

Compiler lowers to `EmitEvent(arg_count)`. VM pops args (first is
the name), calls `runtime.emit_event(&name, &rest)`, ignores the
result, pushes `Void`. Errors from the handler are silently
discarded.

**Tracked — `mutation("name")`:**

```ogh
state save = mutation("save_document");

Flex {
  on_click: fn () {
    save.trigger(document_id);
  },
  children: [
    if (save.pending) { Text { text: "Saving..." } }
    else if (save.status == "error") { Text { text: save.error } }
    else { Text { text: "Save" } }
  ]
}
```

`mutation("name")` produces a `Value::Mutation` —
an `Rc<RefCell<MutationState { event_name, status, data, error
}>>`. The mutation is paired with `state` so it persists across
rerenders.

`save.trigger` returns a `Value::BoundTrigger` over the same
`Rc`. Calling it short-circuits in `OpCode::Call`:
1. Pop args.
2. Mark mutation `Pending`.
3. Call `runtime.emit_event(&name, &args)`.
4. Write the result back: `Ok(value)` → `status = Success, data
   = value, error = ""`. `Err(message)` → `status = Error, data
   = Void, error = message`.
5. `request_rerender()`.
6. Push `Void`.

Properties available on a mutation: `status` (string),
`pending` (bool), `data` (the last `Ok` value), `error` (string),
`trigger` (a callable bound trigger).

### Tenets — host events

- **`event()` discards results; mutations carry them.** This
  is the asymmetry from `INTENT §2`. Authors who care about
  success/failure use `mutation(...)`. Authors who don't, use
  `event(...)`.

  *Why:* a synchronous `event() -> result` would force the
  handler to either compute synchronously (forcing a host-side
  blocking architecture) or block the VM's main thread on async
  work. The mutation handle is the explicit choice: trigger
  returns immediately, the result lands on the next render.

  *Drift indicators:*
  - `event()` returning a non-`Void` value.
  - A mutation handler that produces results outside of
    `Ok`/`Err` of the `Result`.

- **Mutations are `Rc`-shared, identity-compared.** Two
  separate `mutation("foo")` calls produce *distinct*
  `MutationState`s even though they share an event name. The
  same mutation observed through two different state reads is
  the *same* `Rc`, so updates are visible to both.

  *Why:* allows multiple components to share a mutation handle
  via state, while not conflating two unrelated tracked flows
  that happen to dispatch the same event name.

  *Drift indicators:*
  - `Value::Mutation`'s `PartialEq` becoming structural rather
    than `Rc::ptr_eq`.
  - A code path that clones a `MutationState` (deep copy) when
    reading from state.

- **`m.trigger` is a single-use intermediate, not a storable
  value.** `Value::BoundTrigger` exists for the duration of one
  call and shouldn't be assigned to state. Storing it works
  today but isn't an interface guarantee.

  *Drift indicators:*
  - User-facing documentation that describes BoundTrigger as
    a first-class value.

- **A trigger always requests a rerender, even when the handler
  did nothing.** `OpCode::Call`'s `BoundTrigger` arm calls
  `request_rerender` unconditionally after the handler runs.
  Authors relying on this for "always re-render after a
  trigger" are well-served; authors who want to avoid the
  rerender cost have no opt-out.

  *Drift indicators:*
  - Dropping the unconditional rerender request would speed up
    `Result<Value::Void, _>` handlers but would also break
    UIs that depend on mutation state being visible after the
    next render.

### Handler shape

```rust
RuntimeConfig::new()
    .with_event_handler("save_document", |args: &[Value]| {
        let id = match args.first() {
            Some(Value::String(s)) => s.clone(),
            _ => return Err("save_document requires a string id".to_string()),
        };
        match save_to_disk(&id) {
            Ok(()) => Ok(Value::Void),
            Err(e) => Err(e.to_string()),
        }
    });
```

Handlers are `Fn(&[Value]) -> Result<Value, String> + Send +
Sync`. The runtime stores them as `Arc<dyn ...>` so they can be
cloned cheaply across threads (the `Send + Sync` bound is what
lets the runtime be wrapped in `Arc<Mutex<...>>` and shared).

The `Result` is what differentiates `event()` from
`mutation()`. Both call the *same* registered handler; only
the call site decides whether to track the result.

---

## Open questions (for the design-review phase)

- **Hit-test order is painter-order (declaration order), not
  reverse-painter.** This is the inverse of every other UI
  framework I've seen. A button rendered "on top" of a
  background in CSS catches clicks first; in Ogham, the
  background catches first because it's declared first. This
  feels like a bug worth confirming. Verify by writing an
  example with two overlapping siblings.
- **`block_interactions: true` is the default** for
  `FlexWidget`. The docs (and the descriptor parser) treat it
  as opt-out. Reasonable for opaque containers; surprising for
  invisible layout helpers. Worth documenting in the user guide
  with examples.
- **`listener_fired` only gates redundant listener invocation,
  not propagation.** An ancestor *could* propagate the event
  upward even after a descendant fired — but pointer events
  break on first child, so they don't actually walk further.
  Non-pointer events do walk further (reverse order, no break),
  but they don't read `listener_fired`. The flag's semantic is
  effectively "did a descendant in *this* pointer dispatch
  fire a listener?". Document precisely.
- **Hit-testing ignores transforms.** Documented as a property
  of `transform` (paint-only). Authors who animate scale or
  rotation should be aware. The fix involves walking the
  effects stack at hit-test time.
- **There's no "captured" phase** in the DOM sense. Pointer
  events go straight from `UI` to root; ancestors run their
  handlers *after* descendants. (Pointer *capture* — a widget
  owning the pointer until release — exists since 2026-09-07 and
  is a different thing; see above.)
- **Custom events are name-keyed strings.** No type checking on
  the args. `mouse_down` and `key_down` and `please_save_now`
  go through the same dispatch; a typo is silently a no-op.
- **`event()` discards handler errors.** A misbehaving handler
  (a host that throws on every call) is invisible to `.ogh`
  unless wrapped in a mutation. Could be at minimum an
  `eprintln!` for debug builds.
- **Mutations always trigger a rerender.** Authors with handlers
  that return `Ok(Void)` and produce no observable effect get
  a wasted rerender. Cheap to gate this on whether
  `mutation.data` actually changed.
- **The text input fires `on_change` via a synthetic event
  dispatch through itself.** `handle_event` recurses into
  `self.handle_event(Event::with_value("on_change", ...), ...)`
  using a fresh `EventContext`. This is a clever way to reuse
  the listener-firing machinery, but the recursion shape is
  surprising. Audit whether a direct `fire_listeners` call
  would be cleaner.
- **There's no `prevent_default` or stop-propagation.** The
  current model is "return `true` to consume". That's enough
  for first-child-wins pointer dispatch but doesn't let a child
  selectively suppress its parent's hover-dependent visuals.
