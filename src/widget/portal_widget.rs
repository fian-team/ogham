//! Phase 2 Portal: lifts its children's paint and hit-test out of
//! the parent's clip and order. Renders into a per-frame
//! `portal_layer` on the UI; the renderer paints that layer in
//! Pass B, after the main tree.
//!
//! API surface (three properties):
//! - `open: bool` — when true, children are mounted into the
//!   portal layer; when false, children are reconciled out (entry
//!   /exit animations apply normally).
//! - `focus_trap: bool` — parsed in M3, wired in M4. Marks the
//!   portal as input-blocking; `Runtime::has_input_blocking_portal`
//!   returns true while any open portal has it set.
//! - `children: array<widget>` — the portal's contents.
//!
//! Backdrop, dismiss-on-outside, anchor positioning, and
//! escape-to-dismiss are *not* properties — they're consumer-side
//! composition with regular widgets.

use super::flex_widget::FlexWidget;
use super::style::{Direction, FlexStyle, Size};
use super::{
    PortalInfo, RenderEffects, TickResult, UpdateResult, Widget, WidgetRef,
};
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
    pub focus_trap: bool,
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
            focus_trap: false,
            owned_path_prefix: String::new(),
        }
    }

    /// True if this portal is currently open and should defer
    /// to the per-frame portal_layer for paint + hit-test.
    pub fn is_open(&self) -> bool {
        self.open
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
            open: self.open,
            focus_trap: self.focus_trap,
        })
    }

    fn owned_path_prefix(&self) -> &str {
        &self.owned_path_prefix
    }

    fn update(&mut self, new_widget: WidgetRef) -> UpdateResult {
        let mut new_guard = new_widget.lock().expect("widget lock poisoned");
        let new_portal = match new_guard.downcast_mut::<PortalWidget>() {
            Some(p) => p,
            None => return UpdateResult::REPLACE,
        };
        let was_open = self.open;
        let open_changed = self.open != new_portal.open;
        let trap_changed = self.focus_trap != new_portal.focus_trap;
        self.open = new_portal.open;
        self.focus_trap = new_portal.focus_trap;
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
            }
        } else {
            // Open in both old and new (or open: false → true).
            // Reconcile children normally.
            let mut new_children = std::mem::take(&mut new_portal.inner.children);
            self.inner.reconcile_children(&mut new_children)
        };
        UpdateResult {
            absorbed: true,
            needs_layout: open_changed || trap_changed || inner_result.needs_layout,
            needs_repaint: open_changed
                || trap_changed
                || inner_result.needs_repaint,
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
        // path in `UI::call_event`; this handle_event covers
        // non-click events delivered to focused widgets etc.
        // We forward unconditionally — a closed portal whose
        // children are still ghosting needs key events to flow
        // (e.g. a focused text input mid-fade).
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

    fn tick_animations(&mut self, dt: f32) -> TickResult {
        if !self.open {
            // Even when closed, we still tick exit-animation
            // springs on children that began exiting in the
            // previous reconcile.
            return self.inner.tick_animations(dt);
        }
        self.inner.tick_animations(dt)
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

    fn key(&self) -> Option<&str> {
        self.inner.key()
    }
}
