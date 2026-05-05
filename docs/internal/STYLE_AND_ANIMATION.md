# Ogham — Style and Animation

> **Status: Live contract.**
>
> The animatable subset of `FlexStyle`, the spring math that
> drives transitions, and the per-frame integration that produces
> the rendered style. Layout consumers read `effective_style`,
> which equals `declared_style` when settled and holds
> interpolated spring values mid-transition. Authoring shape (the
> `transition: { ... }` declaration) lives in the user-facing
> guide; this doc covers what runs underneath.

---

## Authority

- Spring math, `AnimationState`, `TransitionSet`:
  [`src/widget/animation.rs`](../../src/widget/animation.rs).
- Style types: [`src/widget/style.rs`](../../src/widget/style.rs).
- Per-frame integration into the widget: `FlexWidget::tick_own_animations`
  in [`src/widget/flex_widget.rs`](../../src/widget/flex_widget.rs).

---

## At a glance

```
Author writes:     style: { transition: { background_color: "spring", ... } }
Builder parses:    TransitionSet (per-property TransitionConfig)
Reconcile:         FlexWidget::update → animations.retarget(old, target)
Per-frame:         UI::tick_animations(dt)
                     → FlexWidget::tick_animations
                       → tick_own_animations
                         → AnimationState::tick(dt)
                         → AnimationState::render_onto(target) → self.style
Render:            uses self.style (= effective_style)
```

The springs *don't* drive layout directly; they drive `self.style`
on the FlexWidget, which the layout pass reads. So any
spring-driven property that affects layout (padding, margin,
border width, gap, corner radius, text size) requests a layout
pass each tick.

---

## Animatable properties

The exhaustive list:

| Property            | Type            | Spring shape     | Layout-affecting |
|---------------------|-----------------|-------------------|---|
| `background_color`  | `Color`         | `ColorSprings` (4 channels) | no |
| `text_color`        | `Color`         | `ColorSprings`    | no |
| `border`            | `Border` (4 sides × {width, color, style}) | `BorderSprings` (8 widths/colors; *style not animated*) | yes (via width) |
| `corner_radius`     | `CornerRadii` (4 corners) | `CornerRadiiSprings` | yes |
| `padding`           | `Spacing`       | `SpacingSprings` (4 sides) | yes |
| `margin`            | `Spacing`       | `SpacingSprings`  | yes |
| `gap`               | `f32`           | `Spring`          | yes |
| `text_size`         | `Option<f32>`   | `Spring`          | yes |
| `opacity`           | `Opacity` (f32) | `Spring`          | no (paint-only) |
| `transform`         | 5 scalars       | `TransformSprings` | no (paint-only) |

Anything else snaps. The `transitions` field on `FlexStyle` is a
`TransitionSet` with one `Option<TransitionConfig>` per
animatable property; `None` means snap, `Some(cfg)` means use the
configured spring.

### Tenets — the property set is closed

- **The animatable set is closed and lives on `TransitionSet`.**
  Adding a new animatable property means: (a) extending
  `TransitionSet` with a field, (b) extending `AnimationState`
  with a spring (or compound spring) field, (c) handling it in
  `retarget`, `tick`, `render_onto`, and `is_empty`/`has_layout_effects`,
  (d) parsing it in `parse_transition_value` /
  `parse_transition_entry`. The parallel structure across these
  files is load-bearing.

  *Why:* a transition declaration that referenced a non-animatable
  property would be confusing — the author writes "transition the
  X" and gets a snap. Closing the set makes the contract explicit
  on both sides.

  *Drift indicators:*
  - A property added to `TransitionSet` without a corresponding
    spring field on `AnimationState`.
  - A property handled in `render_onto` but not in `retarget` —
    transitions wouldn't fire but a never-touched spring would
    persist.
  - Non-animatable fields like `direction` or `flex_wrap` added
    to `TransitionSet` (they would be no-ops; they should be
    explicitly excluded).

---

## Spring math

```rust
struct Spring {
    current: f32,
    velocity: f32,
    target: f32,
    config: TransitionConfig { stiffness, damping },
}
```

Default config: `stiffness = 170.0, damping = 26.0` (≈300 ms
settle, comparable to react-spring's default).

Integration: sub-stepped semi-implicit Euler:

```rust
const MAX_SUB_DT: f32 = 1.0 / 120.0;
let mut remaining = dt;
while remaining > 0.0 {
    let step = remaining.min(MAX_SUB_DT);
    remaining -= step;
    let delta = self.target - self.current;
    let accel = self.config.stiffness * delta - self.config.damping * self.velocity;
    self.velocity += accel * step;
    self.current += self.velocity * step;
}
```

Settle threshold: `|target - current| < 0.01 && |velocity| < 0.01`.
On settle, `current` snaps exactly to `target` and `velocity`
zeroes — so subsequent ticks observe nothing to do.

### Tenets — spring math

- **Sub-stepping at 1/120 s is the integration stability cap.**
  At default stiffness (170) and 60 Hz, each frame is exactly
  two sub-steps. Larger `dt` (e.g. recovery from a minimized
  window stalling for 5 s) is clamped per sub-step so the
  Euler integrator stays stable rather than NaN'ing.

  *Why:* a single-step Euler integrator at large `dt` overshoots
  by orders of magnitude with stiff springs. Tested in
  `spring_clamps_huge_dt`.

  *Drift indicators:*
  - Switching to single-step Euler "for performance" without
    a stiffness ceiling.
  - Sub-step cap raised above 1/120 s and stiffness configs
    that exceed ~500 — the integrator becomes unstable.

- **Settle thresholds are absolute, not relative.** 0.01 is
  small enough that sub-pixel sizes / 1-of-255 color steps are
  imperceptible. Color values are integers 0-255, sizes are in
  logical pixels — the threshold is meaningful in both.

  *Drift indicators:*
  - Per-property thresholds being introduced — the absolute
    value works because all spring units are roughly
    comparable. Property-specific thresholds risk getting
    them wrong relative to each other.

- **Velocity survives retargeting.** `set_target(new)` does NOT
  reset velocity. A spring midway through `0 → 100` retargeted
  to `-100` continues with its current velocity, decelerating
  and reversing through 0. Tested in
  `interrupted_target_carries_velocity`.

  *Why:* visual continuity. Without velocity preservation,
  rapid hover-on/off would feel "snappy" — a spring would
  reset to zero velocity each retarget and look wooden.

  *Drift indicators:*
  - A retarget that resets velocity to 0.
  - A retarget that scales velocity (e.g. "preserve 50%") —
    inconsistent with the documented semantic.

---

## `AnimationState::retarget`

Called from `FlexWidget::update` (on reconcile) and
`FlexWidget::set_hovered`. The arguments are:
- `old: &FlexStyle` — the *currently rendered* style (read from
  `self.style`, which holds interpolated values mid-transition).
- `target: &FlexStyle` — the *new* target the springs should pull
  toward.

For each property:
1. If the new style has no `Some(cfg)` for the property, **clear
   the spring** for it. `target` overrides any existing
   transition.
2. If a spring already exists for this property, **always
   retarget** — preserves current + velocity. Important for
   rapid hover on/off; without this, a second hover-on while
   the first is still settling would discard mid-flight state.
3. Otherwise, if `old != target` for that property, *create* a
   spring at `old`'s value and set its target to `target`.

### Tenets — retargeting

- **Existing springs always retarget; new springs only spawn on
  `old != target`.** This asymmetry is load-bearing.

  *Why:* the old-vs-target check exists so an unchanged property
  doesn't spawn a no-op spring on every reconcile (would burn
  CPU on every frame the widget rerenders). But once a spring
  is *live*, we always update its target — the `old` we get
  from `self.style` may equal a stale interpolated value rather
  than the user's old target, so the gate would falsely skip.
  Tested in animation comments.

  *Drift indicators:*
  - Symmetry restored ("retarget only on change") — would lose
    the spring's awareness of rapid back-and-forth changes.

- **`render_onto(target)` overlays current spring values onto a
  clone of `target`.** Properties without active springs are
  taken from `target` directly. The result becomes
  `self.style` for layout/render this frame.

  *Why:* this is what makes "spring-driven property + non-spring
  property" combinations work. A widget with a spring on
  `background_color` and a snap on `padding` reads its current
  `background_color` from the spring and its current `padding`
  from `target` — both end up in `self.style` correctly.

  *Drift indicators:*
  - Rendering by mutating the *original* declared style.
  - A render that doesn't include a property even when a spring
    is active for it.

---

## Per-frame integration (`tick_own_animations`)

```rust
fn tick_own_animations(&mut self, dt: f32) -> TickResult {
    if self.animations.is_empty() { return TickResult::NONE; }
    let still_moving = self.animations.tick(dt);
    let layout_effects = self.animations.has_layout_effects();
    let target = self.target_style().clone();
    self.style = self.animations.render_onto(&target);
    if self.animations.is_empty() { self.style = target; }
    TickResult { needs_repaint: true,
                 needs_layout: layout_effects && still_moving,
                 still_animating: still_moving }
}
```

- `animations.tick(dt)` advances each spring; springs that
  settle are cleared (their `Option<...>` becomes `None`) so
  later iterations skip them.
- The return value is `true` while *any* spring is still moving.
- Layout effects only ever drive `needs_layout = true` while
  some spring is still moving — once everything settles, the
  caller stops being asked to relayout.

`tick_own_animations` is called from `FlexWidget::tick_animations`,
which also ticks smooth scroll and recurses into children. The
top-level `UI::tick_animations(dt)` is the only entry point the
host calls; everything else is internal.

### Tenets — per-frame

- **Layout-affecting spring ticks request `needs_layout`.**
  `has_layout_effects()` returns `true` when any of `border,
  padding, margin, corner_radius, gap, text_size` has a spring.
  Each tick where a layout-affecting spring is still moving
  re-runs layout.

  *Why:* a padding animation has to relayout each frame to
  produce a visible animation. Skipping the relayout would
  freeze the spring at a constant rendering even though `current`
  is changing.

  *Drift indicators:*
  - A new layout-affecting property added without
    `has_layout_effects` updated to mention it.
  - Suppressing the relayout for "performance" — would break
    the animations that need it.

- **Snap exactly to target on settle.** When `animations`
  becomes empty after `tick()`, `self.style = target`. Without
  this snap, the rendered style might be off-by-spring-threshold
  for a single frame after settling.

  *Drift indicators:*
  - A change that drops the post-tick re-set (the comment
    explicitly says it's load-bearing).

- **Debug build: stuck-spring warning every 60 frames.**
  `layout_anim_frames` increments every frame where a
  layout-affecting spring reports `still_moving`; at 30, 90,
  150, ... it logs a warning. This catches the regression
  where a relayout invalidates a property that hasn't actually
  changed and the spring never settles.

  *Drift indicators:*
  - Warning suppression in debug builds.
  - The threshold being raised so the warning never fires —
    the threshold is calibrated to "300 ms is the longest a
    settled animation should take, so 60 frames means
    something's wrong".

---

## Style fields and their semantics

(Full schema in [`src/widget/style.rs`](../../src/widget/style.rs).
Layout properties are in [`FLEX.md`](FLEX.md); this section is
the *paint-related* and *animatable* fields.)

- **`background_color: Option<Color>`** — fills the layout rect
  inside borders, with corner radii applied. `None` = no fill.
  Animatable per channel.
- **`background_image: Option<String>`** — path resolved by
  `ImageCache`. Painted under `background_color` if both are
  set. Not animatable.
- **`border: Border`** — four sides, each with `width`, `color`,
  `style: BorderStyle (Solid | Dashed | Dotted)`. Width and
  color are animatable; style snaps.
- **`corner_radii: CornerRadii`** — top-left / top-right /
  bottom-left / bottom-right. Each animatable.
- **`text_size: Option<f32>`** / **`text_color: Option<Color>`**
  — inherited by descendant text widgets that don't specify
  their own. (TextWidget reads its own
  `TextStyle::size`/`color`; FlexStyle's text fields are mostly
  TODO — see open questions.)
- **`opacity: Opacity`** — paint-time alpha (`[0,1]`). 1.0 means
  no layer composite; <1 triggers a Skia layer.
- **`transform: Transform`** — affine: `translate_x/y`,
  `scale_x/y`, `rotate` (degrees, clockwise). Pivots around the
  widget's layout center.

### Tenets — style semantics

- **`opacity` and `transform` are paint-only.** They don't
  affect layout, they don't affect hit-testing, they don't
  participate in basis arithmetic. A widget rotated 45° is laid
  out as if rotated 0°; clicks land in the un-transformed bounds.

  *Why:* layout-aware transforms would force every layout call
  to walk the parent's effects stack, and hit-testing would
  need to invert the transform on every recurse. The cost is
  high; the use cases where layout *needs* to know are rare.

  *Drift indicators:*
  - A `Transform` consumer in `FlexWidget::layout` or
    `get_dimensions`.
  - A `contains_point` change that applies the inverse
    transform.

- **Border style (Solid/Dashed/Dotted) is non-animatable on
  purpose.** Only width and color are. Authors who want to
  switch styles transition between them with snap behavior.

  *Why:* there's no continuous interpolation between
  "solid" and "dashed" that produces a recognizable animation
  — you'd want either both halves rendered with cross-fade, or
  a snap. Snap is the simpler choice.

  *Drift indicators:*
  - A "border style cross-fade" feature that bypasses
    `BorderSprings::apply_to`.

- **Text styling lives in two places.** `TextStyle` (on
  `TextWidget`) and `FlexStyle::text_size`/`text_color` (on
  every Flex). The Flex fields are partially-implemented
  inheritance hooks. Audit whether they should be removed or
  fully wired up.

  *Drift indicators:*
  - Adding a third place where text styling lives.
  - Removing the Flex-side fields without updating the
    transitions parser to drop `text_color` / `text_size` from
    its TransitionSet.

---

## How spring state survives reconcile

The widget tree's `Arc<Mutex<dyn Widget>>` instances persist
across rerenders (see [INTENT §3](INTENT.md#3-widgets-reconcile-they-dont-rebuild)).
On reconcile, the live FlexWidget's `update` is called with a
freshly-built FlexWidget (the new descriptors). The live one:

1. Snapshots `old_rendered = self.style.clone()` (captures
   mid-flight values).
2. Adopts the new declared/hover/initial/exit styles and other
   props.
3. Computes `new_target = self.target_style()`.
4. If transitions are declared: `animations.retarget(old_rendered,
   new_target)`. The springs continue from `old_rendered` toward
   `new_target`, preserving any in-flight velocity.
5. If transitions aren't declared: clear `animations` and snap
   `self.style = new_target`.
6. Recurse into `reconcile_children`.

This is what makes hover, exit, and entry animations all
composable: every change to the widget's target style runs
through `retarget` with the *currently rendered* values as the
starting point.

### Tenets — reconcile + animation

- **`old_rendered` is captured *before* the new style is
  applied.** Otherwise the spring's starting point would be the
  new target — which would cancel the animation.

  *Drift indicators:*
  - Code that captures `self.style.clone()` *after* mutating
    `self.declared_style`.

- **A property losing its `Some(cfg)` in transitions clears
  the spring on `retarget`.** A widget whose author removed
  `transition: { background_color: "spring" }` snaps from then
  on; any in-flight spring is dropped.

  *Drift indicators:*
  - A change that "preserves" the spring while transitions
    are removed — would render with stale interpolation
    against a target the user no longer wants animated.

---

## Tests

Inline tests in `animation.rs` cover: spring settling, large-dt
clamping, retargeting velocity preservation, color channel
independence. There aren't dedicated tests for `AnimationState::tick`
clearing settled springs or for `render_onto` overlaying — those
are exercised through `FlexWidget` reconciliation tests.

---

## Open questions (for the design-review phase)

- **`stiffness` / `damping` are exposed as raw numbers.** Authors
  pick from physics knobs rather than UX intent
  ("snappy"/"smooth"/"slow"). React-spring's named presets feel
  better for typical use; the raw numbers are still useful for
  fine-tuning. Consider adding presets while keeping raw access.
- **No control over duration directly.** Stiffness + damping +
  initial displacement determines the perceived duration.
  Authors who want "settle in 200 ms regardless" can't ask for
  it. Could add a `duration` shape that solves for spring
  parameters.
- **No "ease" or non-spring transitions.** Cubic bezier and
  similar are not implemented; spring-only is the contract.
  Adequate for UI purposes; might surprise authors used to
  `ease-in-out`.
- **Per-tick `tick_animations` walks every widget.** Widgets
  with `animations.is_empty()` early-return, so the cost is
  bounded by tree depth + children, but a tree with thousands
  of widgets pays for the walk every frame regardless. Profile
  if it ever matters.
- **Animations are per-property, not per-keyframe.** No way to
  declare "background goes to red, *then* to blue". Authors
  with multi-step animations have to use `state` and trigger
  retargets manually.
- **Transform `rotate` is degrees, but most physics-style
  springs work in linear scalars.** A rotate spring from 0° to
  360° doesn't take the short path through 720°; it takes the
  literal interpolation 0 → 360. Probably the right choice
  (authors who want 360 = 0 wrap deliberately) but worth noting.
- **`text_color` and `text_size` are on `FlexStyle` *and*
  animatable in `TransitionSet`** — but `FlexStyle::text_color`
  / `text_size` aren't actually inherited by child Text widgets
  in any path I can find. So animating them on a Flex has no
  visible effect. Audit and either wire up inheritance or
  remove the fields from the transition set.
- **`AnimationState` is one-per-widget.** Authors can't run
  *concurrent* animations of the same property (e.g. layered
  overlays animating the same opacity). Not a current need; not
  cheap to add.
