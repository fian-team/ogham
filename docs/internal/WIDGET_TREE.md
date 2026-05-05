# Ogham — Widget Tree (Builder + Reconciliation)

> **Status: Live contract.**
>
> The seam between the VM's value output and the live widget tree.
> Covers the builder (descriptor → `WidgetRef`) and the
> reconciliation algorithm (live tree absorbs new descriptors in
> place). Layout-specific behavior is in [`FLEX.md`](FLEX.md);
> animation rules are in [`STYLE_AND_ANIMATION.md`](STYLE_AND_ANIMATION.md);
> exit lifecycle is in [`ANIMATION_LIFECYCLE.md`](ANIMATION_LIFECYCLE.md); event
> dispatch is in [`EVENTS.md`](EVENTS.md).

---

## At a glance

```
Value::Widget(WidgetDescriptor)
  → widget::builder::widget_value_to_widget_ref
      registry lookup → factory(registry, runtime, descriptor)
  → Arc<Mutex<dyn Widget>> (a WidgetRef)

Subsequent rerenders:
  new tree of WidgetRefs
  → UI::reconcile(new_root)
      Widget::update(new_root) recursively
  → existing tree absorbed in place
```

The first render builds a fresh tree. Every render after that
*reconciles*: the live tree's `Widget::update` is asked to
absorb the new descriptors, preserving identity (and therefore
animation, hover, scroll, focus) wherever shapes match.

**Authority:**
- Builder: [`src/widget/builder.rs`](../../src/widget/builder.rs).
- `Widget` trait + `UI::reconcile`:
  [`src/widget/mod.rs`](../../src/widget/mod.rs).
- Per-widget `update`: each widget's file under `src/widget/`.
- Canonical `reconcile_children`:
  [`src/widget/flex_widget.rs`](../../src/widget/flex_widget.rs).

---

## Builder

### What it does

`widget_value_to_widget_ref(registry, runtime, value)`:
1. Validates that `value` is a `Value::Widget`.
2. Lowercases the widget's identifier and looks up a factory in
   the `WidgetRegistry`.
3. Calls the factory with `(registry, runtime, descriptor)`.

The factory does the type-specific work: parsing the descriptor's
`HashMap<String, Value>` into typed fields, recursively building
children, wiring up event listeners.

### `WidgetRegistry`

A `HashMap<String, WidgetFactory>` keyed by lowercased type name.
`with_defaults()` populates: `flex`, `text`, `textinput`, `svg`,
`image`, `grid`, `presence`. `RuntimeConfig::with_widget` adds
custom widgets (and overrides built-ins on collision).

### Tenets

- **Lookup is case-insensitive; storage is lowercased.** The
  registry insert lowercases; the lookup lowercases. Authors can
  write `Flex { ... }` or `flex { ... }`; both resolve to the
  same factory.

  *Why:* the language uses capitalized widget identifiers as a
  convention but doesn't enforce them. The registry has to be
  case-permissive to let authors use whatever convention the
  host expects.

  *Drift indicators:*
  - A factory storage path that bypasses lowercasing (would
    cause a casing mismatch to silently fail to find the
    widget).
  - A tooling change that *requires* a particular casing.

- **Event listeners close over `Arc<Mutex<Runtime>>`.** Wiring
  `mouse_down: fn () { ... }` produces a `Box<dyn Fn(&Event)>`
  that locks the runtime, calls
  `runtime.call_bytecode_closure(&closure, &[])`, and logs any
  error to stderr. The closure is captured by clone, so the
  listener stays valid as long as the listener box itself is
  alive.

  *Why:* listeners run far from the call site that built them.
  The runtime needs to be reachable to invoke the closure;
  re-locking the mutex per dispatch is the simplest model that
  works (see "open questions" in [RUNTIME.md](RUNTIME.md) for
  the cost).

  *Drift indicators:*
  - A listener that captures the closure but not the runtime
    (no way to invoke).
  - A listener path that bypasses
    `call_bytecode_closure` (would skip the call-stack
    save/restore the VM relies on for state).

- **Children property accepts both `Value::Array(...)` and a
  single `Value::Widget(...)`.** Authors can write `children: [
  Foo { } ]` or `children: Foo { }`; both work. Non-widget
  array elements are silently skipped (e.g. mixing `Value::Void`
  in a `for` expression doesn't crash the build).

  *Why:* the silent skip is an authoring convenience — `for
  (...)` expressions that conditionally produce widgets vs.
  `Void` work without explicit filtering.

  *Drift indicators:*
  - A factory that errors on a mixed-type children array
    (would break authoring patterns).
  - A factory that *doesn't* accept the single-widget form
    (would force authors to wrap singletons in `[ ... ]`).

- **Property parsing is permissive.** Unknown style keys are
  silently dropped (`apply_flex_style_from_map`'s catch-all
  `_ => {}`). Wrong-shape values are silently dropped (e.g. a
  number where a color map is expected just leaves the field
  default).

  *Why:* style maps are written by humans and frequently
  partial; failing the build on a typo is hostile. The trade-off
  is that *real* typos go unnoticed. Reasonable for a UI
  language; worth interrogating in design-review.

  *Drift indicators:*
  - A factory that errors on an unknown property — would break
    forward-compatibility (authors couldn't add new properties
    that older runtimes ignore).

### Built-in factory roster (one-liners)

- **`flex` / `Flex`** — see `create_flex_widget`. Builds
  `FlexWidget`, parses style/hover_style/initial/exit/key, wires
  pointer event listeners (`mouse_down`, `mouse_up`,
  `mouse_enter`, `mouse_leave`), recursively builds children,
  applies entry transition.
- **`text` / `Text`** — see `create_text_widget`. Required `text`
  property; coerces ints/floats/booleans to strings. Default
  width/height = `Shrink`.
- **`textinput` / `TextInput`** — text editing field; emits
  `on_change(value: string)`.
- **`svg` / `Svg`** — Skia SVG DOM rendering.
- **`image` / `Image`** — Skia image rendering with caching.
- **`grid` / `Grid`** — grid layout with placement properties.
- **`presence` / `Presence`** — generation-keyed sequencer; see
  [ANIMATION_LIFECYCLE.md](ANIMATION_LIFECYCLE.md).

---

## Reconciliation

### Top-level entry: `UI::reconcile(new_root)`

If the new root is the same `Arc` as the current root,
`UpdateResult::UNCHANGED`. Otherwise, lock the current root and
call `Widget::update(new_root)`. The result bubbles up to
`Ogham::update`, which calls `mark_needs_layout` /
`mark_needs_repaint` based on the flags.

After reconcile, the focused-widget pointer is checked: if
nothing else holds an `Arc` to it (`Arc::strong_count == 1`), the
widget was removed from the tree and focus is cleared.

### Per-widget `Widget::update`

The contract: receive a `WidgetRef` to a freshly-built widget.
Return an `UpdateResult` indicating whether the receiver
absorbed the new widget (`absorbed: true`) and what dirty flags
should bubble up. If the receiver can't absorb (different type),
return `UpdateResult::REPLACE` and the parent
(`reconcile_children`) swaps the `WidgetRef`.

In practice:

```rust
fn update(&mut self, new_widget: WidgetRef) -> UpdateResult {
    let mut new_widget = new_widget.lock().expect("widget lock poisoned");
    if let Some(new_typed) = new_widget.downcast_mut::<Self>() {
        // copy props from new_typed onto self
        // call self.reconcile_children(&mut new_typed.children) (or equivalent)
        UpdateResult { absorbed: true, needs_layout: ..., needs_repaint: ... }
    } else {
        UpdateResult::REPLACE
    }
}
```

### `FlexWidget::reconcile_children` — the canonical algorithm

(Full source at `src/widget/flex_widget.rs:273`.)

1. **Pre-pass: cancel matching exits.** For every old child that
   is currently exiting *and* whose key appears in the new
   children, call `cancel_exit` on it. This is what makes
   re-mounting a key mid-exit recover gracefully.

2. **Build `key → old_index` map** of the now-stable old
   children. Only the first occurrence of each key is recorded.

3. **Match new children against old, in order:**
   - **Keyed new child:** look up by key in `old_by_key`. If the
     match isn't already consumed, take it.
   - **Unkeyed new child:** advance an unkeyed cursor through
     old children, skipping consumed slots, *keyed* old
     children, and *exiting* old children. The first remaining
     unkeyed-non-exiting old child is the match.
   - If a match exists:
     - If the `Arc` is identical (`Arc::ptr_eq`), reuse — this
       is the cheap path when the new tree happens to share
       widgets with the old.
     - Otherwise call `child.update(new_child)`. If
       `absorbed`, push the existing widget into `next`. If
       `!absorbed` (type mismatch), try `child.begin_exit()`:
       if it returns `true`, push *both* the old (as a ghost)
       and the new; otherwise drop the old and push the new.
   - If no match exists, push the new widget. `needs_layout` is
     bubbled up.

4. **Handle un-consumed old children.** Walk the old children
   in order; for any that wasn't consumed:
   - If already exiting, leave it (it stays a ghost).
   - Otherwise, attempt `begin_exit()`. If it returns `true`,
     splice the ghost back into `next` near its original index.
     If `false`, drop it (and bubble `needs_layout`).

5. **Replace `self.children` with `next`** and return the
   aggregated `UpdateResult`.

### Tenets

- **Reconciliation matches by `key`, falls back to position.**
  See [INTENT §5](INTENT.md#5-reconciliation-matches-by-key-falls-back-to-position).

- **Exiting ghosts are never matched by unkeyed position.** They
  keep their slot in `self.children` but the unkeyed cursor
  skips over them so a new (unkeyed) sibling doesn't accidentally
  adopt the ghost's identity.

  *Why:* if a ghost could be adopted by a new unkeyed sibling,
  the new sibling would inherit the ghost's mid-exit animation
  state, which is exactly the wrong starting point for an entry
  animation. Skipping ghosts keeps the entry/exit lifecycle
  clean.

  *Drift indicators:*
  - An unkeyed-cursor change that doesn't filter out
    `is_exiting()` widgets.

- **Type mismatch on a keyed match tries to ghost the old
  before replacing.** When `update` returns `!absorbed`, the
  matched old widget is asked if it can `begin_exit`. If yes,
  the new ghost stays in the tree until its springs settle and
  the new widget mounts alongside it. If no, the old is dropped
  immediately.

  *Why:* it's the only way to animate a "page swap" where the
  outgoing and incoming widgets are different types (e.g. a
  Flex panel replacing a Text placeholder).

  *Drift indicators:*
  - A type-mismatch path that always drops, even when
    `begin_exit` succeeds.
  - Ghosts being placed at the *end* of `next` instead of at
    their original position — sibling layout would shift each
    frame and animations would look chaotic.

- **`Arc::ptr_eq` shortcut is the cheapest possible
  reconciliation.** When the new tree happens to share
  `WidgetRef`s with the old (e.g. a memoized helper that
  returns the same `Arc`), the `update` call is skipped
  entirely.

  *Why:* opens a fast path for authors who memoize widget
  construction. Today's builder always allocates new
  `WidgetRef`s, so this path rarely fires for tree roots — but
  it fires for any inner widget that happens to be passed
  through unchanged from an outer scope.

- **`UpdateResult::needs_layout` is conservative.** A change
  bubbles `needs_layout = true` whenever a layout-affecting
  prop differs (`FlexStyle::layout_equal` is the deliberate
  filter — paint-only fields like `background_color`,
  `transform`, and `opacity` are excluded).

  *Why:* a host_state change that produces identical widget
  output should not trigger a relayout. The
  `paint_only ∈ layout_equal` set is the line; widening it
  reduces relayouts but risks missing a real layout-affecting
  change.

  *Drift indicators:*
  - A new layout-affecting field on `FlexStyle` that's not
    listed in `layout_equal`.
  - A non-layout-affecting field added *to* `layout_equal`
    (would force relayouts on hover state changes, etc.).

---

## `Widget` trait — what implementors must provide

(See `src/widget/mod.rs` for the full signatures.)

**Required methods:**
- `get_type(&self) -> &str` — debug/diagnostics.
- `get_dimensions(...) -> (f32, f32)` — measure.
- `get_children(&self) -> Vec<WidgetRef>` — for the rendering walker.
- `get_basis(direction)`, `get_children_basis()` — flex basis
  arithmetic.
- `get_fixed_width()`, `get_fixed_height()` — return Some when
  the size is statically known.
- `handle_event(event, ctx, self_ref) -> bool` — see
  [EVENTS.md](EVENTS.md).
- `layout(...)` — resolve self + descendants' rects.
- `update(new_widget) -> UpdateResult` — reconciliation entry.
- `contains_point(point) -> bool` — hit-test.

**Optional overrides (with sensible defaults):**
- `get_children_mut`, `is_absolute_positioned`,
  `get_absolute_offset` — layout exceptions.
- `set_hovered`, `is_hovered` — hover state.
- `fire_listeners(name, event)` — direct listener invocation
  (used by hover for `mouse_enter` / `mouse_leave`).
- `render(ctx, focused, image_cache)` — paint self.
- `get_layout_rect()` — return parent-relative rect.
- `scroll_offset()` — for scroll containers; affects both
  rendering translate and hit-test point shift.
- `key()` — identity for reconciliation.
- `render_effects()` — opacity + transform.
- `is_exiting`, `begin_exit`, `cancel_exit`, `is_exit_complete` —
  see [ANIMATION_LIFECYCLE.md](ANIMATION_LIFECYCLE.md).
- `tick_animations(dt)` — see
  [STYLE_AND_ANIMATION.md](STYLE_AND_ANIMATION.md).
- `needs_post_render`, `post_render` — used by scroll containers
  to pop their clip rect.

### Tenets

- **`Downcast` is required.** All widgets implement `Downcast`
  via `downcast_rs` so reconciliation can type-check matches.
  Without it, `Widget::update` couldn't tell whether the new
  widget is the same type as the receiver.

  *Drift indicators:*
  - A widget that implements only `Widget` and not `Downcast`
    via the `impl_downcast!` macro.

- **Children are `Vec<WidgetRef>`, not a generic.** All
  containers expose children the same way so reconciliation
  doesn't need to know the container type.

- **`Widget` trait methods are object-safe.** `WidgetRef =
  Arc<Mutex<dyn Widget>>`; the trait must remain dyn-compatible.
  No generic methods, no `Self`-returning methods.

  *Drift indicators:*
  - A trait method with a generic parameter.
  - Helpers added to the trait that should be free functions
    instead.

---

## Coordinates and the rendering walker

Every widget's `layout` rect is in *parent-relative* coordinates
— `layout.x`, `layout.y` are offsets from the parent's content
origin, not from the screen.

The Skia walker (`SkiaEnv::draw_widget_recursive`) keeps the
canvas at the parent's origin when calling `widget.render(...)`,
then translates by `(layout.x, layout.y) - scroll_offset` before
recursing into children. Hit-testing
(`UI::update_hover_recursive`, `FlexWidget::handle_event`) does
the inverse: shifts the event point by `-(origin) + scroll`
before recursing. This is why `Widget::contains_point` only ever
needs to know the widget's *own* layout rect.

### Tenets

- **Layout rects are parent-relative.** A widget's `layout.x` is
  meaningless without the parent's origin. Don't store
  screen-relative coordinates on widgets.

  *Why:* parent-relative layout makes `transform` / scroll /
  reorder cheap — moving a parent doesn't require relaying out
  the entire subtree.

  *Drift indicators:*
  - A widget that stores screen-relative coords for
    convenience and forgets to update them on every layout pass.

- **The renderer and the hit-tester walk the tree symmetrically.**
  Every translate the renderer does, the hit-tester does in
  reverse. If they drift, click hot-zones won't match the
  visible widget.

  *Drift indicators:*
  - A new render-time translation (e.g. a special-position
    mode) without a corresponding hit-test shift.

---

## Open questions (for the design-review phase)

- **`Widget::update` requires the receiver to know its own
  concrete type to absorb props.** A reflection-based or
  prop-map-based update could let third-party widgets be
  absorbed by built-in containers. Today, every widget type
  has its own bespoke `update`.
- **Reconciliation is per-frame, not incremental.** A render
  produces a complete fresh descriptor tree even if 99% of it
  is unchanged. React-shaped systems usually have memoization
  at the component level. Worth interrogating once profiling
  evidence exists.
- **Permissive property parsing trades safety for ergonomics.**
  A typo'd property silently drops. A debug mode that warns
  on unknown properties would help authors without breaking
  forward compatibility.
- **`Arc<Mutex<dyn Widget>>` is heavy.** Every widget access
  takes a mutex lock. Single-threaded UIs don't need mutexes —
  `Rc<RefCell<...>>` would be cheaper. The `Arc<Mutex<...>>`
  exists because the `Surface::draw` method takes
  `&mut UI`, but `tick_animations` and `reconcile` also need
  internal mutability. Audit whether `Send + Sync` of widgets
  is actually required by any consumer.
- **The unkeyed-position fallback is asymmetric**: keyed new
  children look up by key first, but keyed old children that
  *don't* appear in the new key set still go through the
  exiting ghost path. Authors who key only some siblings get
  surprising behavior when an unkeyed sibling is removed
  (matched against a keyed old child, which then goes through
  type-mismatch handling). Worth testing.
- **Listener wiring rebuilds boxed closures every render.** Each
  rerender allocates fresh `Box<dyn Fn(&Event)>` for every
  listener, even when the listener body hasn't changed. The
  reconcile path swaps them in (`std::mem::swap`), so identity
  isn't an issue, but it's a per-frame allocation. Audit if a
  bottleneck.
