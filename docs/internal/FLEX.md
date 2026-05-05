# Ogham — Flex Layout

> **Status: Live contract.**
>
> The flexbox-like layout model `FlexWidget` implements: `Size`
> variants, axis arithmetic, alignment, wrap, gap/padding/margin/
> border insets, scrolling, and hit-testing. Style transitions
> live in [`STYLE_AND_ANIMATION.md`](STYLE_AND_ANIMATION.md);
> entry/exit animations live in [`ANIMATION_LIFECYCLE.md`](ANIMATION_LIFECYCLE.md);
> reconciliation is in [`WIDGET_TREE.md`](WIDGET_TREE.md).
>
> `FlexWidget` is the workhorse of the widget tree; this is the
> longest internal doc for a reason. See
> [INTENT §4](INTENT.md#4-flex-owns-the-heavy-machinery-other-containers-wrap-it).

---

## At a glance

```
FlexStyle                       Layout pass:
  position                        get_dimensions  → (width, height)
  width / height (Size)           layout(...)     → walk children
  direction                                         place each at (x, y)
  main_alignment                                    set its own layout rect
  cross_alignment
  flex_wrap                     Hit testing:
  gap                             contains_point  → axis-aligned bounds check
  padding / margin / border       handle_event    → walk children in declaration order
  background / border / radius                       (with point shifted into local space)
  text_size / text_color
  overflow                      Render:
  opacity                         render          → background, border, scroll clip
  transform                       (children rendered by Surface walker)
  transitions
```

**Authority:**
- [`src/widget/flex_widget.rs`](../../src/widget/flex_widget.rs)
  for the widget impl.
- [`src/widget/style.rs`](../../src/widget/style.rs) for the
  style types (`FlexStyle`, `Size`, `Alignment`, `Direction`,
  `Position`, `Overflow`, `Padding`, `Margin`, `Border`,
  `CornerRadii`, `Color`).

---

## `Size`

```rust
enum Size {
    Grow(f32),    // basis weight; share grow-pool proportionally
    Shrink,       // size to children's natural extent (default)
    Fixed(f32),   // exact number of logical pixels
    Percent(f32), // unimplemented; resolves to 0
}
```

- `Fixed(v)`: deterministic. The widget always reports `v` for
  this axis.
- `Shrink`: size to content. The widget asks each child for its
  dimensions and takes the natural extent (sum on the main axis,
  max on the cross axis), plus its own insets.
- `Grow(basis)`: share the pool. The parent allocates pool space
  proportionally to each Grow child's basis: `basis_i /
  Σ(basis_j) * available`. A Grow child whose siblings are all
  Shrink/Fixed gets the entire remaining pool.
- `Percent`: stubbed. Currently resolves to 0 inside
  `get_dimensions` and is a no-op. Real implementation will need
  to handle parent-relative resolution timing.

### Size determination

`get_dimensions(ctx, parent_direction, parent_width,
parent_available_width, parent_height, parent_available_height,
sibling_basis)` is the per-widget measure call. Three layers of
arithmetic:

1. **Width and height resolve independently** by matching on
   `Size` for each axis.
2. **Shrink** triggers a recursive measure of children with
   careful axis bookkeeping:
   - Children measured along the main axis see
     `parent_available_*` reduced by the parent's
     `get_children_fixed_*` siblings on the same axis.
   - The Shrink result is *clamped* to the parent's available
     space — important so a Shrink container with overflowing
     content doesn't blow past the parent's edge.
3. **Grow** divides the available pool by sibling basis sum (or
   takes everything if alone).

Width is resolved first; height resolution can use the resolved
width as `self_content_width` (subtracting the widget's own
horizontal inset). This is load-bearing for wrap-aware height —
without it, a 280-wide sidebar would hand its children the full
window width during height measurement, and they'd all "fit" on
one line.

### Tenets

- **`Shrink` clamps to parent constraints; `Grow` doesn't.** A
  Shrink container whose children measure 4000 px tall sitting
  in a 800 px parent measures *800 px*, not 4000. A Grow
  container always reports its allocated pool slice.

  *Why:* without the clamp, Shrink children push past parent
  edges, and `total_main_size` calculations for parent
  alignment become wildly wrong. The clamp matches CSS's
  `overflow: visible` behavior — content overflows
  *visually* (handled by `overflow: hidden`/`scroll`) but
  reported size is the parent's allocation.

  *Drift indicators:*
  - Shrink results that exceed `parent_available_*` /
    `parent_*` reaching the layout pass.
  - A "uncapped Shrink" mode added without a corresponding
    overflow-handling mechanism.

- **Pre-pass: measure Shrink siblings before allocating Grow
  pool.** `layout()` walks the children once just to sum
  Shrink-on-main-axis sizes (`shrink_main_total`), then
  subtracts that from the available main-axis pool before
  handing it to Grow children.

  *Why:* without this, a Grow child takes the full
  `available_main` and pushes Shrink siblings past the parent's
  edge. The bug looked like "row with mixed Shrink + Grow:
  Shrink children ended up out of bounds". Pre-pass measurement
  fixes it cleanly.

  *Drift indicators:*
  - A layout change that skips the pre-pass for performance —
    breaks mixed-Shrink-and-Grow rows.

- **`Percent` is a known TODO; today it's 0.** Authors using
  `Percent(0.5)` will see a 0-sized widget. The parser accepts
  `width: "50%"`-style values? Actually no — `parse_size_value`
  in `builder.rs` accepts `"grow"`, `"shrink"`, integers (→
  Fixed), but not percentage strings. So `Percent` is unreachable
  from authoring today. Keep the variant in the enum; resolve
  the timing question (parent-relative or content-relative? at
  what point in the layout pass?) before exposing.

---

## `Direction`

```rust
enum Direction {
    Row,           // main axis = horizontal, children laid left-to-right
    Column,        // main axis = vertical, children laid top-to-bottom
    RowReverse,    // main axis = horizontal, children right-to-left
    ColumnReverse, // main axis = vertical, children bottom-to-top
}
```

`is_row()`, `is_reverse()`, `main_axis()`, `cross_axis()`,
`update_main_axis_position(x, y, delta)`, `get_grow_size`,
`get_shrink_size`, `get_shrink_max_size` are the helpers used
across layout. Reverse directions flip the cursor's starting
position — content is placed from the *end* of the main axis
inward.

### Tenets

- **The main axis is determined by direction, not by widget
  type.** A `Row` Flex's main axis is horizontal; a `Column`
  Flex's is vertical. Children's `Size::Grow` resolves along
  the *parent*'s main axis only. A Grow-width child inside a
  Column parent doesn't grow — width on a column-direction
  parent is cross-axis, which is fed parent_width directly.

  *Why:* this is the standard flexbox rule. Inverting it would
  let a child override its parent's flow direction, which is
  not a coherent design.

  *Drift indicators:*
  - A Grow child whose own direction influences whether it
    grows along its parent's main axis.

---

## `Alignment`

```rust
enum Alignment {
    Start,        // pack to the main-axis start (default)
    Center,       // center the group
    End,          // pack to the main-axis end
    SpaceBetween, // distribute, no padding at edges
    SpaceAround,  // distribute, half-space at edges
}
```

Two helpers:
- `get_offset(item_size, available_space)` — used for `Start`,
  `Center`, `End` to position the *group*.
- `get_spacing(total_items, available_space, total_item_size)` —
  used for `SpaceBetween` / `SpaceAround` to compute inter-item
  gap.

`main_alignment` aligns children along the flex direction;
`cross_alignment` aligns them perpendicular.

### Tenets

- **`SpaceBetween` / `SpaceAround` ignore explicit `gap`.** When
  `main_alignment` is `SpaceBetween` or `SpaceAround`, the
  computed inter-item spacing replaces the declared gap.

  *Why:* otherwise the gap would compound with the spacing,
  producing visually wrong layouts.

  *Drift indicators:*
  - Gap being added on top of `SpaceBetween` spacing.
  - A new alignment mode that tries to combine the two; the
    coherent way to do that is to make gap a minimum (CSS
    `gap` + `justify-content: space-between` actually does
    work this way, so worth interrogating in design-review).

---

## `Position`

```rust
enum Position {
    Static,
    Relative(f32, f32),
    Absolute(f32, f32),
}
```

- **`Static`**: in-flow; participates in flex layout.
- **`Relative(x, y)`**: in-flow but rendered with a paint-time
  offset. Currently *not implemented* in the rendering or hit
  path — `Relative` is recognized by the parser but doesn't
  affect layout. (Open question.)
- **`Absolute(x, y)`**: removed from flex flow. Laid out
  separately at `(parent.inset_left + x, parent.inset_top + y)`,
  consuming no space. `is_absolute_positioned()` returns `true`,
  which makes the layout walker skip it in basis / dimension
  calculations.

### Tenets

- **Absolute children have no main-axis pool share.** They are
  positioned after normal-flow children are laid out, at offsets
  relative to the parent's content origin. They consume zero
  main-axis space.

  *Why:* matches CSS `position: absolute` from the perspective
  of the surrounding flow. Authors use it for overlays /
  tooltips / badges.

  *Drift indicators:*
  - Absolute children participating in `total_main_size` or
    `get_children_basis` (would cause sibling shrink to think
    space was taken when it isn't).
  - Absolute children whose offsets are *parent-content-area*
    relative changing to *layout-rect* relative without
    documenting the change.

---

## `Padding`, `Margin`, `Border` (insets)

All three are four-sided `Spacing { top, right, bottom, left }`.
- **Padding**: inside the border, before content.
- **Margin**: outside the border, before parent's content area.
- **Border**: between padding and margin; rendered as a stroke.

`FlexStyle::inset_*()` helpers:
- `inset_left()`, `inset_top()`, `inset_right()`, `inset_bottom()`
  sum padding + margin + border on each side.
- `horizontal_inset()`, `vertical_inset()` sum across.

### Tenets

- **Insets reduce the *content area*, not the layout rect.** The
  widget's `layout` rect is `width × height` total (including
  insets). Children are positioned starting at `(inset_left,
  inset_top)` and given `content_width = width -
  horizontal_inset()`.

  *Drift indicators:*
  - A child whose layout rect's `(x, y)` is in the widget's
    margin area — children should be inside `(inset_left,
    inset_top)`.
  - `get_dimensions` returning a content area instead of the
    full layout extent.

- **`background_color` / `background_image` paint inside the
  margin, behind the border.** Authors who want a "card" with
  a gap to its sibling use margin (paints empty); for a
  surrounded breathing-room around content, padding (paints
  background color).

  *Drift indicators:*
  - Background paint extending outside the layout rect's
    content + border area.

---

## Flex wrap

```rust
flex_wrap: bool
```

- Only meaningful for row-direction. Column wrap is unsupported
  and ignored.
- Authoring: `flex_wrap: "wrap"` or `flex_wrap: true` enables.

When wrap is enabled and direction is `Row`:
- Each child sits on the current line until adding it would
  exceed the container's main-axis content width.
- A new line starts at `(inset_left, prev_cursor_y +
  line_max_height + gap)`.
- No flex-grow distribution happens *within* a wrapped line.
  Children keep their natural dimensions.
- `gap` separates children on the same line and lines from each
  other.

### Tenets

- **Wrap is row-only.** Column wrap is intentionally unsupported.

  *Why:* not needed by any current consumer. The arithmetic for
  cross-axis wrap is similar but the use cases differ — a
  vertical "wrap" usually wants explicit columns, which is
  more naturally handled by `Grid`. See open questions.

  *Drift indicators:*
  - A wrap path added to column directions that doesn't also
    wire up cross-axis cursor tracking — would silently emit
    broken layouts.

- **Wrapped lines don't redistribute Grow.** A Grow child in a
  wrapped row takes its natural (Shrink-equivalent) size on its
  line. CSS's `flex-wrap: wrap; flex: 1 1 0` redistributes per
  line; Ogham doesn't.

  *Drift indicators:*
  - A wrap implementation that does try to redistribute Grow
    per line — would produce different results from the current
    "stable per-line natural sizing" and break consumers who
    rely on it.

---

## Overflow

```rust
enum Overflow {
    Visible,  // default; children draw outside
    Hidden,   // clip to content area
    Scroll,   // clip + accept wheel input
}
```

- `Visible`: no clipping.
- `Hidden`: render pushes a clip rect; children outside it
  aren't drawn but still take layout space and respond to
  events.
- `Scroll`: same clip as `Hidden`, plus the widget tracks
  `scroll_y` (eased toward `scroll_y_target` by smooth-scroll
  decay), receives scroll events, and shifts hit-tests +
  rendering by `-scroll_offset` for descendants.

### Tenets

- **Scroll containers clip on render but expose their full
  layout bounds for events.** A child sitting "below" the visible
  area is still hit-tested if the user can move their cursor
  there — they just have to scroll into view first. The hit
  point is shifted by `+scroll_y` before recursion, which
  matches the rendered position.

  *Drift indicators:*
  - A scroll container whose rendering offset doesn't have a
    corresponding hit-test shift (clicks would land on wrong
    children).

- **Smooth scrolling is a per-frame ease, not per-tick.**
  `tick_smooth_scroll(dt)` runs every frame the widget
  animates; the decay constant is `SCROLL_DECAY = 18.0` (~150
  ms settle for a single wheel notch). The eased path is what
  gives the "feels right" trackpad-style scrolling.

  *Drift indicators:*
  - Removing the smooth ease and snapping to `scroll_y_target`
    would feel like CSS-default scroll on a high-end desktop.
    Probably worse for the UI.
  - Smooth scroll mid-tick that doesn't request a repaint —
    the easing would freeze until the next event-driven
    rerender.

---

## Layout pass walkthrough

A `layout(ctx, cursor_x, cursor_y, parent_direction,
parent_width, parent_available_width, parent_height,
parent_available_height, sibling_basis)` call:

1. **Resolve own dimensions** via `get_dimensions(...)`.
2. **Stash own layout rect**: `self.layout = Some(Rect { x:
   cursor_x, y: cursor_y, width, height })`.
3. **Compute content area**: subtract horizontal_inset and
   vertical_inset.
4. **Pre-pass**: measure Shrink siblings on the main axis.
5. **Pool calculation**: subtract Shrink total from available
   main-axis space; Grow children divide that pool.
6. **Per-child measure**: compute `(w, h)` for every
   non-absolute child via `get_dimensions` once, with the right
   pool depending on whether the child is Grow or not.
7. **Branch**:
   - **Wrap path** (row + flex_wrap): per-line cursor with
     wrap-on-overflow. No alignment math beyond start-of-line.
   - **Normal path**: compute total_main_size, alignment
     spacing, initial offset, then walk children placing each.
8. **Position absolute children** at `(parent.inset_left +
   offset_x, parent.inset_top + offset_y)`, calling their
   layout with the parent's content area as `parent_*`.

### Tenets

- **`get_dimensions` is called once per child per layout pass.**
  Children's dimensions are cached in `child_dims: Vec<Option<(f32,
  f32)>>` and reused for total-size computation and the actual
  child layout call.

  *Why:* before this caching, complex trees called
  `get_dimensions` multiple times per child per pass, which
  multiplied the cost (especially through Shrink children that
  recurse). The cache saves the work.

  *Drift indicators:*
  - A new layout-time consumer of dimensions that calls
    `get_dimensions` directly instead of using `child_dims`.
  - Dimension recomputation between the pre-pass and the
    placement loop.

- **`layout()` writes the rect; `get_dimensions()` doesn't.**
  Calling `get_dimensions` is read-only on the widget. The
  cached `child_dims` are computed in `layout()` immediately
  before they're consumed; nothing else mutates a child's
  layout rect.

  *Drift indicators:*
  - `get_dimensions` writing to `self.layout` (would corrupt
    multi-pass measurement during height resolution).

- **`get_children_basis()` excludes absolute children.** All
  basis arithmetic that distributes pool space ignores
  `is_absolute_positioned() == true` siblings.

  *Drift indicators:*
  - A new sibling-iteration helper that doesn't filter
    absolute children.

---

## Hit testing (`contains_point` and `handle_event`)

`FlexWidget::contains_point(point)` is an axis-aligned bounds
check: subtract margin from the layout rect to get the content
area, then test inclusion. Pure local check; no parent context.

`handle_event` for pointer events, in order:
1. If `!contains_point(point)`: return `false`. The walker won't
   recurse.
2. If a scroll event and `Overflow::Scroll`: update
   `scroll_y_target`, return `true`.
3. Shift the event point into the widget's content space:
   `local_event = event.shift_point(-origin.x + scroll_x,
   -origin.y + scroll_y)`.
4. Walk children in declaration order, invoking
   `child.handle_event(&local_event, ctx, child_ref)`. First
   `true` consumes (`break`).
5. Decide whether to fire own listeners:
   `!ctx.listener_fired && self.event_listeners.contains_key(&event.name)`.
   If yes, fire and set `ctx.listener_fired = true`.
6. Return `self.block_interactions || child_consumed || my_fired`.

See [EVENTS.md](EVENTS.md) for the gating logic in detail.

### Tenets

- **`contains_point` excludes margin from the content area.**
  Margin is empty space around the widget; clicks on margin
  are not on the widget.

  *Drift indicators:*
  - Hit test that includes margin (would catch clicks on
    "negative space" around cards).

---

## Render

`render(ctx, focused, image_cache)` paints:
1. Background image (if set) into the layout rect.
2. Background color (if set) into the layout rect with corner
   radii.
3. Border (if any side has non-zero width).
4. Pushes a clip rect when `overflow != Visible`.

The Skia walker (`SkiaEnv::draw_widget_recursive`) handles the
recursion: it calls `render(...)`, translates the canvas by
`(layout.x - scroll_x, layout.y - scroll_y)`, recurses into
children, and pops the clip via `post_render` if
`needs_post_render` returned true.

`render_effects()` returns optional opacity + transform with
the widget's center as pivot. The walker pushes/pops these
*around* the widget and its descendants — paint-only, doesn't
affect layout.

---

## Tests

Inline `#[cfg(test)]` modules cover keyed reordering, presence
sequencing, and a few alignment edge cases. They're not
comprehensive layout acceptance tests — visual regressions are
caught manually for now.

---

## Open questions (for the design-review phase)

- **`Position::Relative` is parsed but not implemented.**
  Authors using `position: { type: "relative", x: 10, y: 0 }`
  see no effect today. Either implement it (paint-time offset
  with no flow effect) or reject it at the builder.
- **`Percent` resolves to 0.** Either implement it or remove
  the variant. Implementation would need to decide whether the
  base is parent's content area, parent's layout, available
  space, or available-after-fixed-siblings — each is different
  and has trade-offs.
- **Wrap is row-only.** Column wrap is doable but not
  prioritized; authors fall back to `Grid`. Worth deciding if
  parity with CSS flex-wrap is a goal.
- **`SpaceBetween` / `SpaceAround` discard `gap`.** CSS treats
  gap as a *minimum* with `justify-content`. Worth aligning.
- **`get_dimensions` doesn't memoize across calls.** Re-laying
  out the same widget calls `get_dimensions` again. Within a
  single layout pass it's cached; across passes (e.g.
  pre-measure for parent Shrink decisions plus the actual
  layout) it isn't. Profile-driven.
- **The cache `child_dims` is recomputed every layout, even
  when the children haven't changed.** A change-detection layer
  could memoize but would have to invalidate on style /
  child-content / animation changes — likely net negative
  unless the layout pass becomes a measured bottleneck.
- **No subpixel rounding policy.** Layout produces `f32`s; the
  Skia backend multiplies by DPI. Whether subpixel positions
  round or truncate isn't formalized.
- **`Direction::Reverse` arithmetic is added on top of forward
  arithmetic.** It works but the code path is harder to reason
  about than a clean "iterate children in reverse + place
  forward" implementation. Audit and possibly refactor.
- **Margin collapsing isn't implemented.** Two stacked
  Column children with vertical margins both contribute their
  margin (CSS would collapse adjacent margins). Authors get
  surprised. Not necessarily wrong, but worth documenting.
- **Hit-testing on the widget's `layout` rect, before children,
  means pointer events that land in *padding* are caught by the
  parent before any "child outside the parent due to negative
  margin" sees them.** Negative margins aren't supported anyway
  (Spacing is f32 with no sign-checking but layout treats it as
  positive); confirm.
- **`block_interactions` defaults to `true`** — see
  [EVENTS.md](EVENTS.md) discussion.
