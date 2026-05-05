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
/// per-frame `UI.portal_layer`, rendering in front of all
/// base-tree siblings.
pub mod portal_widget;
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

/// Phase 2 Portal: per-frame entry on `UI.portal_layer`. The
/// renderer pushes one of these whenever it walks past a Portal
/// node with `open: true` in the main render pass; Pass B
/// iterates the layer and paints each portal's children with the
/// viewport as the clip rect.
#[derive(Clone)]
pub struct PortalEntry {
    pub widget: WidgetRef,
    /// The rect the portal node would have occupied if it weren't
    /// a portal. Used as the layout origin for the children when
    /// painted in Pass B (so transforms work as anchor offsets).
    pub parent_rect: rect::Rect,
    pub focus_trap: bool,
}

/// Phase 2 Portal: returned by `Widget::as_portal()` to mark a
/// widget as a Portal. Used by the renderer to detect the defer-
/// to-portal-layer branch and by the runtime API
/// `has_input_blocking_portal()` to derive UL's overlay-active
/// boolean.
#[derive(Clone, Copy, Debug)]
pub struct PortalInfo {
    pub open: bool,
    pub focus_trap: bool,
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
    /// Phase 2 Portal: per-frame portal layer. Cleared at start
    /// of each render pass; populated by the main render walk
    /// when it encounters open portals; consumed by Pass B
    /// (Skia's `draw` and the hit-test path).
    pub portal_layer: Vec<PortalEntry>,
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
            portal_layer: Vec::new(),
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
        // Phase 2 Portal: search the portal layer first
        // (top-most-portal first via reverse iteration). The
        // portal_layer was populated by the most recent draw()
        // call. A portal child whose layout covers the
        // viewport (the backdrop pattern) swallows clicks
        // naturally. Falls through to the base tree only if no
        // portal claims the click.
        let entries = self.portal_layer.clone();
        for entry in entries.iter().rev() {
            // Translate the click into the portal's child
            // coordinate space (subtract the portal's
            // parent_rect origin, just like a normal
            // widget's child-coords translation).
            let child_point = Point::new(
                point.x() - entry.parent_rect.x,
                point.y() - entry.parent_rect.y,
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
            // Backdrop pattern: even if no specific child
            // claims, an open focus_trap portal should swallow
            // the click rather than let it fall through to the
            // base tree. M3 leaves this as fall-through;
            // backdrops are explicit Flex children that
            // contain the entire viewport, so a backdrop with
            // an on_click handler will catch via the loop
            // above. Modal portals without a backdrop will
            // leak clicks — documented limitation; M4 wires
            // focus_trap to gate this.
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

    /// Phase 2 M4: returns `true` if any portal currently in
    /// `portal_layer` has `focus_trap: true`. Hosts use this to
    /// derive their own input-gating booleans (UL audit:
    /// replaces the manual `overlay_active: bool` plumbing).
    /// Reflects the most recent draw's portal_layer state.
    pub fn has_input_blocking_portal(&self) -> bool {
        self.portal_layer.iter().any(|e| e.focus_trap)
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
    /// their captured `previous_focus`. Called after every
    /// `draw()` and any state change that may have flipped a
    /// portal's focus_trap.
    pub fn sync_focus_stack(&mut self) {
        // Walk current focus_trap entries, push any not yet
        // tracked.
        for entry in &self.portal_layer {
            if !entry.focus_trap {
                continue;
            }
            let already_tracked = self
                .focus_stack
                .iter()
                .any(|r| Arc::ptr_eq(&r.portal, &entry.widget));
            if !already_tracked {
                let prev = self.focused.clone();
                self.focus_stack.push(FocusRestoration {
                    portal: entry.widget.clone(),
                    previous_focus: prev,
                });
            }
        }
        // Walk current stack from top, pop any whose portal is
        // no longer in the layer.
        let mut still_active = self.focus_stack.clone();
        still_active.retain(|r| {
            self.portal_layer
                .iter()
                .any(|e| e.focus_trap && Arc::ptr_eq(&e.widget, &r.portal))
        });
        // Pop loop: detect popped entries and restore their
        // previous_focus in reverse order.
        while self.focus_stack.len() > still_active.len() {
            if let Some(popped) = self.focus_stack.pop() {
                // Only restore if the popped entry isn't still
                // present further up the surviving stack
                // (would be unusual but safe to check).
                self.focused = popped.previous_focus;
            }
        }
        self.focus_stack = still_active;
    }

    /// Phase 2 M4: clear all M4 state (focus stack +
    /// portal_layer + focused). Called on hot-reload to
    /// prevent stale focus restoration into a torn-down tree.
    pub fn clear_lifecycle_state(&mut self) {
        self.focus_stack.clear();
        self.portal_layer.clear();
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
