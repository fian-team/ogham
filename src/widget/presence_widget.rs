//! Lifecycle-sequencing container: holds one generation of content at
//! a time and sequences transitions between generations.
//!
//! Authors use it to gate page/route transitions on exit animations:
//!
//! ```text
//! Presence {
//!   key: current_page_id,
//!   mode: "wait",          // optional; default "pop"
//!   children: [ route_body() ]
//! }
//! ```
//!
//! When `key` changes, every current child gets `begin_exit()`. What
//! happens next depends on `mode` (see [`PresenceMode`]):
//!
//! - **`pop`** (default): exiting children are popped out of layout
//!   flow — pinned as ghosts at their last layout rect — and the new
//!   generation mounts immediately, playing its entry animations while
//!   the ghosts fade out above it. Ghosts receive no input and drain
//!   individually as their springs settle.
//! - **`wait`**: children with exit animations (or exit-capable
//!   descendants) stay in the tree as in-flow ghosts until their
//!   springs settle; the new content is held aside as `pending` and
//!   only mounted once no ghosts remain. For deliberately sequenced
//!   choreography.
//!
//! A generation's children ordinarily **flow**, like any Flex's.
//! `stack: true` layers them instead — every child on the whole content
//! box, the last one declared on top and first to be offered a press.
//! That is what the outlet renders more than one visible view with
//! (`lorekeeper/docs/ROUTING.md` §13.5), and it is opt-in because two
//! things in one generation are usually two things side by side.
//!
//! Design doc: `docs/internal/PRESENCE_POP.md`.

use super::flex_widget::FlexWidget;
use super::style::{Direction, FlexStyle, Size};
use super::{RenderEffects, TickResult, UpdateResult, Widget, WidgetRef};
use crate::widget::event::{Event, EventContext};
use crate::widget::point::Point;
use crate::widget::rect::Rect;
use crate::widget::LayoutContext;

/// Transition sequencing policy for a [`PresenceWidget`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PresenceMode {
    /// Overlapped: on a key change the outgoing generation is popped
    /// out of layout flow (pinned at its last layout rect as a ghost,
    /// painted above the live children, hit-test-invisible) and the
    /// incoming generation mounts immediately.
    #[default]
    Pop,
    /// Serial: the outgoing generation exits fully before the incoming
    /// one mounts. The pre-pop behaviour, retained for deliberately
    /// sequenced choreography.
    Wait,
}

/// A popped exiting child (pop mode): out of layout flow, pinned at
/// the rect it occupied when its generation was replaced.
struct Ghost {
    widget: WidgetRef,
    /// Layout rect at pop time, in Presence content space (the same
    /// parent-relative space `inner`'s children lay out in). Frozen —
    /// the ghost's interior keeps reflowing inside it, the rect
    /// itself never moves.
    rect: Rect,
}

/// Container widget that sequences transitions between generations of
/// content. See module docs for semantics.
pub struct PresenceWidget {
    /// Inner flex that owns layout, rendering, and the currently-visible
    /// children (including any in-flow exit ghosts in wait mode).
    pub inner: FlexWidget,
    /// Author-provided discriminator. When this changes across update(),
    /// the current content begins exiting; `mode` decides whether the
    /// incoming content mounts immediately (pop) or is staged (wait).
    pub generation_key: Option<String>,
    /// Transition sequencing policy. Adopted from the incoming widget
    /// on every update; applies to transitions started after that.
    pub mode: PresenceMode,
    /// Pop mode: exiting children pinned outside layout flow. Ticked,
    /// laid out (inside their frozen rect), and rendered by this
    /// widget; drained individually as each settles. Never consulted
    /// by reconciliation — a ghost's key colliding with an incoming
    /// child's must not cancel its exit (PRESENCE_POP.md §4).
    ghosts: Vec<Ghost>,
    /// Whether a parent-initiated exit is in flight (`begin_exit` was
    /// accepted). Needed alongside `inner.is_exiting()` because a
    /// Presence can ghost purely on account of its ghost pile, and a
    /// live Presence with ghosts must NOT read as exiting to its
    /// parent's reconciler.
    exiting: bool,
    /// Children staged to mount once the current generation's exit
    /// animations settle (wait mode). `None` whenever no wait
    /// transition is in flight.
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
            mode: PresenceMode::default(),
            ghosts: Vec::new(),
            exiting: false,
            pending_children: None,
            pending_key: None,
        }
    }

    /// Pop the whole current generation out of layout flow (pop mode
    /// key change): every child gets `begin_exit()`; exit-capable
    /// children that have a layout rect become ghosts pinned at it,
    /// everything else is dropped. Returns the `owned_path_prefix` of
    /// EVERY outgoing child — ghosted or dropped — because pop mode
    /// flushes lifecycle prefixes at replacement time, not at drain
    /// time: the incoming generation may re-own the same call-stack
    /// paths this very cycle, and a late flush would clobber its
    /// freshly-registered state (PRESENCE_POP.md §7).
    fn pop_current(&mut self) -> Vec<String> {
        let children = std::mem::take(&mut self.inner.children);
        let mut drained = Vec::new();
        for child in children {
            let (can_ghost, prefix, rect) = {
                let mut g = child.lock().expect("widget lock poisoned");
                let p = g.owned_path_prefix().to_string();
                let r = g.get_layout_rect().cloned();
                (g.begin_exit(), p, r)
            };
            if !prefix.is_empty() {
                drained.push(prefix);
            }
            if can_ghost {
                if let Some(rect) = rect {
                    self.ghosts.push(Ghost {
                        widget: child,
                        rect,
                    });
                }
                // No rect = never laid out; nothing visible to fade.
                // Dropped by falling out of scope.
            }
        }
        drained
    }

    /// Attempt to exit every current child. Children that can't exit
    /// (no exit_style, no exit-capable descendants) are dropped
    /// immediately; the rest stay in `inner.children` as ghosts.
    /// Returns the `owned_path_prefix` of every dropped child so the
    /// caller can push them into the `UpdateResult.drained_path_prefixes`
    /// for drain-time hook flushing.
    fn begin_exit_on_current(&mut self) -> Vec<String> {
        let children = std::mem::take(&mut self.inner.children);
        let mut kept = Vec::with_capacity(children.len());
        let mut drained = Vec::new();
        for child in children {
            let (can_ghost, prefix) = {
                let mut g = child.lock().expect("widget lock poisoned");
                let p = g.owned_path_prefix().to_string();
                (g.begin_exit(), p)
            };
            if can_ghost {
                kept.push(child);
            } else if !prefix.is_empty() {
                drained.push(prefix);
            }
        }
        self.inner.children = kept;
        drained
    }

    /// Called when the key reverts to the current generation mid-exit.
    /// Cancels the pending mount and unwinds all in-flight exits on
    /// current children so they transition back to their normal state.
    /// Returns the `owned_path_prefix` of every cancelled exit so the
    /// caller can push them into the
    /// `UpdateResult.cancelled_unmount_prefixes`.
    fn cancel_pending(&mut self) -> Vec<String> {
        self.pending_children = None;
        self.pending_key = None;
        let children = self.inner.children.clone();
        let mut cancelled = Vec::new();
        for child in &children {
            let mut g = child.lock().expect("widget lock poisoned");
            let prefix = g.owned_path_prefix().to_string();
            g.cancel_exit();
            if !prefix.is_empty() {
                cancelled.push(prefix);
            }
        }
        cancelled
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
    fn update(&mut self, new_widget: WidgetRef) -> UpdateResult {
        let mut new_widget_guard = new_widget.lock().expect("widget lock poisoned");
        let new_presence = match new_widget_guard.downcast_mut::<PresenceWidget>() {
            Some(p) => p,
            None => return UpdateResult::replace(),
        };

        let new_key = new_presence.generation_key.clone();

        // Adopt the new inner styling (width/height/direction etc. — anything
        // the author declared via `style:` on the Presence). Keeps layout
        // responsive to authored changes without going through
        // reconcile_children's FlexWidget::update path. The clobber of `style`
        // (vs honoring an in-progress spring) is intentional: the Presence's
        // inner Flex has no transitions exposed via the builder, so `style`
        // and `declared_style` are always equal. If transitions ever get
        // exposed on Presence, replace this with a proper retarget path.
        //
        // Asked before the clobber, because the same-generation path below
        // reports what actually changed and this is one of the two things
        // that can have.
        let own_layout_changed = !self
            .inner
            .declared_style
            .layout_equal(&new_presence.inner.declared_style);
        let own_paint_changed = !self
            .inner
            .declared_style
            .paint_equal(&new_presence.inner.declared_style);
        self.inner.declared_style = new_presence.inner.declared_style.clone();
        self.inner.style = self.inner.declared_style.clone();

        // Adopt the incoming mode. It governs transitions started from
        // here on; an in-flight wait transition (staged pending) still
        // completes under wait rules, and existing pop ghosts drain
        // regardless of mode.
        self.mode = new_presence.mode;
        // And the incoming stacking, for the same reason the style above
        // is adopted: it is a declaration on the widget, and a rerender is
        // where a changed declaration arrives.
        self.inner.stack = new_presence.inner.stack;

        if new_key == self.generation_key {
            // Same generation — just reconcile children normally. If a
            // transition was in flight (author reverted the key mid-
            // exit), cancel it and unwind the exits.
            let mut cancelled = if self.pending_children.is_some() {
                self.cancel_pending()
            } else {
                Vec::new()
            };
            let mut new_children = std::mem::take(&mut new_presence.inner.children);
            let inner_result = self.inner.reconcile_children(&mut new_children);
            // **Report what changed, not that something might have.** This
            // returned a flat `layout_changed()` and threw `inner_result`
            // away, so *any* rerender of a document rooted in a Presence —
            // which is every document the route tier mounts — laid the whole
            // tree out again, whatever had actually moved. A host animating
            // one opacity through host_state therefore paid a full layout per
            // frame; `untold_lore`'s three-second title arrival showed up as
            // ~135 `layout()` calls a second under the runtime's own
            // dirty-marking warning, and the field it was blamed on was
            // innocent. Nothing about the same-generation path needs a
            // relayout of its own: reconciling the children is exactly the
            // question, and the Presence's own inner style is asked above.
            let mut result = UpdateResult {
                absorbed: true,
                needs_layout: own_layout_changed || inner_result.needs_layout,
                needs_repaint: own_layout_changed
                    || own_paint_changed
                    || inner_result.needs_repaint,
                cancelled_unmount_prefixes: Vec::new(),
                drained_path_prefixes: Vec::new(),
            };
            cancelled.extend(inner_result.cancelled_unmount_prefixes);
            result.cancelled_unmount_prefixes = cancelled;
            result.drained_path_prefixes = inner_result.drained_path_prefixes;
            return result;
        }

        // Generation changed. Pop mode (no wait transition in flight):
        // pop the outgoing generation onto the ghost pile and mount the
        // newcomer immediately. Rapid key changes just accumulate ghost
        // cohorts; a revert to a prior key is not a special case — the
        // old generation's ghost stays dying and a fresh subtree mounts.
        if self.mode == PresenceMode::Pop && self.pending_children.is_none() {
            let drained = self.pop_current();
            self.inner.children = std::mem::take(&mut new_presence.inner.children);
            self.generation_key = new_key;

            let mut result = UpdateResult::layout_changed();
            result.drained_path_prefixes = drained;
            return result;
        }

        // Wait mode. If no transition is in flight yet, start one by
        // exiting current children. If a transition is already in
        // flight (rapid key changes — or a wait transition surviving a
        // mode flip to pop), the current exits continue; we just
        // replace the pending content with the latest.
        let drained = if self.pending_children.is_none() {
            self.begin_exit_on_current()
        } else {
            Vec::new()
        };
        let new_children = std::mem::take(&mut new_presence.inner.children);
        self.pending_children = Some(new_children);
        self.pending_key = new_key;

        // If the outgoing content had nothing to animate (all children
        // dropped instantly), commit right away.
        if self.inner.children.is_empty() {
            self.commit_pending();
        }

        let mut result = UpdateResult::layout_changed();
        result.drained_path_prefixes = drained;
        result
    }

    fn tick_animations(&mut self, ctx: &mut crate::widget::event::TickContext) -> TickResult {
        let mut result = self.inner.tick_animations(ctx);

        // Wait mode: inner.tick_animations drains exit-complete
        // children. Once the current generation is empty, we can
        // mount pending.
        if self.pending_children.is_some() && self.inner.children.is_empty() {
            self.commit_pending();
            result.needs_layout = true;
            result.needs_repaint = true;
        }

        // Pop mode: advance ghost springs, then drop ghosts whose
        // exits have settled. Ghosts are out of flow, so a drain needs
        // a repaint but no relayout — and pushes no prefixes, because
        // pop_current already flushed them at replacement time.
        for ghost in &self.ghosts {
            let ghost_result = {
                let mut g = ghost.widget.lock().expect("widget lock poisoned");
                g.tick_animations(ctx)
            };
            result = result.merge(ghost_result);
        }
        let before = self.ghosts.len();
        self.ghosts.retain(|ghost| {
            !ghost
                .widget
                .lock()
                .expect("widget lock poisoned")
                .is_exit_complete()
        });
        if self.ghosts.len() != before {
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
        // Ghosts come last: the render walk paints children in order,
        // so ghosts-last means the dying generation composites above
        // the live one. They are excluded from hit-testing by the
        // global exiting-widgets-are-hit-test-invisible invariant, not
        // by omission here.
        let mut children = self.inner.get_children();
        children.extend(self.ghosts.iter().map(|g| g.widget.clone()));
        children
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
        // Ghosts re-lay out inside their frozen rect every pass so
        // interior layout-affecting exit animations keep reflowing;
        // the rect itself never moves. Frozen size doubles as both
        // parent and available dims (sibling_basis 0) so a Grow ghost
        // resolves to exactly the slot it died in.
        let direction = self.inner.style.direction.clone();
        for ghost in &self.ghosts {
            let mut g = ghost.widget.lock().expect("widget lock poisoned");
            g.layout(
                ctx,
                ghost.rect.x,
                ghost.rect.y,
                &direction,
                ghost.rect.width,
                ghost.rect.width,
                ghost.rect.height,
                ghost.rect.height,
                0.0,
            );
        }
    }

    fn contains_point(&self, point: &Point) -> bool {
        self.inner.contains_point(point)
    }

    fn get_children_mut(&mut self) -> Vec<WidgetRef> {
        // Same set and order as get_children — see the note there.
        let mut children = self.inner.get_children_mut();
        children.extend(self.ghosts.iter().map(|g| g.widget.clone()));
        children
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

    // ---- Exit lifecycle (parent removing this Presence) --------------

    fn is_exiting(&self) -> bool {
        // Own flag, not the ghost pile: a live Presence mid-pop-
        // transition must not read as an exiting ghost to its parent's
        // reconciler.
        self.exiting || self.inner.is_exiting()
    }

    fn begin_exit(&mut self) -> bool {
        // If the parent is removing this Presence entirely, drop any
        // pending mount — it would never become visible. Ghosts stay:
        // they are already exiting and visible, and their settling
        // gates our exit completion.
        self.pending_children = None;
        self.pending_key = None;
        let inner_accepted = self.inner.begin_exit();
        if inner_accepted || !self.ghosts.is_empty() {
            self.exiting = true;
        }
        self.exiting
    }

    fn cancel_exit(&mut self) {
        self.exiting = false;
        self.inner.cancel_exit();
        // Ghosts are from a dead generation — they keep dying.
    }

    fn is_exit_complete(&self) -> bool {
        if !self.is_exiting() {
            return false;
        }
        // The inner guard matters: a Presence can be exiting purely on
        // account of its ghost pile, and FlexWidget::is_exit_complete
        // reports false whenever the flex itself isn't exiting.
        let inner_done = !self.inner.is_exiting() || self.inner.is_exit_complete();
        inner_done
            && self.ghosts.iter().all(|ghost| {
                ghost
                    .widget
                    .lock()
                    .expect("widget lock poisoned")
                    .is_exit_complete()
            })
    }

    fn restart_entry_animation(&mut self) {
        // Pending children (if any) are freshly-built and already at
        // their initial state — no-op for them. We only restart on the
        // live generation by delegating to inner so the cascade reaches
        // every animated descendant. Ghosts keep dying: a re-promoted
        // screen may briefly show a stale ghost finishing its fade.
        self.exiting = false;
        self.inner.restart_entry_animation();
    }

    fn add_group_delay(&mut self, secs: f32) {
        self.inner.add_group_delay(secs);
    }

    fn strip_inherited_group_delay(&mut self) {
        self.inner.strip_inherited_group_delay();
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
        // starts an animation. Carries a layout rect (as any child
        // that has rendered at least one frame would) so pop mode can
        // pin it as a ghost.
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
        w.layout = Some(Rect::new(5.0, 10.0, 100.0, 50.0));
        Arc::new(Mutex::new(w))
    }

    fn no_exit_child() -> WidgetRef {
        // A plain FlexWidget with no exit_style, so begin_exit returns
        // false and the Presence drops it immediately.
        Arc::new(Mutex::new(FlexWidget::new()))
    }

    fn presence_ref(mode: PresenceMode, key: Option<&str>, children: Vec<WidgetRef>) -> WidgetRef {
        // `mode` matters on the incoming widget too: update() adopts it.
        let mut p = PresenceWidget::new();
        p.mode = mode;
        p.generation_key = key.map(|s| s.to_string());
        p.inner.children = children;
        Arc::new(Mutex::new(p))
    }

    /// A plain child at a stated opacity and padding, so a reconcile can be
    /// handed a paint-only change or a geometry one.
    fn styled_child(opacity: f32, padding: f32) -> WidgetRef {
        let mut w = FlexWidget::new();
        let mut style = FlexStyle::default();
        style.opacity = crate::widget::style::Opacity(opacity);
        style.padding = crate::widget::style::Spacing::new(padding, padding, padding, padding);
        w.declared_style = style.clone();
        w.style = style;
        Arc::new(Mutex::new(w))
    }

    /// **A same-generation reconcile reports what changed**, and a
    /// paint-only change is not a relayout.
    ///
    /// This path returned a flat `UpdateResult::layout_changed()` and threw
    /// the children's own result away, so *any* rerender of a document
    /// rooted in a Presence — every document the route tier mounts — laid
    /// the whole tree out again. A host animating one opacity through
    /// host_state paid a full layout every frame: `untold_lore`'s title
    /// arrival tripped the runtime's own dirty-marking warning at ~135
    /// `layout()` calls a second, and the projected field it was blamed on
    /// was innocent.
    #[test]
    fn a_same_generation_reconcile_relayouts_only_when_geometry_moved() {
        let mut presence = PresenceWidget::new();
        presence.generation_key = Some("title".to_string());
        presence.inner.children = vec![styled_child(1.0, 8.0)];

        // Same opacity, same padding: nothing at all.
        let result = presence.update(presence_ref(
            PresenceMode::Wait,
            Some("title"),
            vec![styled_child(1.0, 8.0)],
        ));
        assert!(!result.needs_layout, "an identical tree moved nothing");

        // Opacity alone — the shape of a fade driven from host state.
        let result = presence.update(presence_ref(
            PresenceMode::Wait,
            Some("title"),
            vec![styled_child(0.4, 8.0)],
        ));
        assert!(
            !result.needs_layout,
            "an opacity is paint: a fade must not relayout the tree"
        );
        assert!(result.needs_repaint, "…but it does have to be redrawn");

        // Padding — geometry, and it must still reach the layout pass.
        let result = presence.update(presence_ref(
            PresenceMode::Wait,
            Some("title"),
            vec![styled_child(0.4, 20.0)],
        ));
        assert!(result.needs_layout, "geometry still relayouts");
    }

    /// The Presence's *own* inner style is adopted here rather than through
    /// `FlexWidget::update`, so it is the one change `reconcile_children`
    /// cannot see — and it has to be asked before the clobber.
    #[test]
    fn the_presences_own_geometry_still_relayouts() {
        let mut presence = PresenceWidget::new();
        presence.generation_key = Some("title".to_string());
        presence.inner.declared_style.gap = 4.0;

        let mut incoming = PresenceWidget::new();
        incoming.generation_key = Some("title".to_string());
        incoming.inner.declared_style.gap = 12.0;
        let result = presence.update(Arc::new(Mutex::new(incoming)));
        assert!(
            result.needs_layout,
            "the Presence's own gap moved its children"
        );
    }

    #[test]
    fn initial_mount_shows_children() {
        let p = PresenceWidget::new();
        assert!(p.inner.children.is_empty());
        assert!(p.pending_children.is_none());
    }

    #[test]
    fn restart_entry_animation_cascades_into_inner() {
        // Presence delegates restart to inner.restart_entry_animation,
        // which cascades to the live children. Verifies that a child
        // with `initial:` declared replays its entry when the Presence
        // is restarted (route promotion path).
        use crate::widget::style::{Color, FlexStyle};

        let exit_child: WidgetRef = {
            let mut w = FlexWidget::new();
            let mut declared = FlexStyle::default();
            declared.background_color = Some(Color::new(255, 255, 255, 255));
            declared.transitions.background_color = Some(TransitionConfig::DEFAULT);
            let mut initial = declared.clone();
            initial.background_color = Some(Color::new(0, 0, 0, 255));
            w.declared_style = declared.clone();
            w.initial_style = Some(initial);
            w.style = declared;
            Arc::new(Mutex::new(w))
        };

        let mut presence = PresenceWidget::new();
        presence.generation_key = Some("a".to_string());
        presence.inner.children = vec![exit_child.clone()];

        presence.restart_entry_animation();

        let g = exit_child.lock().expect("widget lock poisoned");
        let c = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
        assert_eq!(
            c.style.background_color,
            Some(Color::new(0, 0, 0, 255)),
            "Presence's restart must cascade into inner children"
        );
    }

    #[test]
    fn update_adopts_new_inner_style_on_same_key() {
        // A Presence's `style:` overrides (parsed by the builder) need to
        // propagate across same-key reconciles. Without this, a Presence
        // whose author switches from Grow to Shrink (or rotates direction)
        // never picks up the new layout.
        use crate::widget::style::{Direction, Size};
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        // Defaults: Grow × Grow column.
        assert_eq!(live.inner.declared_style.width, Size::Grow(1.0));
        assert_eq!(live.inner.declared_style.height, Size::Grow(1.0));

        let new = {
            let mut p = PresenceWidget::new();
            p.generation_key = Some("a".to_string());
            p.inner.declared_style.height = Size::Shrink;
            p.inner.declared_style.direction = Direction::Row;
            p.inner.style = p.inner.declared_style.clone();
            Arc::new(Mutex::new(p))
        };
        live.update(new);

        assert_eq!(live.inner.declared_style.height, Size::Shrink);
        assert_eq!(live.inner.declared_style.direction, Direction::Row);
        assert_eq!(live.inner.style.height, Size::Shrink);
    }

    #[test]
    fn update_adopts_new_inner_style_on_generation_swap() {
        // Same propagation must happen across a generation key change so
        // a route that swaps content AND restyles the Presence (e.g.,
        // shrink in one route, grow in another) lands at the right size.
        use crate::widget::style::Size;
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        live.inner.children = vec![exit_capable_child()];

        let new = {
            let mut p = PresenceWidget::new();
            p.generation_key = Some("b".to_string());
            p.inner.declared_style.height = Size::Shrink;
            p.inner.style = p.inner.declared_style.clone();
            p.inner.children = vec![exit_capable_child()];
            Arc::new(Mutex::new(p))
        };
        live.update(new);

        assert_eq!(live.inner.declared_style.height, Size::Shrink);
        assert_eq!(live.inner.style.height, Size::Shrink);
    }

    #[test]
    fn key_change_pushes_drained_prefix_for_no_exit_child() {
        // Phase 3 M3 propagation: when a Presence's key changes
        // and the outgoing child can't ghost (no exit_style), the
        // child is dropped immediately. Its owned_path_prefix
        // must be pushed to UpdateResult.drained_path_prefixes
        // so the runtime can flush its lifecycle hooks.
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        let owned: WidgetRef = {
            let mut w = FlexWidget::new();
            w.owned_path_prefix = "panel".to_string();
            Arc::new(Mutex::new(w))
        };
        live.inner.children = vec![owned];

        let new = presence_ref(PresenceMode::Pop, "b".into(), vec![no_exit_child()]);
        let result = live.update(new);
        assert!(
            result.drained_path_prefixes.iter().any(|p| p == "panel"),
            "no-exit child's owned_path_prefix should be in drained_path_prefixes; got {:?}",
            result.drained_path_prefixes
        );
    }

    #[test]
    fn cancel_pending_pushes_cancelled_prefix() {
        // Phase 3 M3 propagation: when the key reverts mid-exit,
        // each child whose exit gets cancelled must push its
        // owned_path_prefix to cancelled_unmount_prefixes so the
        // runtime clears any candidate_unmount staged earlier.
        let mut live = PresenceWidget::new();
        live.mode = PresenceMode::Wait;
        live.generation_key = Some("a".to_string());
        let exit_child: WidgetRef = {
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
            w.owned_path_prefix = "panel".to_string();
            Arc::new(Mutex::new(w))
        };
        live.inner.children = vec![exit_child];

        // Step 1: key change → exit begins, pending staged.
        let new_b = presence_ref(PresenceMode::Wait, "b".into(), vec![exit_capable_child()]);
        live.update(new_b);
        assert!(live.pending_children.is_some());

        // Step 2: revert to "a" → cancel_pending unwinds the
        // exit. The cancelled prefix should appear in the
        // returned UpdateResult.
        let revert = presence_ref(PresenceMode::Wait, "a".into(), vec![exit_capable_child()]);
        let result = live.update(revert);
        assert!(
            result
                .cancelled_unmount_prefixes
                .iter()
                .any(|p| p == "panel"),
            "cancelled exit's prefix should be reported; got {:?}",
            result.cancelled_unmount_prefixes
        );
    }

    #[test]
    fn key_change_without_exit_capability_swaps_immediately() {
        // Holds in both modes; runs under the pop default here.
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        live.inner.children = vec![no_exit_child()];

        let new = presence_ref(PresenceMode::Pop, "b".into(), vec![no_exit_child()]);
        live.update(new);

        assert_eq!(live.generation_key.as_deref(), Some("b"));
        assert!(live.pending_children.is_none());
        assert!(live.ghosts.is_empty());
        assert_eq!(live.inner.children.len(), 1);
    }

    #[test]
    fn key_change_with_exits_holds_pending_until_settled() {
        let mut live = PresenceWidget::new();
        live.mode = PresenceMode::Wait;
        live.generation_key = Some("a".to_string());
        live.inner.children = vec![exit_capable_child()];

        let new = presence_ref(PresenceMode::Wait, "b".into(), vec![exit_capable_child()]);
        live.update(new);

        // Exit is in flight: old child still present as ghost, pending
        // staged, generation_key unchanged.
        assert!(live.pending_children.is_some());
        assert_eq!(live.generation_key.as_deref(), Some("a"));
        assert_eq!(live.inner.children.len(), 1);

        // Tick until the exit spring settles and the ghost drains.
        let mut committed = false;
        for _ in 0..240 {
            {
                let mut ctx = crate::widget::event::TickContext::new(1.0 / 60.0);
                live.tick_animations(&mut ctx);
            }
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
        live.mode = PresenceMode::Wait;
        live.generation_key = Some("a".to_string());
        live.inner.children = vec![exit_capable_child()];

        // a -> b: exit starts, b pending.
        let new_b = presence_ref(PresenceMode::Wait, "b".into(), vec![exit_capable_child()]);
        live.update(new_b);
        assert_eq!(live.pending_key.as_deref(), Some("b"));

        // b -> c before b mounts: pending replaced with c, exits continue.
        let new_c = presence_ref(PresenceMode::Wait, "c".into(), vec![exit_capable_child()]);
        live.update(new_c);
        assert_eq!(live.pending_key.as_deref(), Some("c"));
        assert_eq!(live.generation_key.as_deref(), Some("a"));
        // Still exactly one ghost — we didn't restart the exit.
        assert_eq!(live.inner.children.len(), 1);

        for _ in 0..240 {
            {
                let mut ctx = crate::widget::event::TickContext::new(1.0 / 60.0);
                live.tick_animations(&mut ctx);
            }
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
        live.mode = PresenceMode::Wait;
        live.generation_key = Some("a".to_string());
        let a_child = exit_capable_child();
        let a_id = Arc::as_ptr(&a_child) as *const ();
        live.inner.children = vec![a_child];

        // a -> b: exit begins on a, b pending.
        let new_b = presence_ref(PresenceMode::Wait, "b".into(), vec![exit_capable_child()]);
        live.update(new_b);
        assert!(live.pending_children.is_some());

        // Confirm the 'a' child is actually exiting.
        {
            let g = live.inner.children[0].lock().unwrap();
            assert!(g.is_exiting());
        }

        // Tick a bit so the exit has some progress (interrupt mid-flight).
        for _ in 0..5 {
            {
                let mut ctx = crate::widget::event::TickContext::new(1.0 / 60.0);
                live.tick_animations(&mut ctx);
            }
        }

        // b -> a: revert. Pending dropped; exit on current unwound.
        let new_a = presence_ref(PresenceMode::Wait, "a".into(), vec![exit_capable_child()]);
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

    // ---- Pop mode ----------------------------------------------------

    fn tick_once(live: &mut PresenceWidget) -> (TickResult, Vec<String>) {
        let mut ctx = crate::widget::event::TickContext::new(1.0 / 60.0);
        let result = live.tick_animations(&mut ctx);
        (result, ctx.drained_path_prefixes)
    }

    #[test]
    fn pop_key_change_mounts_immediately_and_pins_ghost() {
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        let old = exit_capable_child();
        let old_id = Arc::as_ptr(&old) as *const ();
        live.inner.children = vec![old];

        let new = presence_ref(PresenceMode::Pop, "b".into(), vec![exit_capable_child()]);
        live.update(new);

        // New generation is live NOW — no pending, key advanced.
        assert_eq!(live.generation_key.as_deref(), Some("b"));
        assert!(live.pending_children.is_none());
        assert_eq!(live.inner.children.len(), 1);
        assert_ne!(Arc::as_ptr(&live.inner.children[0]) as *const (), old_id);

        // Old child is a ghost, exiting, pinned at its pop-time rect.
        assert_eq!(live.ghosts.len(), 1);
        assert_eq!(Arc::as_ptr(&live.ghosts[0].widget) as *const (), old_id);
        let rect = &live.ghosts[0].rect;
        assert_eq!((rect.x, rect.y), (5.0, 10.0));
        assert_eq!((rect.width, rect.height), (100.0, 50.0));
        assert!(live.ghosts[0].widget.lock().unwrap().is_exiting());
    }

    #[test]
    fn pop_flushes_prefix_at_pop_time_not_at_drain() {
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        let owned = exit_capable_child();
        owned
            .lock()
            .unwrap()
            .downcast_mut::<FlexWidget>()
            .unwrap()
            .owned_path_prefix = "panel".to_string();
        live.inner.children = vec![owned];

        let new = presence_ref(PresenceMode::Pop, "b".into(), vec![no_exit_child()]);
        let result = live.update(new);

        // Ghosted — but the prefix is reported at pop time (§7).
        assert_eq!(live.ghosts.len(), 1);
        assert!(
            result.drained_path_prefixes.iter().any(|p| p == "panel"),
            "pop must flush the outgoing generation's prefix at replacement; got {:?}",
            result.drained_path_prefixes
        );

        // ...and never again at drain.
        for _ in 0..240 {
            let (_, drained) = tick_once(&mut live);
            assert!(
                drained.is_empty(),
                "ghost drain must not re-push prefixes; got {:?}",
                drained
            );
            if live.ghosts.is_empty() {
                return;
            }
        }
        panic!("ghost should have drained within 240 ticks");
    }

    #[test]
    fn pop_ghost_drains_on_settle_with_repaint_but_no_relayout() {
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        live.inner.children = vec![exit_capable_child()];

        // New generation without springs so inner tick stays quiet and
        // the assertions read the ghost path alone.
        let new = presence_ref(PresenceMode::Pop, "b".into(), vec![no_exit_child()]);
        live.update(new);
        assert_eq!(live.ghosts.len(), 1);

        for _ in 0..240 {
            let (result, _) = tick_once(&mut live);
            if live.ghosts.is_empty() {
                assert!(result.needs_repaint, "drain tick must request a repaint");
                assert!(
                    !result.needs_layout,
                    "ghosts are out of flow; their drain must not trigger relayout"
                );
                return;
            }
        }
        panic!("ghost should have drained within 240 ticks");
    }

    #[test]
    fn pop_rapid_key_changes_accumulate_ghost_cohorts() {
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        live.inner.children = vec![exit_capable_child()];

        let new_b = presence_ref(PresenceMode::Pop, "b".into(), vec![exit_capable_child()]);
        live.update(new_b);
        let new_c = presence_ref(PresenceMode::Pop, "c".into(), vec![exit_capable_child()]);
        live.update(new_c);

        // Two dying cohorts (a's child, b's child); c is live.
        assert_eq!(live.ghosts.len(), 2);
        assert_eq!(live.generation_key.as_deref(), Some("c"));
        assert_eq!(live.inner.children.len(), 1);

        // The pile self-drains.
        for _ in 0..240 {
            let _ = tick_once(&mut live);
            if live.ghosts.is_empty() {
                return;
            }
        }
        panic!("ghost cohorts should have drained within 240 ticks");
    }

    #[test]
    fn pop_revert_mounts_fresh_subtree_while_ghost_keeps_dying() {
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        let a_child = exit_capable_child();
        let a_id = Arc::as_ptr(&a_child) as *const ();
        live.inner.children = vec![a_child];

        let new_b = presence_ref(PresenceMode::Pop, "b".into(), vec![exit_capable_child()]);
        live.update(new_b);
        // b -> a revert: NOT a special case in pop mode.
        let new_a = presence_ref(PresenceMode::Pop, "a".into(), vec![exit_capable_child()]);
        live.update(new_a);

        assert_eq!(live.generation_key.as_deref(), Some("a"));
        // The original 'a' child is still a ghost, still exiting — a
        // FRESH 'a' subtree mounted instead.
        assert_eq!(live.ghosts.len(), 2);
        let ghost_ids: Vec<*const ()> = live
            .ghosts
            .iter()
            .map(|g| Arc::as_ptr(&g.widget) as *const ())
            .collect();
        assert!(ghost_ids.contains(&a_id));
        assert!(live
            .ghosts
            .iter()
            .all(|g| g.widget.lock().unwrap().is_exiting()));
        assert_ne!(Arc::as_ptr(&live.inner.children[0]) as *const (), a_id);
    }

    #[test]
    fn pop_child_without_layout_rect_is_dropped_not_ghosted() {
        // Exit-capable but never laid out: nothing visible to fade, so
        // it drops immediately with its prefix flushed.
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        let unlaid = exit_capable_child();
        {
            let mut g = unlaid.lock().unwrap();
            let f = g.downcast_mut::<FlexWidget>().unwrap();
            f.layout = None;
            f.owned_path_prefix = "panel".to_string();
        }
        live.inner.children = vec![unlaid];

        let new = presence_ref(PresenceMode::Pop, "b".into(), vec![no_exit_child()]);
        let result = live.update(new);

        assert!(live.ghosts.is_empty());
        assert!(result.drained_path_prefixes.iter().any(|p| p == "panel"));
    }

    #[test]
    fn presence_with_only_ghosts_still_ghosts_for_its_parent() {
        // Parent removes a Presence whose live generation has no exit
        // capability but whose ghost pile is still draining: it must
        // report exit-capable and complete only when the pile empties.
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        live.inner.children = vec![exit_capable_child()];
        let new = presence_ref(PresenceMode::Pop, "b".into(), vec![no_exit_child()]);
        live.update(new);
        assert_eq!(live.ghosts.len(), 1);

        assert!(live.begin_exit(), "ghost pile alone must accept the exit");
        assert!(live.is_exiting());
        assert!(!live.is_exit_complete());

        for _ in 0..240 {
            let _ = tick_once(&mut live);
            if live.is_exit_complete() {
                return;
            }
        }
        panic!("presence exit should complete once ghosts drain");
    }

    #[test]
    fn live_presence_with_ghosts_is_not_exiting() {
        // A parent's reconciler must not mistake a mid-pop-transition
        // Presence for an exiting ghost.
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        live.inner.children = vec![exit_capable_child()];
        let new = presence_ref(PresenceMode::Pop, "b".into(), vec![exit_capable_child()]);
        live.update(new);

        assert_eq!(live.ghosts.len(), 1);
        assert!(!live.is_exiting());
        assert!(!live.is_exit_complete());
    }

    #[test]
    fn get_children_appends_ghosts_last_for_paint_order() {
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        let old = exit_capable_child();
        let old_id = Arc::as_ptr(&old) as *const ();
        live.inner.children = vec![old];
        let new = presence_ref(PresenceMode::Pop, "b".into(), vec![exit_capable_child()]);
        live.update(new);

        let children = live.get_children();
        assert_eq!(children.len(), 2);
        // Live child first, ghost last (= painted on top).
        assert_ne!(Arc::as_ptr(&children[0]) as *const (), old_id);
        assert_eq!(Arc::as_ptr(&children[1]) as *const (), old_id);
    }

    #[test]
    fn ghost_relayout_stays_inside_frozen_rect() {
        let mut live = PresenceWidget::new();
        live.generation_key = Some("a".to_string());
        // A grow child: under the frozen constraints it must resolve
        // to exactly the slot it died in, not the Presence's current
        // flow. (A shrink ghost re-measures its content instead —
        // that's what keeps interior exit animations reflowing.)
        let child = exit_capable_child();
        {
            let mut g = child.lock().unwrap();
            let f = g.downcast_mut::<FlexWidget>().unwrap();
            f.declared_style.width = Size::Grow(1.0);
            f.declared_style.height = Size::Grow(1.0);
            f.style = f.declared_style.clone();
        }
        live.inner.children = vec![child];
        let new = presence_ref(PresenceMode::Pop, "b".into(), vec![no_exit_child()]);
        live.update(new);

        let ctx = LayoutContext {
            font_collection: None,
            default_font: None,
            measure_grow_width_as_shrink: false,
            measure_grow_height_as_shrink: false,
        };
        live.layout(
            &ctx,
            0.0,
            0.0,
            &Direction::Column,
            800.0,
            800.0,
            600.0,
            600.0,
            0.0,
        );

        // The ghost's rect is re-derived from the frozen slot, not
        // from the Presence's current flow.
        let g = live.ghosts[0].widget.lock().unwrap();
        let rect = g.get_layout_rect().expect("ghost laid out");
        assert_eq!((rect.x, rect.y), (5.0, 10.0));
        assert_eq!((rect.width, rect.height), (100.0, 50.0));
    }

    #[test]
    fn update_adopts_incoming_mode() {
        let mut live = PresenceWidget::new();
        assert_eq!(live.mode, PresenceMode::Pop);
        live.generation_key = Some("a".to_string());

        let new = presence_ref(PresenceMode::Wait, "a".into(), vec![]);
        live.update(new);
        assert_eq!(live.mode, PresenceMode::Wait);
    }

    #[test]
    fn in_flight_wait_transition_survives_mode_flip_to_pop() {
        // A staged pending completes under wait rules even if the
        // author flips the mode mid-transition; the flip governs
        // future transitions.
        let mut live = PresenceWidget::new();
        live.mode = PresenceMode::Wait;
        live.generation_key = Some("a".to_string());
        live.inner.children = vec![exit_capable_child()];

        let new_b = presence_ref(PresenceMode::Wait, "b".into(), vec![exit_capable_child()]);
        live.update(new_b);
        assert!(live.pending_children.is_some());

        let new_c = presence_ref(PresenceMode::Pop, "c".into(), vec![exit_capable_child()]);
        live.update(new_c);

        // Wait rules: pending replaced, no immediate mount, no ghosts.
        assert_eq!(live.mode, PresenceMode::Pop);
        assert_eq!(live.pending_key.as_deref(), Some("c"));
        assert_eq!(live.generation_key.as_deref(), Some("a"));
        assert!(live.ghosts.is_empty());
    }
}
