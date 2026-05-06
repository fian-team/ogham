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
/// Flexbox-like layout widget.
pub mod flex_widget;
/// Lifecycle-sequencing container that holds one generation of content
/// at a time and waits for exits before mounting the next.
pub mod presence_widget;
/// Phase 2 Portal widget — defers paint + hit-test to the
/// per-frame `UI.portal_layers`, rendering in front of all
/// base-tree siblings.
pub mod portal_widget;
/// Phase 2.5 M0: named portal layers + per-layer backdrop
/// policies. Portal widgets declare a layer; the renderer
/// dispatches to per-layer storage and paints in priority
/// order.
pub mod portal_layer;
/// Grid layout widget.
pub mod grid_widget;
pub mod image;
/// Image rendering widget.
pub mod image_widget;
/// Convenience macros for working with widgets.
#[macro_use]
pub mod event;
pub mod point;
pub mod rect;
pub mod style;
/// SVG rendering widget.
pub mod svg_widget;
/// Text input field widget.
pub mod text_input_widget;
/// Text rendering widget.
pub mod text_widget;

/// Constructs UI widgets from runtime `Value::Widget` descriptors.
pub mod builder;

use std::sync::{Arc, Mutex};

use skia_safe::textlayout::FontCollection;

use crate::widget::{
    event::{Event, EventContext},
    image::ImageCache,
    point::Point,
    style::{Border, Color, CornerRadii, Direction, TextStyle, Transform},
};

/// Context passed through the layout tree during a layout pass.
/// Carries the font collection and default font so that text widgets
/// can measure text without relying on thread-locals.
pub struct LayoutContext<'a> {
    pub font_collection: Option<&'a FontCollection>,
    pub default_font: Option<&'a str>,
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
#[derive(Clone, Copy, Debug)]
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
}

impl UI {
    pub fn new(root: WidgetRef) -> Self {
        Self {
            root,
            image_cache: ImageCache::new(),
            portal_layers: PortalLayers::new(),
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
        }
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
                self.mark_dirty();
            }
            handled
        } else {
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
                self.mark_dirty();
            }
            handled
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
        let entries: Vec<PortalEntry> =
            self.portal_layers.iter_hit_test_order().cloned().collect();
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
            if entry.layer.default_backdrop()
                == portal_layer::BackdropPolicy::Block
            {
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

    /// Walk the widget tree and set `hovered = true` on every widget in the
    /// path from the root to the deepest widget that contains `point`.
    /// All other widgets are set to `hovered = false`. Returns `true` if
    /// any widget's hover state changed.
    fn update_hover(&mut self, point: &Point) -> bool {
        let root = self.root.clone();
        Self::update_hover_recursive(&root, point)
    }

    fn update_hover_recursive(widget_ref: &WidgetRef, point: &Point) -> bool {
        let mut widget = widget_ref.lock().expect("widget lock poisoned");
        let hit = widget.contains_point(point);

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
            changed |= Self::update_hover_recursive(child, &child_point);
        }

        changed
    }

    /// Updates the bounds of widgets in the hierarchy within the constraints provided (typically the screen size).
    pub fn layout(&mut self, width: f32, height: f32) {
        let dims_changed =
            self.last_layout_width != width || self.last_layout_height != height;
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
        let result = {
            // Check if the root references are the same Arc to avoid deadlock
            if Arc::ptr_eq(&self.root, &new_root) {
                // Same widget reference: nothing to reconcile.
                UpdateResult::UNCHANGED
            } else {
                let mut root = self.root.lock().expect("widget lock poisoned");
                root.update(new_root)
            }
        };
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

    /// Backward-compatible alias for [`needs_layout`].
    pub fn is_dirty(&self) -> bool {
        self.needs_layout
    }

    pub fn get_focused(&self) -> Option<&WidgetRef> {
        self.focused.as_ref()
    }

    /// Phase 2.5 M1: returns `true` if any active portal or
    /// the focused widget declares `CursorPreference::Free`.
    /// Hosts compose this with their own cursor-lock demand
    /// (camera mode, world interaction, etc.). UL audit
    /// example: `cursor_lock = !runtime.wants_cursor_free()
    /// && game_wants_lock`.
    pub fn wants_cursor_free(&self) -> bool {
        // Any portal entry declaring Free contributes.
        let portal_says_free = self.portal_layers.any(|e| {
            e.cursor == portal_layer::CursorPreference::Free
        });
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
        self.focus_stack.retain(|r| {
            trap_entries.iter().any(|(p, _)| Arc::ptr_eq(p, &r.portal))
        });
    }

    /// Phase 2 M4 (extended in P25-M0): clear all M4 state
    /// (focus stack + portal_layers + focused). Called on
    /// hot-reload to prevent stale focus restoration into a
    /// torn-down tree.
    pub fn clear_lifecycle_state(&mut self) {
        self.focus_stack.clear();
        self.portal_layers.clear();
        self.focused = None;
    }

    /// Advance all active animations in the widget tree by `dt` seconds.
    /// Called once per frame before `layout()`. Marks the UI dirty as
    /// appropriate: layout-affecting transitions request a full layout
    /// pass; color-only transitions only request a repaint.
    pub fn tick_animations(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        let result = {
            let mut root = self.root.lock().expect("widget lock poisoned");
            root.tick_animations(dt)
        };
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
    fn fill_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: &CornerRadii,
        color: &Color,
    );
    fn draw_border(
        &mut self,
        border: &Border,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: &CornerRadii,
    );
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
    fn draw_svg_dom(
        &mut self,
        dom: &skia_safe::svg::Dom,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    );

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
#[derive(Debug, Clone, Copy, Default)]
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
}

impl UpdateResult {
    /// Type mismatch: the old widget could not absorb the new one, so the
    /// caller will replace it. A replacement always implies relayout +
    /// repaint of the affected subtree.
    pub const REPLACE: Self = Self {
        absorbed: false,
        needs_layout: true,
        needs_repaint: true,
    };

    /// Type matched and every prop was identical: no work needed.
    pub const UNCHANGED: Self = Self {
        absorbed: true,
        needs_layout: false,
        needs_repaint: false,
    };

    /// Type matched but layout-affecting props differ.
    pub const LAYOUT_CHANGED: Self = Self {
        absorbed: true,
        needs_layout: true,
        needs_repaint: true,
    };

    /// Aggregate two results. `absorbed` from `self` is preserved (it
    /// describes the parent widget's own absorption); the `needs_*` flags
    /// are unioned (any descendant change bubbles up).
    pub fn merge(self, other: UpdateResult) -> UpdateResult {
        UpdateResult {
            absorbed: self.absorbed,
            needs_layout: self.needs_layout || other.needs_layout,
            needs_repaint: self.needs_repaint || other.needs_repaint,
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
    fn render(
        &self,
        _ctx: &mut dyn RenderContext,
        _focused: bool,
        _image_cache: &mut ImageCache,
    ) {
    }

    /// Get the layout rect for this widget (if it has been laid out).
    /// Rects are stored in parent-relative coordinates.
    fn get_layout_rect(&self) -> Option<&Rect> { None }

    /// Scroll offset applied to descendants. The renderer translates the
    /// canvas by `-(dx, dy)` and the hit-tester offsets the event point by
    /// `+(dx, dy)` before recursing into this widget's children. Defaults
    /// to `(0, 0)` — scrolling containers override.
    fn scroll_offset(&self) -> (f32, f32) { (0.0, 0.0) }

    /// Stable identity for this widget, used during reconciliation to
    /// match children across frames even when they are inserted, removed,
    /// or reordered in the declarative tree. Widgets without a key fall
    /// back to position-based matching.
    fn key(&self) -> Option<&str> { None }

    /// Paint-time effects (opacity + transform) applied around this
    /// widget and its descendants. Returns `None` when the widget has
    /// no effects to push, saving a canvas save/restore. Widgets with
    /// effects should resolve their pivot point (typically the widget's
    /// own layout center) inside this method.
    fn render_effects(&self) -> Option<RenderEffects> { None }

    /// Whether this widget is currently playing an exit animation. Such
    /// widgets are kept in their parent's `children` vec until the
    /// animation completes, then dropped.
    fn is_exiting(&self) -> bool { false }

    /// Attempt to put this widget into the "exiting" lifecycle state.
    /// Returns `true` if the widget accepts exiting (i.e. it has an
    /// exit style and will animate out); returns `false` for widgets
    /// that don't support exit animations or have nothing to animate
    /// toward. The parent should drop widgets that return `false`.
    fn begin_exit(&mut self) -> bool { false }

    /// Cancel an in-flight exit so the widget re-enters normal life,
    /// transitioning back toward its declared style. Called when a
    /// matching key reappears in the declarative tree.
    fn cancel_exit(&mut self) {}

    /// True once a widget's exit animation has fully settled and it
    /// is safe for the parent to remove. Non-exiting widgets should
    /// return `false`.
    fn is_exit_complete(&self) -> bool { false }

    /// Advance any in-flight style transitions by `dt` seconds and
    /// recursively tick children. Returns the merged tick result so the
    /// UI root can flag the tree for repaint/layout when animations are
    /// running. Default implementation is a no-op.
    fn tick_animations(&mut self, _dt: f32) -> TickResult { TickResult::NONE }

    /// Returns true if this widget needs post_render called after children render.
    fn needs_post_render(&self) -> bool { false }

    /// Phase 2 Portal: returns Some when this widget is a Portal.
    /// The renderer uses this to detect the defer-to-portal-layer
    /// branch — Portal widgets paint nothing in the main pass and
    /// their children render in Pass B against the viewport.
    fn as_portal(&self) -> Option<PortalInfo> { None }

    /// Phase 2.5 M1: cursor preference for this widget when
    /// focused. Default `None` (no influence). TextInputWidget
    /// returns `Some(Free)` when the user has focused it so
    /// the runtime can declare cursor-free.
    ///
    /// Called by `wants_cursor_free()` on the focused widget
    /// only — non-focused widgets' cursor preferences are
    /// ignored.
    fn cursor_preference_when_focused(
        &self,
    ) -> Option<portal_layer::CursorPreference> {
        None
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
    fn owned_path_prefix(&self) -> &str { "" }

    /// Called after all children have been rendered. Used by scrollable
    /// containers to pop their clip rect.
    fn post_render(
        &self,
        _ctx: &mut dyn RenderContext,
        _image_cache: &mut ImageCache,
    ) {
    }
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
