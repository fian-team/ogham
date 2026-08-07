# Ogham — Surface (Rendering Backend)

> **Status: Live contract.**
>
> The `Surface` and `RenderContext` traits — the only seam where
> Ogham talks to a rendering backend. The Skia implementation
> (`SkiaEnv`) is the reference; custom backends are free to ship
> their own. This doc covers the contract, the responsibilities
> on each side, and the gotchas around DPI scale and text
> measurement.
>
> See [INTENT §6](INTENT.md#6-surface-is-the-only-rendering-seam).

---

## Authority

- Trait definitions: [`src/widget/mod.rs`](../../src/widget/mod.rs)
  (`Surface`, `RenderContext`, `RenderEffects`, `LayoutContext`).
- Reference implementation: [`src/skia.rs`](../../src/skia.rs).
- Per-widget rendering: each widget's `render(...)` method.

---

## At a glance

```
Host:   surface.draw(&mut ui)
   ↓
Surface::draw walks the tree in two passes:
   Pass A — base tree:
     for each widget:
       widget.render_effects()        → push opacity/transform layer
       widget.style.backdrop_filter   → push_backdrop_blur (if set)
       widget.render(ctx, ...)        → background, border, text, etc.
       translate canvas by (layout origin - scroll)
       recurse into widget.get_children()
       widget.post_render(...)        → pop scroll clip rect
       pop_backdrop_blur (if pushed)
       pop opacity/transform layer
       (Portal widgets register a PortalEntry into ui.portal_layers
        and do nothing else in Pass A.)
   Pass B — portal layers (Phase 2/2.5):
     for each open layer in priority order (overlay-modal → popover
                                            → tooltip → toast →
                                            cursor-attached):
       apply layer's BackdropPolicy (Block paints a translucent
                                     backdrop; None paints nothing)
       for each PortalEntry in this layer:
         translate canvas to entry.viewport_origin (viewport-absolute)
         recurse into entry.children using the same Pass-A logic
   ↓
Drag preview (if a drag is in flight):
   ui.active_drag_preview is rendered as a synthetic
   cursor-attached entry positioned at the cursor.
```

Two traits collaborate:
- **`Surface`** — implemented by the backend. One method:
  `draw(&mut self, ui: &mut UI)`. The backend is responsible
  for walking the tree.
- **`RenderContext`** — also implemented by the backend.
  Widgets call its methods (`fill_rect`, `draw_text`, …) from
  inside their `render(...)`. The backend sees the same
  primitive operations regardless of which widget emitted them.

---

## The traits

### `Surface`

```rust
pub trait Surface {
    fn draw(&mut self, ui: &mut UI);
}
```

Yes, that's the whole trait. The backend gets full control over
how the tree is walked: it can choose to push canvas state,
translate for child origins, integrate widget effects, paint
debug overlays, anything else.

### `RenderContext`

```rust
pub trait RenderContext {
    fn fill_rect(&mut self, x, y, w, h, color);
    fn fill_rounded_rect(&mut self, x, y, w, h, radii, color);
    fn draw_border(&mut self, border, x, y, w, h, radii);
    fn draw_image(&mut self, path, x, y, w, h, image_cache);
    fn draw_text(&mut self, text, style, x, y, width);
    fn draw_line(&mut self, x1, y1, x2, y2, width, color);
    fn draw_svg_dom(&mut self, dom, x, y, w, h);

    // Optional, with default no-op implementations:
    fn push_clip_rect(&mut self, x, y, w, h);
    fn pop_clip_rect(&mut self);
    fn push_effects(&mut self, opacity, transform, pivot_x, pivot_y);
    fn pop_effects(&mut self);
    fn push_backdrop_blur(&mut self, x, y, w, h, radii, sigma);
    fn pop_backdrop_blur(&mut self);
    fn with_local_canvas(&mut self, rect, paint) -> bool;
}
```

`push_backdrop_blur` / `pop_backdrop_blur` are the seam for
`backdrop_filter: { blur: N }` style on a `Flex`. Backends that
can sample the canvas (Skia) implement them with
`save_layer` + `image_filter::blur`; backends that can't leave
the trait's no-op default in place — the widget still renders,
just without the frosted-glass capture.

All coordinates are *logical* (pre-DPI). The backend is
responsible for any scaling. The Skia implementation multiplies
every argument by `dpi_scale` inside the trait method body.

### `with_local_canvas` — the `Canvas` painter hatch

```rust
fn with_local_canvas(
    &mut self,
    rect: &Rect,
    paint: &mut dyn FnMut(&mut canvas_widget::Painter),
) -> bool { false }
```

The one method that hands a **backend-native** handle across the
seam, and the only one whose argument is a widget rect rather than
a primitive. It exists for [`CanvasWidget`](WIDGET_TREE.md), whose
pixels are drawn by host Rust — arcs, wedges, gradients along a
path, blend modes — none of which the typed primitives above can
express. See [INTENT §6](INTENT.md#6-surface-is-the-only-rendering-seam),
which carves this out as a *named* exception with its own drift
indicators, and `docs/internal/CANVAS_LEAF.md` for the design.

**The coordinate contract.** The `Painter` handed to the host is a
canvas that has already been:

1. `save()`d,
2. translated to `rect`'s origin **in device pixels**
   (`rect.x * dpi`, `rect.y * dpi`) — the walker's canvas is
   device-scaled at this point, so the translate must be too,
3. scaled by `dpi_scale`,

and is `restore()`d when the painter returns. A painter therefore
draws from `(0, 0)` to `(painter.width(), painter.height())` in
**logical** pixels and never learns either its position in the tree
or the display's DPI. `painter.dpi_scale()` is exposed for the rare
painter that genuinely wants device pixels.

Order matters: scaling before translating would multiply the
translate as well and seat every canvas at `dpi ×` its real origin.

Backends that cannot expose a native canvas keep the `false`
default; the `Canvas` widget then paints nothing and logs once per
painter name in debug builds. A `Canvas` is opt-in host paint, so a
backend that can't service it is not an error.

#### Tenets — the hatch

- **Only `canvas_widget.rs` may call it.** Built-in widgets paint
  through the typed primitives; the hatch is for *host* paint.

  *Why:* the seam's live purpose is keeping `skia_safe` out of
  layout / hit-test / animation so they stay unit-testable
  (INTENT §6). A built-in reaching for the hatch would put Skia
  back into `widget/` through the front door.

  *Drift indicators:*
  - `with_local_canvas` called from any `widget/` module other
    than `canvas_widget.rs`.
  - A second `RenderContext` method returning a backend-native
    handle.
  - `Painter` growing methods that mutate the widget tree or UI.

- **The painter is not sandboxed.** A panicking painter is host
  code panicking on the host's own render thread, and is not
  caught — swallowing it would hide host bugs behind a blank
  rectangle.

  *Drift indicators:*
  - `catch_unwind` around the painter call.
  - A painter that reads or writes runtime host state directly
    instead of receiving `props` and its own captured handles
    (violates INTENT §2's direction of flow).

### Tenets — the contract

- **Coordinates are logical.** Widgets compute layout in logical
  pixels and pass logical coordinates to `RenderContext`. The
  backend multiplies by its own scale.

  *Why:* widget code shouldn't know about DPI. It would have to
  multiply at every paint call, and would either over-scale
  (when a backend already accounts for DPI) or under-scale
  (when assumed). Centralizing the scale in the backend is the
  only sane place.

  *Drift indicators:*
  - A widget that calls `ctx.fill_rect(x * dpi, ...)` directly.
  - A backend that *doesn't* scale and the widget tree
    starts looking right (= the widget tree is doing it,
    which is wrong).

- **`Surface::draw` is read-only over the UI's logical state.**
  The trait takes `&mut UI` because the backend may need to
  mutate cache fields on the UI (e.g. the image cache, font
  collection), but it must not mutate widget state, layout
  rects, hover, focus, animations, or anything else
  load-bearing for the UI.

  *Why:* mutations to layout rects mid-draw produce
  inconsistent renders; mutations to widget state would
  diverge from what the next reconcile/event-dispatch sees.

  *Drift indicators:*
  - A backend that calls `widget.layout(...)` from inside
    `draw`.
  - A backend that mutates `WidgetRef` lock state outside of
    `widget.render(...)`.

- **`RenderContext::push_clip_rect` / `push_effects` use save/
  restore semantics.** Each push must be paired with a pop;
  unbalanced pushes leak canvas state. The Skia impl uses
  `canvas.save()` / `canvas.restore()` (and `save_layer_alpha`
  for non-1.0 opacity).

  *Drift indicators:*
  - A widget that calls `push_*` without a guaranteed `pop_*`.
  - A backend that doesn't implement save/restore (would mean
    transforms / clips leak between widgets).

- **DPI scale lives inside the backend.** Widgets are
  DPI-blind. Adding a new draw primitive should follow the
  Skia precedent of scaling at the trait method's body.

  *Drift indicators:*
  - A trait method that takes "scaled" coordinates.
  - DPI-aware logic in `widget/`.

---

## How the Skia walker drives rendering

`SkiaEnv::draw` runs in two passes:

### Pass A — base tree

`SkiaEnv::draw_widget_recursive` walks the live tree:

1. Determine focus (compare `widget_ref` to UI's focused).
2. Read the widget's `render_effects()`. If non-`None`, call
   `push_effects(opacity, transform, pivot_x, pivot_y)`.
3. If the widget's style has `backdrop_filter: Some(_)`, call
   `push_backdrop_blur(x, y, w, h, radii, sigma)` so the
   widget's own paint composites on top of a blurred capture
   of what's behind it.
4. Lock the widget; call `widget.render(env, focused, image_cache)`
   — the widget paints its own background/border/content using
   the `RenderContext` methods.
5. If the widget is a `PortalWidget` and its `open` is true,
   register its children + viewport-absolute origin as a
   `PortalEntry` on `ui.portal_layers` and *skip the recurse*
   for this widget — its children paint in Pass B.
6. Read the widget's children, layout origin, and scroll offset.
7. Drop the lock.
8. If the origin or scroll requires translation: `env.save();
   env.translate((origin.x - scroll.x) * dpi, (origin.y -
   scroll.y) * dpi)`. Recurse into children. `env.restore()`.
9. If `widget.needs_post_render()`: re-lock and call
   `post_render(...)`. (Used by scroll containers to pop their
   clip rect after children render.)
10. Pop any pushed `backdrop_blur` and effect layers in reverse
    order.

### Pass B — portal layers

After Pass A returns, the walker iterates open layers in
priority order (`overlay-modal` → `popover` → `tooltip` →
`toast` → `cursor-attached`). For each layer:

1. If the layer's `BackdropPolicy` is `Block` and at least one
   entry is open, paint a translucent runtime backdrop covering
   the viewport.
2. For each `PortalEntry` in the layer:
   - Translate the canvas to the entry's viewport-absolute
     origin.
   - Recurse into the entry's children using the Pass-A logic.

If `ui.active_drag_preview` is `Some`, the backend synthesizes
a `cursor-attached` entry from the cursor position and paints
it during the `cursor-attached` layer pass.

### Tenets — Skia walker

- **Pass A and Pass B share the same recurse logic.** Pass B's
  per-entry recurse uses the same
  effects/clip/scroll/post_render code path as Pass A. The
  difference is the *starting transform*: Pass A starts at the
  viewport origin; Pass B starts at each entry's
  viewport-absolute origin.

  *Why:* portals would otherwise require duplicating the entire
  walker, and any drift between the two paths would produce
  invisible bugs (e.g. opacity not applied inside a portal).

  *Drift indicators:*
  - A Pass-B path that re-implements `push_effects` /
    backdrop-blur / scroll handling instead of reusing the
    Pass-A code.
  - Portal entries painted at parent-relative coordinates (the
    pre-Phase-2.5 bug).

- **Layer iteration is fixed and high-priority-last.** The
  iteration order matches the layer enum's priority order;
  `cursor-attached` paints last so the drag preview sits on
  top of every other layer.

  *Drift indicators:*
  - A backend that picks a different layer order.
  - A new layer registered at runtime (the layer set is
    fixed; see [WIDGET_TREE.md → Portal layers](WIDGET_TREE.md#portal-layers)).

- **Walker translates between widgets, not within them.** Each
  widget paints in its own parent-relative coordinate system.
  The walker translates the canvas to the *child*'s parent
  (the current widget's content origin) before recursing. This
  matches how layout stores rects (parent-relative) and how
  the hit-tester walks events (also parent-relative).

  *Why:* see `WIDGET_TREE.md` for the symmetry argument. If
  the walker translated *inside* a widget instead of between
  it and its children, every widget would have to know its
  global position to paint correctly.

  *Drift indicators:*
  - A walker change that translates inside a widget's
    `render` call but doesn't translate back.
  - A widget that paints in screen coordinates (via global
    state passed through some side channel).

- **Effects layers wrap the widget *and* its descendants.**
  `push_effects` is called *before* `render`; `pop_effects` is
  called *after* recursion. So a widget's opacity affects its
  background AND its children. This is what makes "fade out a
  panel" work without the panel's children painting at full
  opacity over the faded background.

  *Drift indicators:*
  - Walker that pops effects between widget paint and child
    recursion.
  - Per-child effect layers (would composite N times for N
    children).

- **`post_render` runs after children, used by scrollers to
  pop the clip rect.** The widget can't pop in `render`
  because the clip needs to apply to children. Two-call
  rendering is awkward but specifically for this case.

  *Drift indicators:*
  - A scroll widget that pops its clip rect in `render` (would
    leak children outside the clip).
  - A new use of `post_render` for something other than
    clipping cleanup — should be rare.

---

## Per-widget render methods

Widgets implement `Widget::render(&self, ctx: &mut dyn
RenderContext, focused: bool, image_cache: &mut ImageCache)`.

Today's widgets:
- **`FlexWidget::render`** — paints background image,
  background color, border, and pushes a clip rect when
  `overflow != Visible`. Children are painted by the walker,
  not by this method.
- **`PresenceWidget`** — delegates to inner Flex.
- **`TextWidget::render`** — calls `ctx.draw_text(text, style,
  x, y, width)`. Width is read from layout rect.
- **`TextInputWidget::render`** — paints background like Flex,
  then calls `draw_text` with the current value, plus
  cursor/selection paint. Focus state is observed via the
  `focused: bool` argument.
- **`SvgWidget::render`** — calls `ctx.draw_svg_dom(dom, x, y,
  w, h)`. The DOM is parsed at construction.
- **`ImageWidget::render`** — calls `ctx.draw_image(path, x,
  y, w, h, image_cache)`.
- **`GridWidget::render`** — paints its inner Flex.
- **`CanvasWidget::render`** — calls
  `ctx.with_local_canvas(margin_box, &mut |p| painter(p, props))`.
  Paints nothing itself; the host painter owns every pixel inside
  the margin box.

### Tenets — render methods

- **`render` is `&self`, not `&mut self`.** Widgets must paint
  without mutating themselves. State changes happen during
  reconcile, event handling, or `tick_animations` — never
  during paint.

  *Why:* the walker locks the widget once for `render`, drops
  the lock, then re-locks for children. A `&mut` `render`
  would let widgets mutate while children are still being
  painted, which is racy by design.

  *Drift indicators:*
  - A widget that uses interior mutability (Cell, RefCell)
    inside `render` to update state.
  - A `render(&mut self)` override.

- **Text measurement is the gotcha for non-Skia backends.**
  `TextWidget::get_dimensions` calls `build_paragraph(ctx)`,
  which uses Skia's `ParagraphBuilder` directly via a
  `thread_local!` cache. This is the one place layout reaches
  into Skia.

  *Why:* there's no portable text-shaping in Rust today, and
  Skia's text shaper produces results matching what
  `draw_text` will paint. The alternative (rolling our own)
  would mean different "measure" and "paint" results, which
  would shift text mid-frame.

  *Drift indicators:*
  - More widgets reaching into Skia from layout.
  - A non-Skia backend trying to ship — they'll hit this and
    have to either bring Skia along for measurement, or
    re-implement text layout against their own shaper. Document
    this constraint loudly.

- **The image cache lives on the UI, not the backend.**
  `ImageCache` is loaded with images on demand by
  `RenderContext::draw_image`. The cache survives across
  frames and reloads (it's part of `UI`, which Ogham
  reconciles into rather than rebuilds from scratch).

  *Drift indicators:*
  - A backend that maintains its own image cache without
    using the one passed in (would re-load on every frame
    or reload).

---

## DPI scale

Skia's environment carries a `dpi_scale: f32`. Widgets pass
logical coordinates; the backend multiplies. The scale can
change at runtime via `SkiaEnv::set_dpi_scale(...)` (e.g. when
the window moves between displays).

Stroke widths, font sizes, and dimensions all scale. The Skia
impl has:
- `scale_coord(x)` — a position component.
- `scale_dim(w)` — a size component.
- `scale_stroke(w)` — a stroke width.
- `scale_font_size(s)` — a font size.

(All four currently multiply by `dpi_scale`. The separation
exists in case any of them ever needs different math, e.g.
stroke widths sometimes round to integer pixels for crisp
edges.)

### Tenets — DPI

- **Custom backends own their DPI policy.** A backend without
  a Retina-shaped notion (e.g. a TUI renderer for testing)
  is fine to skip the scale entirely. The trait doesn't
  expose `dpi_scale` to widgets.

  *Drift indicators:*
  - A trait method that asks the backend for its scale —
    couples widgets to a Skia-shaped concept.

---

## Custom backends — what to know

If you're shipping a non-Skia `Surface`:

1. **Implement `RenderContext`** for your backend type. The
   primitives are simple — a fill, a fill-with-radii, a border
   (which you may decompose into 4 lines or one stroked
   rrect), an image draw, a text draw. SVG draw might be
   skippable if you don't care about SVG widgets.
2. **Implement `Surface::draw(&mut self, ui: &mut UI)`** and
   walk the tree. The Skia walker
   (`SkiaEnv::draw_widget_recursive`) is your reference.
   Re-implement the recurse-with-translate-and-effects pattern.
3. **Coordinate scaling is your call.** If you don't have a DPI
   concept, just use the logical coordinates as-is.
4. **Save/restore semantics for clip rects and effects layers.**
   Whatever your backend's analogue is for "save canvas state"
   and "restore", use it inside `push_*` / `pop_*`.
5. **Text measurement is hard.** `TextWidget` calls into Skia's
   measurement directly. Either bring Skia along for measurement
   only, or re-implement `TextWidget` (replace it via
   `RuntimeConfig::with_widget`) with one that uses your own
   shaper.

### Tenets — custom backends

- **Widgets should never know which backend is rendering them.**
  No `if let Some(skia) = ctx.downcast::<SkiaEnv>()` paths.
  Every primitive a widget needs goes through the trait.

  *Drift indicators:*
  - A widget that uses `Any`-style downcasting on the
    `RenderContext`.
  - A trait method added that reflects a Skia-specific
    concept (e.g. `set_paint_style`).

- **Text measurement leakage is a known seam-leak.** Documented
  here as drift to fix, not as license to spread. Any new
  layout-time call into Skia is wrong.

  *Drift indicators:*
  - `use skia_safe::` outside of `src/skia.rs`,
    `src/widget/text_widget.rs`, `src/widget/text_input_widget.rs`,
    and `src/widget/svg_widget.rs` — plus
    `src/widget/canvas_widget.rs`, where `Painter` carries a
    native canvas under INTENT §6's named exception.

---

## Open questions (for the design-review phase)

- **Text measurement reaching directly into Skia is the largest
  active drift from `INTENT §6`.** Custom backends *cannot*
  ship without bringing Skia along just for measurement. A
  `LayoutContext::measure_text(text, style)` callback that the
  embedder provides would be the cleanest fix; the gotcha is
  that text width affects layout, so the measurement has to be
  available during the layout pass.
- **`Surface::draw` having a single method means backends have
  to re-implement the tree walker.** A library helper that
  takes a `RenderContext + Surface` and walks the tree
  generically would let backends just implement
  `RenderContext`. Worth doing; the only reason it isn't
  factored is that today's only backend is Skia.
- **`push_effects` is called even when the layer has no
  effect (opacity = 1, transform = identity).** The Skia
  impl's `render_effects()` returns `None` for the no-op case,
  but if a widget ever explicitly returns `Some(default)`,
  it still pushes a save/restore. Audit.
- **`save_layer_alpha` is expensive.** Every <1.0 opacity
  triggers an offscreen layer. A widget tree with many
  semi-transparent leaves pays for one composite per leaf.
  Could be batched if a parent has opacity but multiple
  children don't — but that's a non-trivial backend optimization.
- **`draw_svg_dom` takes a `skia_safe::svg::Dom`** in the
  trait signature. That's a Skia type leaking into the trait —
  inconsistent with `INTENT §6`. Either fork the type to a
  Skia-agnostic SVG abstraction, or document that the SVG
  widget is Skia-only.
- **Image cache is `ImageCache` (concrete)**, not a trait.
  Custom backends need an image cache; should the type be
  generic / trait-shaped? The current code lets the cache
  load images using `skia_safe::Image` internally — another
  Skia leak. Audit when designing custom backends in earnest.
- **No primitive for paths or polygons.** A backend can
  implement `draw_line` repeatedly for paths but the
  triangle-strip / generic-shape case is awkward. If *widgets*
  ever need them, add primitives rather than letting them
  reach for `skia_safe`. (*Host* paint has an answer already —
  `with_local_canvas` — and a typed `draw_path` was considered
  and rejected for it: the real consumers need colour matrices,
  path effects, blend modes and offscreen surfaces too, and half
  a hatch is worse than a named one. See `CANVAS_LEAF.md` §5.)
- **`focused: bool` on `render` is a UI-state leak into paint.**
  The `is_focused` check is more naturally a method on the
  widget (`is_focused()`), but the UI knows focus and the
  widget doesn't. Today's design works; consider whether the
  passing-through is worth the awkwardness.
