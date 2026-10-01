//! Phase 2 Portal: lifts its children's paint and hit-test out of
//! the parent's clip and order. Renders into a per-frame
//! `portal_layer` on the UI; the renderer paints that layer in
//! Pass B, after the main tree.
//!
//! API surface (three properties):
//! - `open: bool` — when true, children are mounted into the
//!   portal layer; when false, children are reconciled out (entry
//!   /exit animations apply normally).
//! - `open: "hover"` — open exactly while the widget the Portal is
//!   declared inside is hovered, with no state in the document: the
//!   tooltip's opener. The portal follows its parent's hover
//!   ([`Widget::hovers_with_parent`]); on the first frame of a hover
//!   it mounts the content it was last given and replays its entry
//!   (so a delayed `initial` fade is the tooltip's show delay), and on
//!   the first frame after it takes the content back at once — a
//!   tooltip does not linger while the pointer moves on.
//! - `focus_trap: bool` — parsed in M3, wired in M4. Marks the
//!   portal as input-blocking; `Runtime::has_input_blocking_portal`
//!   returns true while any open portal has it set.
//! - `children: array<widget>` — the portal's contents.
//! - `anchor` / `anchor_policy` / `anchor_offset` — seat the
//!   subtree at a host-set viewport point instead of at the slot
//!   it was declared in. See [`super::portal_layer::resolve_anchor`].
//!   `anchor: "parent"` seats it against the laid-out box of the
//!   widget the Portal was declared inside — below its bottom-left,
//!   or above it under `flip` — with no host coordinates at all.
//!   `anchor: "press"` seats it at the last pointer press
//!   ([`super::PRESS_ANCHOR`]): a context menu opens where the
//!   right-click was, a popover where the click was.
//! - `backdrop: "none" | "dismiss" | "block"` — overrides the
//!   layer's default press policy for this entry.
//! - `dismiss: fn () {}` — fires when a press lands outside the
//!   portal's content while its policy is `dismiss`.
//!
//! Escape-to-dismiss is still the host's or a `keydown:` listener's.
//! **Anchoring is the exception** to "positioning is composition",
//! and only because the policies need the subtree's measured size:
//! `.ogh` cannot see it, so an author cannot express "flip above
//! the pointer when the card would overrun the bottom" no matter
//! how the tree is composed. Outside-press dismissal joined it for
//! a neighbouring reason: the full-viewport catch child it used to
//! be composed from lays out inside the parent's box, and a
//! parent-anchored popover's parent is a button.

use super::flex_widget::FlexWidget;
use super::portal_layer::{AnchorPolicy, BackdropPolicy, CursorPreference, PortalLayer};
use super::style::{Direction, FlexStyle, Size};
use super::{PortalInfo, RenderEffects, TickResult, UpdateResult, Widget, WidgetRef};
use crate::widget::event::{Event, EventContext};
use crate::widget::point::Point;
use crate::widget::rect::Rect;
use crate::widget::LayoutContext;

pub struct PortalWidget {
    /// Inner flex that owns layout, rendering, and the children.
    /// Sized to grow so the portal child has the viewport as its
    /// layout box during Pass B (the renderer overrides the clip
    /// with the viewport bounds).
    pub inner: FlexWidget,
    pub open: bool,
    /// `open: "hover"`: open while the parent is hovered.
    pub open_on_hover: bool,
    /// The parent's hover, adopted by the hover walk.
    hovered: bool,
    /// Whether the hover-opened content is mounted right now. Moves one
    /// tick behind `hovered`, because mounting needs a layout pass and
    /// a hover change only asks for a repaint.
    hover_open: bool,
    /// The content a hover-opened portal holds while it is not showing:
    /// the last children the document gave it, out of the tree.
    resting: Vec<WidgetRef>,
    pub focus_trap: bool,
    /// Phase 2.5 M0: which named layer this portal renders
    /// into. Determines paint priority and backdrop policy.
    /// Defaults to [`PortalLayer::OverlayModal`] for
    /// backward compatibility with Phase 2 (which had a
    /// single unnamed layer that behaved like a modal layer).
    pub layer: PortalLayer,
    /// Phase 2.5 M1: cursor preference. None means "use the
    /// layer's default" (OverlayModal/Popover → Free, others
    /// → Inherit). Some(_) overrides.
    pub cursor: Option<CursorPreference>,
    /// Host anchor id. `Some(id)` means this portal's viewport
    /// origin comes from `UI`'s anchor map rather than from the
    /// slot it was declared in — and that it renders nothing on
    /// frames where the host hasn't set that id.
    pub anchor: Option<String>,
    /// How [`Self::anchor`]'s point is seated against the
    /// viewport. Defaults to [`AnchorPolicy::Clamp`]; inert
    /// while `anchor` is `None`.
    pub anchor_policy: AnchorPolicy,
    /// Fixed `(x, y)` nudge applied before the policy. Inert
    /// while neither anchor mode is set.
    pub anchor_offset: (f32, f32),
    /// `anchor: "parent"`: seat the subtree against the laid-out
    /// box of the nearest ancestor with a non-zero rect, resolved
    /// in Pass A from the walk's own accumulated frame. Mutually
    /// exclusive with a host anchor id.
    pub anchor_parent: bool,
    /// Per-entry press policy. `None` means the layer's default.
    pub backdrop: Option<BackdropPolicy>,
    /// `dismiss:` listeners — fired by the hit-test path when a
    /// press lands outside this portal's content while its
    /// effective policy is [`BackdropPolicy::Dismiss`].
    pub dismiss_listeners: Vec<Box<dyn Fn(&Event)>>,
    /// Phase 2 lifecycle: the call-stack path captured at
    /// descriptor-build time. Children's hooks (state cells,
    /// effects, on_unmount) live under this path; flushing the
    /// prefix on portal removal cleans them up.
    pub owned_path_prefix: String,
}

impl PortalWidget {
    pub fn new() -> Self {
        let mut style = FlexStyle::default();
        // Portal itself takes no layout space — children paint
        // into the viewport in Pass B. The inner Flex is grow
        // so children passed through it can size against the
        // available area without explicit dimensions.
        style.width = Size::Grow(1.0);
        style.height = Size::Grow(1.0);
        style.direction = Direction::Column;
        let mut inner = FlexWidget::with_style(style);
        // Don't intercept clicks on the portal itself — the
        // children handle them in Pass B.
        inner.block_interactions = false;
        Self {
            inner,
            open: false,
            open_on_hover: false,
            hovered: false,
            hover_open: false,
            resting: Vec::new(),
            focus_trap: false,
            layer: PortalLayer::OverlayModal,
            cursor: None,
            anchor: None,
            anchor_policy: AnchorPolicy::default(),
            anchor_offset: (0.0, 0.0),
            anchor_parent: false,
            backdrop: None,
            dismiss_listeners: Vec::new(),
            owned_path_prefix: String::new(),
        }
    }

    /// Resolve the effective cursor preference: explicit
    /// override if set, otherwise the layer's default.
    pub fn effective_cursor(&self) -> CursorPreference {
        self.cursor.unwrap_or_else(|| self.layer.default_cursor())
    }

    /// The press policy this entry is settled by: its own
    /// `backdrop:` if declared, otherwise the layer's default.
    pub fn effective_backdrop(&self) -> BackdropPolicy {
        self.backdrop
            .unwrap_or_else(|| self.layer.default_backdrop())
    }

    /// Seat the content a hover-opened portal shows on its next hover.
    pub fn set_resting(&mut self, children: Vec<WidgetRef>) {
        self.resting = children;
    }

    /// True if this portal is currently open and should defer
    /// to the per-frame portal_layer for paint + hit-test.
    pub fn is_open(&self) -> bool {
        self.open || self.hover_open
    }
}

impl Default for PortalWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for PortalWidget {
    fn get_type(&self) -> &str {
        "portal"
    }

    fn as_portal(&self) -> Option<PortalInfo> {
        Some(PortalInfo {
            open: self.is_open(),
            focus_trap: self.focus_trap,
            layer: self.layer,
            cursor: self.effective_cursor(),
            anchor: self.anchor.clone(),
            anchor_policy: self.anchor_policy,
            anchor_offset: self.anchor_offset,
            anchor_parent: self.anchor_parent,
            backdrop: self.effective_backdrop(),
        })
    }

    fn fire_listeners(&self, event_name: &str, event: &Event) {
        if event_name == "dismiss" {
            for listener in &self.dismiss_listeners {
                listener(event);
            }
        }
    }

    fn fire_event_listener(&self, event: &Event) -> bool {
        if event.name == "dismiss" && !self.dismiss_listeners.is_empty() {
            self.fire_listeners("dismiss", event);
            return true;
        }
        false
    }

    fn owned_path_prefix(&self) -> &str {
        &self.owned_path_prefix
    }

    fn update(&mut self, new_widget: WidgetRef) -> UpdateResult {
        let mut new_guard = new_widget.lock().expect("widget lock poisoned");
        let new_portal = match new_guard.downcast_mut::<PortalWidget>() {
            Some(p) => p,
            None => return UpdateResult::replace(),
        };
        // A hover-opened portal takes the document's children into the
        // tree only while it is showing; otherwise they rest until the
        // next hover.
        if new_portal.open_on_hover {
            self.open_on_hover = true;
            self.open = false;
            self.layer = new_portal.layer;
            self.cursor = new_portal.cursor;
            self.anchor = new_portal.anchor.take();
            self.anchor_policy = new_portal.anchor_policy;
            self.anchor_offset = new_portal.anchor_offset;
            self.anchor_parent = new_portal.anchor_parent;
            self.backdrop = new_portal.backdrop;
            self.owned_path_prefix = new_portal.owned_path_prefix.clone();
            let incoming = if new_portal.resting.is_empty() {
                std::mem::take(&mut new_portal.inner.children)
            } else {
                std::mem::take(&mut new_portal.resting)
            };
            if self.hover_open {
                let mut incoming = incoming;
                return self.inner.reconcile_children(&mut incoming);
            }
            self.resting = incoming;
            return UpdateResult {
                absorbed: true,
                needs_layout: false,
                needs_repaint: false,
                cancelled_unmount_prefixes: Vec::new(),
                drained_path_prefixes: Vec::new(),
            };
        }
        self.open_on_hover = false;
        self.hover_open = false;
        self.resting.clear();

        let was_open = self.open;
        let open_changed = self.open != new_portal.open;
        let trap_changed = self.focus_trap != new_portal.focus_trap;
        let layer_changed = self.layer != new_portal.layer;
        let cursor_changed = self.cursor != new_portal.cursor;
        let _ = cursor_changed; // doesn't itself force a relayout

        // Anchoring only moves where Pass B seats the subtree —
        // the inner flex lays out against the same box either
        // way — so a change here is a repaint, never a relayout.
        let anchor_changed = self.anchor != new_portal.anchor
            || self.anchor_policy != new_portal.anchor_policy
            || self.anchor_offset != new_portal.anchor_offset
            || self.anchor_parent != new_portal.anchor_parent;
        self.open = new_portal.open;
        self.focus_trap = new_portal.focus_trap;
        self.layer = new_portal.layer;
        self.cursor = new_portal.cursor;
        self.anchor = new_portal.anchor.take();
        self.anchor_policy = new_portal.anchor_policy;
        self.anchor_offset = new_portal.anchor_offset;
        self.anchor_parent = new_portal.anchor_parent;
        self.backdrop = new_portal.backdrop;
        // Closures can't be cloned; the freshly built portal carries the
        // listeners this render produced, so swap them in.
        std::mem::swap(
            &mut self.dismiss_listeners,
            &mut new_portal.dismiss_listeners,
        );
        // owned_path_prefix is captured at descriptor-build time
        // and shouldn't change for the same path; copy anyway.
        self.owned_path_prefix = new_portal.owned_path_prefix.clone();
        // Reconcile children through the inner flex. The
        // important case: open flipping true → false. We pass an
        // EMPTY descriptor list to reconcile_children so it
        // triggers begin_exit on every current child, producing
        // ghosts. The renderer still paints the portal in Pass B
        // while ghosts remain (Skia draw_widget_recursive
        // checks is_exiting for the close-with-ghosts case).
        // Once exit animations settle, drain_exited_children
        // removes them from inner.children.
        let inner_result = if !self.open && was_open {
            // open: true → false. Reconcile against empty so
            // current children begin_exit.
            let mut empty: Vec<WidgetRef> = Vec::new();
            self.inner.reconcile_children(&mut empty)
        } else if !self.open {
            // Both old and new closed. Don't churn — keep any
            // ghosts ticking down naturally.
            UpdateResult {
                absorbed: true,
                needs_layout: false,
                needs_repaint: false,
                cancelled_unmount_prefixes: Vec::new(),
                drained_path_prefixes: Vec::new(),
            }
        } else {
            // Open in both old and new (or open: false → true).
            // Reconcile children normally.
            let mut new_children = std::mem::take(&mut new_portal.inner.children);
            self.inner.reconcile_children(&mut new_children)
        };
        UpdateResult {
            absorbed: true,
            needs_layout: open_changed
                || trap_changed
                || layer_changed
                || inner_result.needs_layout,
            needs_repaint: open_changed
                || trap_changed
                || layer_changed
                || anchor_changed
                || inner_result.needs_repaint,
            cancelled_unmount_prefixes: inner_result.cancelled_unmount_prefixes,
            drained_path_prefixes: inner_result.drained_path_prefixes,
        }
    }

    // ---- Layout: zero-effort. The portal node itself takes no
    // space in the parent's flow. The inner flex still lays out
    // children so that Pass B can paint with valid layout rects.

    fn get_dimensions(
        &self,
        _ctx: &LayoutContext,
        _parent_direction: &Direction,
        _parent_width: f32,
        _parent_available_width: f32,
        _parent_height: f32,
        _parent_available_height: f32,
        _sibling_basis: f32,
    ) -> (f32, f32) {
        // Portal contributes no layout space to the parent.
        (0.0, 0.0)
    }

    fn get_basis(&self, _direction: &Direction) -> f32 {
        0.0
    }

    fn get_children_basis(&self) -> f32 {
        0.0
    }

    fn get_fixed_width(&self) -> Option<f32> {
        Some(0.0)
    }

    fn get_fixed_height(&self) -> Option<f32> {
        Some(0.0)
    }

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
    ) {
        // Run the inner layout against the available space — the
        // portal_layer painter uses this layout to position
        // children inside the viewport in Pass B.
        self.inner.layout(
            ctx,
            cursor_x,
            cursor_y,
            parent_direction,
            parent_width,
            parent_available_width,
            parent_height,
            parent_available_height,
            sibling_basis,
        );
    }

    fn get_layout_rect(&self) -> Option<&Rect> {
        self.inner.get_layout_rect()
    }

    // ---- Children + delegation to inner -------------------------------

    fn get_children(&self) -> Vec<WidgetRef> {
        // Always expose inner children so animation ticks +
        // hit-test can descend. When closed but children are
        // still ghosting through their exit animation (the
        // open=true→false case), they need to keep ticking and
        // remain hit-testable until drain.
        self.inner.get_children()
    }

    fn get_children_mut(&mut self) -> Vec<WidgetRef> {
        self.inner.get_children_mut()
    }

    fn handle_event(
        &mut self,
        event: &Event,
        ctx: &mut EventContext,
        self_ref: &WidgetRef,
    ) -> bool {
        // Click-routing is handled via the portal_layer hit-test
        // path in `UI::call_event`, at the entry's *painted*
        // position. A pointer event arriving here came down the
        // base tree at the declaration site, which is where an
        // anchored portal's content is not — forwarding it would
        // make the content clickable where nothing draws.
        if event.point.is_some() {
            return false;
        }
        // Non-pointer events are forwarded unconditionally — a
        // closed portal whose children are still ghosting needs
        // key events to flow (e.g. a focused text input mid-fade).
        self.inner.handle_event(event, ctx, self_ref)
    }

    fn contains_point(&self, _point: &Point) -> bool {
        // Portal node itself is invisible in the base tree's
        // hit-test pass; the portal_layer hit-test handles its
        // contents.
        false
    }

    fn render(
        &self,
        _ctx: &mut dyn crate::widget::RenderContext,
        _focused: bool,
        _image_cache: &mut crate::widget::image::ImageCache,
    ) {
        // Portal node paints nothing in the main pass — the
        // renderer detects `as_portal()` and defers to Pass B.
    }

    fn render_effects(&self) -> Option<RenderEffects> {
        None
    }

    fn hovers_with_parent(&self) -> bool {
        self.open_on_hover
    }

    fn set_hovered(&mut self, hovered: bool) {
        self.hovered = hovered;
    }

    fn is_hovered(&self) -> bool {
        self.hovered
    }

    fn tick_animations(&mut self, ctx: &mut crate::widget::event::TickContext) -> TickResult {
        if self.open_on_hover && self.hovered != self.hover_open {
            if self.hovered {
                let content = std::mem::take(&mut self.resting);
                for child in &content {
                    child
                        .lock()
                        .expect("widget lock poisoned")
                        .restart_entry_animation();
                }
                self.inner.children = content;
            } else {
                self.resting = std::mem::take(&mut self.inner.children);
            }
            self.hover_open = self.hovered;
            let inner = self.inner.tick_animations(ctx);
            return TickResult {
                needs_repaint: true,
                needs_layout: true,
                still_animating: inner.still_animating,
            };
        }
        if !self.open {
            // Even when closed, we still tick exit-animation
            // springs on children that began exiting in the
            // previous reconcile.
            return self.inner.tick_animations(ctx);
        }
        self.inner.tick_animations(ctx)
    }

    // ---- Exit lifecycle: delegate so reconcile cascades --------------

    fn is_exiting(&self) -> bool {
        self.inner.is_exiting()
    }

    fn begin_exit(&mut self) -> bool {
        self.inner.begin_exit()
    }

    fn cancel_exit(&mut self) {
        self.inner.cancel_exit();
    }

    fn is_exit_complete(&self) -> bool {
        self.inner.is_exit_complete()
    }

    fn restart_entry_animation(&mut self) {
        // Delegate so the portal's contents re-play entry alongside the
        // rest of the tree when the host re-promotes this Ogham.
        self.inner.restart_entry_animation();
    }

    fn key(&self) -> Option<&str> {
        self.inner.key()
    }
}
