# Ogham — Anchored portal entries: chrome at host-computed coordinates

> **Status: M0–M5 shipped 2026-08-06. M6 (the regency tooltip
> migration, §6) is outstanding and happens in that repo.**
>
> Where the shipped code and this plan differ, the code is authority;
> the divergences are listed in §4.1. The integration-facing writeup
> lives in [`AGENTS.md` → *Anchored portals*](../../AGENTS.md); the
> subsystem contract lives in
> [`LIFECYCLE_AND_PORTAL.md` → *The anchor contract*](LIFECYCLE_AND_PORTAL.md).
>
> Drafted 2026-08-06. Closes the second deferral in lorekeeper
> `docs/CANVAS_COMPOSITION.md` §3 ("Anchored portal entries (deferred)").
>
> This plan adds one Portal property, one `UI`/`Ogham` API pair, and a
> position override in one place in the Skia Pass-A walk. It changes no
> tenet, no language surface, and no existing widget.
>
> Companion to [`CANVAS_LEAF.md`](CANVAS_LEAF.md). The two features are
> **inverse seams** and are deliberately separate: a `Canvas` is host
> *paint* positioned by ogham *layout*; an anchored portal is ogham
> *content* positioned by host *geometry*. Neither substitutes for the
> other.

---

## 1. Why — three consumers, one shape

**Cursor-following chrome.** Regency's action tooltip
(`ManorScreen::draw_tooltip`, `regency-client/src/manor.rs:4558`) is
~90 lines of Skia that exists only because ogham chrome cannot follow
the pointer. It hand-rolls: multi-run coloured text on one line, a
rounded card with a hairline, edge clamping
(`x.min(w - card_w - 8.0).max(8.0)`), and flip-above-when-overflowing
(`if cy + 22.0 + card_h > h - 8.0`).

**World-anchored chrome.** `regency-sheet/src/manor.rs:2713`'s
`draw_label` paints room names at map coordinates; the crate has 15
direct text-draw sites. `CANVAS_COMPOSITION.md` names this exact case:

> **Build when** a live surface actually wants map-anchored chrome —
> most plausibly the player client's nameplates/status chips, or when a
> game's hand-painted map text starts re-hitting ogham-class text bugs
> (#4, #14, #17) that the ogham text stack has already fixed.

The second condition has effectively fired: regency has independently
reimplemented tracking (`draw_tracked_centered`), engraving
(`draw_engraved_centered`), centred layout, and word wrap
(`wheel.rs`'s `wrap()`) in `regency-sheet`, while ogham's own text stack
carries the fixes for LEARNINGS #4 (invisible centred text), #14
(intrinsic-width wrapping) and #17 (string escapes). **Two independently
buggy text stacks in one binary**, and only one of them is maintained.

**Popovers generally.** Lorekeeper's ref-picker
(`editor_host/data/ui/widgets.ogh`'s `ref_picker`) is mounted separately
by the host because a popover cannot be positioned at the field that
opened it.

All three want the same thing: *an ogham subtree, laid out and painted
by ogham, at a viewport position the host computes each frame.*

---

## 2. What already exists — the drag preview is the prototype

`skia.rs:930` already does this, for exactly one hardcoded case:

```rust
if let Some(preview_state) = ui.active_drag_preview().cloned() {
    ui.portal_layers.push(PortalEntry {
        widget: preview_state.preview.clone(),
        viewport_rect: Rect::new(
            preview_state.cursor.x(), preview_state.cursor.y(),
            preview_rect.width.max(0.0), preview_rect.height.max(0.0),
        ),
        layer: PortalLayer::CursorAttached,
        focus_trap: false,
        cursor: CursorPreference::Inherit,
    });
}
```

A host-held point becomes a `PortalEntry`'s viewport-absolute origin.
That is the whole mechanism. This plan **generalises it** from one
built-in consumer (drag previews) to a named-anchor map any Portal can
opt into.

Everything downstream already works off `viewport_rect`:

- `paint_portal_entry` (`skia.rs:1155`) translates by
  `viewport_rect.{x,y} * dpi` and recurses children with
  `accumulated_translate = viewport_rect.{x,y}`, so nested portals stay
  correct.
- Hit-testing walks `portal_layers.iter_hit_test_order()`
  (`widget/mod.rs:543`, `:801`, `:847`), so **anchoring fixes hit-testing
  for free** — an anchored tooltip is clickable where it is drawn.
- `UI::blocks_point` includes portal layers, so anchored chrome occludes
  world picking with no extra work.

There is no new coordinate space, no new paint path, and no new
hit-test path. There is one field to override and one place to override
it.

---

## 3. The design

### 3.1 `.ogh` surface

```ogh
Portal {
  layer: "tooltip",
  anchor: "action-tooltip",          // NEW: names a host-set anchor
  anchor_policy: "flip",             // NEW, optional: "clamp" | "flip" | "raw"
  anchor_offset: { x: 14, y: 22 },   // NEW, optional: applied before policy
  open: tooltip_open,
  children: [ ... ordinary ogham chrome ... ],
}
```

- **`anchor`** — a string id. When present, the entry's viewport origin
  comes from the host's anchor for that id instead of from Pass-A
  translate accumulation. When the id has no host-set anchor this frame,
  the portal **does not render** (§3.4) — the honest behaviour for
  "the thing I point at is gone."
- **`anchor_policy`** — how the anchor point is resolved against the
  viewport once the subtree's size is known:
  - `"raw"` — use the point as-is. Escape hatch; may go off-screen.
  - `"clamp"` (default) — clamp so the subtree's box stays inside the
    viewport with an 8 px inset.
  - `"flip"` — clamp horizontally; vertically, if the box would overrun
    the bottom, place it *above* the anchor instead. This is exactly
    regency's tooltip rule, and it is the one thing `.ogh` cannot express
    today because it needs the measured size.
- **`anchor_offset`** — a fixed nudge applied before policy, so a
  cursor tooltip sits below-right of the pointer rather than under it.

Absent `anchor`, a Portal behaves precisely as it does today. This is
purely additive.

### 3.2 Host surface

```rust
// Per frame, before `frame()`/`draw()`:
ogham.set_anchor("action-tooltip", cursor_x, cursor_y);
ogham.clear_anchor("hover-room-label");   // or just stop setting it
```

On `UI`:

```rust
pub fn set_anchor(&mut self, id: impl Into<String>, point: Point);
pub fn clear_anchor(&mut self, id: &str);
pub fn clear_anchors(&mut self);          // all
pub fn anchor(&self, id: &str) -> Option<Point>;
```

Storage: `UI.anchors: HashMap<String, Point>`.

**Anchors are host state, not frame state.** They persist until changed
or cleared — the same contract as injected host state, and the reason
a host that only moves a nameplate when the entity moves does not pay
per frame. `clear_lifecycle_state()` (hot reload) clears them, matching
INTENT §7: a reload drops what it cannot verify still means anything.

For world-space anchors the host projects world → screen itself and
calls `set_anchor`. Ogham does not learn about cameras, and must not.

### 3.3 Where the override happens

One place: `SkiaEnv::draw_widget_recursive`'s portal branch
(`skia.rs:1029-1069`). Today:

```rust
let viewport_rect = Rect::new(
    local_rect.x + accumulated_translate.0,
    local_rect.y + accumulated_translate.1,
    local_rect.width, local_rect.height,
);
```

Becomes: if `info.anchor` is `Some(id)` and `ui.anchors` has it, resolve
the origin from the anchor + offset + policy against `local_rect`'s
measured size and the viewport; otherwise the existing expression.

`PortalInfo` gains `anchor: Option<String>`, `anchor_policy: AnchorPolicy`,
`anchor_offset: (f32, f32)`. `PortalEntry` needs no new field — the
resolved position lands in `viewport_rect` exactly as before, which is
why paint, nesting, hit-test, focus and occlusion all follow for free.

**Size is available.** `local_rect` is the portal's laid-out rect from
the layout pass that already ran this frame, so `flip` and `clamp` have
a real measured height to work with. This is the piece `.ogh` cannot
reach and the reason the policy lives here rather than in the language.

### 3.4 Missing anchors, and other loud failures

Consistent with `CANVAS_LEAF.md` §3.5 and with the framework's standing
problem of silent degradation:

- **`anchor` names an id with no host anchor** → the entry is not
  pushed; the portal paints nothing this frame. This is correct
  behaviour, not an error (the anchored thing is gone), and it is
  documented. A debug-build one-shot log per id makes "my tooltip
  vanished" diagnosable.
- **`anchor` on a Portal with `focus_trap: true`** → rejected at build
  time. A focus-trapping modal that follows the cursor is a design
  error, and allowing it invites a modal the user cannot reach.
- **Unknown `anchor_policy` string** → `BridgeError::InvalidPropertyType`
  listing the three valid names. (Contrast `position: "relative"`, which
  parses and silently does nothing — the failure mode this framework
  keeps repeating and should stop.)

---

## 4. Milestones

**M0 — Anchor storage + API.** *(Shipped.)* (~70 LOC + 4 tests) `UI.anchors`;
`set_anchor` / `clear_anchor` / `clear_anchors` / `anchor`; `Ogham`
forwarders; cleared by `clear_lifecycle_state`. Tests: set/get/clear;
survives a rerender; cleared by hot reload.

**M1 — Portal properties + `PortalInfo`.** *(Shipped.)* (~90 LOC + 5 tests)
`anchor`, `anchor_policy`, `anchor_offset` parsed in `builder.rs`;
`AnchorPolicy` enum with `from_source_name` / `source_name` /
`all_names_for_diagnostic`, mirroring `PortalLayer`'s shape exactly.
Tests: round-trip parse; unknown policy errors; `anchor` +
`focus_trap` rejected; absent `anchor` unchanged.

**M2 — Resolution in Pass A.** *(Shipped.)* (~110 LOC + 8 tests) The override in
`draw_widget_recursive`; `resolve_anchor(point, offset, policy, size,
viewport) -> (f32, f32)` as a **pure free function** in
`portal_layer.rs` so the three policies are unit-testable without a
window. Tests: raw passes through; clamp holds the 8 px inset on all
four edges; flip goes above only when the bottom would overrun; flip
still clamps horizontally; a missing anchor skips the entry.

**M3 — Hit-test + occlusion parity.** *(Shipped.)* (~20 LOC + 5 tests) Mostly
verification, since hit-testing already reads `viewport_rect`. Tests:
a click at the anchored position reaches the portal's child; a click at
the portal's *declaration* site does not; `UI::blocks_point` is true
under an anchored tooltip; an anchored portal in `overlay-modal` still
honours `Block` backdrop policy.

**M4 — Retire the drag-preview special case.** *(Shipped.)* (~40 LOC, −30)
Re-express `skia.rs:930`'s synthesis on top of anchors: the UI sets a
reserved `"__drag_preview"` anchor and the existing path becomes an
ordinary anchored `CursorAttached` entry. **Behaviour-preserving**;
the existing `drag_preview.rs` tests are the acceptance suite. This
milestone is what proves the generalisation is actually general — if
it doesn't fit, the design is wrong and M0–M3 should be revisited.

**M5 — Docs.** *(Shipped.)* `AGENTS.md` gains an "Anchored portals" section under
the existing Portal docs (per `OGHAM_GAPS.md` §2: a feature absent from
`AGENTS.md` is a feature consumers will conclude does not exist);
`LIFECYCLE_AND_PORTAL.md` gains the anchor contract;
`examples/portals/anchored_tooltip.ogh`.

**M6 — Regency migration.** *(Outstanding.)* §6.

**Estimate:** ~330 LOC net + ~22 tests; ~1.5 person-days. Cheaper than
`CANVAS_LEAF.md` because the mechanism already exists and this
generalises it.

**Actual:** ~340 LOC of implementation + 40 tests in
`tests/anchored_portals.rs` + 3 unit tests in `portal_layer.rs`.

### 4.1 As built — deltas from this plan

Everything above landed as written except for the following. The plan
was drafted from a code read; these are the things building it surfaced.

1. **§3.3's "resolve … against `local_rect`'s measured size" is wrong,
   and it is the one substantive error in the plan.** A `PortalWidget`'s
   `get_layout_rect()` delegates to its *inner* `FlexWidget`, which
   `PortalWidget::new` sizes `Grow(1.0)` / `Grow(1.0)`. Its rect is
   therefore the whole available box, not the tooltip card. Clamping a
   viewport-sized box against the viewport pins every anchored portal to
   the inset corner — the feature would have looked completely broken
   while every unit test on `resolve_anchor` passed.

   As built, the policies resolve against the **children's extent**:
   `max(child.x + child.width)` × `max(child.y + child.height)` over the
   portal's children, computed by `SkiaEnv::children_extent`. Child
   layout rects are parent-relative, which is the invariant
   `draw_widget_recursive` already relies on. The unanchored path is
   untouched and still uses `local_rect`'s size.

   The claim §3.3 was *actually* reaching for still holds, and is the
   crux of the design: the size is available at Pass A because layout
   already ran this frame. It just isn't on the node the plan named.

2. **`PortalInfo` loses `Copy`.** `anchor: Option<String>` can't be
   `Copy`, and interning or borrowing the id to keep it would be
   ceremony for nothing — every consumer takes `as_portal()`'s return by
   value and reads fields off it. `#[derive(Clone, Debug)]` now.

3. **The `__` anchor-id prefix is reserved and rejected in `.ogh`.** Not
   in the plan. M4 puts the drag preview at `__drag_preview`; without
   the reservation a userspace `Portal { anchor: "__drag_preview" }`
   would silently attach itself to the drag cursor. Eight lines in the
   builder, and it makes M4's "reserved anchor" actually reserved.

4. **`resolve_anchor` treats a non-positive viewport dimension as "no
   viewport" and skips clamping on that axis.** Before the first layout
   pass the root has no rect, so the viewport reads `(0, 0)`; clamping
   against it would park everything at the inset corner, which reads as
   a positioning bug rather than as "not laid out yet".

5. **`flip` mirrors `anchor_offset.y` rather than dropping it**, and
   clamps the flipped result. A card flipped above the pointer clears it
   by the same margin it would have had below (`point.y - offset.y -
   height`), which is what regency's hand-rolled `cy - 22.0 - card_h`
   does; and a flipped card in a short viewport still gets pulled back
   under the top inset instead of off-screen. The plan specified neither.

6. **`Ogham::set_anchor` takes a `Point`, not two `f32`s.** §3.2's host
   example and its `UI` signature disagreed with each other; `Point`
   matches `dispatch_drag_start` and the rest of the pointer-facing API.

7. **`set_anchor` is change-gated.** Setting the same point twice is a
   no-op rather than a repaint, so a host that calls it unconditionally
   every frame doesn't force a repaint it didn't earn.

8. **`clear_lifecycle_state` clearing anchors is belt-and-braces, not
   the mechanism.** `Ogham::reload_file` builds a whole new `UI`, whose
   anchor map starts empty — anchors were already going to be dropped by
   a reload. The explicit clear makes the contract true for any other
   caller. The user-visible consequence is worth stating out loud
   (and now is, in `AGENTS.md`): a host that sets an anchor *once* must
   re-set it after a hot reload.

9. **M4 could not make the anchor map the sole source of truth.**
   `DragPreviewState.cursor` is a public field that `tests/drag_preview.rs`
   asserts on directly, and that suite was frozen. It survives as the
   host-facing read-back; `UI::seat_drag_preview` is the single writer
   for both it and the anchor, so they cannot drift. See the M4 note
   below.

10. **Two parameters, not one, threaded through the Pass-A walk.**
    Bundled as a `Copy` `AnchorContext<'a> { anchors, viewport }` in
    `skia.rs`, because `draw_widget_recursive` was already at six
    arguments and `paint_portal_entry` needs the same pair for portals
    nested inside anchored ones. The viewport read also had to move
    *above* the Pass-A walk (it was computed after it, for the Block
    backdrop) since anchored portals resolve during that walk.

**On M4, the falsification test.** It fit. The drag preview's synthesis
in `SkiaEnv::draw` now calls the same `AnchorContext::resolve` an `.ogh`
Portal does, with `AnchorPolicy::Raw` and no offset, reading the point
from `UI.anchors["__drag_preview"]`. `tests/drag_preview.rs` passes
unmodified. The one friction (delta 9) is a frozen public field, not a
mechanism mismatch: nothing about the *positioning* needed a second
path. The plan's claim that everything downstream follows from
`viewport_rect` also survived contact — hit-testing, `blocks_point`,
backdrop policy and portal nesting all work on anchored entries with
**zero** changes outside the one Pass-A branch, which M3 verifies end to
end against a real `SkiaEnv` rather than asserting by construction.

**On testing.** The plan (following `tests/portal_coords.rs`'s
precedent) assumed Pass-A behaviour couldn't be tested without a window.
It can: `skia_safe::surfaces::raster_n32_premul` gives an offscreen
`SkiaEnv`, as `tests/canvas_widget.rs` discovered. Every M2/M3 assertion
in `tests/anchored_portals.rs` therefore runs the real walk instead of
constructing the `PortalEntry` the walk was supposed to produce.

---

## 5. What this deliberately does not do

- **No anchoring to *widgets*.** `anchor` takes a host-supplied point,
  not "the widget with key `foo`". Widget-relative anchoring needs a
  measured-position query and a second layout dependency, and no
  consumer has asked. Popovers that want to sit under their trigger get
  the trigger's rect from the host today (it is in the event) or wait
  for the follow-up.
- **No collision detection against *other* portals.** Policies resolve
  against the viewport only. Two anchored tooltips overlapping is the
  host's problem.
- **No camera/projection awareness.** The host projects. Ogham must not
  learn what a camera is.
- **No inline text spans.** Regency's tooltip paints three coloured runs
  on one line; anchored portals let that become a row of three `Text`
  widgets, which is correct for a single line and wrong if it must wrap.
  Rich text spans are a separate, real gap
  (`stargazer-celia-game/docs/OGHAM_GAPS.md` should grow an entry) and
  are not in scope here.

---

## 6. Regency migration (M6)

**Scope: the action tooltip only.** The manor plan's world-space room
labels are a larger migration with a real risk of regressing the sheet's
typography; they get their own assessment after the tooltip proves the
mechanism at the table.

Steps:

1. **Project the tooltip's content as host state.** `self.tooltip:
   Option<(String, String, String)>` and `self.hover_label:
   Option<(String, String)>` already exist on `ManorScreen` and are
   already computed each frame. Add them to `chrome_state::ClientChrome`
   — they go through the existing `editable`→`Value` visitor in
   `ogh_value.rs` with no new glue.
2. **Set the anchor** from `self.cursor_px` in the per-frame chrome
   update: `ogham.set_anchor("action-tooltip", cx, cy)` when a tooltip
   is live, `clear_anchor` otherwise.
3. **Write the card in `client.ogh`** as a `Portal { layer: "tooltip",
   anchor: "action-tooltip", anchor_policy: "flip",
   anchor_offset: { x: 14, y: 22 } }` — the offset and policy reproduce
   `draw_tooltip`'s `cx + 14.0` / `cy + 22.0` and its flip rule exactly.
   The three coloured runs become a `direction: "row"` Flex of three
   `Text`s (single-line, so no wrap exposure).
4. **Delete** `ManorScreen::draw_tooltip` (~90 lines) and its call site
   at `manor.rs:3656`.
5. **Keep** the suppression rule — `draw_tooltip` returns early when
   `view.scene.is_some() || view.pending_roll.is_some()` ("a modal card
   owns the screen — nothing rides the pointer under the dim"). That is a
   game rule; it becomes the `open:` condition on the Portal.
6. **Verify the two typography losses are actually losses.** The card
   currently uses `font::Face::SmallCaps` at 20 px and `Face::Annotation`
   at 15 px. LEARNINGS #1 recorded "Ogham Text can't letterspace/emboss"
   as a reason to stay in Skia — **that is now false**; `letter_spacing`
   and outer shadows shipped in `c8a9425`. Confirm the ogham render is
   equivalent before deleting the Skia path, and record the result in
   regency's `LEARNINGS.md` either way. If it is *not* equivalent, that
   is a text-stack gap worth writing down rather than working around.

**Expected deletion:** ~90 lines of Skia, one hand-rolled edge-clamp,
one hand-rolled flip rule, and one more consumer of the second text
stack.
