# Ogham — The `Canvas` leaf: a host-painted, layout-participating widget

> **Status: complete. M0–M4 shipped 2026-08-06; M5 (the regency
> migration, §8) landed the same day in `regency` as `fb40a6a`, and the
> dial was confirmed correct in motion by eye.**
>
> §8's deletion list was optimistic: `WHEEL_CENTER_Y_FRAC` and
> `_DANCE` survive, because flex cannot express "prefer 60% of the
> viewport, but rise if a *measured* sibling crowds you" — that is two
> anchors on one axis, and a column has one. The measured card wins and
> the fraction now applies only when the dial stands alone. Read §8
> against `regency`'s commit, not as-written.
>
> Where the shipped code and this plan differ, the code is authority;
> the divergences are listed in §4.1. The
> integration-facing writeup lives in
> [`AGENTS.md` → *Host-painted `Canvas`*](../../AGENTS.md); the seam
> contract lives in [`SURFACE.md`](SURFACE.md); the INTENT §6 amendment
> in §6 below has been applied.
>
> Drafted 2026-08-06. Closes the deferral recorded in lorekeeper
> `docs/CANVAS_COMPOSITION.md` §3 ("`Canvas` widget (deferred)"), whose
> second trigger has now fired — see §1.
>
> This plan adds one built-in widget and **one** new `RenderContext`
> method. It does not change the language, the VM, the reconciler, or
> any existing tenet's behaviour. It *does* require an explicit,
> narrowly-scoped amendment to [`INTENT.md`](INTENT.md) §6 — see §6.
>
> Naming follows lorekeeper's rule for ogham-side work: the vocabulary
> stays application-neutral (`Canvas`, `painter`, `anchor`, `layer` —
> no game-domain terms).

---

## 1. Why now — the trigger that fired

`CANVAS_COMPOSITION.md` §3 deferred this widget and named three
conditions for building it. The second one:

> a layout need where chrome geometry must reflow the canvas and the
> implicit window-minus-sidebar math becomes a real maintenance cost

Regency's quick-time-event dial is that case, shipped:

- **It is widget-sized.** `ManorScreen::wheel_radius(h) =
  (h * 0.085).clamp(48.0, 92.0)` — a 96–184 px dial, not a screen.
- **It sits in a vertical lane with real siblings.**
  `ManorScreen::wheel_center` (`regency-client/src/manor.rs:4091`) is
  hand-rolled column layout: a `min`/`max` clamp seating the dial above
  the search card, with a gap.
- **It is layout-coupled to ogham chrome in both directions.** The
  Skia side holds `const SEARCH_CARD_HEIGHT: f32 = 96.0` — a hardcoded
  guess at the measured height of an *ogham* widget, with the comment
  *"Only the geometry test uses it — the chrome measures the real card
  — but the dial has to be seated above where that card will stand."*
  Meanwhile `ManorScreen::search_card` returns
  `SEARCH_CARD_LIFT_UNDER_WHEEL` (40) instead of `SEARCH_CARD_LIFT`
  (118) while the dial is live, so the ogham chrome reflows because of
  the canvas.
- **A test holds the guess in place.**
  `the_dial_and_what_hangs_under_it_stay_on_the_screen`
  (`manor.rs:5897`) asserts four window heights × two dial seats.

That is bidirectional layout coupling across two rendering systems,
mediated by a magic number and defended by a geometry test. Flex
already does this correctly; the dial is outside it only because it
cannot paint inside it.

**What we are NOT doing.** The full-screen cases — regency's manor
plan, lorekeeper's map editor, small_mercies' map pane, UL's map editor
— stay exactly as they are. They are already adequately served by a
transparent root + `block_interactions: false` + `UI::blocks_point` +
`app`'s Phase-2 pointer capture. Admitting them into the requirements
is how a small widget becomes a rendering-framework pivot. They are
out of scope, deliberately, and §7's drift indicators say so.

---

## 2. What already exists

Half the plumbing is built and has **zero consumers**:

- `RuntimeConfig::custom_widgets: HashMap<String, WidgetFactory>` plus
  the `with_custom_widget(name, factory)` builder
  (`src/runtime/config.rs:112`).
- `Runtime::from_source` merges them into the `WidgetRegistry`
  (`src/runtime/mod.rs:676`), overriding built-ins on a name collision.
- `Ogham::reload_file` clones the config into the new `Runtime`
  (`src/lib.rs:194`), so registrations **already survive hot reload** —
  the same way event handlers do.
- The builder resolves widget identifiers case-insensitively through
  the registry (`src/widget/builder.rs:292`), so `MyThing { … }` in a
  `.ogh` already routes to a host factory today.

So a host can already put a custom widget in the tree. What it cannot
do is **paint** one: `RenderContext` offers `fill_rect`,
`fill_corners_rect`, `draw_border`, `draw_image`, `draw_text`,
`draw_line`, clip, effects, backdrop-blur — and nothing else. A dial
needs arcs, wedges, a comet tail, and a rotated needle.

**This plan therefore adds exactly two things:** a paint escape on
`RenderContext`, and a built-in `Canvas` widget so a host does not have
to implement the 20-method `Widget` trait just to draw a dial.

---

## 3. The design

### 3.1 `.ogh` surface

```ogh
Canvas {
  painter: "wheel_dial",          // names a host-registered painter
  props: { radius: dial_r, dance: dancing },   // optional, Value map
  style: {
    width: dial_w, height: dial_h,
    margin: { bottom: 14 },
    // ...every FlexStyle property a leaf honours
  },
  on_click: fn (e) { event("dial_press"); },   // ordinary listeners
}
```

`Canvas` is a **leaf**: no children, no `children:` property. It
participates in flex layout exactly like `Image` does today, but takes
`width`/`height` from `style` (including `"grow"` / `"shrink"`) rather
than as bare required numbers — the `Image` mistake
(`stargazer-celia-game/docs/OGHAM_GAPS.md` §8) is not repeated.

`painter` is a **required** string. An unregistered painter name is a
loud error, not a silent blank — see §3.5.

`props` is an optional `Value` map handed to the painter verbatim each
frame. It is the *only* channel from `.ogh` into the painter; the
painter reads host state through the host's own Rust, not through the
VM. This keeps INTENT §2 intact: nothing flows out of the painter.

### 3.2 Host surface

```rust
let config = RuntimeConfig::new()
    .with_painter("wheel_dial", move |p: &mut Painter, props: &Value| {
        // p.canvas() is pre-translated to the widget's origin and
        // pre-scaled by DPI: draw in LOCAL LOGICAL coordinates from (0,0).
        // p.width() / p.height() are the laid-out logical size.
        dial::paint(p.canvas(), p.width(), p.height(), &state.lock().unwrap());
    });
```

`Painter` is a new struct in `src/widget/canvas_widget.rs`:

```rust
pub struct Painter<'a> {
    canvas: &'a skia_safe::Canvas,   // pre-translated + pre-scaled
    width: f32,                      // laid-out LOGICAL width
    height: f32,                     // laid-out LOGICAL height
    dpi_scale: f32,                  // for painters that need device px
}
```

**The coordinate contract is the load-bearing ergonomic decision.** The
canvas handed to a painter is already `save()`d, translated to the
widget's origin, and scaled by `dpi_scale`, so the painter draws from
`(0, 0)` to `(width, height)` in logical pixels and never sees DPI or
its position in the tree. `Canvas::render` restores afterward. Regency's
`draw_wheel_check` becomes a near-verbatim lift: `cx`/`cy` become
`width/2.0`, `height/2.0`, and every seating constant disappears.

Painter type alias:

```rust
pub type CanvasPainter =
    Arc<dyn Fn(&mut Painter, &Value) + Send + Sync>;
```

`Send + Sync` matches `WidgetFactory` and `event_handlers`; painters
capture `Arc<Mutex<…>>` host state exactly as event handlers already do.

### 3.3 Where it plugs in

| Concern | Mechanism |
|---|---|
| Registration | `RuntimeConfig::with_painter(name, f)` → `painters: HashMap<String, CanvasPainter>`. Applied in `Runtime::from_source` beside `custom_widgets`. **Survives hot reload for free** (config is cloned into the new Runtime). |
| Lookup | `Runtime.painters`, cloned into the `CanvasWidget` at build time by `create_canvas_widget` — same shape as `widget_registry.clone()` in `Ogham::update`. |
| Layout | `CanvasWidget` implements `get_dimensions` / `get_fixed_width` / `get_fixed_height` from its `FlexStyle` `Size`, reusing `LayoutContext::effective_width`/`effective_height`. |
| Paint | `Widget::render` → `ctx.with_local_canvas(rect, &mut |p| painter(p, &props))`. |
| Events | Reuse the `Image` leaf's `handle_event` shape: hit-test `contains_point`, fire matching `event_listeners`. `blocks_interactions()` returns `true` iff it has any pointer listener — same rule as Flex. |
| Reconcile | `update()` absorbs a new `CanvasWidget`: swaps `props`, `style`, `listeners`; `needs_repaint` on any prop change; `needs_layout` on a size-affecting style change. `UpdateResult::replace()` on a `painter` name change (a different painter is a different widget). |

### 3.4 The `RenderContext` escape

One new method, defaulted so no existing impl breaks:

```rust
/// Hand a host painter a canvas positioned at `rect`'s origin and
/// scaled to device pixels, so it paints in local logical coordinates
/// from (0, 0) to (rect.width, rect.height).
///
/// Backends that cannot expose a native canvas return `false` and the
/// Canvas widget paints nothing — a Canvas is opt-in host paint, and a
/// backend that can't service it is not an error.
fn with_local_canvas(
    &mut self,
    _rect: &Rect,
    _paint: &mut dyn FnMut(&mut Painter),
) -> bool {
    false
}
```

`SkiaEnv` implements it: `save()` → `translate(rect.x * dpi, rect.y * dpi)`
→ `scale(dpi, dpi)` → build `Painter { canvas: self.surface.canvas(), … }`
→ call → `restore()` → `true`.

`SkiaEnv` is the only `RenderContext` impl in the tree (verified — there
are no test fakes), so the default is belt-and-braces for future
backends, not a live branch.

### 3.5 Failure modes, made loud

The framework's recurring failure shape — *silent* degradation
(`Position::Relative` parsing to a no-op; `\u{2212}` shipping to screen;
an unknown font family falling through) — is explicitly not repeated:

- **Unknown painter name** → `BridgeError::InvalidPropertyType` at build
  time, listing the registered names. Strict-mode `host_state` files get
  it at compile time; others at first render, through the existing error
  channel Celia's `Chrome::error` already surfaces.
- **Missing `painter` property** → `BridgeError::MissingProperty`.
- **Backend without `with_local_canvas`** → paints nothing, but logs
  once per painter name (`eprintln!` behind the same gate as the layout
  warning), so "my canvas is blank" has an answer.
- **A panicking painter** → not caught. A painter is host code on the
  host's own render thread; swallowing its panic would hide host bugs.
  Documented in `AGENTS.md`.

---

## 4. Milestones

Each milestone is independently landable and green.

**M0 — `Painter` + the `RenderContext` escape.** *(Shipped.)* (~120 LOC + 4 tests)
`src/widget/canvas_widget.rs` with the `Painter` struct and
`CanvasPainter` alias; `RenderContext::with_local_canvas` defaulted to
`false`; `SkiaEnv`'s impl. Tests: translate/scale correctness against a
recording canvas at dpi 1.0 and 2.0; the default returns `false`.

**M1 — `CanvasWidget` + builder registration.** *(Shipped.)* (~260 LOC + 8 tests)
The widget: layout from `FlexStyle`, leaf `get_children`, `render`
delegating to the painter, `update` absorbing, `contains_point`,
`blocks_interactions`. `create_canvas_widget` in `builder.rs`;
`reg.register("canvas", …)` in `with_defaults`. Tests: sizes from fixed /
`grow` / `shrink`; absorbs on re-render preserving identity; replaces on
painter change; missing/unknown painter errors.

**M2 — `RuntimeConfig::with_painter` + hot-reload survival.** *(Shipped.)*
(~60 LOC + 3 tests) `painters` on config and `Runtime`; applied in
`from_source`; threaded into the widget at build. Tests: a painter
registered pre-`watch` still paints after a `reload()`; a painter
registered on two Ogham instances doesn't cross-contaminate.

**M3 — Events + occlusion.** *(Shipped.)* (~80 LOC + 5 tests) `handle_event` over
`event_listeners`; `blocks_point` / `blocks_interactions` returning true
only with a pointer listener; `declared_cursor` from `style.cursor`.
Tests: click inside/outside; `UI::blocks_point` true under a listening
Canvas and false under a bare one (the property regency's world-picking
gate depends on).

**M4 — Docs.** *(Shipped.)* `AGENTS.md` gains a "Host-painted `Canvas`" section (the
integration guide is where library users actually look — see
`OGHAM_GAPS.md` §2 for what happens when it isn't there); `SURFACE.md`
gains the coordinate contract; `INTENT.md` §6 gains the amendment in §6
below; `examples/canvas.ogh` + a tiny painter in the `client` binary so
the feature is discoverable by the two routes people actually use.

**M5 — Regency migration.** *(Outstanding.)* See `REGENCY_CANVAS_MIGRATION` in §8.

**Estimate:** ~520 LOC + ~20 tests in ogham; ~2 person-days. Lorekeeper
priced the equivalent at "~+400 engine lines"; the delta is the
`Painter` ergonomics and the config plumbing, both of which buy back
the hot-reload story.

**Actual:** ~470 LOC of implementation + 21 tests in
`tests/canvas_widget.rs` + 2 unit tests in `canvas_widget.rs`.

### 4.1 As built — deltas from this plan

Everything above landed as written except for the following. The plan
was drafted from a code read; these are the things building it surfaced.

1. **`ogham::skia_safe` is re-exported from the crate root.** Not in the
   plan, but mandatory in practice: `Painter::canvas()` returns a
   `skia_safe::Canvas`, so a host must name Skia types, and a host that
   depends on `skia-safe` independently can end up with *two* copies in
   the binary — at which point the draw calls stop typechecking with an
   error that blames the wrong thing. Going through the re-export makes
   version agreement structural.

2. **The painter gets the widget's MARGIN box, not its layout rect.**
   `style: { margin: … }` is space *around* a widget; painting into it
   would put the dial's ring outside its own hit-test box (which is
   margin-aware, matching `FlexWidget::contains_point`). Padding and
   border are *not* subtracted — the painter owns the whole interior.

3. **`shrink` on a `Canvas` resolves to the widget's own inset**
   (padding + margin + border), not to a content measurement. A painter
   reports no intrinsic size, so there is nothing else it could mean. Say
   so out loud because `shrink` is the `FlexStyle` default: a `Canvas`
   with no `width`/`height` is invisible, not full-bleed.

4. **`CanvasWidget` implements `set_hovered` / `is_hovered` /
   `fire_listeners`.** The plan listed `mouse_enter` / `mouse_leave`
   among the listeners without noting that `UI::update_hover` drives
   those off a per-widget hover flag. Without the three methods the
   listeners would register and silently never fire — the exact failure
   shape §3.5 exists to prevent.

5. **Painter names are matched exactly, not lowercased.** Widget *type*
   names are language identifiers and are case-folded; a painter name is
   an opaque host-chosen string. Folding it would make
   `with_painter("wheelDial", …)` unreachable from
   `painter: "wheelDial"`.

6. **The "already warned" set for a backend without the hatch is a
   process-global, not a widget field, and is `#[cfg(debug_assertions)]`
   only.** `Widget::render` takes `&self`, and `SURFACE.md` names
   interior mutability inside `render` as a drift indicator; having the
   set survive reconciliation is also the behaviour you want.

7. **`Painter`'s fields are private with accessors.** `canvas()`,
   `width()`, `height()`, `dpi_scale()` — matching how §3.2's own host
   example already used it.

8. **`Canvas` has no `key:`.** Neither does any other leaf
   (`Text`, `Image`, `TextInput`). Add it if a keyed list of Canvases
   ever shows up; the plan didn't call for it and matching the leaf
   precedent keeps `Widget::key` honest.

---

## 5. What this deliberately does not do

- **No `draw_path` on `RenderContext`.** Considered and rejected for
  M0–M5: it would keep `skia_safe` out of host widget code, but it
  cannot reach what the real consumers need (colour matrices, path
  effects, blend modes, offscreen surfaces, mipmapped sampling — all
  live in `cartography-view`). Half a hatch is worse than a named one.
  Revisit if a non-Skia backend is ever real, which INTENT §6 says it
  is not.
- **No canvas *children*.** A `Canvas` is a leaf. Chrome that must sit
  over painted content is a sibling with `position: { type: "absolute" }`,
  or a Portal. Allowing children would mean the painter and the widget
  tree both own the same pixels.
- **No hit-test forwarding into the painter.** The dial takes no pointer
  input (its press is Space → `PlayerCommand::WheelPress`), so a
  path-contains query buys nothing today. Point-in-painter hit-testing
  is a follow-up with its own trigger: the first Canvas whose interior
  needs sub-region picking.
- **No full-screen canvas migration.** §1.

---

## 6. INTENT §6 amendment (required before M0 lands)

INTENT §6 currently says the `Surface` seam survives as a
"paint-isolation + test seam" and lists `use skia_safe::` outside
`src/skia.rs` as a drift indicator. `with_local_canvas` hands a raw
`skia_safe::Canvas` to *host* code, so §6 must be amended explicitly
rather than quietly violated. Proposed addition:

> **Exception — the `Canvas` painter hatch.** `RenderContext::with_local_canvas`
> hands a backend-native canvas to a host-registered painter. This is a
> *named, host-facing* hatch, not convenience drift: the widget tree,
> layout, hit-testing, reconciliation, and animation stay
> backend-agnostic, and no `widget/` code paints through it. The seam's
> purpose — keeping `skia_safe` out of layout/hit-test/animation so they
> stay unit-testable — is untouched.
>
> **Drift indicators for the exception:**
> - Any `widget/` module calling `with_local_canvas` other than
>   `canvas_widget.rs`.
> - A second `RenderContext` method returning a backend-native handle.
> - `Painter` growing methods that mutate the widget tree or the UI.
> - A built-in widget (Flex/Text/Image/Grid/Presence/Portal) painting
>   through the hatch instead of through the typed primitives.

---

## 7. Drift indicators for the feature itself

- A `Canvas` with `children:` — the leaf rule broke.
- A painter reading or writing runtime host state directly rather than
  receiving `props` and its own captured host handles (violates §2's
  direction of flow).
- A painter registered post-hoc on the live `Runtime` instead of on
  `RuntimeConfig` — it will vanish on the next hot reload, silently.
- `SEARCH_CARD_HEIGHT`-shaped constants reappearing: any host arithmetic
  that hardcodes a *measured* ogham dimension. That is the exact debt
  this feature exists to delete.
- A full-screen `Canvas` filling the viewport under a transparent shell.
  That's case B and it already works; a `Canvas` there is pure overhead.

---

## 8. Regency migration (M5)

**Goal: delete the hand-rolled column layout, keep the painter.**

Everything the dial *thinks* stays in Rust: `WheelFeel`, the smoothed
clock, the server's needle arithmetic, the `WheelCue` audio queue, the
preroll window. INTENT §2 and §10 are untouched — the painter is host
code, not `.ogh`.

Steps:

1. **Extract the painter.** `draw_wheel_check(canvas, w, h, palette, view)`
   → `wheel_dial::paint(canvas, w, h, &WheelFeel, &Palette, &PlayerView)`,
   drawing from `(0,0)` to `(w,h)` with `cx = w/2.0`, `cy = h/2.0`.
   Pure mechanical: strip `wheel_center`'s output from the body.
2. **Register it** in `chrome.rs`'s `RuntimeConfig` chain, capturing the
   `Arc<Mutex<…>>` the chrome already holds for its intent channel.
3. **Declare it in `client.ogh`** as a `Canvas` in the existing chrome
   column, above the search card, with `dial_size` projected as host
   state (`wheel_radius(h) * WHEEL_MAX_SWELL * 2.0 + WHEEL_RING_OUT * 2.0`
   stays Rust — it's the painter's own reach, not a seating constant).
4. **Delete:** `SEARCH_CARD_HEIGHT`, `WHEEL_STACK_HEIGHT`,
   `WHEEL_LANE_GAP`, `WHEEL_CENTER_Y_FRAC`, `WHEEL_CENTER_Y_FRAC_DANCE`,
   `ManorScreen::wheel_center`, and the two `SEARCH_CARD_LIFT*` constants
   (the card's lane becomes "is the dial in the column or not").
5. **Replace the geometry test.** `the_dial_and_what_hangs_under_it_stay_on_the_screen`
   asserted four heights × two seats because the constants could drift.
   Under flex it becomes a chrome-render test in the shape Celia already
   uses (`the_shipped_chrome_renders_every_mode`): render the manor
   chrome at 600/720/1080/1440 with a live dial, dancing and not, and
   assert no error and that the dial's laid-out rect is on-screen.
   **Do not simply delete it** — the invariant is still real, only its
   enforcement moves.
6. **Keep** the hotbar-hiding rule (`"a live wheel takes the lane"`,
   `manor.rs:5860`). That is a game rule about what is shown, not
   layout, and it stays in Rust.

**Expected deletion:** ~7 constants, one layout function, and one test's
worth of hand-verified arithmetic, replaced by a flex column that
measures the real card.

**Out of scope for M5:** the manor plan, the scenario wheel
(`regency-client/src/wheel.rs` — full-screen, and its right half is
hand-rolled *text* layout including a `wrap()` function, which argues
for this widget on entirely different grounds and should be assessed
separately), the bleed pass, and every other full-screen Skia surface.
