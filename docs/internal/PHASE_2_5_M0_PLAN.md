# P25-M0 — Portal layer system + viewport-absolute coords

> **Status: Shipped 2026-05-05.** Original planning doc preserved
> below; the design landed substantially as drafted. Layer enum
> (`Main` / `OverlayModal` / `Popover` / `Tooltip` / `Toast` /
> `CursorAttached`), per-layer `BackdropPolicy` + cursor preference,
> two-pass Skia render, viewport-absolute `PortalEntry` coords, and
> hit-test layer-walk all shipped on `main`. The companion
> [`PHASE_2_5_IMPLEMENTATION.md`](PHASE_2_5_IMPLEMENTATION.md)
> trailer records the per-merge "what shipped" details across M0–M5.
> See [`SUBSYSTEMS.md → Portal widget and layers`](SUBSYSTEMS.md)
> and [`SURFACE.md`](SURFACE.md) for the live contract.
>
> Companion to `PHASE_2_5_IMPLEMENTATION.md` (the phase-level plan)
> and `LIFECYCLE_AND_PORTAL.md` (the Phase 2 design this extends).
>
> M0 is the foundation for Phase 2.5: replaces Phase 2's
> single per-frame `portal_layer` with a named-and-priority-
> ordered layer system per UL's `UI_RUNTIME.md` §1. Folds in
> the M3-deferred viewport-absolute coordinate fix.
>
> Original estimate: ~700 LOC + ~15 tests, ~3 person-days.
> Highest-risk P25 merge — the rework touches Skia draw, hit-test,
> `Portal` widget, builder, and `UI`.

---

## Motivation

Phase 2 shipped a minimal Portal:

```ogh
Portal { open: true, focus_trap: true, children: [...] }
```

with the runtime maintaining a single `UI.portal_layer:
Vec<PortalEntry>` populated each frame and consumed by Pass B
+ hit-test. Stacking is mount-order LIFO; there's no
distinction between modal, tooltip, popover, etc.

UL's `UI_RUNTIME.md` §1 specifies a **five-layer named system
with per-layer backdrop policies and priority ordering**:

| Layer | Use | Priority | Default backdrop |
|---|---|---:|---|
| `main` | Default panel layout | 0 | None |
| `overlay-modal` | Full-screen modals | 100 | Block |
| `popover` | Dropdowns, context menus | 200 | None |
| `tooltip` | Hover-spawned tooltips | 300 | None |
| `toast` | Ephemeral notifications | 400 | None |
| `cursor-attached` | Drag previews, custom cursor | 500 | None |

Cross-layer: higher priority always paints on top. Within a
layer: declaration / mount order LIFO.

Per-layer backdrop policy:
- `none` — no dimming, lower layers receive clicks.
- `dim` — lower layers rendered with reduced opacity, still receive clicks.
- `block` — lower layers rendered, pointer events blocked.

`overlay-modal` defaults to `block`; everything else defaults
to `none`. Userspace can render its own backdrop into a layer
for finer control.

Plus: Phase 2 documented a known limitation that
`PortalEntry.parent_rect` is captured in the immediate
parent's coordinate space, not viewport-absolute. Portals
nested below the root render at the wrong viewport position.
The fix lands here because the layer rework already touches
Pass B — folding in the coord fix avoids touching that code
twice.

---

## Concrete design

### PortalLayer enum

New module `src/widget/portal_layer.rs`:

```rust
/// Phase 2.5 M0: named portal layers with priority ordering.
/// Cross-layer rendering is determined by priority (higher
/// paints on top); within-layer ordering is mount-order LIFO.
///
/// Layers are a fixed runtime-known set — not extensible from
/// userspace. Per UL's UI_RUNTIME.md spec: "A new pattern
/// that needs a new layer requires a runtime change — that's
/// a feature, not a bug, because it forces design review of
/// layer-priority decisions."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PortalLayer {
    /// The default panel layout tree. Phase 2's "no portal" case;
    /// widgets that aren't inside any Portal land here implicitly.
    /// A Portal that explicitly declares `layer: "main"` would
    /// participate in normal flex layout — rare but legal.
    Main = 0,
    /// Full-screen modals — escape menu, settings, dialogs.
    /// Default backdrop: Block (lower layers receive no clicks).
    OverlayModal = 100,
    /// Dropdowns, context menus, sub-menus.
    /// Default backdrop: None.
    Popover = 200,
    /// Hover-spawned tooltips.
    /// Default backdrop: None.
    Tooltip = 300,
    /// Ephemeral notifications — toast queue.
    /// Default backdrop: None.
    Toast = 400,
    /// Drag previews, custom cursor effects.
    /// Default backdrop: None.
    CursorAttached = 500,
}

impl PortalLayer {
    /// All layers in priority order (low → high). Used to
    /// allocate the per-frame storage and to iterate Pass B.
    pub const ALL: [PortalLayer; 6] = [
        Self::Main,
        Self::OverlayModal,
        Self::Popover,
        Self::Tooltip,
        Self::Toast,
        Self::CursorAttached,
    ];

    pub fn priority(self) -> u32 {
        self as u32
    }

    pub fn default_backdrop(self) -> BackdropPolicy {
        match self {
            Self::OverlayModal => BackdropPolicy::Block,
            _ => BackdropPolicy::None,
        }
    }

    /// Parse a string layer name to the enum. Returns `None`
    /// for unknown names — caller surfaces the diagnostic.
    pub fn from_source_name(name: &str) -> Option<Self> {
        match name {
            "main" => Some(Self::Main),
            "overlay-modal" => Some(Self::OverlayModal),
            "popover" => Some(Self::Popover),
            "tooltip" => Some(Self::Tooltip),
            "toast" => Some(Self::Toast),
            "cursor-attached" => Some(Self::CursorAttached),
            _ => None,
        }
    }

    /// String name as it appears in `.ogh` source. Used by
    /// LSP hover and error messages.
    pub fn source_name(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::OverlayModal => "overlay-modal",
            Self::Popover => "popover",
            Self::Tooltip => "tooltip",
            Self::Toast => "toast",
            Self::CursorAttached => "cursor-attached",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackdropPolicy {
    /// No dimming, lower layers receive clicks normally.
    None,
    /// Lower layers rendered with reduced opacity (visual
    /// dimming only); clicks still pass through.
    Dim,
    /// Lower layers rendered, pointer events blocked.
    Block,
}
```

### PortalEntry shape change

```rust
// before (Phase 2):
pub struct PortalEntry {
    pub widget: WidgetRef,
    pub parent_rect: Rect,
    pub focus_trap: bool,
}

// after (P25-M0):
pub struct PortalEntry {
    pub widget: WidgetRef,
    /// VIEWPORT-ABSOLUTE rect of the portal's slot. Pass B
    /// translates by parent_rect.{x,y} from the viewport
    /// origin without further accumulation.
    pub parent_rect: Rect,
    pub layer: PortalLayer,
    pub focus_trap: bool,
}
```

Note: `parent_rect` semantics change (parent-relative →
viewport-absolute). The field name stays for Phase 2 callers
that don't care about the semantic shift — but we add a doc
note. If we want the rename, do it in M0 since this is the
breaking change to the shape; later merges shouldn't ripple.

**Decision**: rename to `viewport_rect: Rect` for clarity.
Phase 2 callers all live in our codebase; the rename is a
~6-call-site sed.

### PortalInfo extension

```rust
// before (Phase 2):
pub struct PortalInfo {
    pub open: bool,
    pub focus_trap: bool,
}

// after (P25-M0):
pub struct PortalInfo {
    pub open: bool,
    pub focus_trap: bool,
    pub layer: PortalLayer,
}
```

`PortalWidget::as_portal()` returns the layer alongside open
and focus_trap so the renderer can dispatch without a second
widget lock.

### UI.portal_layers

```rust
// before (Phase 2):
pub struct UI {
    pub portal_layer: Vec<PortalEntry>,
    // ...
}

// after (P25-M0):
pub struct UI {
    /// Per-frame portal layers, indexed by PortalLayer. Each
    /// layer's Vec contains entries in mount order. Pass B
    /// iterates layers low-priority-to-high; within a layer,
    /// LIFO (forward iteration paints last-mounted on top).
    /// Hit-test iterates high-priority-to-low; within a layer,
    /// reverse-mount-order (top-most-mount first).
    pub portal_layers: PortalLayers,
    // ...
}

/// Per-frame portal layer storage. Backed by an array indexed
/// by PortalLayer (cardinality 6). Cleared at the start of
/// each render pass.
#[derive(Clone, Default)]
pub struct PortalLayers {
    layers: [Vec<PortalEntry>; 6],
}

impl PortalLayers {
    pub fn clear(&mut self) {
        for v in &mut self.layers {
            v.clear();
        }
    }

    pub fn push(&mut self, entry: PortalEntry) {
        let idx = entry.layer as usize;
        self.layers[idx].push(entry);
    }

    /// Iterate layers in priority order (low to high). For
    /// each layer, mount order. Used by Pass B paint.
    pub fn iter_paint_order(&self) -> impl Iterator<Item = &PortalEntry> {
        PortalLayer::ALL
            .iter()
            .flat_map(move |layer| self.layers[*layer as usize].iter())
    }

    /// Iterate layers high-priority-to-low; within a layer,
    /// reverse-mount-order (top-most-mount first). Used by
    /// hit-test.
    pub fn iter_hit_test_order(&self) -> impl Iterator<Item = &PortalEntry> {
        PortalLayer::ALL
            .iter()
            .rev()
            .flat_map(move |layer| self.layers[*layer as usize].iter().rev())
    }

    /// Return all entries in a specific layer (mount order).
    pub fn entries_in(&self, layer: PortalLayer) -> &[PortalEntry] {
        &self.layers[layer as usize]
    }

    /// True if any entry in any layer satisfies the predicate.
    /// Used by `has_input_blocking_portal`.
    pub fn any<P: Fn(&PortalEntry) -> bool>(&self, p: P) -> bool {
        self.layers.iter().any(|v| v.iter().any(&p))
    }
}
```

The `[Vec<PortalEntry>; 6]` array is cache-friendly and
stable-sized; no HashMap allocation. Indexing by `layer as
usize` is a u32→usize cast.

### Portal widget property

```ogh
Portal {
  layer: "tooltip",       // NEW: defaults to "overlay-modal"
  open: true,
  focus_trap: false,
  children: [...],
}
```

Builder reads `layer` as a `Value::String`, calls
`PortalLayer::from_source_name`, surfaces a diagnostic for
unknown names.

PortalWidget gains:
```rust
pub struct PortalWidget {
    pub inner: FlexWidget,
    pub open: bool,
    pub focus_trap: bool,
    pub layer: PortalLayer,        // NEW; defaults to OverlayModal
    pub owned_path_prefix: String,
}
```

### Renderer changes

```rust
// Skia draw, after Pass A:
ui.portal_layers.clear();
let focused = ui.get_focused().cloned();
Self::draw_widget_recursive(
    self,
    &ui.root,
    focused.as_ref(),
    &mut ui.image_cache,
    &mut ui.portal_layers,
    /* accumulated_translate */ (0.0, 0.0),  // NEW
);

// Pass B: walk in priority order, paint each.
let entries: Vec<PortalEntry> = ui
    .portal_layers
    .iter_paint_order()
    .cloned()
    .collect();
for entry in entries {
    Self::paint_portal_entry(self, &entry, focused.as_ref(),
                             &mut ui.image_cache);
}

ui.sync_focus_stack();
```

`draw_widget_recursive` gains an `accumulated_translate:
(f32, f32)` parameter. Each level adds its own translate
(child_origin - scroll); when pushing to portal_layer, the
captured `viewport_rect` is `widget_layout_rect.x +
accumulated_translate.0, ...` — viewport-absolute.

`paint_portal_entry` translates from origin (0,0) by
`entry.viewport_rect.x, .y` — no parent-chain accumulation
needed.

Backdrop policy applies between layers in Pass B: when
moving to a higher-priority layer with `Dim` policy, render a
viewport-sized translucent black rect first; with `Block`,
render the rect AND mark subsequent hit-test for lower
layers as gated. (For M0, "Dim" can be a TODO — only
`overlay-modal` defaults to a non-`None` policy and that's
`Block`. Implement Block; document Dim as not-yet-rendered.)

### Hit-test rewrite

```rust
fn handle_click_event(&mut self, event: &Event, point: &Point,
                       ctx: &mut EventContext) -> bool {
    // Walk portal_layers high-priority-to-low, then within
    // each layer reverse-mount-order.
    let mut block_lower = false;
    for entry in self.portal_layers.iter_hit_test_order() {
        let child_point = Point::new(
            point.x() - entry.viewport_rect.x,
            point.y() - entry.viewport_rect.y,
        );
        let widget_ref = entry.widget.clone();
        let mut widget = widget_ref.lock().expect("widget lock poisoned");
        let children = widget.get_children_mut();
        drop(widget);

        let mut handled = false;
        for child in &children {
            let mut g = child.lock().expect("widget lock poisoned");
            if g.contains_point(&child_point) {
                if g.handle_event(event, ctx, child) {
                    return true;
                }
                handled = true;  // a child was hit even if it didn't claim
            }
        }

        // If this layer's policy blocks lower layers and any
        // child was hit (or the layer is full-viewport-modal),
        // suppress fall-through.
        let policy = entry.layer.default_backdrop();
        if policy == BackdropPolicy::Block {
            block_lower = true;
        }
    }

    if block_lower {
        return false;  // policy gates fall-through to base tree
    }

    // Fall through to the base tree.
    let mut root = self.root.lock().expect("widget lock poisoned");
    if root.contains_point(point) {
        return root.handle_event(event, ctx, &self.root.clone());
    }
    false
}
```

The `block_lower` logic implements the spec: a Block-policy
layer with any open entry suppresses base-tree clicks. Within
a layer, individual entries don't suppress siblings — that's
within-layer mount-order LIFO behavior (top-most-mount first
gets a shot).

### `has_input_blocking_portal` change

```rust
// before (Phase 2):
pub fn has_input_blocking_portal(&self) -> bool {
    self.portal_layer.iter().any(|e| e.focus_trap)
}

// after (P25-M0):
pub fn has_input_blocking_portal(&self) -> bool {
    // Only walk the OverlayModal layer — that's where modals
    // live. A focus_trap in a popover or tooltip is unusual
    // and shouldn't gate world input.
    self.portal_layers
        .entries_in(PortalLayer::OverlayModal)
        .iter()
        .any(|e| e.focus_trap)
}
```

This is a behavior change vs Phase 2 (which checked all
portals regardless of layer). Phase 2 had no tooltip-with-
focus_trap concept anyway, so the behavior is equivalent for
existing code, more correct for the future.

### Builder diagnostic

```rust
fn create_portal_widget(...) -> Result<WidgetRef, BridgeError> {
    let mut portal = PortalWidget::new();
    portal.owned_path_prefix = descriptor.owned_path.clone();

    // NEW: parse layer property
    if let Some(value) = descriptor.properties.get("layer") {
        match value {
            Value::String(name) => match PortalLayer::from_source_name(name) {
                Some(layer) => portal.layer = layer,
                None => {
                    return Err(BridgeError::InvalidPropertyType(
                        "layer".to_string(),
                        format!(
                            "Portal expects 'layer' to be one of: \
                             main, overlay-modal, popover, tooltip, \
                             toast, cursor-attached. Got: {:?}",
                            name
                        ),
                    ));
                }
            },
            other => {
                return Err(BridgeError::InvalidPropertyType(
                    "layer".to_string(),
                    format!(
                        "Portal expects 'layer' as a string; got {:?}",
                        other
                    ),
                ));
            }
        }
    }
    // ... rest as before (open, focus_trap, children)
}
```

---

## Resolved decisions

The 3 P25-M0 open questions from `PHASE_2_5_IMPLEMENTATION.md`,
resolved here.

1. **Does `block` layer policy subsume `focus_trap: true`?**
   ✓ **Keep them independent.** Layer policy gates *clicks*
   (backdrop pointer-event blocking); focus_trap gates *focus
   moves* (try_set_focus rejection). They address different
   concerns. A modal might have backdrop-block + focus-trap
   (the canonical case); a tooltip might have neither
   (default). A modal that opts out of focus_trap is still a
   modal. Both flags stay first-class.

2. **Per-layer backdrop policy: hardcoded or configurable?**
   ✓ **Hardcoded** in `PortalLayer::default_backdrop()`.
   Userspace can still render its own backdrop into a layer
   for finer control (per UL's spec "render its own backdrop
   into the same layer if it wants finer control"). Per-layer
   defaults are a small tightly-scoped set; making them
   configurable adds API surface for unclear gain.

3. **Coordinate-fix scope: just Pass B, or also nested-portal
   recursion?** ✓ **Both, naturally.** With viewport-absolute
   coords on `PortalEntry.viewport_rect`, Pass B's
   `paint_portal_entry` translates from viewport origin
   directly. Nested portals (rare; portal-inside-portal-
   children) push to the same `portal_layers` during their
   own draw_widget_recursive call — so they get the right
   viewport coords for free. The Phase 2 throwaway `nested:
   Vec` parameter to `paint_portal_entry` goes away.

---

## Implementation order

Sub-step ordering within M0. ~3 person-days; budget shape:

### Day 1: foundation + Portal widget (~250 LOC)

1. **Define `PortalLayer` and `BackdropPolicy` enums** in
   new `src/widget/portal_layer.rs`. Implement the helpers
   (`ALL`, `priority`, `default_backdrop`, `from_source_name`,
   `source_name`).
2. **Define `PortalLayers` struct** in `src/widget/mod.rs`
   alongside `PortalEntry`. Implement clear, push,
   iter_paint_order, iter_hit_test_order, entries_in, any.
3. **Extend `PortalEntry`**: rename `parent_rect` →
   `viewport_rect`; add `layer: PortalLayer`. Update field
   docs.
4. **Extend `PortalInfo`** with `layer: PortalLayer`.
5. **Update `PortalWidget`**: add `layer: PortalLayer` field
   (default `OverlayModal`). `as_portal()` includes it.
6. **Builder change** (`create_portal_widget`): parse the
   `layer` property; fall through to default; reject unknown
   string with diagnostic.
7. **Build check**: `cargo build` clean. Existing tests
   probably failing because `portal_layer` field renamed —
   that's expected; test migration is Day 2.

### Day 2: renderer + hit-test + UI changes (~300 LOC)

8. **Replace `UI.portal_layer`** with `UI.portal_layers:
   PortalLayers`. Initialize in `UI::new`. Update
   `clear_lifecycle_state` to clear it.
9. **Renderer Pass A** (`Skia::draw_widget_recursive`): add
   `accumulated_translate: (f32, f32)` param. Each recursion
   adds its own translate. When pushing to portal_layers,
   capture `viewport_rect` as
   `(layout.x + accumulated_translate.0, layout.y + accumulated_translate.1, w, h)`.
   Update `paint_portal_entry` to drop the throwaway nested
   layer (recursion now goes back through the main path with
   correct accumulated_translate = (0,0) at portal root).
10. **Renderer Pass B**: iterate `portal_layers.iter_paint_order()`,
    paint each entry. Apply backdrop policy at layer boundaries
    (Block: render full-viewport translucent black before the
    layer's first entry; Dim: TODO note; None: skip).
11. **Hit-test rewrite** (`UI::handle_click_event`): walk
    `portal_layers.iter_hit_test_order()` first; track
    `block_lower` from layer policies; fall through to base
    tree only if no Block policy gates it.
12. **`has_input_blocking_portal`**: walk only the
    `OverlayModal` layer.
13. **Build check**: green. Existing tests still failing.

### Day 3: test migration + new tests (~150 LOC tests + fixes)

14. **Migrate existing portal/focus_trap tests** that poke
    `ui.portal_layer` directly. The `make_portal` helper in
    `tests/focus_trap.rs` and the `entry()` helper need to
    set `layer: PortalLayer::OverlayModal` explicitly. Tests
    assertions on portal_layer length need updates.
15. **Write new tests** per the matrix below.
16. **Workspace test gate**: green.
17. **UL build check**: green.
18. **Manual smoke**: layer-mixing scenario in a test `.ogh`
    paints in the right order (tooltip above modal).

---

## Test matrix

`tests/portal.rs` extensions (existing file):

1. `portal_with_no_layer_property_defaults_to_overlay_modal` —
   parsing default.
2. `portal_layer_property_parses_each_named_layer` — round-trip
   each of the 6 names.
3. `portal_layer_property_rejects_unknown_layer_name` —
   `layer: "foo"` → BridgeError with the helpful message.
4. `portal_layer_property_rejects_non_string_value` —
   `layer: 42` → BridgeError.
5. `multiple_portals_in_same_layer_stack_lifo` — push two
   into Tooltip; iter_paint_order returns mount-order.
6. `multiple_portals_across_layers_paint_low_to_high` —
   push one in OverlayModal, one in Tooltip; iter_paint_order
   returns OverlayModal first (low priority), then Tooltip.
7. `iter_hit_test_order_walks_high_priority_first_then_lifo` —
   verifies the dual order.
8. `closed_portal_with_ghosts_still_routes_through_layer` —
   regression for Phase 2's M3 audit fix; ghost children
   still paint with the right layer assignment.

`tests/focus_trap.rs` (regressions):

9. `has_input_blocking_portal_only_walks_overlay_modal_layer` —
   focus_trap in Tooltip layer doesn't trip the gate.
10. `focus_trap_in_overlay_modal_does_trip_the_gate` —
    sanity that the OverlayModal-only walk still fires.

`tests/portal_coords.rs` (new):

11. `pass_b_translate_includes_parent_chain_translates` —
    capture viewport_rect from a portal nested 3 levels
    deep; verify it's the cumulative translate, not local.
12. `nested_portal_inside_translated_parent_renders_at_correct_viewport_position` —
    integration: parent translated by (50, 100); portal
    layout (10, 20); viewport_rect should be (60, 120, ...).
13. `portal_at_root_unchanged_from_phase_2_behavior` —
    when accumulated_translate is (0, 0), viewport_rect ==
    layout_rect.

`tests/portal_block_policy.rs` (new):

14. `overlay_modal_block_policy_swallows_clicks_to_main_tree` —
    open modal, click outside any portal child, click does
    not reach base tree.
15. `tooltip_none_policy_lets_clicks_fall_through` —
    open tooltip, click outside it, click reaches base tree.

Total: 15 tests. The Phase 2.5 plan estimated ~15 — matches.

---

## Risks + mitigations

| # | Risk | Mitigation |
|---|---|---|
| 1 | Test migration touches every test poking `ui.portal_layer` directly | Day 3 budget includes migration; helper functions in `focus_trap.rs` updated once cover most call sites |
| 2 | Backdrop policy semantics ambiguous in implementation (when does Block "trigger" vs just "intend to block"?) | Block triggers when ANY entry exists in the OverlayModal layer; tested by #14. The empty-layer case is a no-op |
| 3 | `accumulated_translate` doesn't account for `render_effects` transforms | Phase 2 transforms are render-only (don't affect layout); they don't shift portal slot positions. Document this; verify the assumption holds in test #11. If it doesn't, scope creep alert |
| 4 | Performance: PortalLayers is `[Vec; 6]` even when the page has zero portals | Vec::new() is no-alloc; the empty-state cost is 6 × `(usize, usize, *mut)` = ~144 bytes per UI. Negligible |
| 5 | Pass B Block-policy backdrop rendering overlaps with consumer-rendered backdrops (the Modal() example fn renders its own backdrop child) | Both render; backdrop child appears on top of the runtime backdrop. Visually fine but slightly wasteful. Could deprecate consumer backdrops for OverlayModal; punt to docs revision pass |
| 6 | LSP doesn't know about the new layer property — hover on `layer:` returns generic | M0 doesn't ship LSP changes for the layer property; defer to M5's docs work or a follow-up. Not a blocker for adoption |

---

## What M0 does NOT include

- LSP hover for the `layer` property (no `HoverInfo::Property`
  variant for layer-specific hover; falls back to generic
  property name hover).
- Drag-related layer interactions (Phase 3 territory).
- Per-portal cursor declaration — that's M1's job; Portal's
  `cursor` property is added there.
- `Dim` backdrop policy implementation — declared in the
  enum, defaults map exists, but Pass B treats it as None.
  Document. None of the 6 layers default to Dim, so this is
  a TODO not a regression.
- Focus_trap script API or runtime exposure — that's M2.

---

## Validation gate

- `cargo build --workspace` clean.
- `cargo test --workspace` green:
  - All Phase 2 tests pass (regression coverage).
  - All 15 new M0 tests pass.
  - Migrated test helpers work for both Phase 2 portal and
    new layer-aware portal fixtures.
- `cd ../untold_lore && cargo build` clean.
- Manual smoke: `Portal { layer: "tooltip", ... }` paints
  above `Portal { layer: "overlay-modal", ... }` in a test
  `.ogh`; click on the tooltip doesn't dismiss the modal
  beneath it (per Block + None policies); click on the
  modal's backdrop dismisses (per consumer-defined click
  handler in the Modal() example fn).

---

## What ships at M0

When the gate passes:

- `Portal` widget accepts `layer: "tooltip" | "popover" | ...`
  (defaulting to `"overlay-modal"`).
- `PortalLayer` and `BackdropPolicy` enums exposed in
  `widget::` module.
- `UI.portal_layers: PortalLayers` per-frame state.
- `PortalEntry.viewport_rect` is viewport-absolute (not
  parent-relative).
- Pass B paints layers low-priority-to-high; within a layer,
  mount order.
- Hit-test walks layers high-priority-to-low; backdrop policy
  Block gates fall-through to base tree.
- `Ogham::has_input_blocking_portal()` only walks
  `OverlayModal`.
- 15 new tests; all Phase 2 tests still pass.
- Documented limitations: Dim policy not implemented; LSP
  doesn't surface layer hover; drag/cursor/focus script API
  remain pending (M1/M2/Phase 3).
