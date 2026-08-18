//! UI library designed to render 2D GUIs using a flexbox-like layout system.
//! This framework draws inspiration from the document object model, CSS (in particular flexbox, as noted
//! above), and React's virtual DOM and reconciliation process.
//!
//! # The UI Object
//! Every UI has, at its core, an instance of the UI struct. This contains both the current render tree
//! (represented as a hierarchy of widgets) and centralized state such as a reference to the active
//! (focused) widget or cached images to avoid reloading images on each render. It provides methods
//! for interacting with widgets via events, such as clicks or key presses.
//!
//! # 2D Rendering
//! Rendering is currently handled through Skia and as such requires a Skia Surface to draw to. This is
//! subject to change at a later date, as tightly coupling the rendered output to a specific engine such
//! as Skia is not ideal and limits what contexts this framework can be used in.
//!
//! Despite shipping with Skia rendering supported by default, alternative solutions simply need to
//! implement Surface for their own backend in order to work with the framework.

/// Spring-driven style transitions.
pub mod animation;
/// Host-painted `Canvas` leaf: the `Painter` handle, the `CanvasPainter`
/// alias, and the widget that seats host paint inside flex layout.
pub mod canvas_widget;
/// Flexbox-like layout widget.
pub mod flex_widget;
/// Grid layout widget.
pub mod grid_widget;
pub mod image;
/// Image rendering widget.
pub mod image_widget;
/// Phase 2.5 M0: named portal layers + per-layer backdrop
/// policies. Portal widgets declare a layer; the renderer
/// dispatches to per-layer storage and paints in priority
/// order.
pub mod portal_layer;
/// Phase 2 Portal widget — defers paint + hit-test to the
/// per-frame `UI.portal_layers`, rendering in front of all
/// base-tree siblings.
pub mod portal_widget;
/// Lifecycle-sequencing container that holds one generation of content
/// at a time and waits for exits before mounting the next.
pub mod presence_widget;
/// Convenience macros for working with widgets.
#[macro_use]
pub mod event;
pub mod point;
pub mod rect;
pub mod style;
/// Text input field widget.
pub mod text_input_widget;
pub mod text_layout;
/// Text rendering widget.
pub mod text_widget;

/// Constructs UI widgets from runtime `Value::Widget` descriptors.
pub mod builder;
pub mod vocabulary;

use std::sync::{Arc, Mutex};

use skia_safe::textlayout::FontCollection;

use crate::widget::{
    event::{Event, EventContext},
    image::ImageCache,
    point::Point,
    style::{Border, Color, Corners, Direction, InnerGlow, Shadow, Size, TextStyle, Transform},
};

/// Context passed through the layout tree during a layout pass.
/// Carries the font collection and default font so that text widgets
/// can measure text without relying on thread-locals — plus the
/// shrink-measurement flags (see [`LayoutContext::measuring_width`]).
#[derive(Clone, Copy)]
pub struct LayoutContext<'a> {
    pub font_collection: Option<&'a FontCollection>,
    pub default_font: Option<&'a str>,
    /// While a `Shrink` parent measures its own WIDTH, `Grow`-width
    /// descendants resolve as `Shrink` (their content size). A shrink
    /// parent has no leftover space for a grow child to claim — without
    /// this, the child resolves against whatever budget the measurement
    /// happened to pass (historically an ancestor's size, ballooning the
    /// parent to the viewport). After measurement the parent's size is
    /// fixed, and the real `layout` pass stretches grow children into it
    /// as usual.
    pub measure_grow_width_as_shrink: bool,
    /// The height twin of [`measure_grow_width_as_shrink`].
    ///
    /// [`measure_grow_width_as_shrink`]: Self::measure_grow_width_as_shrink
    pub measure_grow_height_as_shrink: bool,
}

impl<'a> LayoutContext<'a> {
    /// This context, flagged for a parent measuring its shrink WIDTH:
    /// grow widths in the measured subtree resolve as content.
    pub fn measuring_width(&self) -> LayoutContext<'a> {
        LayoutContext {
            measure_grow_width_as_shrink: true,
            ..*self
        }
    }

    /// This context, flagged for a parent measuring its shrink HEIGHT.
    pub fn measuring_height(&self) -> LayoutContext<'a> {
        LayoutContext {
            measure_grow_height_as_shrink: true,
            ..*self
        }
    }

    /// A widget's effective width sizing under this context (grow reads
    /// as shrink while a shrink ancestor measures its width).
    pub fn effective_width(&self, size: Size) -> Size {
        match size {
            Size::Grow(_) if self.measure_grow_width_as_shrink => Size::Shrink,
            _ => size,
        }
    }

    /// The height twin of [`effective_width`](Self::effective_width).
    pub fn effective_height(&self, size: Size) -> Size {
        match size {
            Size::Grow(_) if self.measure_grow_height_as_shrink => Size::Shrink,
            _ => size,
        }
    }
}

/// Phase 2 Portal (extended in P25-M0): per-frame entry on
/// `UI.portal_layers`. The renderer pushes one of these
/// whenever it walks past a Portal node that should appear in
/// the portal layer (open, or open=false but with ghost
/// children mid-exit-animation). Pass B iterates the layers
/// in priority order and paints each entry's children with
/// the viewport as the clip rect.
#[derive(Clone)]
pub struct PortalEntry {
    pub widget: WidgetRef,
    /// VIEWPORT-ABSOLUTE rect of the portal's slot — captured
    /// during Pass A by accumulating parent translates. Pass
    /// B translates by `viewport_rect.{x, y}` from the
    /// viewport origin without further accumulation.
    ///
    /// Phase 2.5 M0 fix: previously this was parent-relative
    /// (`parent_rect`) and portals nested below the root
    /// rendered at the wrong viewport position. Renamed to
    /// reflect the new semantics.
    pub viewport_rect: rect::Rect,
    /// Which layer this entry belongs to. Determines paint
    /// priority and backdrop policy.
    pub layer: portal_layer::PortalLayer,
    pub focus_trap: bool,
    /// Phase 2.5 M1: cursor preference declared by the
    /// portal. Free → contributes to `wants_cursor_free`.
    /// Inherit → no influence.
    pub cursor: portal_layer::CursorPreference,
}

/// Phase 2 Portal (extended in P25-M0): returned by
/// `Widget::as_portal()` to mark a widget as a Portal. Used
/// by the renderer to detect the defer-to-portal-layer branch
/// and by the runtime API `has_input_blocking_portal()` to
/// derive UL's overlay-active boolean.
///
/// Not `Copy` since the `anchor` id landed here — every consumer
/// takes it by value out of `as_portal()` and reads fields off
/// it, so `Clone` is enough.
#[derive(Clone, Debug)]
pub struct PortalInfo {
    pub open: bool,
    pub focus_trap: bool,
    /// Which layer the Portal declares. Defaults to
    /// `OverlayModal` for Portals that don't specify a layer
    /// (matches Phase 2's single-layer behavior).
    pub layer: portal_layer::PortalLayer,
    /// Phase 2.5 M1: cursor preference. Defaults to the
    /// layer's `default_cursor()` if the Portal doesn't
    /// specify; can be overridden via the `cursor` property.
    pub cursor: portal_layer::CursorPreference,
    /// The host anchor id this Portal seats itself at, if it
    /// declared one. `Some(id)` switches the renderer from
    /// translate accumulation to [`UI::anchor`] lookup — and
    /// makes the Portal render *nothing* on frames where the id
    /// has no anchor.
    pub anchor: Option<String>,
    /// How [`Self::anchor`]'s point is seated against the
    /// viewport once the subtree's size is known. Ignored when
    /// `anchor` is `None`.
    pub anchor_policy: portal_layer::AnchorPolicy,
    /// Fixed `(x, y)` nudge applied to the anchor point *before*
    /// the policy, so cursor chrome can sit below-right of the
    /// pointer rather than under it. Ignored when `anchor` is
    /// `None`.
    pub anchor_offset: (f32, f32),
}

/// Phase 2.5 M0: per-frame storage for portal entries, keyed
/// by [`portal_layer::PortalLayer`]. Backed by a fixed-size
/// `[Vec; 6]` indexed by the layer enum's discriminant —
/// cache-friendly, no HashMap allocation, empty layers cost
/// nothing.
///
/// Cleared at the start of each render pass; populated during
/// Pass A walk; consumed by Pass B paint and hit-test.
#[derive(Clone, Default)]
pub struct PortalLayers {
    layers: [Vec<PortalEntry>; 6],
}

impl PortalLayers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Clear every layer's entries. Called at frame start.
    pub fn clear(&mut self) {
        for v in &mut self.layers {
            v.clear();
        }
    }

    /// Push an entry into its declared layer. Mount order is
    /// preserved within a layer.
    pub fn push(&mut self, entry: PortalEntry) {
        let idx = entry.layer.array_index();
        self.layers[idx].push(entry);
    }

    /// Iterate layers in PRIORITY order (low → high); within a
    /// layer, mount order. Used by Pass B paint — higher-
    /// priority layers paint on top.
    pub fn iter_paint_order(&self) -> impl Iterator<Item = &PortalEntry> {
        portal_layer::PortalLayer::ALL
            .iter()
            .flat_map(move |layer| self.layers[layer.array_index()].iter())
    }

    /// Iterate layers in REVERSE priority order (high → low);
    /// within a layer, reverse mount order (top-most-mount
    /// first). Used by hit-test — closest-to-cursor wins.
    pub fn iter_hit_test_order(&self) -> impl Iterator<Item = &PortalEntry> {
        portal_layer::PortalLayer::ALL
            .iter()
            .rev()
            .flat_map(move |layer| self.layers[layer.array_index()].iter().rev())
    }

    /// Entries in a specific layer (mount order). Used by
    /// `has_input_blocking_portal` (walks only `OverlayModal`).
    pub fn entries_in(&self, layer: portal_layer::PortalLayer) -> &[PortalEntry] {
        &self.layers[layer.array_index()]
    }

    /// True if any entry in any layer satisfies the predicate.
    pub fn any<P: Fn(&PortalEntry) -> bool>(&self, p: P) -> bool {
        self.layers.iter().any(|v| v.iter().any(&p))
    }

    /// Total entry count across all layers. Used by tests and
    /// debug assertions.
    pub fn len(&self) -> usize {
        self.layers.iter().map(|v| v.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.layers.iter().all(|v| v.is_empty())
    }

    /// Apply a predicate across every layer's entries. Used by
    /// tests that simulate "this portal closed" without going
    /// through the renderer.
    pub fn retain<F: FnMut(&PortalEntry) -> bool>(&mut self, mut pred: F) {
        for v in &mut self.layers {
            v.retain(&mut pred);
        }
    }
}

/// Phase 2 M4: a stack entry tracking what to restore when a
/// focus_trap portal unmounts. Pushed when a focus_trap portal
/// first appears in `UI.portal_layer`; popped when the portal
/// is no longer in the layer (closed or unmounted).
#[derive(Clone)]
pub struct FocusRestoration {
    /// Identifies which portal in the layer this restoration
    /// belongs to — matched by `Arc::ptr_eq` with the portal's
    /// `WidgetRef`.
    pub portal: WidgetRef,
    /// What `UI.focused` was when this portal mounted; restored
    /// when it pops.
    pub previous_focus: Option<WidgetRef>,
}

/// The UI root containing the widget tree and global state.
pub struct UI {
    /// The root element in the widget hierarchy.
    pub root: WidgetRef,
    /// Cached images to prevent reloading on render.
    pub image_cache: ImageCache,
    /// Phase 2 Portal (extended in P25-M0): per-frame portal
    /// layer storage, keyed by named layer. Cleared at start
    /// of each render pass; populated by the main render walk
    /// when it encounters open portals (or portals with mid-
    /// exit-animation ghosts); consumed by Pass B (Skia's
    /// `draw` and the hit-test path).
    pub portal_layers: PortalLayers,
    /// Host-set anchor points, keyed by the id an anchored
    /// `Portal` names in `anchor:`. The renderer resolves each
    /// anchored portal's viewport origin from this map instead
    /// of from Pass-A translate accumulation.
    ///
    /// **Anchors are host state, not frame state.** They persist
    /// until changed or cleared — the same contract as injected
    /// host state — so a host that only moves a nameplate when
    /// its entity moves doesn't pay per frame. An id with no
    /// entry means the anchored thing is gone, and its portal
    /// simply doesn't render (see `skia.rs`'s portal branch).
    ///
    /// Not `pub`: the renderer reads it as a sibling field
    /// alongside `portal_layers`, but hosts go through
    /// [`UI::set_anchor`] / [`UI::clear_anchor`] so the repaint
    /// marking can't be skipped.
    pub(crate) anchors: std::collections::HashMap<String, Point>,
    /// Phase 2 M4: focus restoration stack. Persists across
    /// frames; reconciled from `portal_layer` via
    /// `sync_focus_stack` after each render. Top of stack
    /// determines whether `try_set_focus` accepts a move.
    pub focus_stack: Vec<FocusRestoration>,
    /// Set when the widget tree structure or content changed and a full
    /// flexbox layout pass is required (expensive: involves Skia text
    /// measurement). Cleared by `layout()`.
    needs_layout: bool,
    /// Set when visual appearance changed (e.g. hover state) but widget
    /// sizes and positions are unaffected. The Skia draw pass runs every
    /// frame regardless, so this flag is informational — it does NOT gate
    /// any rendering.  Cleared by `layout()`.
    needs_repaint: bool,
    /// Currently-focused widget, if any.
    focused: Option<WidgetRef>,
    /// Font collection with registered custom fonts. Shared with the
    /// rendering backend and made available to widgets during layout via
    /// a thread-local.
    pub font_collection: Option<FontCollection>,
    /// Default font family applied to all text widgets that don't specify
    /// their own `font` in their style.
    pub default_font: Option<String>,
    /// Last dimensions passed to `layout()`. A layout pass is forced when
    /// the dimensions change even if `needs_layout` is false, because
    /// grow/shrink sizing depends on the available space.
    last_layout_width: f32,
    last_layout_height: f32,
    /// Debug-only: counts layout invocations per second to detect
    /// unnecessary dirty-marking regressions.
    #[cfg(debug_assertions)]
    layout_count: u32,
    #[cfg(debug_assertions)]
    layout_window_start: Option<std::time::Instant>,
    /// Debug-only: per-source breakdown of what dirtied the layout for
    /// the warning print. Indices: 0=rerender, 1=event, 2=animation, 3=dims.
    #[cfg(debug_assertions)]
    dirty_sources: [u32; 4],
    /// Debug-only: which source set `needs_layout = true` most recently.
    /// Used to attribute the next `layout()` call to the right bucket.
    #[cfg(debug_assertions)]
    last_dirty_source: u8,
    /// Phase 3 M0: path prefixes whose owning widgets drained
    /// during the most recent `tick_animations`. The runtime
    /// drains these via [`UI::take_drained_prefixes`] at the
    /// next render boundary so it can flush owned hooks /
    /// state cells / effects for the corresponding subtrees.
    pending_drained_prefixes: Vec<String>,
    /// Phase 3 M0: path prefixes whose pending unmount was
    /// cancelled (re-mount during exit animation) since the
    /// last drain. The runtime consumes these via
    /// [`UI::take_cancelled_unmount_prefixes`] to remove
    /// matching entries from any pending unmount queue.
    pending_cancelled_unmount_prefixes: Vec<String>,
    /// Phase 3 M2: in-flight drag preview state. When `Some`,
    /// the renderer pushes the originator's `drag_preview()`
    /// subtree into the `CursorAttached` portal layer
    /// positioned at `(point.x, point.y)`. Set by
    /// `dispatch_drag_start`, updated by `dispatch_drag_move`,
    /// cleared by `dispatch_drag_end`. `None` outside of an
    /// active drag.
    active_drag_preview: Option<DragPreviewState>,
    /// The last pointer position `call_event` saw (`mouse_move`), in the
    /// window coordinates layout runs in; `None` until the first move.
    /// Hosts read it for pointer-anchored effects (a backdrop's parallax,
    /// a spotlight) without shadowing the event stream themselves.
    last_mouse: Option<Point>,
}

/// The anchor id the runtime keeps its own drag preview at.
///
/// The drag preview was the prototype anchoring generalised from
/// (one host-held point becoming a `PortalEntry`'s viewport
/// origin); since M4 it *is* an anchored entry, seated through
/// exactly the path a `Portal { anchor: … }` uses. Nothing about
/// it is special-cased in the renderer any more.
///
/// The `__` prefix is reserved: the builder rejects `.ogh`
/// anchors starting with it, so no userspace Portal can attach
/// itself to the drag cursor by naming this id.
pub const DRAG_PREVIEW_ANCHOR: &str = "__drag_preview";

/// Phase 3 M2: state captured by `UI` during an in-flight
/// drag so the renderer can paint a drag preview attached to
/// the cursor each frame.
#[derive(Clone)]
pub struct DragPreviewState {
    /// Subtree to render. Snapshot of the originator's
    /// `drag_preview()` at `dispatch_drag_start` time so the
    /// preview survives the originator being unmounted
    /// mid-drag.
    pub preview: WidgetRef,
    /// Current cursor position. Updated each
    /// `dispatch_drag_move`.
    ///
    /// Since M4 the *renderer* reads the position from the
    /// [`DRAG_PREVIEW_ANCHOR`] anchor rather than from here; this
    /// field remains the host-facing read-back of the same point
    /// (`active_drag_preview().cursor`), written alongside the
    /// anchor by [`UI::seat_drag_preview`].
    pub cursor: Point,
}

impl UI {
    pub fn new(root: WidgetRef) -> Self {
        Self {
            root,
            image_cache: ImageCache::new(),
            portal_layers: PortalLayers::new(),
            anchors: std::collections::HashMap::new(),
            focus_stack: Vec::new(),
            needs_layout: true,
            needs_repaint: true,
            focused: None,
            font_collection: None,
            default_font: None,
            last_layout_width: 0.0,
            last_layout_height: 0.0,
            #[cfg(debug_assertions)]
            layout_count: 0,
            #[cfg(debug_assertions)]
            layout_window_start: None,
            #[cfg(debug_assertions)]
            dirty_sources: [0; 4],
            #[cfg(debug_assertions)]
            last_dirty_source: 0,
            pending_drained_prefixes: Vec::new(),
            pending_cancelled_unmount_prefixes: Vec::new(),
            active_drag_preview: None,
            last_mouse: None,
        }
    }

    /// The last pointer position seen by `call_event` (`mouse_move`);
    /// `None` until the pointer first moves over the window.
    pub fn last_mouse(&self) -> Option<&Point> {
        self.last_mouse.as_ref()
    }

    /// Place the anchor `id` at `point`, in the same viewport
    /// coordinates layout runs in. Any `Portal { anchor: id }`
    /// seats its subtree there on the next draw.
    ///
    /// Idempotent and cheap: setting the same point again is a
    /// no-op, so a host that calls this unconditionally every
    /// frame doesn't force a repaint it didn't earn.
    ///
    /// For world-anchored chrome the host projects world → screen
    /// itself and passes the result. Ogham does not learn about
    /// cameras, and must not.
    pub fn set_anchor(&mut self, id: impl Into<String>, point: Point) {
        let id = id.into();
        let moved = match self.anchors.get(&id) {
            Some(existing) => existing.x() != point.x() || existing.y() != point.y(),
            None => true,
        };
        if moved {
            self.anchors.insert(id, point);
            // Position-only change: the subtree's layout is
            // unaffected, only where Pass B seats it. Same
            // reasoning as `dispatch_drag_move`.
            self.mark_needs_repaint();
        }
    }

    /// Drop the anchor `id`. Portals naming it stop rendering
    /// until it is set again — the honest behaviour for "the
    /// thing I point at is gone". Idempotent.
    pub fn clear_anchor(&mut self, id: &str) {
        if self.anchors.remove(id).is_some() {
            self.mark_needs_repaint();
        }
    }

    /// Drop every anchor. For hosts tearing down a screen whose
    /// anchors all became meaningless at once.
    pub fn clear_anchors(&mut self) {
        if !self.anchors.is_empty() {
            self.anchors.clear();
            self.mark_needs_repaint();
        }
    }

    /// The point currently set for anchor `id`, if any. Mostly
    /// for hosts that want to read back what they set (and for
    /// tests); the renderer reads the map directly.
    pub fn anchor(&self, id: &str) -> Option<Point> {
        self.anchors.get(id).cloned()
    }

    /// Phase 3 M2: read-only access to the in-flight drag
    /// preview (if any). Renderers consult this each frame
    /// and push the preview into the `CursorAttached` layer
    /// at the cursor position.
    pub fn active_drag_preview(&self) -> Option<&DragPreviewState> {
        self.active_drag_preview.as_ref()
    }

    /// Drop any in-flight drag preview. Used by hosts that need to
    /// abort a drag without firing `drag_end` — e.g., when an
    /// orchestrator swaps which Ogham is active mid-drag. Idempotent.
    pub fn clear_active_drag(&mut self) {
        if self.active_drag_preview.is_some() {
            self.active_drag_preview = None;
            self.mark_needs_repaint();
        }
        self.clear_anchor(DRAG_PREVIEW_ANCHOR);
    }

    /// Move the drag preview to `point`: the reserved anchor the
    /// renderer seats it from, plus the host-facing read-back on
    /// [`DragPreviewState::cursor`].
    ///
    /// The single writer for both, so the two can't drift. They
    /// are two views of one point rather than two sources of
    /// truth — the anchor is what draws, `cursor` is what
    /// `active_drag_preview()` reports.
    fn seat_drag_preview(&mut self, point: Point) {
        if let Some(preview) = self.active_drag_preview.as_mut() {
            preview.cursor = point.clone();
        }
        self.set_anchor(DRAG_PREVIEW_ANCHOR, point);
    }

    pub fn set_font_collection(&mut self, fc: FontCollection) {
        self.font_collection = Some(fc);
    }

    pub fn set_default_font(&mut self, name: String) {
        self.default_font = Some(name);
    }

    pub fn call_event(&mut self, event: &Event) -> bool {
        if event.name == "mouse_move" {
            if let Some(point) = &event.point {
                self.last_mouse = Some(point.clone());
                let changed = self.update_hover(point);
                if changed {
                    // Hover only affects visual appearance (effective_style in
                    // render), not widget sizes or positions. A repaint is
                    // sufficient — no layout pass needed.
                    self.mark_needs_repaint();
                }
                return changed;
            }
            return false;
        }

        if let Some(point) = &event.point {
            // For click events, clear focus before handling
            // Create context without focused widget since we're clearing focus
            let mut ctx = EventContext::new();
            self.focused = None;

            // For click events, we need to find all widgets that contain the point
            // and call their event handlers in order from child to parent
            let handled = self.handle_click_event(event, point, &mut ctx);

            // Process focus request from context. Phase 2 M4:
            // route through try_set_focus so a focus_trap
            // portal can reject moves outside its subtree.
            if let Some(focus_target) = ctx.take_focus_request() {
                self.try_set_focus(focus_target);
            }

            if handled {
                self.mark_after_event(&ctx);
            }
            handled
        } else {
            // Focus-traversal keys are handled at the UI level, but only while
            // an input is focused — so Tab / Escape pass through to the host
            // (game hotkeys etc.) when the user isn't in a field. Mirrors the
            // character-key gate (`consumes_character_key`).
            if self.focused.is_some() {
                if let Some(kb) = event
                    .keyboard_data
                    .as_ref()
                    .filter(|_| event.name == "keydown")
                {
                    match kb.key_code {
                        // Tab / Shift-Tab → move focus through the ring.
                        Some(9) => {
                            self.focus_next(kb.modifiers.shift);
                            self.mark_needs_repaint();
                            return true;
                        }
                        // Escape → blur the focused field.
                        Some(27) => {
                            self.focused = None;
                            self.mark_needs_repaint();
                            return true;
                        }
                        _ => {}
                    }
                }
            }

            // For non-click events, pass the focused widget to the context
            // so widgets can check if they're focused
            let mut ctx = EventContext::with_focused(self.focused.clone());

            // The root widget (FlexWidget) will handle propagating to its children
            let mut root = self.root.lock().expect("widget lock poisoned");
            let handled = root.handle_event(event, &mut ctx, &self.root.clone());
            drop(root);

            // Process focus request from context. Phase 2 M4:
            // route through try_set_focus so a focus_trap
            // portal can reject moves outside its subtree.
            if let Some(focus_target) = ctx.take_focus_request() {
                self.try_set_focus(focus_target);
            }

            if handled {
                self.mark_after_event(&ctx);
            }
            handled
        }
    }

    /// Choose between layout-invalidation and repaint-only after a handled
    /// event, based on whether any widget reported a layout-affecting
    /// mutation via `EventContext::request_layout`. Most listeners just
    /// fire host events — the host's response flows back through reconcile,
    /// so a same-frame layout pass is unnecessary. Today only a `TextInput`
    /// with `width: Shrink` ingesting a character opts in.
    fn mark_after_event(&mut self, ctx: &EventContext) {
        if ctx.needs_layout {
            self.mark_dirty();
        } else {
            self.mark_needs_repaint();
        }
    }

    fn handle_click_event(&mut self, event: &Event, point: &Point, ctx: &mut EventContext) -> bool {
        // Phase 2.5 M0: walk portal_layers high-priority-to-low,
        // within a layer reverse-mount-order (top-most-mount
        // first). Track block_lower from layer policies; if
        // any layer with a Block policy has any open entry,
        // fall-through to the base tree is suppressed (per
        // UI_RUNTIME.md §1's "lower layers receive nothing if
        // the topmost layer's policy is `block`").
        let mut block_lower = false;
        // Use the convenience entries() collection rather than
        // borrowing self in a closure — handle_event needs &mut.
        let entries: Vec<PortalEntry> = self.portal_layers.iter_hit_test_order().cloned().collect();
        for entry in &entries {
            // Translate the click into the portal's child
            // coordinate space — viewport_rect is now
            // viewport-absolute (P25-M0), so this subtraction
            // gives the child-relative point directly.
            let child_point = Point::new(
                point.x() - entry.viewport_rect.x,
                point.y() - entry.viewport_rect.y,
            );
            let widget_ref = entry.widget.clone();
            let mut widget = widget_ref.lock().expect("widget lock poisoned");
            // Check children directly because the Portal node
            // itself returns false from contains_point.
            let children = widget.get_children_mut();
            drop(widget);
            for child in &children {
                let mut g = child.lock().expect("widget lock poisoned");
                if g.contains_point(&child_point) {
                    let handled = g.handle_event(event, ctx, child);
                    if handled {
                        return true;
                    }
                }
            }
            // Layer-policy gate: a Block-policy layer with any
            // open entry suppresses fall-through to the base
            // tree. Even if no specific child claimed the
            // click, the modal "swallows" it.
            if entry.layer.default_backdrop() == portal_layer::BackdropPolicy::Block {
                block_lower = true;
            }
        }

        if block_lower {
            return false;
        }

        // Fall through to the base tree.
        let mut root = self.root.lock().expect("widget lock poisoned");
        if root.contains_point(point) {
            return root.handle_event(event, ctx, &self.root.clone());
        }
        false
    }

    /// Phase 3 M1: dispatch `drag_start` directly on the
    /// originating widget. Constructs a [`event::DragState`]
    /// seeded from `payload` + `point`, fires the event, and
    /// returns the seeded DragState so the host's input pump
    /// can thread it through subsequent `dispatch_drag_move`
    /// / `dispatch_drag_end` calls.
    pub fn dispatch_drag_start(
        &mut self,
        origin: WidgetRef,
        payload: crate::runtime::value::Value,
        point: Point,
    ) -> event::DragState {
        let drag_state = event::DragState {
            origin_widget: Some(origin.clone()),
            payload: Some(payload.clone()),
            start_position: point.clone(),
            current_position: point.clone(),
            past_dead_zone: true,
        };
        let preview_widget = {
            let g = origin.lock().expect("widget lock poisoned");
            g.drag_preview()
        };
        if let Some(preview) = preview_widget {
            self.active_drag_preview = Some(DragPreviewState {
                preview,
                cursor: point.clone(),
            });
            self.seat_drag_preview(point.clone());
            // Preview needs an initial layout pass and a
            // repaint to appear on screen.
            self.mark_needs_layout();
        }
        let event = Event::drag("drag_start", point, Some(payload));
        let g = origin.lock().expect("widget lock poisoned");
        g.fire_event_listener(&event);
        drop(g);
        drag_state
    }

    /// Phase 3 M1: dispatch `drag_move` on the deepest widget
    /// under `point`. Updates `state.current_position` in place.
    /// Returns the widget that received the event, if any.
    pub fn dispatch_drag_move(
        &mut self,
        state: &mut event::DragState,
        point: Point,
    ) -> Option<WidgetRef> {
        state.current_position = point.clone();
        if self.active_drag_preview.is_some() {
            // Cursor changed — paint pass needs to re-emit
            // the CursorAttached entry at the new
            // viewport_rect. Layout doesn't have to re-run
            // (preview's intrinsic size is unchanged);
            // `set_anchor` marks the repaint.
            self.seat_drag_preview(point.clone());
        }
        let payload = state
            .payload
            .clone()
            .unwrap_or(crate::runtime::value::Value::Void);
        let target = self.hit_test_drag_target(&point);
        if let Some(target_ref) = target.clone() {
            let event = Event::drag("drag_move", point, Some(payload));
            let g = target_ref.lock().expect("widget lock poisoned");
            g.fire_event_listener(&event);
        }
        target
    }

    /// Phase 3 M1: dispatch `drag_end`. Walks portal layers
    /// (high priority → low) then the base tree to find the
    /// deepest widget whose `accepts_drop(payload)` is true;
    /// fires `drag_end` on that widget. Falls back to the
    /// originator if no target accepts. Returns the widget
    /// that received `drag_end`, if any.
    pub fn dispatch_drag_end(
        &mut self,
        state: &mut event::DragState,
        point: Point,
    ) -> Option<WidgetRef> {
        state.current_position = point.clone();
        let payload = state
            .payload
            .clone()
            .unwrap_or(crate::runtime::value::Value::Void);
        let target = self
            .hit_test_drop_target(&payload, &point)
            .or_else(|| state.origin_widget.clone());
        if let Some(target_ref) = target.clone() {
            let event = Event::drag("drag_end", point, Some(payload));
            let g = target_ref.lock().expect("widget lock poisoned");
            g.fire_event_listener(&event);
            drop(g);
        }
        // Drag is over — clear the preview so the renderer
        // stops painting it. Repaint so the cursor-attached
        // entry disappears from the next frame.
        let had_preview = self.active_drag_preview.is_some();
        self.active_drag_preview = None;
        if had_preview {
            self.mark_needs_repaint();
        }
        // The preview's anchor goes with it; a stale
        // `__drag_preview` point would otherwise outlive the
        // drag that set it.
        self.clear_anchor(DRAG_PREVIEW_ANCHOR);
        target
    }

    /// Phase 3 M2: dispatch a `contextmenu` event on the
    /// deepest widget at `point`. Returns `true` if a listener
    /// fired. Hosts wire this to right-click in their input
    /// pump; the event is distinct from `mouse_down` so left-
    /// vs right-click handling can diverge cleanly.
    /// Whether a pointer press at `point` would be consumed by the chrome:
    /// the side-effect-free twin of `call_event("mouse_down", …)` — no
    /// listeners fire, no focus moves. Hosts that mix ogham chrome with
    /// their own canvas-layer hit-testing (world hover, tooltips, click
    /// selection) should gate that layer on this, or the world keeps
    /// reacting underneath panels. Checks portal layers in hit-test order,
    /// then the base tree.
    pub fn blocks_point(&self, point: &Point) -> bool {
        for entry in self.portal_layers.iter_hit_test_order() {
            let child_point = Point::new(
                point.x() - entry.viewport_rect.x,
                point.y() - entry.viewport_rect.y,
            );
            let widget = entry.widget.lock().expect("widget lock poisoned");
            for child in widget.get_children() {
                if child
                    .lock()
                    .expect("widget lock poisoned")
                    .blocks_point(&child_point)
                {
                    return true;
                }
            }
        }
        self.root
            .lock()
            .expect("widget lock poisoned")
            .blocks_point(point)
    }

    /// The pointer role under the cursor right now — the CSS `cursor` of
    /// the deepest *hovered* widget (leaf-wins), or `Default` where the
    /// chrome declares nothing. Reads the hover chain [`update_hover`]
    /// already tagged from the last `mouse_move`, so it stays current
    /// wherever pointer events are routed into this UI (the host needs no
    /// cursor coordinate of its own). The companion to [`hovered_blocks`]:
    /// a host reads this to decide whether the chrome under the pointer
    /// wants the interactive glyph, and `hovered_blocks` to know whether
    /// the world underneath still gets a vote.
    ///
    /// [`update_hover`]: Self::update_hover
    /// [`hovered_blocks`]: Self::hovered_blocks
    pub fn hovered_cursor(&self) -> style::CursorRole {
        fn walk(widget: &WidgetRef) -> style::CursorRole {
            let g = widget.lock().expect("widget lock poisoned");
            if !g.is_hovered() {
                return style::CursorRole::Default;
            }
            let own = g.declared_cursor();
            let children = g.get_children();
            drop(g);
            // Leaf-wins: a deeper hovered descendant overrides our own.
            for child in &children {
                let deeper = walk(child);
                if deeper != style::CursorRole::Default {
                    return deeper;
                }
            }
            own
        }
        walk(&self.root)
    }

    /// Whether the chrome under the pointer would consume a press — any
    /// widget in the hovered chain that `block_interactions` or carries a
    /// pointer listener. The chain twin of [`blocks_point`](Self::blocks_point)
    /// (no coordinate needed), for hosts that gate world hover on the hover
    /// state this UI already tracks.
    pub fn hovered_blocks(&self) -> bool {
        fn walk(widget: &WidgetRef) -> bool {
            let g = widget.lock().expect("widget lock poisoned");
            if !g.is_hovered() {
                return false;
            }
            if g.blocks_interactions() {
                return true;
            }
            let children = g.get_children();
            drop(g);
            children.iter().any(|child| walk(child))
        }
        walk(&self.root)
    }

    pub fn dispatch_contextmenu(&mut self, point: Point) -> bool {
        let target = self.hit_test_drag_target(&point);
        if let Some(target_ref) = target {
            let mut event = Event::with_point("contextmenu".to_string(), point);
            event.payload = None;
            let g = target_ref.lock().expect("widget lock poisoned");
            g.fire_event_listener(&event)
        } else {
            false
        }
    }

    /// Phase 3 M1: find the deepest widget at `point`,
    /// walking portal layers high→low then the base tree.
    /// Used by `dispatch_drag_move` to deliver hover-style
    /// drag_move dispatches to the widget under the cursor
    /// regardless of whether it accepts the drop.
    pub fn hit_test_drag_target(&self, point: &Point) -> Option<WidgetRef> {
        let entries: Vec<PortalEntry> = self.portal_layers.iter_hit_test_order().cloned().collect();
        for entry in &entries {
            let child_point = Point::new(
                point.x() - entry.viewport_rect.x,
                point.y() - entry.viewport_rect.y,
            );
            let children = {
                let widget = entry.widget.lock().expect("widget lock poisoned");
                widget.get_children()
            };
            for child in &children {
                let contains = {
                    let g = child.lock().expect("widget lock poisoned");
                    g.contains_point(&child_point)
                };
                if contains {
                    if let Some(deep) = Self::deepest_at(child, &child_point) {
                        return Some(deep);
                    }
                    return Some(child.clone());
                }
            }
            if entry.layer.default_backdrop() == portal_layer::BackdropPolicy::Block {
                return None;
            }
        }
        let root = self.root.clone();
        let contains = {
            let g = root.lock().expect("widget lock poisoned");
            g.contains_point(point)
        };
        if contains {
            Self::deepest_at(&root, point).or(Some(root))
        } else {
            None
        }
    }

    /// Phase 3 M1: like `hit_test_drag_target` but only returns
    /// widgets whose `accepts_drop(payload)` returns true.
    /// `dispatch_drag_end` uses this to pick the drop target.
    pub fn hit_test_drop_target(
        &self,
        payload: &crate::runtime::value::Value,
        point: &Point,
    ) -> Option<WidgetRef> {
        let entries: Vec<PortalEntry> = self.portal_layers.iter_hit_test_order().cloned().collect();
        for entry in &entries {
            let child_point = Point::new(
                point.x() - entry.viewport_rect.x,
                point.y() - entry.viewport_rect.y,
            );
            let children = {
                let widget = entry.widget.lock().expect("widget lock poisoned");
                widget.get_children()
            };
            for child in &children {
                let contains = {
                    let g = child.lock().expect("widget lock poisoned");
                    g.contains_point(&child_point)
                };
                if contains {
                    if let Some(target) = Self::deepest_drop_target(child, &child_point, payload) {
                        return Some(target);
                    }
                }
            }
            if entry.layer.default_backdrop() == portal_layer::BackdropPolicy::Block {
                return None;
            }
        }
        let root = self.root.clone();
        let contains = {
            let g = root.lock().expect("widget lock poisoned");
            g.contains_point(point)
        };
        if contains {
            Self::deepest_drop_target(&root, point, payload)
        } else {
            None
        }
    }

    /// Recurse into `widget` looking for the deepest descendant
    /// whose layout rect contains `point`. Returns `None` only
    /// if none of `widget`'s descendants contain the point.
    fn deepest_at(widget: &WidgetRef, point: &Point) -> Option<WidgetRef> {
        let (origin, children) = {
            let g = widget.lock().expect("widget lock poisoned");
            let o = g
                .get_layout_rect()
                .map(|r| (r.x, r.y))
                .unwrap_or((0.0, 0.0));
            (o, g.get_children())
        };
        let local = Point::new(point.x() - origin.0, point.y() - origin.1);
        for child in children.iter().rev() {
            let contains = {
                let cg = child.lock().expect("widget lock poisoned");
                // Exiting widgets are hit-test-invisible
                // (PRESENCE_POP.md §6).
                !cg.is_exiting() && cg.contains_point(&local)
            };
            if contains {
                if let Some(deeper) = Self::deepest_at(child, &local) {
                    return Some(deeper);
                }
                return Some(child.clone());
            }
        }
        None
    }

    /// Recurse into `widget` looking for the deepest descendant
    /// whose `accepts_drop(payload)` is true and whose layout
    /// rect contains `point`. Returns the widget itself if it
    /// is the only acceptor.
    fn deepest_drop_target(
        widget: &WidgetRef,
        point: &Point,
        payload: &crate::runtime::value::Value,
    ) -> Option<WidgetRef> {
        let (origin, children, self_accepts) = {
            let g = widget.lock().expect("widget lock poisoned");
            let o = g
                .get_layout_rect()
                .map(|r| (r.x, r.y))
                .unwrap_or((0.0, 0.0));
            (o, g.get_children(), g.accepts_drop(payload))
        };
        let local = Point::new(point.x() - origin.0, point.y() - origin.1);
        for child in children.iter().rev() {
            let contains = {
                let cg = child.lock().expect("widget lock poisoned");
                // Exiting widgets are hit-test-invisible
                // (PRESENCE_POP.md §6).
                !cg.is_exiting() && cg.contains_point(&local)
            };
            if contains {
                if let Some(deeper) = Self::deepest_drop_target(child, &local, payload) {
                    return Some(deeper);
                }
            }
        }
        if self_accepts {
            Some(widget.clone())
        } else {
            None
        }
    }

    /// Walk the widget tree and set `hovered = true` on every widget in the
    /// path from the root to the deepest widget that contains `point`.
    /// All other widgets are set to `hovered = false`. Returns `true` if
    /// any widget's hover state changed.
    fn update_hover(&mut self, point: &Point) -> bool {
        let root = self.root.clone();
        Self::update_hover_recursive(&root, point, false)
    }

    fn update_hover_recursive(widget_ref: &WidgetRef, point: &Point, suppressed: bool) -> bool {
        let mut widget = widget_ref.lock().expect("widget lock poisoned");
        // Exiting subtrees are hit-test-invisible (PRESENCE_POP.md §6):
        // suppress hover for this widget and everything below it so a
        // ghost can't steal hover from the live content it overlaps.
        // Suppression still recurses — descendants that were hovered
        // before the exit began get their hover cleared (and their
        // mouse_leave fired) like any other miss.
        let suppressed = suppressed || widget.is_exiting();
        let hit = !suppressed && widget.contains_point(point);

        let was_hovered = widget.is_hovered();
        widget.set_hovered(hit);
        let mut changed = was_hovered != hit;

        if !was_hovered && hit {
            let event = Event::new("mouse_enter".to_string());
            widget.fire_listeners("mouse_enter", &event);
        } else if was_hovered && !hit {
            let event = Event::new("mouse_leave".to_string());
            widget.fire_listeners("mouse_leave", &event);
        }

        // Transform the point into this widget's own content coordinate
        // space before recursing: subtract its origin and add any scroll
        // offset, mirroring the canvas translate in the render walker.
        let origin = widget
            .get_layout_rect()
            .map(|r| (r.x, r.y))
            .unwrap_or((0.0, 0.0));
        let (scroll_x, scroll_y) = widget.scroll_offset();
        let child_point = Point::new(
            point.x() - origin.0 + scroll_x,
            point.y() - origin.1 + scroll_y,
        );

        let children = widget.get_children_mut();
        drop(widget);

        for child in &children {
            changed |= Self::update_hover_recursive(child, &child_point, suppressed);
        }

        changed
    }

    /// Updates the bounds of widgets in the hierarchy within the constraints provided (typically the screen size).
    pub fn layout(&mut self, width: f32, height: f32) {
        let dims_changed = self.last_layout_width != width || self.last_layout_height != height;
        if !self.needs_layout && !dims_changed {
            return;
        }
        #[cfg(debug_assertions)]
        let attributed_source = if dims_changed && !self.needs_layout {
            3 // dims-only
        } else {
            self.last_dirty_source as usize
        };
        self.needs_layout = false;
        self.needs_repaint = false;
        self.last_layout_width = width;
        self.last_layout_height = height;

        #[cfg(debug_assertions)]
        {
            let now = std::time::Instant::now();
            let start = self.layout_window_start.get_or_insert(now);
            self.layout_count += 1;
            self.dirty_sources[attributed_source.min(3)] += 1;
            if now.duration_since(*start).as_secs_f32() >= 1.0 {
                if self.layout_count > 5 {
                    eprintln!(
                        "[ogham] WARNING: layout() called {} times in the last second \
                         (rerender={}, event={}, anim={}, dims={}) \
                         — check for unnecessary dirty-marking",
                        self.layout_count,
                        self.dirty_sources[0],
                        self.dirty_sources[1],
                        self.dirty_sources[2],
                        self.dirty_sources[3],
                    );
                }
                self.layout_count = 0;
                self.dirty_sources = [0; 4];
                self.layout_window_start = Some(now);
            }
        }

        let ctx = LayoutContext {
            font_collection: self.font_collection.as_ref(),
            default_font: self.default_font.as_deref(),
            measure_grow_width_as_shrink: false,
            measure_grow_height_as_shrink: false,
        };

        let mut root = self.root.lock().expect("widget lock poisoned");
        root.layout(
            &ctx,
            0.0,
            0.0,
            &Direction::Column,
            width,
            width,
            height,
            height,
            0.0,
        );
        drop(root);

        // Phase 3 M2: lay out the in-flight drag preview (if
        // any) at the local origin (0,0) sized at its declared
        // dimensions. The Skia paint path translates the
        // preview by the cursor position via the synthesized
        // CursorAttached PortalEntry's viewport_rect; laying
        // out at (0,0) keeps the preview's own layout rect
        // relative so paint doesn't double-offset by the
        // cursor. Authors are expected to give drag_preview
        // widgets explicit width/height (or grow within a
        // fixed-size wrapper); auto-sizing against an
        // unbounded viewport is undefined.
        if let Some(state) = self.active_drag_preview.as_ref() {
            let preview_ref = state.preview.clone();
            let mut preview = preview_ref.lock().expect("widget lock poisoned");
            let pw = preview.get_fixed_width().unwrap_or(0.0);
            let ph = preview.get_fixed_height().unwrap_or(0.0);
            preview.layout(&ctx, 0.0, 0.0, &Direction::Column, pw, pw, ph, ph, 0.0);
        }
    }

    /// Reconcile the current hierarchy with a newly-provided hierarchy.
    /// Elements that are matched (of the same type) will be updated in place,
    /// whereas elements that are not matched (did not exist in the previous
    /// hierarchy or are of incompatible types) will be replaced along with
    /// all of their descendants.
    ///
    /// Returns an [`UpdateResult`] aggregating across the tree — the caller
    /// uses `needs_layout` / `needs_repaint` to decide whether to invalidate
    /// the layout cache. Does **not** itself trigger a layout pass.
    pub fn reconcile(&mut self, new_root: WidgetRef) -> UpdateResult {
        let mut result = {
            // Check if the root references are the same Arc to avoid deadlock
            if Arc::ptr_eq(&self.root, &new_root) {
                // Same widget reference: nothing to reconcile.
                UpdateResult::unchanged()
            } else {
                let mut root = self.root.lock().expect("widget lock poisoned");
                root.update(new_root)
            }
        };
        // Phase 3 M3: harvest cancelled-unmount prefixes
        // surfaced during the reconcile (cancel_exit on
        // ghosts whose key reappeared). The runtime will
        // consume these via process_drain_queues.
        self.pending_cancelled_unmount_prefixes
            .append(&mut result.cancelled_unmount_prefixes);
        // Phase 3 M3: harvest immediate-drop prefixes for
        // widgets that disappeared without an exit animation.
        // Same drain channel as ghost-completion drains.
        self.pending_drained_prefixes
            .append(&mut result.drained_path_prefixes);
        if let Some(focused_widget) = self.focused.as_ref() {
            let focused_ref_count = Arc::strong_count(focused_widget);
            // If there's only one reference to the focused widget, it must have
            // been removed from the hierarchy and is therefore no longer a valid
            // focus target.
            if focused_ref_count == 1 {
                self.focused = None;
            }
        }
        result
    }

    /// Mark the UI as needing a full layout pass (structural / content change).
    /// Also implies a repaint is needed.
    pub fn mark_needs_layout(&mut self) {
        self.needs_layout = true;
        self.needs_repaint = true;
        #[cfg(debug_assertions)]
        {
            self.last_dirty_source = 0; // rerender (called from lib::update)
        }
    }

    /// Mark the UI as needing a visual refresh only (e.g. hover state change).
    /// Does **not** trigger a layout pass.
    pub fn mark_needs_repaint(&mut self) {
        self.needs_repaint = true;
    }

    /// Backward-compatible alias for [`mark_needs_layout`].  Used by
    /// `call_event` when an event handler reports `handled=true`, so we
    /// attribute the resulting dirty-marking to "event" rather than the
    /// rerender path.
    pub fn mark_dirty(&mut self) {
        self.needs_layout = true;
        self.needs_repaint = true;
        #[cfg(debug_assertions)]
        {
            self.last_dirty_source = 1; // event
        }
    }

    /// Whether a full layout pass is required.
    pub fn needs_layout(&self) -> bool {
        self.needs_layout
    }

    /// Whether a visual repaint is needed (always true when layout is needed).
    pub fn needs_repaint(&self) -> bool {
        self.needs_repaint
    }

    /// Acknowledge a presented frame: clear the repaint flag so
    /// [`needs_repaint`](Self::needs_repaint) acts as a damage flag for
    /// redraw-on-demand render loops. Everything that changes the tree's
    /// appearance (events, reconcile, animation ticks) re-marks it.
    pub fn clear_needs_repaint(&mut self) {
        self.needs_repaint = false;
    }

    /// Backward-compatible alias for [`needs_layout`].
    pub fn is_dirty(&self) -> bool {
        self.needs_layout
    }

    pub fn get_focused(&self) -> Option<&WidgetRef> {
        self.focused.as_ref()
    }

    /// Phase 2.5 M2: returns `true` if the focused widget
    /// claims `Key::Character(_)` events. Hosts call this
    /// before pumping character events to game-side input
    /// queries; if true, character keys are consumed by the
    /// runtime and don't reach the game pump.
    ///
    /// Per UL's UI_RUNTIME.md §2: ONLY `Key::Character(_)`
    /// is suppressed. `Escape`, `F1..F12`, arrow keys, `Tab`,
    /// and modifier keys (Ctrl/Alt/Meta/Shift) are NOT
    /// suppressed by focus alone — those pass through to the
    /// game pump unconditionally.
    pub fn consumes_character_key(&self) -> bool {
        match self.focused.as_ref() {
            Some(focused) => {
                let g = focused.lock().expect("widget lock poisoned");
                g.claims_character_keys()
            }
            None => false,
        }
    }

    /// Phase 2.5 M1: returns `true` if any active portal or
    /// the focused widget declares `CursorPreference::Free`.
    /// Hosts compose this with their own cursor-lock demand
    /// (camera mode, world interaction, etc.). UL audit
    /// example: `cursor_lock = !runtime.wants_cursor_free()
    /// && game_wants_lock`.
    pub fn wants_cursor_free(&self) -> bool {
        // Any portal entry declaring Free contributes.
        let portal_says_free = self
            .portal_layers
            .any(|e| e.cursor == portal_layer::CursorPreference::Free);
        if portal_says_free {
            return true;
        }
        // Focused widget can also declare Free (TextInput).
        if let Some(focused) = self.focused.as_ref() {
            let g = focused.lock().expect("widget lock poisoned");
            if let Some(pref) = g.cursor_preference_when_focused() {
                if pref == portal_layer::CursorPreference::Free {
                    return true;
                }
            }
        }
        false
    }

    /// Phase 2 M4 (refined in P25-M0): returns `true` if any
    /// portal in the OverlayModal layer has `focus_trap: true`.
    /// Hosts use this to derive their own input-gating
    /// booleans (UL audit: replaces the manual
    /// `overlay_active: bool` plumbing). Reflects the most
    /// recent draw's portal_layers state.
    ///
    /// Walks ONLY the `OverlayModal` layer — a focus_trap in
    /// a tooltip / popover is unusual and shouldn't gate
    /// world input. Phase 2 walked all entries; this is more
    /// correct for the multi-layer surface.
    pub fn has_input_blocking_portal(&self) -> bool {
        self.portal_layers
            .entries_in(portal_layer::PortalLayer::OverlayModal)
            .iter()
            .any(|e| e.focus_trap)
    }

    /// Phase 2 M4: attempt to move focus to `target`. Rejects
    /// the move if a focus_trap portal is active and `target`
    /// is not within its subtree. Returns `true` on accept.
    /// Direct callers can use this instead of writing
    /// `self.focused = Some(target)` to honor the focus trap.
    pub fn try_set_focus(&mut self, target: WidgetRef) -> bool {
        if let Some(top) = self.focus_stack.last() {
            if !widget_subtree_contains(&top.portal, &target) {
                return false;
            }
        }
        self.focused = Some(target);
        true
    }

    /// Move keyboard focus to the next (`reverse = false`) or previous
    /// (`reverse = true`) focusable widget in tree order, wrapping around.
    /// Returns `true` if focus moved.
    ///
    /// Tab order is the live tree walked pre-order. When a `focus_trap` portal
    /// is active, traversal is confined to that portal's subtree (the top of
    /// `focus_stack`); otherwise it ranges over the base tree. Inputs inside a
    /// non-trapping portal are not yet reachable by Tab — a known limitation.
    pub fn focus_next(&mut self, reverse: bool) -> bool {
        let root = match self.focus_stack.last() {
            Some(top) => top.portal.clone(),
            None => self.root.clone(),
        };
        let mut focusables = Vec::new();
        collect_focusables(&root, &mut focusables);
        if focusables.is_empty() {
            return false;
        }

        let current = self
            .focused
            .as_ref()
            .and_then(|f| focusables.iter().position(|w| Arc::ptr_eq(w, f)));

        let n = focusables.len();
        let next_idx = match current {
            Some(i) => {
                if reverse {
                    (i + n - 1) % n
                } else {
                    (i + 1) % n
                }
            }
            // Nothing in the ring is focused yet: Tab → first, Shift-Tab → last.
            None => {
                if reverse {
                    n - 1
                } else {
                    0
                }
            }
        };

        let target = focusables[next_idx].clone();
        self.try_set_focus(target)
    }

    /// Phase 2 M4: reconcile `focus_stack` with the current
    /// `portal_layer`. Pushes restoration entries for newly-
    /// open focus_trap portals; pops entries for portals that
    /// have left the layer (closed or unmounted), restoring
    /// their captured `previous_focus` for the LIFO close case.
    /// Called after every `draw()` and any state change that
    /// may have flipped a portal's focus_trap.
    ///
    /// Pop policy: walk from top. While the top entry's portal
    /// is no longer in `portal_layer`, pop it and restore its
    /// `previous_focus` (LIFO — the common case where the
    /// most-recently-opened portal closes). Once the top is
    /// still present, stop popping and restoring; any deeper
    /// stale entries are filtered out *without* restoration —
    /// restoring their saved `previous_focus` would set focus
    /// to a target outside the surviving top's trapped subtree,
    /// which `try_set_focus` would reject anyway.
    pub fn sync_focus_stack(&mut self) {
        // Collect candidate focus_trap entries from across all
        // layers. The closure-vs-mutable-self limitation means
        // we walk via the iter helpers and clone refs as
        // needed.
        let trap_entries: Vec<(WidgetRef, ())> = self
            .portal_layers
            .iter_paint_order()
            .filter(|e| e.focus_trap)
            .map(|e| (e.widget.clone(), ()))
            .collect();

        // Push: new focus_trap entries get a restoration point.
        for (portal, _) in &trap_entries {
            let already = self
                .focus_stack
                .iter()
                .any(|r| Arc::ptr_eq(&r.portal, portal));
            if !already {
                let prev = self.focused.clone();
                self.focus_stack.push(FocusRestoration {
                    portal: portal.clone(),
                    previous_focus: prev,
                });
            }
        }
        // Pop from top while stale; restore each popped's
        // previous_focus. Stops at the first surviving entry.
        while let Some(top) = self.focus_stack.last() {
            let still_present = trap_entries
                .iter()
                .any(|(p, _)| Arc::ptr_eq(p, &top.portal));
            if still_present {
                break;
            }
            let popped = self.focus_stack.pop().unwrap();
            self.focused = popped.previous_focus;
        }
        // Filter out any deeper stale entries silently. These
        // are entries below a still-active top — non-top closes
        // are rare and don't invalidate the surviving trap, so
        // restoring their previous_focus would do more harm than
        // good (try_set_focus would reject moves outside the
        // surviving top's subtree anyway).
        self.focus_stack
            .retain(|r| trap_entries.iter().any(|(p, _)| Arc::ptr_eq(p, &r.portal)));
    }

    /// Phase 2 M4 (extended in P25-M0 / P3-M2): clear all
    /// per-tree-instance state (focus stack + portal_layers +
    /// focused widget + in-flight drag preview + pending
    /// drain-queue prefixes). Called on hot-reload to prevent
    /// stale references into a torn-down tree.
    pub fn clear_lifecycle_state(&mut self) {
        self.focus_stack.clear();
        self.portal_layers.clear();
        self.focused = None;
        // Phase 3 M2: drop the drag preview if any was in
        // flight when the reload landed — its WidgetRef
        // points into the old tree.
        self.active_drag_preview = None;
        // Drop host-set anchors. Per INTENT §7 a reload drops
        // what it cannot verify still means anything, and an
        // anchor id is a name in the *old* `.ogh` — the reloaded
        // program may not have a Portal for it any more. Hosts
        // that set an anchor per frame re-establish it on the
        // next one; hosts that set it once must re-set it after
        // a reload (documented in AGENTS.md).
        self.anchors.clear();
        // Phase 3 M3: drop pending drain prefixes — they
        // refer to paths in the old runtime's state map,
        // which is also being replaced.
        self.pending_drained_prefixes.clear();
        self.pending_cancelled_unmount_prefixes.clear();
    }

    /// Advance all active animations in the widget tree by `dt` seconds.
    /// Called once per frame before `layout()`. Marks the UI dirty as
    /// appropriate: layout-affecting transitions request a full layout
    /// pass; color-only transitions only request a repaint.
    ///
    /// Phase 3 M0: builds a [`event::TickContext`] that widgets can use
    /// to record drained / cancelled-unmount path prefixes. The vecs
    /// are appended to the UI's pending-prefix fields so the runtime
    /// can drain them at the next render boundary.
    pub fn tick_animations(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        let mut ctx = event::TickContext::new(dt);
        let result = {
            let mut root = self.root.lock().expect("widget lock poisoned");
            root.tick_animations(&mut ctx)
        };
        self.pending_drained_prefixes
            .append(&mut ctx.drained_path_prefixes);
        self.pending_cancelled_unmount_prefixes
            .append(&mut ctx.cancelled_unmount_prefixes);
        if result.needs_layout {
            self.needs_layout = true;
            self.needs_repaint = true;
            #[cfg(debug_assertions)]
            {
                self.last_dirty_source = 2; // animation
            }
        } else if result.needs_repaint {
            self.needs_repaint = true;
        }
    }

    /// Phase 3 M0: take and clear the path prefixes whose owning
    /// widgets drained since the last call. The runtime calls this
    /// at the next render boundary to flush owned hooks/state for
    /// the corresponding subtrees.
    pub fn take_drained_prefixes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_drained_prefixes)
    }

    /// Phase 3 M0: take and clear the path prefixes whose pending
    /// unmount was cancelled since the last call. The runtime uses
    /// these to remove matching entries from any pending unmount
    /// queue before drain-time runs.
    pub fn take_cancelled_unmount_prefixes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_cancelled_unmount_prefixes)
    }

    /// Read-only length of the drain prefix queue. Useful for hosts
    /// (and tests) that want to verify draining happened without
    /// consuming the queue.
    pub fn pending_drained_prefixes_len(&self) -> usize {
        self.pending_drained_prefixes.len()
    }
}

/// The Surface trait must be implemented for a given renderer (such as Skia) to draw the widget tree to a bitmap.
pub trait Surface {
    fn draw(&mut self, ui: &mut UI);
}

/// Abstraction over renderer primitives. Widgets call these methods from
/// their `render` implementation. All coordinates are in logical (pre-DPI)
/// space; the implementation is responsible for any scaling.
pub trait RenderContext {
    fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: &Color);
    /// Fill a rect whose corners follow `corners` — any mix of `Sharp`,
    /// `Round`, and `Chamfer`. Backends are expected to fast-path the
    /// all-sharp case (call `fill_rect` instead) and the pure-round
    /// case (rounded-rect primitive) where they can.
    fn fill_corners_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        corners: &Corners,
        color: &Color,
    );
    fn draw_border(&mut self, border: &Border, x: f32, y: f32, w: f32, h: f32, corners: &Corners);
    fn draw_image(
        &mut self,
        path: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        image_cache: &mut ImageCache,
    );
    fn draw_text(&mut self, text: &str, style: &TextStyle, x: f32, y: f32, width: f32);
    fn draw_line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, width: f32, color: &Color);

    /// Push a clip rectangle. All subsequent drawing is clipped to this rect
    /// until `pop_clip_rect()` is called. Uses save/restore semantics.
    fn push_clip_rect(&mut self, _x: f32, _y: f32, _w: f32, _h: f32) {}

    /// Pop the most recently pushed clip rectangle.
    fn pop_clip_rect(&mut self) {}

    /// Push a paint-time effect layer for a widget: opacity applied as
    /// a layer composite, plus an affine transform pivoting around
    /// `(pivot_x, pivot_y)`. Backends that don't support layered
    /// compositing can skip the alpha. Paired with `pop_effects`.
    fn push_effects(
        &mut self,
        _opacity: f32,
        _transform: &Transform,
        _pivot_x: f32,
        _pivot_y: f32,
    ) {
    }

    /// Pop the most recently pushed effects layer.
    fn pop_effects(&mut self) {}

    /// Begin a backdrop-filter layer over the rect `(x, y, w, h)`,
    /// clipped to `radii`. Skia-style backends apply a Gaussian blur
    /// of `sigma` to the canvas content already painted under the
    /// rect; the layer is then ready to receive the panel's own
    /// translucent paint, which composites *on top* of the blurred
    /// capture. Backends that can't do backdrop sampling can leave
    /// this a no-op — the panel still renders, just without the
    /// frosted-glass effect. Always paired with `pop_backdrop_blur`.
    fn push_backdrop_blur(
        &mut self,
        _x: f32,
        _y: f32,
        _w: f32,
        _h: f32,
        _corners: &Corners,
        _sigma: f32,
    ) {
    }

    /// Pop the most recently pushed backdrop-filter layer, compositing
    /// it back onto the underlying canvas.
    fn pop_backdrop_blur(&mut self) {}

    /// Paint an inner (inset) glow inside the border box. `corners`
    /// is the same per-corner shape that the background fill / border
    /// already use, so the glow traces the inside of the panel
    /// silhouette. Default impl is a no-op so non-Skia backends
    /// compile; Skia draws a stroked path with a `MaskFilter::blur`
    /// clipped to the border-box interior.
    fn draw_inner_glow(
        &mut self,
        _x: f32,
        _y: f32,
        _w: f32,
        _h: f32,
        _corners: &Corners,
        _glow: &InnerGlow,
    ) {
    }

    /// Paint an outer drop shadow under the border box (CSS non-inset
    /// `box-shadow`): the panel silhouette offset and blurred, drawn
    /// BEFORE the panel's own background so the fill sits on top.
    /// Default impl is a no-op so non-Skia backends compile.
    fn draw_shadow(
        &mut self,
        _x: f32,
        _y: f32,
        _w: f32,
        _h: f32,
        _corners: &Corners,
        _shadow: &Shadow,
    ) {
    }

    /// The `Canvas` painter hatch: hand `paint` a backend-native canvas
    /// positioned at `rect`'s origin and scaled to device pixels, so the
    /// host painter draws in local *logical* coordinates from `(0, 0)` to
    /// `(rect.width, rect.height)` and never sees DPI or its position in
    /// the widget tree. Implementations must save canvas state before the
    /// call and restore it after, exactly like `push_clip_rect`.
    ///
    /// This is the one place a backend-native handle crosses the
    /// `RenderContext` boundary, and it is deliberate: it is a *named,
    /// host-facing* hatch, not convenience drift. See
    /// [`INTENT.md`] §6 for the exception and its own drift indicators —
    /// in particular, no `widget/` module other than
    /// [`canvas_widget`](crate::widget::canvas_widget) may call this, and
    /// no built-in widget may paint through it instead of through the
    /// typed primitives above.
    ///
    /// Returns `true` if the painter ran. Backends that cannot expose a
    /// native canvas keep this default and the `Canvas` widget paints
    /// nothing: a `Canvas` is opt-in host paint, and a backend that can't
    /// service it is not an error. `CanvasWidget::render` logs once per
    /// painter name in debug builds so a blank rectangle has an answer.
    ///
    /// [`INTENT.md`]: ../../../docs/internal/INTENT.md
    fn with_local_canvas(
        &mut self,
        _rect: &Rect,
        _paint: &mut dyn FnMut(&mut canvas_widget::Painter),
    ) -> bool {
        false
    }
}

use downcast_rs::{impl_downcast, Downcast};
use rect::Rect;

/// Paint-time effects applied to a widget and its descendants: an
/// opacity layer and an affine transform pivoting around `(pivot_x,
/// pivot_y)`. Pure rendering concern — does not affect layout or
/// hit-testing.
#[derive(Debug, Clone, Copy)]
pub struct RenderEffects {
    pub opacity: f32,
    pub transform: Transform,
    pub pivot_x: f32,
    pub pivot_y: f32,
}

/// Result of a per-frame animation tick. Bubbled up the widget tree so
/// the UI root can decide whether to mark layout dirty or trigger another
/// tick next frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct TickResult {
    pub needs_repaint: bool,
    pub needs_layout: bool,
    pub still_animating: bool,
}

impl TickResult {
    pub const NONE: Self = Self {
        needs_repaint: false,
        needs_layout: false,
        still_animating: false,
    };

    pub fn merge(self, other: TickResult) -> TickResult {
        TickResult {
            needs_repaint: self.needs_repaint || other.needs_repaint,
            needs_layout: self.needs_layout || other.needs_layout,
            still_animating: self.still_animating || other.still_animating,
        }
    }
}

/// Result of reconciling one widget against a new descriptor. Bubbled up
/// the widget tree by `reconcile()` / `reconcile_children()` so the UI
/// root can decide whether to mark layout dirty — host_state changes that
/// produce identical widget output should NOT trigger a relayout.
///
/// Phase 3 M0: gains `cancelled_unmount_prefixes` for the
/// drain-time unmount machinery (M3). Cancel-mid-exit happens
/// during reconcile (not tick), so the cancel-prefix
/// communication piggybacks on UpdateResult rather than
/// `TickContext`. Loses Copy as a result; was already a
/// small struct of bools so the impact is minimal.
#[derive(Debug, Clone, Default)]
pub struct UpdateResult {
    /// True if the new widget's type matched the old widget's type, so
    /// props were copied in place rather than the old widget being
    /// replaced. False means the caller (`reconcile_children`) should
    /// swap the old WidgetRef out for the new one.
    pub absorbed: bool,
    /// True if any prop that affects layout (size, content, children) is
    /// different from the cached value. The UI relayouts the whole tree
    /// when this bubbles up — a future change can localise this further.
    pub needs_layout: bool,
    /// True if a render-only prop (color, opacity) changed. Implies a
    /// repaint without a relayout.
    pub needs_repaint: bool,
    /// Phase 3 M0: path prefixes whose pending unmount was
    /// cancelled during this reconcile (cancel_exit on a
    /// widget that was previously exiting). UI accumulates
    /// these into `pending_cancelled_prefixes` for the next
    /// frame's `Runtime::process_drain_queues` to consume.
    pub cancelled_unmount_prefixes: Vec<String>,
    /// Phase 3 M3: path prefixes whose owning widget was
    /// dropped *immediately* during this reconcile (no exit
    /// animation; e.g. type mismatch with no exit_style, or
    /// a child orphaned without ghost capability). UI
    /// harvests these into `pending_drained_prefixes` so the
    /// runtime flushes lifecycle hooks at the next render
    /// boundary — same drain channel as ghost-completion.
    pub drained_path_prefixes: Vec<String>,
}

impl UpdateResult {
    /// Type mismatch: the old widget could not absorb the new one, so the
    /// caller will replace it. A replacement always implies relayout +
    /// repaint of the affected subtree.
    ///
    /// Phase 3 M0: was a `const`; now a fn because
    /// `cancelled_unmount_prefixes: Vec` isn't const-eligible.
    pub fn replace() -> Self {
        Self {
            absorbed: false,
            needs_layout: true,
            needs_repaint: true,
            cancelled_unmount_prefixes: Vec::new(),
            drained_path_prefixes: Vec::new(),
        }
    }

    /// Type matched and every prop was identical: no work needed.
    pub fn unchanged() -> Self {
        Self {
            absorbed: true,
            needs_layout: false,
            needs_repaint: false,
            cancelled_unmount_prefixes: Vec::new(),
            drained_path_prefixes: Vec::new(),
        }
    }

    /// Type matched but layout-affecting props differ.
    pub fn layout_changed() -> Self {
        Self {
            absorbed: true,
            needs_layout: true,
            needs_repaint: true,
            cancelled_unmount_prefixes: Vec::new(),
            drained_path_prefixes: Vec::new(),
        }
    }

    /// Aggregate two results. `absorbed` from `self` is preserved (it
    /// describes the parent widget's own absorption); the `needs_*` flags
    /// are unioned (any descendant change bubbles up). Phase 3 M0:
    /// cancelled_unmount_prefixes are concatenated.
    pub fn merge(mut self, mut other: UpdateResult) -> UpdateResult {
        self.cancelled_unmount_prefixes
            .append(&mut other.cancelled_unmount_prefixes);
        self.drained_path_prefixes
            .append(&mut other.drained_path_prefixes);
        UpdateResult {
            absorbed: self.absorbed,
            needs_layout: self.needs_layout || other.needs_layout,
            needs_repaint: self.needs_repaint || other.needs_repaint,
            cancelled_unmount_prefixes: self.cancelled_unmount_prefixes,
            drained_path_prefixes: self.drained_path_prefixes,
        }
    }
}

/// All widgets (boxes, text inputs, etc) must implement the Widget trait.
/// Can be used to implement custom rendering systems (e.g. grid instead of
/// flexbox).
pub trait Widget: Downcast {
    fn get_type(&self) -> &str;
    fn get_dimensions(
        &self,
        ctx: &LayoutContext,
        parent_direction: &Direction,
        parent_width: f32,
        parent_available_width: f32,
        parent_height: f32,
        parent_available_height: f32,
        sibling_basis: f32,
    ) -> (f32, f32);
    fn get_children(&self) -> Vec<WidgetRef>;
    fn get_basis(&self, direction: &Direction) -> f32;
    fn get_children_basis(&self) -> f32;
    fn get_children_fixed_width(&self) -> f32 {
        0.0
    }
    fn get_children_fixed_height(&self) -> f32 {
        0.0
    }
    fn get_fixed_width(&self) -> Option<f32>;
    fn get_fixed_height(&self) -> Option<f32>;
    fn handle_event(&mut self, event: &Event, ctx: &mut EventContext, self_ref: &WidgetRef)
        -> bool;
    fn layout(
        &mut self,
        ctx: &LayoutContext,
        cursor_x: f32,
        cursor_y: f32,
        parent_direction: &Direction,
        parent_width: f32,
        parent_available_width: f32,
        parent_height: f32,
        parent_available_height: f32,
        sibling_basis: f32,
    );
    /// Accepts a reference to a widget. If the widget is of the same type,
    /// the current widget will be updated in place. Otherwise, the current
    /// widget will be replaced along with all of its descendants. The
    /// returned [`UpdateResult`] reports whether the new widget was
    /// absorbed and whether layout-affecting props actually differed.
    fn update(&mut self, new_widget: WidgetRef) -> UpdateResult;
    fn contains_point(&self, point: &Point) -> bool;

    /// Whether a pointer press at `point` (parent-relative, same space as
    /// `handle_event`) would be consumed by this widget subtree — the
    /// side-effect-free twin of `handle_event`'s hit-test verdict
    /// (`block_interactions || child || listener`). Hosts use the UI-level
    /// wrapper ([`UI::blocks_point`]) to occlude game-world hover and
    /// hit-testing under chrome. Leaf widgets that don't consume presses
    /// keep this default.
    fn blocks_point(&self, _point: &Point) -> bool {
        false
    }

    /// This widget's own declared [`CursorRole`](crate::widget::style::CursorRole)
    /// (CSS `cursor`) — non-recursive, decoupled from event listeners. The
    /// UI-level [`UI::hovered_cursor`] resolves the effective role leaf-wins
    /// over the hovered chain; a widget declaring none keeps this default.
    fn declared_cursor(&self) -> style::CursorRole {
        style::CursorRole::Default
    }

    /// Whether this widget itself would consume a pointer press — the
    /// non-recursive predicate behind [`blocks_point`](Self::blocks_point)
    /// (`block_interactions` or a pointer listener). [`UI::hovered_blocks`]
    /// reads it over the hovered chain so a host can tell "the chrome under
    /// the pointer eats the click" from "the world shows through."
    fn blocks_interactions(&self) -> bool {
        false
    }
    // fn is_focused(&self) -> bool;
    // fn focus(&mut self);
    // fn unfocus(&mut self);
    fn get_children_mut(&mut self) -> Vec<WidgetRef> {
        Vec::new()
    }

    /// Returns `true` if this widget uses absolute positioning and should be
    /// excluded from the normal flex flow.
    fn is_absolute_positioned(&self) -> bool {
        false
    }

    /// For absolute-positioned widgets, returns the `(offset_x, offset_y)`.
    /// Returns `None` for non-absolute widgets.
    fn get_absolute_offset(&self) -> Option<(f32, f32)> {
        None
    }

    /// Mark this widget as hovered or not. Widgets that store a `hover_style`
    /// use this flag to decide whether to merge the override into their
    /// effective style.
    fn set_hovered(&mut self, _hovered: bool) {}

    /// Returns whether this widget is currently hovered.
    fn is_hovered(&self) -> bool {
        false
    }

    /// Fire registered event listeners for the given event name. Default is a
    /// no-op for widgets that don't support event listeners.
    fn fire_listeners(&self, _event_name: &str, _event: &Event) {}

    /// Render this widget using the provided render context. The default
    /// implementation is a no-op; widgets override this to draw themselves.
    fn render(&self, _ctx: &mut dyn RenderContext, _focused: bool, _image_cache: &mut ImageCache) {}

    /// Get the layout rect for this widget (if it has been laid out).
    /// Rects are stored in parent-relative coordinates.
    fn get_layout_rect(&self) -> Option<&Rect> {
        None
    }

    /// Scroll offset applied to descendants. The renderer translates the
    /// canvas by `-(dx, dy)` and the hit-tester offsets the event point by
    /// `+(dx, dy)` before recursing into this widget's children. Defaults
    /// to `(0, 0)` — scrolling containers override.
    fn scroll_offset(&self) -> (f32, f32) {
        (0.0, 0.0)
    }

    /// Stable identity for this widget, used during reconciliation to
    /// match children across frames even when they are inserted, removed,
    /// or reordered in the declarative tree. Widgets without a key fall
    /// back to position-based matching.
    fn key(&self) -> Option<&str> {
        None
    }

    /// Paint-time effects (opacity + transform) applied around this
    /// widget and its descendants. Returns `None` when the widget has
    /// no effects to push, saving a canvas save/restore. Widgets with
    /// effects should resolve their pivot point (typically the widget's
    /// own layout center) inside this method.
    fn render_effects(&self) -> Option<RenderEffects> {
        None
    }

    /// Whether this widget is currently playing an exit animation. Such
    /// widgets are kept in their parent's `children` vec until the
    /// animation completes, then dropped.
    fn is_exiting(&self) -> bool {
        false
    }

    /// Attempt to put this widget into the "exiting" lifecycle state.
    /// Returns `true` if the widget accepts exiting (i.e. it has an
    /// exit style and will animate out); returns `false` for widgets
    /// that don't support exit animations or have nothing to animate
    /// toward. The parent should drop widgets that return `false`.
    fn begin_exit(&mut self) -> bool {
        false
    }

    /// Cancel an in-flight exit so the widget re-enters normal life,
    /// transitioning back toward its declared style. Called when a
    /// matching key reappears in the declarative tree.
    fn cancel_exit(&mut self) {}

    /// True once a widget's exit animation has fully settled and it
    /// is safe for the parent to remove. Non-exiting widgets should
    /// return `false`.
    fn is_exit_complete(&self) -> bool {
        false
    }

    /// Re-seed this widget at its `initial_style` and retarget springs
    /// toward `declared_style`, then cascade to children. Used by the
    /// host to re-play entry animations when an Ogham instance is
    /// re-activated after an earlier exit. Widgets with no
    /// `initial_style` (or no enabled transitions) are no-ops.
    fn restart_entry_animation(&mut self) {}

    /// Shift every active spring delay in this subtree by `secs` and
    /// record the shift, so [`Widget::strip_inherited_group_delay`] can
    /// undo it. Staggered containers (`FlexStyle::stagger`) call this on
    /// each child at group moments — construction, entry restart, exit
    /// cascade — to offset the children into a cascade. No-op for
    /// widgets without springs.
    fn add_group_delay(&mut self, secs: f32) {
        let _ = secs;
    }

    /// Remove whatever group delay this widget inherited from *ancestor*
    /// staggered containers, leaving cascades injected by containers
    /// inside this subtree intact. Reconciliation calls this on
    /// brand-new children spliced into an already-live parent: such a
    /// widget mounts individually, not with its group, so it must not
    /// sit out a cascade slot that isn't happening.
    fn strip_inherited_group_delay(&mut self) {}

    /// Advance any in-flight style transitions and recursively
    /// tick children. Returns the merged tick result so the UI
    /// root can flag the tree for repaint/layout when animations
    /// are running. Default implementation is a no-op.
    ///
    /// Phase 3 M0: takes a `&mut TickContext` instead of bare
    /// `dt` so widgets can push side-effects (drained
    /// owned-path-prefixes, cancelled exits) into the context's
    /// vecs. UI's `tick_animations` drains the context and
    /// stores them on UI's pending fields for the next render
    /// to consume via Runtime::process_drain_queues.
    fn tick_animations(&mut self, ctx: &mut event::TickContext) -> TickResult {
        let _ = ctx;
        TickResult::NONE
    }

    /// Returns true if this widget needs post_render called after children render.
    fn needs_post_render(&self) -> bool {
        false
    }

    /// Phase 2 Portal: returns Some when this widget is a Portal.
    /// The renderer uses this to detect the defer-to-portal-layer
    /// branch — Portal widgets paint nothing in the main pass and
    /// their children render in Pass B against the viewport.
    fn as_portal(&self) -> Option<PortalInfo> {
        None
    }

    /// Phase 2.5 M1: cursor preference for this widget when
    /// focused. Default `None` (no influence). TextInputWidget
    /// returns `Some(Free)` when the user has focused it so
    /// the runtime can declare cursor-free.
    ///
    /// Called by `wants_cursor_free()` on the focused widget
    /// only — non-focused widgets' cursor preferences are
    /// ignored.
    fn cursor_preference_when_focused(&self) -> Option<portal_layer::CursorPreference> {
        None
    }

    /// Phase 2.5 M2: returns true when this widget consumes
    /// `Key::Character(_)` events. The host's input pump
    /// consults `Runtime::consumes_character_key` (which
    /// checks the focused widget) before populating
    /// `pressed()` / `held()` queries with character events.
    /// TextInputWidget overrides → true.
    fn claims_character_keys(&self) -> bool {
        false
    }

    /// Whether this widget participates in keyboard focus traversal
    /// (Tab / Shift-Tab). Default `false`; `TextInputWidget` overrides → true.
    /// Used by `UI::focus_next` to build the tab-order ring in tree order.
    fn is_focusable(&self) -> bool {
        false
    }

    /// Phase 2 lifecycle: the call-stack path at which this widget
    /// was constructed. Used to identify which paths a draining
    /// widget "owns" — when the widget is removed from the tree
    /// (drain after exit animation), the runtime queues any
    /// `unmount_hooks` / `effects` whose key-path starts with this
    /// prefix for the next frame's pre-layout drain step.
    ///
    /// The actual flush is performed by
    /// `StateManager::flush_for_path_prefix`, called from the
    /// drain path. Widgets only need to expose their prefix.
    ///
    /// Default `""` means "owns no specific paths" (most widgets —
    /// only function-call containers like `FlexWidget` produced
    /// by an `fn` invocation own a path).
    fn owned_path_prefix(&self) -> &str {
        ""
    }

    /// Phase 3 M1: drag payload declared on this widget. When
    /// `Some(_)`, this widget is a valid drag source; the input
    /// pump constructs a [`event::DragState`] with this payload
    /// and dispatches `drag_start` once the cursor moves past
    /// the dead-zone (see [`Widget::drag_dead_zone`]).
    /// Default `None` means "not draggable".
    fn drag_payload(&self) -> Option<&crate::runtime::value::Value> {
        None
    }

    /// Phase 3 M1: per-widget dead-zone override (in logical
    /// pixels). Returning `None` defers to the host's default
    /// (4px). Returning `Some(0.0)` makes this widget drag
    /// instantly on `mouse_down`.
    fn drag_dead_zone(&self) -> Option<f32> {
        None
    }

    /// Phase 3 M1: predicate determining whether this widget
    /// accepts a drop with the given payload. Used by the
    /// drop-target hit-test during a drag — only widgets
    /// returning `true` can receive `drag_end`. Default `false`
    /// (most widgets are not drop targets).
    fn accepts_drop(&self, _payload: &crate::runtime::value::Value) -> bool {
        false
    }

    /// Phase 3 M1: invoke any registered listeners for
    /// `event.name` on this widget. Used by the drag dispatch
    /// path, which has already chosen the target via hit-test
    /// and shouldn't re-run hit-test gating inside
    /// `handle_event`. Returns `true` if any listener fired.
    /// Default: no-op (most widgets don't carry listeners).
    fn fire_event_listener(&self, _event: &Event) -> bool {
        false
    }

    /// Phase 3 M2: optional drag preview. When this widget is
    /// the source of an in-flight drag, the returned subtree
    /// renders attached to the cursor in the
    /// [`portal_layer::PortalLayer::CursorAttached`] layer.
    /// Default `None` (no preview).
    fn drag_preview(&self) -> Option<WidgetRef> {
        None
    }

    /// Called after all children have been rendered. Used by scrollable
    /// containers to pop their clip rect.
    fn post_render(&self, _ctx: &mut dyn RenderContext, _image_cache: &mut ImageCache) {}
}
impl_downcast!(Widget);

/// Utility type alias for a widget reference. Widget are almost always
/// wrapped in an Arc and Mutex to support references and mutability
/// across the entire tree.
pub type WidgetRef = Arc<Mutex<dyn Widget>>;

/// Phase 2 M4: depth-first search to determine whether
/// `target` is reachable from `root` via the get_children
/// chain. Used by `try_set_focus` to verify that a focus
/// move stays within a focus_trap portal's subtree. Bounded
/// recursion via an explicit visited cap.
fn widget_subtree_contains(root: &WidgetRef, target: &WidgetRef) -> bool {
    if Arc::ptr_eq(root, target) {
        return true;
    }
    let children = {
        let g = root.lock().expect("widget lock poisoned");
        g.get_children()
    };
    for child in &children {
        if Arc::ptr_eq(child, target) {
            return true;
        }
        if widget_subtree_contains(child, target) {
            return true;
        }
    }
    false
}

/// Pre-order DFS collecting every focusable widget under `node` (inclusive)
/// into `out`, in tree (declaration) order. This is the tab-order ring for
/// `UI::focus_next`.
fn collect_focusables(node: &WidgetRef, out: &mut Vec<WidgetRef>) {
    let children = {
        let g = node.lock().expect("widget lock poisoned");
        if g.is_focusable() {
            out.push(node.clone());
        }
        g.get_children()
    };
    for child in &children {
        collect_focusables(child, out);
    }
}
