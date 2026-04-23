//! Lifecycle-sequencing container: holds one generation of content at
//! a time and sequences transitions between generations, waiting for
//! the outgoing subtree to finish exiting before mounting the incoming
//! one.
//!
//! Authors use it to gate page/route transitions on exit animations:
//!
//! ```text
//! Presence {
//!   key: current_page_id,
//!   children: [ route_body() ]
//! }
//! ```
//!
//! When `key` changes, every current child gets `begin_exit()`. Children
//! with exit animations (or that host exiting descendants) stay in the
//! tree as ghosts until their springs settle; the new content is held
//! aside as `pending` and only mounted once no ghosts remain. The new
//! widgets then play their own entry animations as normal.

use super::flex_widget::FlexWidget;
use super::style::{Direction, FlexStyle, Size};
use super::{RenderEffects, TickResult, Widget, WidgetRef};
use crate::widget::event::{Event, EventContext};
use crate::widget::point::Point;
use crate::widget::rect::Rect;
use crate::widget::LayoutContext;

/// Container widget that sequences transitions between generations of
/// content. See module docs for semantics.
pub struct PresenceWidget {
    /// Inner flex that owns layout, rendering, and the currently-visible
    /// children (including any exit ghosts while transitioning).
    pub inner: FlexWidget,
    /// Author-provided discriminator. When this changes across update(),
    /// the current content begins exiting and the incoming content is
    /// staged as `pending_children`.
    pub generation_key: Option<String>,
    /// Children staged to mount once the current generation's exit
    /// animations settle. `None` whenever no transition is in flight.
    pending_children: Option<Vec<WidgetRef>>,
    /// Key associated with `pending_children`.
    pending_key: Option<String>,
}

impl PresenceWidget {
    pub fn new() -> Self {
        // A Presence behaves as an invisible grow-container by default so
        // it fills whatever slot its parent allocates. Authors rarely
        // need to style it.
        let mut style = FlexStyle::default();
        style.width = Size::Grow(1.0);
        style.height = Size::Grow(1.0);
        style.direction = Direction::Column;
        let mut inner = FlexWidget::with_style(style);
        inner.block_interactions = false;
        Self {
            inner,
            generation_key: None,
            pending_children: None,
            pending_key: None,
        }
    }

    /// Attempt to exit every current child. Children that can't exit
    /// (no exit_style, no exit-capable descendants) are dropped
    /// immediately; the rest stay in `inner.children` as ghosts.
    fn begin_exit_on_current(&mut self) {
        let children = std::mem::take(&mut self.inner.children);
        let mut kept = Vec::with_capacity(children.len());
        for child in children {
            let can_ghost = {
                let mut g = child.lock().expect("widget lock poisoned");
                g.begin_exit()
            };
            if can_ghost {
                kept.push(child);
            }
        }
        self.inner.children = kept;
    }

    /// Called when the key reverts to the current generation mid-exit.
    /// Cancels the pending mount and unwinds all in-flight exits on
    /// current children so they transition back to their normal state.
    fn cancel_pending(&mut self) {
        self.pending_children = None;
        self.pending_key = None;
        let children = self.inner.children.clone();
        for child in &children {
            let mut g = child.lock().expect("widget lock poisoned");
            g.cancel_exit();
        }
    }

    /// Swap in the pending children. Called from `tick_animations`
    /// once all exiting children have drained.
    fn commit_pending(&mut self) {
        if let Some(pending) = self.pending_children.take() {
            self.inner.children = pending;
            self.generation_key = self.pending_key.take();
        }
    }
}

impl Default for PresenceWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for PresenceWidget {
    fn update(&mut self, new_widget: WidgetRef) -> bool {
        let mut new_widget_guard = new_widget.lock().expect("widget lock poisoned");
        let new_presence = match new_widget_guard.downcast_mut::<PresenceWidget>() {
            Some(p) => p,
            None => return false,
        };

        let new_key = new_presence.generation_key.clone();

        if new_key == self.generation_key {
            // Same generation — just reconcile children normally. If a
            // transition was in flight (author reverted the key mid-
            // exit), cancel it and unwind the exits.
            if self.pending_children.is_some() {
                self.cancel_pending();
            }
            let mut new_children = std::mem::take(&mut new_presence.inner.children);
            self.inner.reconcile_children(&mut new_children);
            return true;
        }

        // Generation changed. If no transition is in flight yet, start
        // one by exiting current children. If a transition is already in
        // flight (rapid key changes), the current exits continue — we
        // just replace the pending content with the latest.
        if self.pending_children.is_none() {
            self.begin_exit_on_current();
        }
        let new_children = std::mem::take(&mut new_presence.inner.children);
        self.pending_children = Some(new_children);
        self.pending_key = new_key;

        // If the outgoing content had nothing to animate (all children
        // dropped instantly), commit right away.
        if self.inner.children.is_empty() {
            self.commit_pending();
        }

        true
    }

    fn tick_animations(&mut self, dt: f32) -> TickResult {
        let mut result = self.inner.tick_animations(dt);

        // inner.tick_animations drains exit-complete children. Once the
        // current generation is empty, we can mount pending.
        if self.pending_children.is_some() && self.inner.children.is_empty() {
            self.commit_pending();
            result.needs_layout = true;
            result.needs_repaint = true;
        }

        result
    }

    // ---- Delegation to inner FlexWidget -------------------------------

    fn get_type(&self) -> &str {
        "presence"
    }

    fn get_dimensions(
        &self,
        ctx: &LayoutContext,
        parent_direction: &Direction,
        parent_width: f32,
        parent_available_width: f32,
        parent_height: f32,
        parent_available_height: f32,
        sibling_basis: f32,
    ) -> (f32, f32) {
        self.inner.get_dimensions(
            ctx,
            parent_direction,
            parent_width,
            parent_available_width,
            parent_height,
            parent_available_height,
            sibling_basis,
        )
    }

    fn get_children(&self) -> Vec<WidgetRef> {
        self.inner.get_children()
    }

    fn get_basis(&self, direction: &Direction) -> f32 {
        self.inner.get_basis(direction)
    }

    fn get_children_basis(&self) -> f32 {
        self.inner.get_children_basis()
    }

    fn get_children_fixed_width(&self) -> f32 {
        self.inner.get_children_fixed_width()
    }

    fn get_children_fixed_height(&self) -> f32 {
        self.inner.get_children_fixed_height()
    }

    fn get_fixed_width(&self) -> Option<f32> {
        self.inner.get_fixed_width()
    }

    fn get_fixed_height(&self) -> Option<f32> {
        self.inner.get_fixed_height()
    }

    fn handle_event(
        &mut self,
        event: &Event,
        ctx: &mut EventContext,
        self_ref: &WidgetRef,
    ) -> bool {
        self.inner.handle_event(event, ctx, self_ref)
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

    fn contains_point(&self, point: &Point) -> bool {
        self.inner.contains_point(point)
    }

    fn get_children_mut(&mut self) -> Vec<WidgetRef> {
        self.inner.get_children_mut()
    }

    fn is_absolute_positioned(&self) -> bool {
        self.inner.is_absolute_positioned()
    }

    fn get_absolute_offset(&self) -> Option<(f32, f32)> {
        self.inner.get_absolute_offset()
    }

    fn set_hovered(&mut self, hovered: bool) {
        self.inner.set_hovered(hovered);
    }

    fn is_hovered(&self) -> bool {
        self.inner.is_hovered()
    }

    fn fire_listeners(&self, event_name: &str, event: &Event) {
        self.inner.fire_listeners(event_name, event);
    }

    fn render(
        &self,
        ctx: &mut dyn crate::widget::RenderContext,
        focused: bool,
        image_cache: &mut crate::widget::image::ImageCache,
    ) {
        self.inner.render(ctx, focused, image_cache);
    }

    fn get_layout_rect(&self) -> Option<&Rect> {
        self.inner.get_layout_rect()
    }

    fn scroll_offset(&self) -> (f32, f32) {
        self.inner.scroll_offset()
    }

    fn needs_post_render(&self) -> bool {
        self.inner.needs_post_render()
    }

    fn post_render(
        &self,
        ctx: &mut dyn crate::widget::RenderContext,
        image_cache: &mut crate::widget::image::ImageCache,
    ) {
        self.inner.post_render(ctx, image_cache);
    }

    fn render_effects(&self) -> Option<RenderEffects> {
        self.inner.render_effects()
    }

    // ---- Exit lifecycle: delegate to inner so parent cascades work ----

    fn is_exiting(&self) -> bool {
        self.inner.is_exiting()
    }

    fn begin_exit(&mut self) -> bool {
        // If the parent is removing this Presence entirely, drop any
        // pending mount — it would never become visible.
        self.pending_children = None;
        self.pending_key = None;
        self.inner.begin_exit()
    }

    fn cancel_exit(&mut self) {
        self.inner.cancel_exit();
    }

    fn is_exit_complete(&self) -> bool {
        self.inner.is_exit_complete()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::animation::TransitionConfig;
    use crate::widget::flex_widget::FlexWidget;
    use crate::widget::style::{Color, FlexStyle};
    use std::sync::{Arc, Mutex};

    fn exit_capable_child() -> WidgetRef {
        // A FlexWidget with an exit style so its begin_exit actually
        // starts an animation.
        let mut w = FlexWidget::new();
        let mut style = FlexStyle::default();
        style.background_color = Some(Color::new(255, 255, 255, 255));
        style.transitions.background_color = Some(TransitionConfig::DEFAULT);
        style.transitions.opacity = Some(TransitionConfig::DEFAULT);
        let mut exit = style.clone();
        exit.opacity = crate::widget::style::Opacity(0.0);
        w.declared_style = style.clone();
        w.style = style;
        w.exit_style = Some(exit);
        Arc::new(Mutex::new(w))
    }

    fn no_exit_child() -> WidgetRef {
        // A plain FlexWidget with no exit_style, so begin_exit returns
        // false and the Presence drops it immediately.
        Arc::new(Mutex::new(FlexWidget::new()))
    }

    fn presence_ref(
        key: Option<&str>,
        children: Vec<WidgetRef>,
    ) -> WidgetRef {
        let mut p = PresenceWidget::new();
        p.generation_key = key.map(|s| s.to_string());
        p.inner.children = children;
        Arc::new(Mutex::new(p))
    }

    #[test]
    fn initial_mount_shows_children() {
        let p = PresenceWidget::new();
        assert!(p.inner.children.is_empty());
        assert!(p.pending_children.is_none());
    }

    #[test]
    fn key_change_without_exit_capability_swaps_immediately() {
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        live.inner.children = vec![no_exit_child()];

        let new = presence_ref("b".into(), vec![no_exit_child()]);
        live.update(new);

        assert_eq!(live.generation_key.as_deref(), Some("b"));
        assert!(live.pending_children.is_none());
        assert_eq!(live.inner.children.len(), 1);
    }

    #[test]
    fn key_change_with_exits_holds_pending_until_settled() {
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        live.inner.children = vec![exit_capable_child()];

        let new = presence_ref("b".into(), vec![exit_capable_child()]);
        live.update(new);

        // Exit is in flight: old child still present as ghost, pending
        // staged, generation_key unchanged.
        assert!(live.pending_children.is_some());
        assert_eq!(live.generation_key.as_deref(), Some("a"));
        assert_eq!(live.inner.children.len(), 1);

        // Tick until the exit spring settles and the ghost drains.
        let mut committed = false;
        for _ in 0..240 {
            live.tick_animations(1.0 / 60.0);
            if live.pending_children.is_none() {
                committed = true;
                break;
            }
        }
        assert!(committed, "pending should commit once ghost settles");
        assert_eq!(live.generation_key.as_deref(), Some("b"));
        assert_eq!(live.inner.children.len(), 1);
    }

    #[test]
    fn rapid_key_changes_replace_pending() {
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        live.inner.children = vec![exit_capable_child()];

        // a -> b: exit starts, b pending.
        let new_b = presence_ref("b".into(), vec![exit_capable_child()]);
        live.update(new_b);
        assert_eq!(live.pending_key.as_deref(), Some("b"));

        // b -> c before b mounts: pending replaced with c, exits continue.
        let new_c = presence_ref("c".into(), vec![exit_capable_child()]);
        live.update(new_c);
        assert_eq!(live.pending_key.as_deref(), Some("c"));
        assert_eq!(live.generation_key.as_deref(), Some("a"));
        // Still exactly one ghost — we didn't restart the exit.
        assert_eq!(live.inner.children.len(), 1);

        for _ in 0..240 {
            live.tick_animations(1.0 / 60.0);
            if live.pending_children.is_none() {
                break;
            }
        }
        assert_eq!(
            live.generation_key.as_deref(),
            Some("c"),
            "final generation should be c, not b"
        );
    }

    #[test]
    fn reverting_key_cancels_pending_and_unwinds_exit() {
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        let a_child = exit_capable_child();
        let a_id = Arc::as_ptr(&a_child) as *const ();
        live.inner.children = vec![a_child];

        // a -> b: exit begins on a, b pending.
        let new_b = presence_ref("b".into(), vec![exit_capable_child()]);
        live.update(new_b);
        assert!(live.pending_children.is_some());

        // Confirm the 'a' child is actually exiting.
        {
            let g = live.inner.children[0].lock().unwrap();
            assert!(g.is_exiting());
        }

        // Tick a bit so the exit has some progress (interrupt mid-flight).
        for _ in 0..5 {
            live.tick_animations(1.0 / 60.0);
        }

        // b -> a: revert. Pending dropped; exit on current unwound.
        let new_a = presence_ref("a".into(), vec![exit_capable_child()]);
        live.update(new_a);
        assert!(live.pending_children.is_none());
        assert_eq!(live.generation_key.as_deref(), Some("a"));

        // The 'a' child we started with should still be in place, no
        // longer exiting.
        assert_eq!(live.inner.children.len(), 1);
        assert_eq!(Arc::as_ptr(&live.inner.children[0]) as *const (), a_id);
        {
            let g = live.inner.children[0].lock().unwrap();
            assert!(!g.is_exiting());
        }
    }
}
