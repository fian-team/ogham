use std::collections::HashMap;
use std::sync::Arc;

use super::animation::AnimationState;
use super::event::*;
use super::point::*;
use super::rect::*;
use super::style::*;
use super::{TickResult, Widget};
use crate::widget::event::EventContext;
use crate::widget::style::Direction;
use crate::widget::{LayoutContext, UpdateResult, WidgetRef};

/// Flexbox-like layout widget. Supports rendering child elements in either a row or a column with styling applied to the surrounding box.
///
/// `style` holds the currently-rendered style — the same value layout and
/// rendering observe. During transitions it is overwritten each frame with
/// the interpolated spring values. The user-declared target style is held
/// in `declared_style`; `hover_style`, if set, is the alternate target
/// used while `hovered` is true.
pub struct FlexWidget {
    pub children: Vec<WidgetRef>,
    pub event_listeners: HashMap<String, Vec<Box<dyn Fn(&Event)>>>,
    pub style: FlexStyle,
    /// Author-declared base style. Animations treat this as their target
    /// when the widget is not hovered. Distinct from `style`, which may
    /// transiently hold interpolated values during an active transition.
    pub declared_style: FlexStyle,
    pub hover_style: Option<FlexStyle>,
    /// Style the widget is born with. When set, the widget starts at
    /// this style on first mount and transitions toward `declared_style`.
    /// `None` means no entry animation (current behavior).
    pub initial_style: Option<FlexStyle>,
    /// Style animations target when the widget is leaving the tree.
    /// `None` means the widget is removed immediately on unmount (no
    /// exit animation).
    pub exit_style: Option<FlexStyle>,
    pub hovered: bool,
    pub block_interactions: bool,
    pub layout: Option<Rect>,
    /// Optional stable identity used to carry persistent state (hover,
    /// scroll, animations) across reconciliation when the widget moves
    /// position in its parent's children list.
    pub key: Option<String>,
    /// Current vertical scroll offset (only used when overflow is Scroll).
    /// Eased toward `scroll_y_target` each tick for smooth wheel scrolling.
    pub scroll_y: f32,
    /// Where the rendered scroll position is animating toward. Wheel input
    /// updates this; `tick_smooth_scroll` advances `scroll_y` toward it.
    pub scroll_y_target: f32,
    /// Total content height (computed during layout).
    content_height: f32,
    /// Viewport height (computed during layout).
    viewport_height: f32,
    /// In-flight spring state for any transitioning style properties.
    /// Populated only when a property actually changes and the style
    /// declares a transition for it.
    pub animations: AnimationState,
    /// True while the widget is playing an exit animation. The widget
    /// stays in the parent's `children` list until its springs settle,
    /// then is dropped on the next reconcile pass.
    pub exiting: bool,
    /// Net spring-delay offset injected into this subtree by staggered
    /// ancestor containers (`FlexStyle::stagger`). Tracked so reconcile
    /// can strip exactly the inherited amount from a subtree that turns
    /// out to mount individually rather than with its group, while
    /// cascades injected by containers *inside* the subtree survive.
    pub group_delay: f32,
    /// Debug-only: consecutive frames where this widget reported
    /// `layout_effects && still_moving` from `tick_own_animations`.
    /// Used to identify a stuck spring by emitting a single warning
    /// after a threshold of frames.
    #[cfg(debug_assertions)]
    pub layout_anim_frames: u32,

    /// Phase 2 lifecycle: the call-stack path at which this widget
    /// was constructed. Set by the builder when constructing from
    /// a `Value::Widget` descriptor, using
    /// `runtime.state.get_call_stack_path()`. Empty string means
    /// "no path" (top-level module or test construction).
    /// Used by the drain machinery to flush matching lifecycle
    /// hooks. See [`Widget::owned_path_prefix`].
    pub owned_path_prefix: String,

    /// Phase 3 M1: optional drag payload declared on this
    /// widget. When `Some(_)`, this widget can originate a
    /// drag; the input pump uses the value as the
    /// [`event::DragState::payload`] when `drag_start` fires.
    /// `None` means the widget is not a drag source.
    pub drag_payload: Option<crate::runtime::value::Value>,

    /// Phase 3 M1: per-widget dead-zone override (in logical
    /// pixels). `None` defers to the host default (4px).
    pub drag_dead_zone: Option<f32>,

    /// Phase 3 M1: optional `accepts_drop(payload) -> bool`
    /// predicate. When set, this widget can be a drop target;
    /// the drop-target hit-test consults the predicate with
    /// the in-flight drag's payload. `None` means the widget
    /// is not a drop target (default).
    pub accepts_drop_predicate: Option<Box<dyn Fn(&crate::runtime::value::Value) -> bool>>,

    /// Phase 3 M2: optional drag preview widget. When this
    /// FlexWidget is the source of an in-flight drag, the
    /// preview subtree renders attached to the cursor in the
    /// `CursorAttached` portal layer.
    pub drag_preview: Option<WidgetRef>,
}

impl FlexWidget {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            event_listeners: HashMap::new(),
            style: FlexStyle::default(),
            declared_style: FlexStyle::default(),
            hover_style: None,
            initial_style: None,
            exit_style: None,
            hovered: false,
            block_interactions: true,
            layout: None,
            key: None,
            scroll_y: 0.0,
            scroll_y_target: 0.0,
            content_height: 0.0,
            viewport_height: 0.0,
            animations: AnimationState::default(),
            exiting: false,
            group_delay: 0.0,
            #[cfg(debug_assertions)]
            layout_anim_frames: 0,
            owned_path_prefix: String::new(),
            drag_payload: None,
            drag_dead_zone: None,
            accepts_drop_predicate: None,
            drag_preview: None,
        }
    }

    pub fn with_style(style: FlexStyle) -> Self {
        Self {
            children: Vec::new(),
            event_listeners: HashMap::new(),
            style: style.clone(),
            declared_style: style,
            hover_style: None,
            initial_style: None,
            exit_style: None,
            hovered: false,
            block_interactions: true,
            layout: None,
            key: None,
            scroll_y: 0.0,
            scroll_y_target: 0.0,
            content_height: 0.0,
            viewport_height: 0.0,
            animations: AnimationState::default(),
            exiting: false,
            group_delay: 0.0,
            #[cfg(debug_assertions)]
            layout_anim_frames: 0,
            owned_path_prefix: String::new(),
            drag_payload: None,
            drag_dead_zone: None,
            accepts_drop_predicate: None,
            drag_preview: None,
        }
    }

    /// Returns the style to use for layout and rendering. Since `style`
    /// is kept in sync with the current interpolated value during
    /// transitions, this simply returns a reference to it.
    pub fn effective_style(&self) -> &FlexStyle {
        &self.style
    }

    /// The style animations are currently pulling toward. Priority:
    /// (1) `exit_style` when the widget is exiting,
    /// (2) `hover_style` when hovered,
    /// (3) the declared base style.
    fn target_style(&self) -> &FlexStyle {
        if self.exiting {
            if let Some(ref s) = self.exit_style {
                return s;
            }
        }
        if self.hovered {
            if let Some(ref s) = self.hover_style {
                return s;
            }
        }
        &self.declared_style
    }

    pub fn add_child(&mut self, child: WidgetRef) {
        self.children.push(child);
    }

    /// Seed the widget at `initial_style` and retarget its springs
    /// toward `declared_style`, producing an entry animation on first
    /// mount. No-op when `initial_style` is absent or the declared
    /// style has no transitions enabled.
    ///
    /// Called by the widget builder right after construction so the
    /// first rendered frame is already at the initial values.
    pub fn apply_entry_transition(&mut self) {
        let initial = match self.initial_style.as_ref() {
            Some(s) => s.clone(),
            None => return,
        };
        let target = self.declared_style.clone();
        if !target.transitions.any_enabled() {
            return;
        }
        self.style = initial.clone();
        self.animations.retarget(&initial, &target);
        if self.animations.is_empty() {
            // initial and declared are identical on all transition-
            // declared properties — nothing to animate.
            self.style = target;
        }
    }

    /// Inject this container's entry-stagger offsets into each child's
    /// subtree (see [`FlexStyle::stagger`]). Called at the two entry
    /// group moments — construction (by the builder, once children's
    /// springs exist) and entry restart — never on reconcile, so
    /// individually inserted children don't inherit a cascade slot.
    pub fn apply_child_stagger_offsets(&mut self) {
        let Some(stagger) = self.declared_style.stagger else {
            return;
        };
        if stagger.step <= 0.0 {
            return;
        }
        for (i, child) in self.children.iter().enumerate() {
            if i == 0 {
                continue;
            }
            let mut g = child.lock().expect("widget lock poisoned");
            g.add_group_delay(i as f32 * stagger.step);
        }
    }

    /// Ease `scroll_y` toward `scroll_y_target` using framerate-independent
    /// exponential decay. Returns a tick result that requests a repaint
    /// while the offset is still moving so the animation keeps running.
    fn tick_smooth_scroll(&mut self, dt: f32) -> TickResult {
        if self.style.overflow != Overflow::Scroll {
            return TickResult::NONE;
        }
        let delta = self.scroll_y_target - self.scroll_y;
        if delta.abs() < 0.5 {
            if self.scroll_y != self.scroll_y_target {
                self.scroll_y = self.scroll_y_target;
                return TickResult {
                    needs_repaint: true,
                    ..TickResult::NONE
                };
            }
            return TickResult::NONE;
        }
        // Decay constant: higher = snappier. ~18/s settles a wheel notch in
        // roughly 150ms while still feeling smooth.
        const SCROLL_DECAY: f32 = 18.0;
        let alpha = 1.0 - (-dt * SCROLL_DECAY).exp();
        self.scroll_y += delta * alpha;
        TickResult {
            needs_repaint: true,
            needs_layout: false,
            still_animating: true,
        }
    }

    /// Advance any in-flight transitions by `dt` seconds and overwrite
    /// `self.style` with the resulting interpolated values. Called once
    /// per frame before layout so the layout pass observes the animated
    /// values directly.
    pub fn tick_own_animations(&mut self, dt: f32) -> TickResult {
        if self.animations.is_empty() {
            return TickResult::NONE;
        }
        let still_moving = self.animations.tick(dt);
        let layout_effects = self.animations.has_layout_effects();

        // Overlay current spring values onto the target to produce the
        // visual style for this frame.
        let target = self.target_style().clone();
        self.style = self.animations.render_onto(&target);

        // If everything just settled, snap exactly to the target so we
        // don't leave a one-tick-off rendering and cleared springs can
        // release memory. `tick()` already cleared settled springs, so
        // any remaining entries are still moving.
        if self.animations.is_empty() {
            self.style = target;
        }

        #[cfg(debug_assertions)]
        {
            if layout_effects && still_moving {
                self.layout_anim_frames = self.layout_anim_frames.saturating_add(1);
                if self.layout_anim_frames % 60 == 30 {
                    eprintln!(
                        "[ogham] layout-affecting spring stuck on widget key={:?} \
                         exiting={} hovered={} \
                         border_width={} padding={} margin={} gap={} text_size={} \
                         ({} frames)",
                        self.key,
                        self.exiting,
                        self.hovered,
                        self.animations
                            .border
                            .as_ref()
                            .is_some_and(|b| b.width_animating()),
                        self.animations.padding.is_some(),
                        self.animations.margin.is_some(),
                        self.animations.gap.is_some(),
                        self.animations.text_size.is_some(),
                        self.layout_anim_frames,
                    );
                }
            } else {
                self.layout_anim_frames = 0;
            }
        }

        TickResult {
            needs_repaint: true,
            needs_layout: layout_effects && still_moving,
            still_animating: still_moving,
        }
    }

    /// Reconcile `self.children` with `new_children`. Keyed children
    /// are matched by key; unkeyed children match by position.
    ///
    /// Exit lifecycle: a keyed child that disappears from `new_children`
    /// becomes a "ghost" — it is kept in `self.children` at its old
    /// position while its exit animation plays, then removed on a later
    /// reconcile pass once settled. A child whose key is re-inserted
    /// while it is still exiting has its exit canceled. Children
    /// without exit capability (e.g. no `exit_style`) are dropped
    /// immediately, matching the old behavior. Unkeyed children can't
    /// be ghosts (no identity to carry).
    /// Returns an [`UpdateResult`] aggregating across the new child set:
    /// `needs_layout` is true if any child was added/removed/replaced or if
    /// any matched child's `update()` reported layout-affecting changes.
    pub fn reconcile_children(&mut self, new_children: &mut Vec<WidgetRef>) -> UpdateResult {
        let mut agg = UpdateResult {
            absorbed: true,
            needs_layout: false,
            needs_repaint: false,
            cancelled_unmount_prefixes: Vec::new(),
            drained_path_prefixes: Vec::new(),
        };
        // Pre-pass: if a new child's key matches a currently-exiting
        // ghost, cancel the exit so the ghost re-enters normal matching.
        // Capture each cancelled child's owned_path_prefix so the UI can
        // remove the prefix from any pending unmount queue.
        let new_key_set: std::collections::HashSet<String> = new_children
            .iter()
            .filter_map(|c| {
                let g = c.lock().expect("widget lock poisoned");
                g.key().map(|s| s.to_string())
            })
            .collect();
        for child in self.children.iter() {
            let should_cancel = {
                let g = child.lock().expect("widget lock poisoned");
                g.is_exiting() && g.key().map_or(false, |k| new_key_set.contains(k))
            };
            if should_cancel {
                let mut g = child.lock().expect("widget lock poisoned");
                let prefix = g.owned_path_prefix().to_string();
                g.cancel_exit();
                if !prefix.is_empty() {
                    agg.cancelled_unmount_prefixes.push(prefix);
                }
            }
        }

        // Build key→index map of the (possibly un-exited) old children.
        let mut old_by_key: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for (i, child) in self.children.iter().enumerate() {
            let guard = child.lock().expect("widget lock poisoned");
            if let Some(k) = guard.key() {
                old_by_key.entry(k.to_string()).or_insert(i);
            }
        }

        // Match new children against old. Keyed children match by key;
        // unkeyed children consume the next unkeyed, non-exiting old
        // child by position. Exiting ghosts are NEVER matched by
        // unkeyed position — they keep their slot independently.
        let mut next: Vec<WidgetRef> = Vec::with_capacity(new_children.len());
        let mut consumed_old: Vec<bool> = vec![false; self.children.len()];
        let mut unkeyed_cursor: usize = 0;

        for new_child in new_children.iter() {
            let new_key: Option<String> = {
                let guard = new_child.lock().expect("widget lock poisoned");
                guard.key().map(|s| s.to_string())
            };

            let matched_old_idx: Option<usize> = if let Some(key) = new_key.as_deref() {
                old_by_key.get(key).copied().filter(|i| !consumed_old[*i])
            } else {
                while unkeyed_cursor < self.children.len() {
                    if consumed_old[unkeyed_cursor] {
                        unkeyed_cursor += 1;
                        continue;
                    }
                    let skip = {
                        let guard = self.children[unkeyed_cursor]
                            .lock()
                            .expect("widget lock poisoned");
                        guard.key().is_some() || guard.is_exiting()
                    };
                    if skip {
                        unkeyed_cursor += 1;
                    } else {
                        break;
                    }
                }
                if unkeyed_cursor < self.children.len() {
                    Some(unkeyed_cursor)
                } else {
                    None
                }
            };

            if let Some(idx) = matched_old_idx {
                consumed_old[idx] = true;
                let same_ref = Arc::ptr_eq(&self.children[idx], new_child);
                if same_ref {
                    next.push(self.children[idx].clone());
                } else {
                    let mut updated_in_place = {
                        let mut child = self.children[idx].lock().expect("widget lock poisoned");
                        child.update(new_child.clone())
                    };
                    agg.needs_layout |= updated_in_place.needs_layout;
                    agg.needs_repaint |= updated_in_place.needs_repaint;
                    agg.cancelled_unmount_prefixes
                        .append(&mut updated_in_place.cancelled_unmount_prefixes);
                    if updated_in_place.absorbed {
                        next.push(self.children[idx].clone());
                    } else {
                        // Type mismatch: the old widget can't absorb the
                        // new one. Try to let it (or its subtree) play
                        // an exit animation before being dropped —
                        // otherwise the button rows inside a panel that
                        // just got swapped would vanish instantly.
                        agg.needs_layout = true;
                        agg.needs_repaint = true;
                        let (can_ghost, prefix) = {
                            let mut g = self.children[idx].lock().expect("widget lock poisoned");
                            let p = g.owned_path_prefix().to_string();
                            (g.begin_exit(), p)
                        };
                        if can_ghost {
                            next.push(self.children[idx].clone());
                        } else if !prefix.is_empty() {
                            // Dropped immediately (no exit). Push the
                            // prefix so the runtime can flush its
                            // unmount hooks at the next render
                            // boundary — drain-time semantics.
                            agg.drained_path_prefixes.push(prefix);
                        }
                        {
                            let mut g = new_child.lock().expect("widget lock poisoned");
                            g.strip_inherited_group_delay();
                        }
                        next.push(new_child.clone());
                    }
                }
            } else {
                // Brand-new keyed child or a fresh tail entry — structural
                // change, layout has to re-flow. It mounts individually,
                // not with its group: strip any cascade offset that
                // staggered ancestors in the (discarded) new tree
                // injected at construction, so the lone newcomer doesn't
                // sit out a cascade slot that isn't happening.
                agg.needs_layout = true;
                agg.needs_repaint = true;
                {
                    let mut g = new_child.lock().expect("widget lock poisoned");
                    g.strip_inherited_group_delay();
                }
                next.push(new_child.clone());
            }
        }

        // Handle unconsumed old children: already-exiting ghosts keep
        // going; newly-orphaned keyed children attempt to begin an exit
        // animation and become ghosts if they can. Everything else is
        // dropped.
        //
        // Ghosts are spliced back into `next` at a position close to
        // their original index so sibling layout stays stable.
        for (old_idx, old_child) in self.children.iter().enumerate() {
            if consumed_old[old_idx] {
                continue;
            }
            let (is_exiting, begin_ok, prefix) = {
                let mut g = old_child.lock().expect("widget lock poisoned");
                let already = g.is_exiting();
                let p = g.owned_path_prefix().to_string();
                let started = if already { true } else { g.begin_exit() };
                (already, started, p)
            };
            if !begin_ok {
                // No exit capability and not already exiting — drop.
                // Dropping a child shifts siblings, so layout needs to re-flow.
                agg.needs_layout = true;
                agg.needs_repaint = true;
                if !prefix.is_empty() {
                    // Push the prefix so the runtime can flush
                    // its unmount hooks immediately — there's no
                    // exit animation to wait for.
                    agg.drained_path_prefixes.push(prefix);
                }
                continue;
            }
            let _ = is_exiting;
            let splice_at = old_idx.min(next.len());
            next.insert(splice_at, old_child.clone());
        }

        self.children = next;
        agg
    }

    /// Drop any children whose exit animation has fully settled. Called
    /// after per-frame ticks so ghosts clean up without needing a
    /// reconcile pass from above. Pushes each drained child's
    /// `owned_path_prefix` (when non-empty) into the tick context so
    /// the UI can flush owned hooks/state for that subtree at the
    /// next render boundary.
    fn drain_exited_children(&mut self, ctx: &mut crate::widget::event::TickContext) {
        self.children.retain(|child| {
            let g = child.lock().expect("widget lock poisoned");
            if g.is_exit_complete() {
                let prefix = g.owned_path_prefix().to_string();
                if !prefix.is_empty() {
                    ctx.drained_path_prefixes.push(prefix);
                }
                false
            } else {
                true
            }
        });
    }

    fn get_children_fixed_on_axis(&self, axis: Axis) -> f32 {
        self.children
            .iter()
            .filter_map(|child| {
                let child = child.lock().expect("widget lock poisoned");
                if child.is_absolute_positioned() {
                    return None;
                }
                match axis {
                    Axis::Horizontal => child.get_fixed_width(),
                    Axis::Vertical => child.get_fixed_height(),
                }
            })
            .sum()
    }
}

impl Widget for FlexWidget {
    fn owned_path_prefix(&self) -> &str {
        &self.owned_path_prefix
    }

    fn drag_payload(&self) -> Option<&crate::runtime::value::Value> {
        self.drag_payload.as_ref()
    }

    fn drag_dead_zone(&self) -> Option<f32> {
        self.drag_dead_zone
    }

    fn accepts_drop(&self, payload: &crate::runtime::value::Value) -> bool {
        self.accepts_drop_predicate
            .as_ref()
            .map(|p| p(payload))
            .unwrap_or(false)
    }

    fn fire_event_listener(&self, event: &Event) -> bool {
        if let Some(listeners) = self.event_listeners.get(&event.name) {
            for listener in listeners {
                listener(event);
            }
            !listeners.is_empty()
        } else {
            false
        }
    }

    fn drag_preview(&self) -> Option<WidgetRef> {
        self.drag_preview.clone()
    }

    fn update(&mut self, new_widget: WidgetRef) -> UpdateResult {
        let mut new_widget = new_widget.lock().expect("widget lock poisoned");
        if let Some(new_flex_widget) = new_widget.downcast_mut::<FlexWidget>() {
            // Compare own props before overwriting so we know whether to
            // bubble a `needs_layout` signal up to our parent. Skips
            // paint-only fields via `layout_equal`.
            let style_changed = !self
                .declared_style
                .layout_equal(&new_flex_widget.declared_style);
            let hover_changed = match (&self.hover_style, &new_flex_widget.hover_style) {
                (None, None) => false,
                (Some(a), Some(b)) => !a.layout_equal(b),
                _ => true,
            };
            let block_changed = self.block_interactions != new_flex_widget.block_interactions;
            let key_changed = self.key != new_flex_widget.key;
            let own_layout_changed = style_changed || hover_changed || block_changed || key_changed;
            // A paint-only style change (background_color, text color, opacity, …)
            // leaves `layout_equal` true, so `style_changed`/`hover_changed` miss it
            // and `own_layout_changed` stays false. Detect ANY visual difference so
            // such a change actually repaints: without this, a colour-only state
            // change — e.g. a selected chip flipping to its accent highlight, whose
            // label text is unchanged — updates the widget's style (below) but is
            // never marked dirty, so it renders stale until an unrelated layout or
            // text change happens to force a full repaint.
            let own_paint_changed = !self
                .declared_style
                .paint_equal(&new_flex_widget.declared_style)
                || match (&self.hover_style, &new_flex_widget.hover_style) {
                    (None, None) => false,
                    (Some(a), Some(b)) => !a.paint_equal(b),
                    _ => true,
                };

            // Snapshot the current rendered style — this is what the user
            // last saw on screen, including any mid-animation values — so
            // transitions seed from there rather than from the old target.
            let old_rendered = self.style.clone();

            // Adopt the new target (and optional hover override). Child
            // widgets carry over their own animation state via reconciliation.
            self.declared_style = new_flex_widget.declared_style.clone();
            self.hover_style = new_flex_widget.hover_style.clone();
            self.initial_style = new_flex_widget.initial_style.clone();
            self.exit_style = new_flex_widget.exit_style.clone();
            self.block_interactions = new_flex_widget.block_interactions;
            self.key = new_flex_widget.key.clone();
            std::mem::swap(
                &mut self.event_listeners,
                &mut new_flex_widget.event_listeners,
            );
            // Phase 3 M1: drag fields. drag_payload + dead_zone
            // are plain Values; the predicate is a Box<dyn Fn>
            // that we swap rather than clone.
            self.drag_payload = new_flex_widget.drag_payload.clone();
            self.drag_dead_zone = new_flex_widget.drag_dead_zone;
            std::mem::swap(
                &mut self.accepts_drop_predicate,
                &mut new_flex_widget.accepts_drop_predicate,
            );
            // Phase 3 M2: drag preview. Swap rather than clone
            // so the preview's own subtree state (animations,
            // reconciled children) carries forward.
            std::mem::swap(&mut self.drag_preview, &mut new_flex_widget.drag_preview);

            let new_target = self.target_style().clone();

            if new_target.transitions.any_enabled() {
                self.animations.retarget(&old_rendered, &new_target);
                if self.animations.is_empty() {
                    // No property actually changed — snap to target.
                    self.style = new_target;
                } else {
                    // Springs carry the transitioned properties from their
                    // current values; everything else — including layout
                    // fields like width/height that declare no transition —
                    // must snap NOW, while this update's needs_layout flag
                    // is in flight. Holding the whole old style here left
                    // the box at its stale size: the tick that later wrote
                    // the new value reports needs_layout only for spring-
                    // driven properties, so the snap never re-laid out.
                    self.style = self.animations.render_onto(&new_target);
                }
            } else {
                // No transitions declared — snap immediately.
                self.animations = AnimationState::default();
                self.style = new_target;
            }

            let children_result = self.reconcile_children(&mut new_flex_widget.children);

            UpdateResult {
                absorbed: true,
                needs_layout: own_layout_changed || children_result.needs_layout,
                needs_repaint: own_layout_changed
                    || own_paint_changed
                    || children_result.needs_repaint,
                cancelled_unmount_prefixes: children_result.cancelled_unmount_prefixes,
                drained_path_prefixes: children_result.drained_path_prefixes,
            }
        } else {
            UpdateResult::replace()
        }
    }

    fn get_basis(&self, direction: &Direction) -> f32 {
        if matches!(self.style.position, Position::Absolute(_, _)) {
            return 0.0;
        }

        if direction.is_row() {
            self.style.width.grow_basis()
        } else {
            self.style.height.grow_basis()
        }
    }

    fn get_children_basis(&self) -> f32 {
        let mut basis = 0.0;
        for child in self.children.iter() {
            let child = child.lock().expect("widget lock poisoned");
            if child.is_absolute_positioned() {
                continue;
            }
            basis += child.get_basis(&self.style.direction);
        }
        basis
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
        // Under a measuring shrink ancestor, grow on the measured axis
        // resolves as shrink — a shrink parent has no leftover space to
        // grow into, so the child's content is its honest contribution
        // (the ancestor's real `layout` pass stretches it afterwards).
        let width = match ctx.effective_width(self.style.width) {
            Size::Fixed(w) => w,
            Size::Shrink => {
                let _occupied_width: f32 = self.get_children_fixed_width();
                let occupied_height = self.get_children_fixed_height();
                let children_basis = self.get_children_basis();

                // Symmetric to the shrink-height path below: a column's WIDTH
                // (cross axis) can depend on a child's HEIGHT (main axis).
                // Resolve each child's real main-axis height budget (fixed +
                // shrink pre-pass + grow pool) so a `height: grow` child isn't
                // measured at ~1px height during width resolution.
                let (col_avail_h, col_avail_h_for_grow) = if !self.style.direction.is_row() {
                    let non_absolute_count = self
                        .children
                        .iter()
                        .filter(|c| {
                            !c.lock()
                                .expect("widget lock poisoned")
                                .is_absolute_positioned()
                        })
                        .count();
                    let gap_space = if non_absolute_count > 1 {
                        self.style.gap * (non_absolute_count - 1) as f32
                    } else {
                        0.0
                    };
                    // Same unbounded-main-axis rule as `layout`: a scroll
                    // column measures its children at natural height.
                    let avail_h = if self.style.overflow == Overflow::Scroll {
                        f32::INFINITY
                    } else {
                        (parent_available_height - occupied_height - gap_space).max(0.0)
                    };
                    let mut shrink_main_total = 0.0;
                    for child_ref in self.children.iter() {
                        let child = child_ref.lock().expect("widget lock poisoned");
                        if child.is_absolute_positioned()
                            || child.get_basis(&self.style.direction) > 0.0
                            || child.get_fixed_height().is_some()
                        {
                            continue;
                        }
                        let (_, ch) = child.get_dimensions(
                            ctx,
                            &self.style.direction,
                            parent_width,
                            parent_available_width,
                            parent_height,
                            avail_h,
                            children_basis,
                        );
                        shrink_main_total += ch;
                    }
                    (avail_h, (avail_h - shrink_main_total).max(0.0))
                } else {
                    (0.0, 0.0)
                };

                // Measuring our own width: grow widths below resolve as
                // content (see `LayoutContext::measuring_width`).
                let measure_ctx = ctx.measuring_width();
                let get_dimensions = |child: &WidgetRef| {
                    let child = child.lock().expect("widget lock poisoned");
                    if child.is_absolute_positioned() {
                        return (0.0, 0.0);
                    }
                    let child_available_width = if self.style.direction.is_row() {
                        0.0
                    } else {
                        parent_available_width
                    };
                    let child_available_height = if !self.style.direction.is_row() {
                        if child.get_basis(&self.style.direction) > 0.0 {
                            col_avail_h_for_grow
                        } else {
                            col_avail_h
                        }
                    } else {
                        parent_available_height - occupied_height
                    };
                    child.get_dimensions(
                        &measure_ctx,
                        &self.style.direction,
                        parent_width,
                        child_available_width,
                        parent_height,
                        child_available_height,
                        children_basis,
                    )
                };
                let child_size = if self.style.direction.is_row() {
                    self.style
                        .direction
                        .get_shrink_size(&self.children, get_dimensions)
                } else {
                    self.style
                        .direction
                        .get_shrink_max_size(&self.children, get_dimensions)
                };

                let non_absolute_count = self
                    .children
                    .iter()
                    .filter(|child| {
                        let child = child.lock().expect("widget lock poisoned");
                        !child.is_absolute_positioned()
                    })
                    .count();
                let gap_size = if non_absolute_count > 1 && self.style.direction.is_row() {
                    self.style.gap * (non_absolute_count - 1) as f32
                } else {
                    0.0
                };

                let unclamped = child_size + self.style.horizontal_inset() + gap_size;

                // Clamp shrink width to parent constraints so it can't exceed
                // the space the parent offers.
                let max_width = if parent_direction.is_row() {
                    parent_available_width // main axis — share of available
                } else {
                    parent_width // cross axis — full parent width
                };
                if max_width > 0.0 {
                    unclamped.min(max_width)
                } else {
                    unclamped
                }
            }
            Size::Grow(basis) => {
                // A child's own `direction` should not affect how its size is allocated by its parent.
                // Width grows along the parent's main axis only when the parent is a row.
                if parent_direction.is_row() {
                    parent_direction.get_grow_size(basis, sibling_basis, parent_available_width)
                } else {
                    parent_width
                }
            }
            Size::Percent(_) => 0.0, // Will be calculated during layout based on parent
        };

        let height = match ctx.effective_height(self.style.height) {
            Size::Fixed(h) => h,
            Size::Shrink => {
                let children_basis = self.get_children_basis();
                // `width` is already resolved by this point — pass it
                // (and self's own content width) down so children see the
                // same width budget they will see during `layout`. Using
                // the outer `parent_width` here was confusing wrap-aware
                // height measurement: a 280-wide sidebar would hand its
                // children the full window width and they would all
                // measure as fitting on one line.
                let self_content_width = (width - self.style.horizontal_inset()).max(0.0);

                // A row's HEIGHT (cross axis) depends on each child's WIDTH
                // (main axis). Measuring children at width 0 — the old behavior
                // — made a `width: grow` child resolve to ~1px (Grow returns its
                // basis when available <= 0), so a `height: shrink` Text laid its
                // paragraph out at ~1px and wrapped one glyph per line, ballooning
                // the row's shrink height. Resolve each child's real main-axis
                // width budget the SAME way `layout()` does (fixed + shrink
                // pre-pass + grow pool) so heights are measured at render width.
                let (row_avail_w, row_avail_w_for_grow) = if self.style.direction.is_row() {
                    let non_absolute_count = self
                        .children
                        .iter()
                        .filter(|c| {
                            !c.lock()
                                .expect("widget lock poisoned")
                                .is_absolute_positioned()
                        })
                        .count();
                    let gap_space = if non_absolute_count > 1 {
                        self.style.gap * (non_absolute_count - 1) as f32
                    } else {
                        0.0
                    };
                    let avail_w =
                        (self_content_width - self.get_children_fixed_width() - gap_space).max(0.0);
                    // Pre-pass: subtract Shrink-on-main siblings from the grow
                    // pool so a grow child isn't measured wider than it renders.
                    let mut shrink_main_total = 0.0;
                    for child_ref in self.children.iter() {
                        let child = child_ref.lock().expect("widget lock poisoned");
                        if child.is_absolute_positioned()
                            || child.get_basis(&self.style.direction) > 0.0
                            || child.get_fixed_width().is_some()
                        {
                            continue;
                        }
                        let (cw, _) = child.get_dimensions(
                            ctx,
                            &self.style.direction,
                            width,
                            avail_w,
                            parent_height,
                            parent_available_height,
                            children_basis,
                        );
                        shrink_main_total += cw;
                    }
                    (avail_w, (avail_w - shrink_main_total).max(0.0))
                } else {
                    (0.0, 0.0)
                };

                // Measuring our own height: grow heights below resolve as
                // content (see `LayoutContext::measuring_height`).
                let measure_ctx = ctx.measuring_height();
                let get_dimensions = |child: &WidgetRef| {
                    let child = child.lock().expect("widget lock poisoned");
                    if child.is_absolute_positioned() {
                        return (0.0, 0.0);
                    }
                    let child_available_width = if self.style.direction.is_row() {
                        if child.get_basis(&self.style.direction) > 0.0 {
                            row_avail_w_for_grow
                        } else {
                            row_avail_w
                        }
                    } else {
                        self_content_width
                    };
                    let child_available_height = if !self.style.direction.is_row() {
                        0.0
                    } else {
                        parent_available_height
                    };
                    child.get_dimensions(
                        &measure_ctx,
                        &self.style.direction,
                        width,
                        child_available_width,
                        parent_height,
                        child_available_height,
                        children_basis,
                    )
                };

                // For wrap-row containers the shrink height is the sum of
                // each line's tallest child plus the gap between lines.
                // We re-run the same wrap walk used by `layout` so the
                // measurement matches. `width` is already resolved here so
                // we can compute the line budget directly.
                if self.style.flex_wrap && self.style.direction.is_row() {
                    let line_main_max = (width - self.style.horizontal_inset()).max(0.0);
                    let mut total: f32 = 0.0;
                    let mut line_max_h: f32 = 0.0;
                    let mut cursor: f32 = 0.0;
                    let mut is_first = true;
                    for child_ref in self.children.iter() {
                        let is_absolute = {
                            let child = child_ref.lock().expect("widget lock poisoned");
                            child.is_absolute_positioned()
                        };
                        if is_absolute {
                            continue;
                        }
                        let (cw, ch) = get_dimensions(child_ref);
                        let projected = if is_first {
                            cursor + cw
                        } else {
                            cursor + self.style.gap + cw
                        };
                        if !is_first && projected > line_main_max {
                            total += line_max_h + self.style.gap;
                            line_max_h = 0.0;
                            cursor = 0.0;
                            is_first = true;
                        }
                        if !is_first {
                            cursor += self.style.gap;
                        }
                        cursor += cw;
                        line_max_h = line_max_h.max(ch);
                        is_first = false;
                    }
                    total += line_max_h;
                    let unclamped = total + self.style.vertical_inset();
                    let max_height = if parent_direction.is_row() {
                        parent_height
                    } else {
                        parent_available_height
                    };
                    return (
                        width,
                        if max_height > 0.0 {
                            unclamped.min(max_height)
                        } else {
                            unclamped
                        },
                    );
                }

                let child_size = if self.style.direction.is_row() {
                    self.style
                        .direction
                        .get_shrink_max_size(&self.children, get_dimensions)
                } else {
                    self.style
                        .direction
                        .get_shrink_size(&self.children, get_dimensions)
                };

                let non_absolute_count = self
                    .children
                    .iter()
                    .filter(|child| {
                        let child = child.lock().expect("widget lock poisoned");
                        !child.is_absolute_positioned()
                    })
                    .count();
                let gap_size = if non_absolute_count > 1 && !self.style.direction.is_row() {
                    self.style.gap * (non_absolute_count - 1) as f32
                } else {
                    0.0
                };

                let unclamped = child_size + self.style.vertical_inset() + gap_size;

                // Clamp shrink height to parent constraints so it can't exceed
                // the space the parent offers.
                let max_height = if parent_direction.is_row() {
                    parent_height // cross axis — full parent height
                } else {
                    parent_available_height // main axis — share of available
                };
                if max_height > 0.0 {
                    unclamped.min(max_height)
                } else {
                    unclamped
                }
            }
            Size::Grow(basis) => {
                if parent_direction.is_row() {
                    parent_height
                } else {
                    self.style.direction.get_grow_size(
                        basis,
                        sibling_basis,
                        parent_available_height,
                    )
                }
            }
            Size::Percent(_) => 0.0, // Will be calculated during layout based on parent
        };
        (width, height)
    }

    fn get_children(&self) -> Vec<WidgetRef> {
        self.children.clone()
    }

    fn handle_event(
        &mut self,
        event: &Event,
        ctx: &mut EventContext,
        _self_ref: &WidgetRef,
    ) -> bool {
        if let Some(point) = &event.point {
            // An exiting widget is hit-test-invisible (PRESENCE_POP.md
            // §6): a half-faded subtree must not eat presses aimed at
            // live content beneath it. Keyboard routing (no point)
            // is left untouched.
            if self.exiting {
                return false;
            }
            // For click events, first check if this widget contains the point
            if self.contains_point(point) {
                // Handle scroll events for scrollable containers
                if let Some((_, dy)) = event.scroll_delta {
                    if self.style.overflow == Overflow::Scroll {
                        let max_scroll = (self.content_height - self.viewport_height).max(0.0);
                        self.scroll_y_target = (self.scroll_y_target - dy).clamp(0.0, max_scroll);
                        return true;
                    }
                }

                // Build an event whose point is in this widget's own content
                // coordinate space, so children (which store parent-relative
                // rects) can hit-test without knowing the ancestor chain.
                let origin = self
                    .layout
                    .as_ref()
                    .map(|r| (r.x, r.y))
                    .unwrap_or((0.0, 0.0));
                let (scroll_x, scroll_y) = if self.style.overflow == Overflow::Scroll {
                    (0.0, self.scroll_y)
                } else {
                    (0.0, 0.0)
                };
                let local_event = event.shift_point(-origin.0 + scroll_x, -origin.1 + scroll_y);

                let mut child_consumed = false;

                // Walk children. A child returning `true` consumes the
                // click for sibling-iteration purposes (so the next sibling
                // doesn't also see it), but it does *not* by itself
                // suppress this widget's own listener — that decision is
                // made below using `ctx.listener_fired`, which tells us
                // whether a real listener actually ran in the subtree as
                // opposed to a layout-only Flex returning `true` purely
                // because `block_interactions` is set.
                for child_ref in self.children.iter() {
                    let mut child = child_ref.lock().expect("widget lock poisoned");
                    if child.handle_event(&local_event, ctx, child_ref) {
                        child_consumed = true;
                        break;
                    }
                }

                let mut my_fired = false;
                if !ctx.listener_fired && self.event_listeners.contains_key(&event.name) {
                    for listener in self.event_listeners.get(&event.name).unwrap() {
                        listener(event);
                    }
                    my_fired = true;
                    ctx.listener_fired = true;
                }

                return self.block_interactions || child_consumed || my_fired;
            }
        } else {
            // For non-click events (like keyboard events), propagate to all children
            let mut event_handled = false;

            // Check children in reverse order (child-most first)
            for child_ref in self.children.iter().rev() {
                let mut child = child_ref.lock().expect("widget lock poisoned");
                if child.handle_event(event, ctx, child_ref) {
                    event_handled = true;
                    // Don't break here - let all children have a chance to handle the event
                }
            }

            // If no child handled the event, check if this widget has event listeners
            if !event_handled && self.event_listeners.contains_key(&event.name) {
                for listener in self.event_listeners.get(&event.name).unwrap() {
                    listener(event);
                }
                println!("Event handled: {}", event.name);
                event_handled = true;
            }

            return event_handled;
        }
        false
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
        let (width, height) = self.get_dimensions(
            ctx,
            parent_direction,
            parent_width,
            parent_available_width,
            parent_height,
            parent_available_height,
            sibling_basis,
        );
        let children_basis = self.get_children_basis();

        // Calculate non-absolute children count for gap calculation
        let non_absolute_count = self
            .children
            .iter()
            .filter(|child| {
                let child = child.lock().expect("widget lock poisoned");
                !child.is_absolute_positioned()
            })
            .count();

        // Calculate gap space that will be used between children
        let gap_space = if non_absolute_count > 1 {
            self.style.gap * (non_absolute_count - 1) as f32
        } else {
            0.0
        };

        // Calculate the content area by subtracting padding, margin, and border
        let content_width = width - self.style.horizontal_inset();
        let content_height = height - self.style.vertical_inset();

        // Available space should only subtract fixed-size children along the *main* axis.
        let available_width = content_width
            - if self.style.direction.is_row() {
                self.get_children_fixed_width() + gap_space
            } else {
                0.0
            };
        let available_height = content_height
            - if self.style.direction.is_row() {
                0.0
            } else {
                self.get_children_fixed_height() + gap_space
            };

        // A scroll container's main axis is unbounded for its children:
        // the whole point of Overflow::Scroll is content past the
        // viewport, so a shrink child must measure its natural height
        // there — never a viewport share (the shrink clamp otherwise
        // squeezes every row past the fold into the fold, and they paint
        // over one another). Grow children keep the finite pool below:
        // growing to the viewport is still the sane reading.
        let child_available_height =
            if self.style.overflow == Overflow::Scroll && !self.style.direction.is_row() {
                f32::INFINITY
            } else {
                available_height
            };

        self.layout = Some(Rect::new(cursor_x, cursor_y, width, height));

        // Per-child grow basis along the parent's main axis. Zero for any
        // child whose main-axis size is not Grow (Fixed, Shrink, Percent,
        // or absolute-positioned).
        let child_main_basis: Vec<f32> = self
            .children
            .iter()
            .map(|child| {
                let child = child.lock().expect("widget lock poisoned");
                if child.is_absolute_positioned() {
                    return 0.0;
                }
                child.get_basis(&self.style.direction)
            })
            .collect();

        // Pre-pass: measure Shrink siblings on the main axis so we can
        // subtract their natural size from the pool we hand to Grow
        // siblings. Without this, a Grow child takes the full
        // `available_main` and pushes Shrink siblings past the parent's
        // edge. Fixed-size children are already excluded via
        // `get_children_fixed_*()` so we skip them here.
        let mut shrink_main_total: f32 = 0.0;
        for (i, child_ref) in self.children.iter().enumerate() {
            let child = child_ref.lock().expect("widget lock poisoned");
            if child.is_absolute_positioned() {
                continue;
            }
            if child_main_basis[i] > 0.0 {
                continue;
            }
            let main_is_fixed = if self.style.direction.is_row() {
                child.get_fixed_width().is_some()
            } else {
                child.get_fixed_height().is_some()
            };
            if main_is_fixed {
                continue;
            }
            let dims = child.get_dimensions(
                ctx,
                &self.style.direction,
                content_width,
                available_width,
                content_height,
                child_available_height,
                children_basis,
            );
            shrink_main_total += if self.style.direction.is_row() {
                dims.0
            } else {
                dims.1
            };
        }

        // Pool that Grow children divide among themselves. Non-grow
        // children still see the full `available_*` so their Shrink-clamp
        // upper bound matches their pre-pass measurement.
        let available_width_for_grow = if self.style.direction.is_row() {
            (available_width - shrink_main_total).max(0.0)
        } else {
            available_width
        };
        let available_height_for_grow = if !self.style.direction.is_row() {
            (available_height - shrink_main_total).max(0.0)
        } else {
            available_height
        };

        // Compute dimensions for every child once and cache the results.
        // Absolute-positioned children get None so they are skipped in the
        // normal-flow calculations below.
        let child_dims: Vec<Option<(f32, f32)>> = self
            .children
            .iter()
            .enumerate()
            .map(|(i, child)| {
                let child = child.lock().expect("widget lock poisoned");
                if child.is_absolute_positioned() {
                    return None;
                }
                let (avail_w, avail_h) = if child_main_basis[i] > 0.0 {
                    (available_width_for_grow, available_height_for_grow)
                } else {
                    (available_width, child_available_height)
                };
                Some(child.get_dimensions(
                    ctx,
                    &self.style.direction,
                    content_width,
                    avail_w,
                    content_height,
                    avail_h,
                    children_basis,
                ))
            })
            .collect();

        // Wrap-aware row layout. Each child sits on the current line until
        // the next would overflow the container's main-axis content width;
        // then the cursor jumps to the start of a new line offset by the
        // tallest child on the previous line. No flex grow distribution
        // happens within a wrapped line — children keep their natural
        // dimensions. Cross-axis (column) wrap is intentionally not
        // supported; it isn't needed by any current caller.
        if self.style.flex_wrap && self.style.direction.is_row() {
            let inset_left = self.style.inset_left();
            let inset_top = self.style.inset_top();
            let mut cursor_x = inset_left;
            let mut cursor_y = inset_top;
            let mut line_max_height: f32 = 0.0;
            let mut is_first_on_line = true;
            let line_main_max = inset_left + content_width;

            for (i, child_ref) in self.children.iter_mut().enumerate() {
                let dims = match child_dims[i] {
                    Some(d) => d,
                    None => continue,
                };
                let projected = if is_first_on_line {
                    cursor_x + dims.0
                } else {
                    cursor_x + self.style.gap + dims.0
                };
                if !is_first_on_line && projected > line_main_max {
                    cursor_x = inset_left;
                    cursor_y += line_max_height + self.style.gap;
                    line_max_height = 0.0;
                    is_first_on_line = true;
                }
                if !is_first_on_line {
                    cursor_x += self.style.gap;
                }
                let (avail_w, avail_h) = if child_main_basis[i] > 0.0 {
                    (available_width_for_grow, available_height_for_grow)
                } else {
                    (available_width, available_height)
                };
                let mut child = child_ref.lock().expect("widget lock poisoned");
                child.layout(
                    ctx,
                    cursor_x,
                    cursor_y,
                    &self.style.direction,
                    content_width,
                    avail_w,
                    content_height,
                    avail_h,
                    children_basis,
                );
                cursor_x += dims.0;
                line_max_height = line_max_height.max(dims.1);
                is_first_on_line = false;
            }

            for child in self.children.iter_mut() {
                let mut child = child.lock().expect("widget lock poisoned");
                if let Some((offset_x, offset_y)) = child.get_absolute_offset() {
                    let child_x = self.style.inset_left() + offset_x;
                    let child_y = self.style.inset_top() + offset_y;
                    child.layout(
                        ctx,
                        child_x,
                        child_y,
                        &self.style.direction,
                        content_width,
                        available_width,
                        content_height,
                        available_height,
                        children_basis,
                    );
                }
            }
            return;
        }

        // Calculate total size of children in main axis (excluding absolute positioned)
        let mut total_main_size = 0.0;
        let mut max_cross_size = 0.0;
        for dims in &child_dims {
            if let Some((w, h)) = dims {
                if self.style.direction.is_row() {
                    total_main_size += w;
                    max_cross_size = f32::max(max_cross_size, *h);
                } else {
                    total_main_size += h;
                    max_cross_size = f32::max(max_cross_size, *w);
                }
            }
        }

        // Calculate spacing and initial offset based on main alignment (excluding absolute positioned)
        // Note: non_absolute_count and gap_space are already calculated above

        // Add gap space between children to total_main_size
        total_main_size += gap_space;

        let spacing = self.style.main_alignment.get_spacing(
            non_absolute_count,
            if self.style.direction.is_row() {
                content_width
            } else {
                content_height
            },
            total_main_size,
        );
        let initial_offset = self.style.main_alignment.get_space_around_offset(spacing);

        // Start positioning children from the content area with initial offset.
        // Coordinates are parent-relative: children are positioned from this
        // widget's own top-left origin, not in absolute screen space. The
        // renderer and hit-test walker are responsible for composing these
        // offsets.
        let mut current_x = self.style.inset_left();
        let mut current_y = self.style.inset_top();

        if self.style.direction.is_reverse() {
            if self.style.direction.is_row() {
                current_x += content_width;
            } else {
                current_y += content_height;
            }
        }

        // Apply main axis alignment offset
        let main_offset = self.style.main_alignment.get_offset(
            total_main_size,
            if self.style.direction.is_row() {
                content_width
            } else {
                content_height
            },
        );
        if self.style.direction.is_row() {
            current_x += main_offset;
        } else {
            current_y += main_offset;
        }

        // Apply initial offset for SpaceAround
        if matches!(self.style.main_alignment, Alignment::SpaceAround) {
            if self.style.direction.is_row() {
                current_x += initial_offset;
            } else {
                current_y += initial_offset;
            }
        }

        // Layout non-absolute children first
        for (i, child) in self.children.iter_mut().enumerate() {
            // Use cached dimensions; None means absolute-positioned, skip it
            let child_dimensions = match child_dims[i] {
                Some(dims) => dims,
                None => continue,
            };
            let mut child = child.lock().expect("widget lock poisoned");

            // Calculate cross axis offset based on cross alignment
            let cross_offset = self.style.cross_alignment.get_offset(
                if self.style.direction.is_row() {
                    child_dimensions.1
                } else {
                    child_dimensions.0
                },
                if self.style.direction.is_row() {
                    content_height
                } else {
                    content_width
                },
            );

            // Apply cross axis offset
            let child_x = if self.style.direction.is_row() {
                current_x
            } else {
                current_x + cross_offset
            };
            let child_y = if self.style.direction.is_row() {
                current_y + cross_offset
            } else {
                current_y
            };

            if self.style.direction.is_reverse() {
                if self.style.direction.is_row() {
                    current_x -= child_dimensions.0;
                } else {
                    current_y -= child_dimensions.1;
                }
            }

            let (avail_w, avail_h) = if child_main_basis[i] > 0.0 {
                (available_width_for_grow, available_height_for_grow)
            } else {
                (available_width, child_available_height)
            };
            child.layout(
                ctx,
                child_x,
                child_y,
                &self.style.direction,
                content_width,
                avail_w,
                content_height,
                avail_h,
                children_basis,
            );

            if !self.style.direction.is_reverse() {
                self.style.direction.update_main_axis_position(
                    &mut current_x,
                    &mut current_y,
                    if self.style.direction.is_row() {
                        child_dimensions.0
                    } else {
                        child_dimensions.1
                    },
                );
            }

            // Add spacing for SpaceBetween and SpaceAround
            if !self.style.direction.is_reverse() && spacing > 0.0 {
                if self.style.direction.is_row() {
                    current_x += spacing;
                } else {
                    current_y += spacing;
                }
            }

            // Add gap between consecutive elements (except for the last element)
            if !self.style.direction.is_reverse() && self.style.gap > 0.0 {
                if self.style.direction.is_row() {
                    current_x += self.style.gap;
                } else {
                    current_y += self.style.gap;
                }
            }
        }

        // Now layout absolute positioned children
        for child in self.children.iter_mut() {
            let mut child = child.lock().expect("widget lock poisoned");
            if let Some((offset_x, offset_y)) = child.get_absolute_offset() {
                let child_x = self.style.inset_left() + offset_x;
                let child_y = self.style.inset_top() + offset_y;

                child.layout(
                    ctx,
                    child_x,
                    child_y,
                    &self.style.direction,
                    content_width,
                    available_width,
                    content_height,
                    available_height,
                    children_basis,
                );
            }
        }

        // Measure content extent so scroll_y can be clamped against it. The
        // scroll offset itself is applied at render/hit-test time via canvas
        // translation, not by mutating child layout rects.
        if self.style.overflow == Overflow::Scroll && !self.style.direction.is_row() {
            let content_top = self.style.inset_top();
            let mut max_bottom: f32 = content_top;
            for child_ref in self.children.iter() {
                let child = child_ref.lock().expect("widget lock poisoned");
                if child.is_absolute_positioned() {
                    continue;
                }
                if let Some(r) = child.get_layout_rect() {
                    max_bottom = max_bottom.max(r.y + r.height);
                }
            }
            // Follow-the-tail (`scroll_follow_end`): decide from the PRE-update
            // extents whether the view sat at the bottom, so content growth
            // can't un-pin it before we compare.
            let first_measure = self.viewport_height == 0.0;
            let was_max = (self.content_height - self.viewport_height).max(0.0);
            let was_pinned = self.scroll_y_target >= was_max - 0.5;
            self.content_height = max_bottom - content_top;
            self.viewport_height = content_height;

            let max_scroll = (self.content_height - self.viewport_height).max(0.0);
            if self.style.scroll_follow_end && (first_measure || was_pinned) {
                self.scroll_y_target = max_scroll;
                if first_measure {
                    // Land on the latest content; easing in from the top on
                    // mount would replay the whole history as an animation.
                    self.scroll_y = max_scroll;
                }
            }
            self.scroll_y_target = self.scroll_y_target.clamp(0.0, max_scroll);
            self.scroll_y = self.scroll_y.clamp(0.0, max_scroll);
        }
    }

    fn get_children_fixed_width(&self) -> f32 {
        self.get_children_fixed_on_axis(Axis::Horizontal)
    }

    fn get_children_fixed_height(&self) -> f32 {
        self.get_children_fixed_on_axis(Axis::Vertical)
    }

    fn get_fixed_width(&self) -> Option<f32> {
        self.style.width.as_fixed()
    }

    fn get_fixed_height(&self) -> Option<f32> {
        self.style.height.as_fixed()
    }

    fn contains_point(&self, point: &Point) -> bool {
        if let Some(layout) = &self.layout {
            // Calculate the content area by excluding margins
            let content_x = layout.x + self.style.margin.get_left();
            let content_y = layout.y + self.style.margin.get_top();
            let content_width =
                layout.width - self.style.margin.get_left() - self.style.margin.get_right();
            let content_height =
                layout.height - self.style.margin.get_top() - self.style.margin.get_bottom();

            point.x() >= content_x
                && point.x() <= content_x + content_width
                && point.y() >= content_y
                && point.y() <= content_y + content_height
        } else {
            false
        }
    }

    fn blocks_point(&self, point: &Point) -> bool {
        // Exiting subtrees are hit-test-invisible; the world beneath a
        // ghost gets its vote back immediately.
        if self.exiting {
            return false;
        }
        if !self.contains_point(point) {
            return false;
        }
        if self.block_interactions {
            return true;
        }
        // A pointer listener on a transparent shell still consumes presses.
        if ["mouse_down", "mouse_up", "contextmenu"]
            .iter()
            .any(|name| self.event_listeners.contains_key(*name))
        {
            return true;
        }
        // Same child-space shift as `handle_event`.
        let origin = self
            .layout
            .as_ref()
            .map(|r| (r.x, r.y))
            .unwrap_or((0.0, 0.0));
        let scroll_y = if self.style.overflow == Overflow::Scroll {
            self.scroll_y
        } else {
            0.0
        };
        let local = Point::new(point.x() - origin.0, point.y() - origin.1 + scroll_y);
        self.children.iter().any(|child| {
            child
                .lock()
                .expect("widget lock poisoned")
                .blocks_point(&local)
        })
    }

    fn declared_cursor(&self) -> CursorRole {
        // The declared style, not the interpolated `style`: the cursor is
        // authored intent, unaffected by hover/animation.
        self.declared_style.cursor
    }

    fn blocks_interactions(&self) -> bool {
        self.block_interactions
            || ["mouse_down", "mouse_up", "contextmenu"]
                .iter()
                .any(|name| self.event_listeners.contains_key(*name))
    }

    fn get_layout_rect(&self) -> Option<&Rect> {
        self.layout.as_ref()
    }

    fn scroll_offset(&self) -> (f32, f32) {
        if self.style.overflow == Overflow::Scroll {
            (0.0, self.scroll_y)
        } else {
            (0.0, 0.0)
        }
    }

    fn get_children_mut(&mut self) -> Vec<WidgetRef> {
        self.children.clone()
    }

    fn get_type(&self) -> &str {
        "box"
    }

    fn set_hovered(&mut self, hovered: bool) {
        if self.hovered == hovered {
            return;
        }
        let old_rendered = self.style.clone();
        self.hovered = hovered;
        let new_target = self.target_style().clone();

        if new_target.transitions.any_enabled() {
            self.animations.retarget(&old_rendered, &new_target);
            if self.animations.is_empty() {
                self.style = new_target;
            } else {
                // Non-transitioned properties snap immediately (see the
                // reconcile path in `update`).
                self.style = self.animations.render_onto(&new_target);
            }
        } else {
            self.animations = AnimationState::default();
            self.style = new_target;
        }
    }

    fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    fn render_effects(&self) -> Option<crate::widget::RenderEffects> {
        let style = self.effective_style();
        let has_opacity = !style.opacity.is_opaque();
        let has_transform = !style.transform.is_identity();
        if !has_opacity && !has_transform {
            return None;
        }
        // Pivot is the widget's layout center in parent-relative
        // coordinates, so that scale/rotate feel natural. Fall back to
        // (0, 0) when the widget hasn't been laid out yet.
        let (pivot_x, pivot_y) = self
            .layout
            .as_ref()
            .map(|r| (r.x + r.width / 2.0, r.y + r.height / 2.0))
            .unwrap_or((0.0, 0.0));
        Some(crate::widget::RenderEffects {
            opacity: style.opacity.value(),
            transform: style.transform,
            pivot_x,
            pivot_y,
        })
    }

    fn tick_animations(&mut self, ctx: &mut crate::widget::event::TickContext) -> TickResult {
        let dt = ctx.dt;
        let mut result = self.tick_own_animations(dt);
        result = result.merge(self.tick_smooth_scroll(dt));
        let children = self.children.clone();
        for child in &children {
            let child_result = {
                let mut guard = child.lock().expect("widget lock poisoned");
                guard.tick_animations(ctx)
            };
            result = result.merge(child_result);
        }
        // A ghost whose exit animation settled this frame is now safe
        // to drop. Removing it triggers one more layout pass so its
        // former slot collapses. The drain pushes the dropped child's
        // owned_path_prefix into `ctx.drained_path_prefixes` so the UI
        // can flush owned hooks at the next render boundary.
        let before = self.children.len();
        self.drain_exited_children(ctx);
        if self.children.len() != before {
            result.needs_layout = true;
            result.needs_repaint = true;
        }
        result
    }

    fn is_exiting(&self) -> bool {
        self.exiting
    }

    fn begin_exit(&mut self) -> bool {
        if self.exiting {
            return true;
        }

        // First try this widget's own exit animation. Requires an
        // exit_style whose transitions list is non-empty — otherwise
        // "animate out" is meaningless.
        if let Some(exit) = self
            .exit_style
            .as_ref()
            .filter(|e| e.transitions.any_enabled())
            .cloned()
        {
            let old_rendered = self.style.clone();
            self.exiting = true;
            self.animations.retarget(&old_rendered, &exit);
            if !self.animations.is_empty() {
                return true;
            }
            // exit_style was declared but matched the current rendered
            // style — fall through to the cascade rather than giving up.
            self.exiting = false;
        }

        // Cascade: if this widget has no own exit animation (or it had
        // nothing to animate), try to begin_exit on each child. If any
        // child can animate out, become a passive ghost so the subtree
        // stays in the tree until its exiting descendants finish.
        //
        // A parent-initiated cascade is a group moment: when this
        // container declares `stagger`, offset each exiting child's
        // springs by its slot so the children peel out in sequence.
        // (Individual removals via reconcile call begin_exit on the
        // orphan directly and never pass through here, so they exit
        // undelayed by design.)
        let mut any_descendant_exiting = false;
        let children = self.children.clone();
        let count = children.len();
        let stagger = self
            .declared_style
            .stagger
            .filter(|s| s.exit_step > 0.0 && count > 1);
        for (i, child) in children.iter().enumerate() {
            let mut g = child.lock().expect("widget lock poisoned");
            if g.begin_exit() {
                if let Some(st) = stagger {
                    let slot = match st.exit_order {
                        StaggerOrder::Forward => i,
                        StaggerOrder::Reverse => count - 1 - i,
                    };
                    if slot > 0 {
                        g.add_group_delay(slot as f32 * st.exit_step);
                    }
                }
                any_descendant_exiting = true;
            }
        }

        if any_descendant_exiting {
            self.exiting = true;
            return true;
        }

        false
    }

    fn cancel_exit(&mut self) {
        if !self.exiting {
            return;
        }
        let old_rendered = self.style.clone();
        self.exiting = false;
        let new_target = self.target_style().clone();
        if new_target.transitions.any_enabled() {
            self.animations.retarget(&old_rendered, &new_target);
            if self.animations.is_empty() {
                self.style = new_target;
            } else {
                // Non-transitioned properties snap immediately (see the
                // reconcile path in `update`).
                self.style = self.animations.render_onto(&new_target);
            }
        } else {
            self.animations = AnimationState::default();
            self.style = new_target;
        }
        // If we were a passive ghost, cascade the cancel so any
        // exiting descendants return to their normal state too.
        let children = self.children.clone();
        for child in &children {
            let mut g = child.lock().expect("widget lock poisoned");
            g.cancel_exit();
        }
    }

    fn is_exit_complete(&self) -> bool {
        if !self.exiting {
            return false;
        }
        // Own animation must be settled (always true for passive ghosts).
        if !self.animations.is_empty() {
            return false;
        }
        // Any exiting descendants must have finished. Non-exiting
        // descendants don't block — they'll be dropped with us.
        for child in self.children.iter() {
            let g = child.lock().expect("widget lock poisoned");
            if g.is_exiting() && !g.is_exit_complete() {
                return false;
            }
        }
        true
    }

    fn restart_entry_animation(&mut self) {
        // Drop any in-flight exit state so the widget treats this as a
        // fresh mount. Without this, calling restart on a still-exiting
        // tree would leave `exiting = true` and the next reconcile would
        // treat the widget as a ghost.
        self.exiting = false;
        // Clear in-flight springs. `apply_entry_transition` below calls
        // `animations.retarget`, which only updates *targets* on existing
        // springs and keeps their `current` values — meaning a widget
        // mid-exit would have its spring snap targets back to declared
        // but render at the mid-exit position on the next tick. We want
        // a true reset: springs that start at `initial` and ease to
        // declared.
        self.animations = AnimationState::default();
        // Fresh springs above — any previously inherited group offset
        // died with them; ancestors re-inject as the restart cascade
        // unwinds back up the tree.
        self.group_delay = 0.0;
        // Re-seed style ↔ initial and arm the spring toward declared.
        // No-op if this widget has no initial_style or transitions.
        self.apply_entry_transition();
        // Cascade so every descendant with its own `initial:` plays again.
        let children = self.children.clone();
        for child in &children {
            let mut g = child.lock().expect("widget lock poisoned");
            g.restart_entry_animation();
        }
        // Children's springs are re-armed; offset them into this
        // container's cascade. Runs after the child cascade so nested
        // staggered containers' own offsets are already in place and
        // ancestor offsets sum on top.
        self.apply_child_stagger_offsets();
    }

    fn add_group_delay(&mut self, secs: f32) {
        if secs == 0.0 {
            return;
        }
        self.animations.add_delay(secs);
        self.group_delay = (self.group_delay + secs).max(0.0);
        let children = self.children.clone();
        for child in &children {
            let mut g = child.lock().expect("widget lock poisoned");
            g.add_group_delay(secs);
        }
    }

    fn strip_inherited_group_delay(&mut self) {
        // Subtracting exactly the inherited amount from the whole
        // subtree leaves cascades injected by containers *inside* it
        // intact: descendants carry `inherited + internal` and keep
        // `internal`.
        let inherited = self.group_delay;
        if inherited > 0.0 {
            self.add_group_delay(-inherited);
        }
    }

    fn is_hovered(&self) -> bool {
        self.hovered
    }

    fn fire_listeners(&self, event_name: &str, event: &Event) {
        if let Some(listeners) = self.event_listeners.get(event_name) {
            for listener in listeners {
                listener(event);
            }
        }
    }

    fn is_absolute_positioned(&self) -> bool {
        matches!(self.style.position, Position::Absolute(_, _))
    }

    fn get_absolute_offset(&self) -> Option<(f32, f32)> {
        match self.style.position {
            Position::Absolute(x, y) => Some((x, y)),
            _ => None,
        }
    }

    fn render(
        &self,
        ctx: &mut dyn crate::widget::RenderContext,
        _focused: bool,
        image_cache: &mut crate::widget::image::ImageCache,
    ) {
        if let Some(layout) = &self.layout {
            let style = self.effective_style();

            let border_box_x = layout.x + style.margin.get_left();
            let border_box_y = layout.y + style.margin.get_top();
            let border_box_width =
                layout.width - style.margin.get_left() - style.margin.get_right();
            let border_box_height =
                layout.height - style.margin.get_top() - style.margin.get_bottom();

            // The drop shadow casts first — everything else (backdrop
            // capture, background, border, children) paints on top of it.
            if let Some(shadow) = style.shadow.as_ref() {
                ctx.draw_shadow(
                    border_box_x,
                    border_box_y,
                    border_box_width,
                    border_box_height,
                    &style.corners,
                    shadow,
                );
            }

            // Backdrop filter (frosted-glass): captures the canvas
            // under the border box, blurs it, and the panel's own
            // background image / colour composite on top of that
            // blurred capture. The border draws *outside* the layer
            // so it stays sharp on the main canvas.
            let backdrop_active = style
                .backdrop_filter
                .map(|bf| bf.is_active())
                .unwrap_or(false);
            if backdrop_active {
                let sigma = style.backdrop_filter.unwrap().blur;
                ctx.push_backdrop_blur(
                    border_box_x,
                    border_box_y,
                    border_box_width,
                    border_box_height,
                    &style.corners,
                    sigma,
                );
            }

            if let Some(background_image_path) = &style.background_image {
                ctx.draw_image(
                    background_image_path,
                    border_box_x,
                    border_box_y,
                    border_box_width,
                    border_box_height,
                    image_cache,
                );
            }

            if let Some(background_color) = &style.background_color {
                if style.corners.is_all_sharp() {
                    ctx.fill_rect(
                        border_box_x,
                        border_box_y,
                        border_box_width,
                        border_box_height,
                        background_color,
                    );
                } else {
                    ctx.fill_corners_rect(
                        border_box_x,
                        border_box_y,
                        border_box_width,
                        border_box_height,
                        &style.corners,
                        background_color,
                    );
                }
            }

            if backdrop_active {
                ctx.pop_backdrop_blur();
            }

            // Inner glow paints after the background fill, before the
            // border and children. CSS `box-shadow: inset` semantics —
            // the glow sits beneath descendants but on top of the panel
            // fill, then the border draws over it sharp.
            if let Some(glow) = style.inner_glow.as_ref() {
                ctx.draw_inner_glow(
                    border_box_x,
                    border_box_y,
                    border_box_width,
                    border_box_height,
                    &style.corners,
                    glow,
                );
            }

            ctx.draw_border(
                &style.border,
                border_box_x,
                border_box_y,
                border_box_width,
                border_box_height,
                &style.corners,
            );

            // For scrollable/hidden overflow, clip children to the border box
            if self.style.overflow != Overflow::Visible {
                ctx.push_clip_rect(
                    border_box_x,
                    border_box_y,
                    border_box_width,
                    border_box_height,
                );
            }
        }
    }

    fn needs_post_render(&self) -> bool {
        self.style.overflow != Overflow::Visible
    }

    fn post_render(
        &self,
        ctx: &mut dyn crate::widget::RenderContext,
        _image_cache: &mut crate::widget::image::ImageCache,
    ) {
        if self.style.overflow != Overflow::Visible && self.layout.is_some() {
            ctx.pop_clip_rect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    // Helper widget for testing - a simple widget with fixed dimensions
    struct TestWidget {
        width: f32,
        height: f32,
        layout: Option<Rect>,
    }

    impl TestWidget {
        fn new(width: f32, height: f32) -> Self {
            Self {
                width,
                height,
                layout: None,
            }
        }
    }

    impl Widget for TestWidget {
        fn get_type(&self) -> &str {
            "test"
        }

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
            (self.width, self.height)
        }

        fn get_children(&self) -> Vec<WidgetRef> {
            Vec::new()
        }

        fn get_basis(&self, _direction: &Direction) -> f32 {
            0.0
        }

        fn get_children_basis(&self) -> f32 {
            0.0
        }

        fn get_fixed_width(&self) -> Option<f32> {
            Some(self.width)
        }

        fn get_fixed_height(&self) -> Option<f32> {
            Some(self.height)
        }

        fn handle_event(
            &mut self,
            _event: &Event,
            _ctx: &mut EventContext,
            _self_ref: &WidgetRef,
        ) -> bool {
            false
        }

        fn layout(
            &mut self,
            _ctx: &LayoutContext,
            cursor_x: f32,
            cursor_y: f32,
            _parent_direction: &Direction,
            _parent_width: f32,
            _parent_available_width: f32,
            _parent_height: f32,
            _parent_available_height: f32,
            _sibling_basis: f32,
        ) {
            self.layout = Some(Rect::new(cursor_x, cursor_y, self.width, self.height));
        }

        fn update(&mut self, _new_widget: WidgetRef) -> UpdateResult {
            UpdateResult::replace()
        }

        fn contains_point(&self, _point: &Point) -> bool {
            false
        }

        fn get_layout_rect(&self) -> Option<&Rect> {
            self.layout.as_ref()
        }
    }

    fn test_ctx() -> LayoutContext<'static> {
        LayoutContext {
            font_collection: None,
            default_font: None,
            measure_grow_width_as_shrink: false,
            measure_grow_height_as_shrink: false,
        }
    }

    /// A `width: grow, height: shrink` widget whose height depends on its
    /// width — a font-free stand-in for a Text that wraps. Mirrors the real
    /// text path: narrower width → more lines → taller. Used to prove a Shrink
    /// row measures it at its render width (one line) rather than width≈0
    /// (one glyph per line → balloon).
    struct WrapTestWidget {
        intrinsic_w: f32,
        line_h: f32,
    }

    impl Widget for WrapTestWidget {
        fn get_type(&self) -> &str {
            "wrap_test"
        }

        fn get_dimensions(
            &self,
            _ctx: &LayoutContext,
            parent_direction: &Direction,
            parent_width: f32,
            parent_available_width: f32,
            _parent_height: f32,
            _parent_available_height: f32,
            sibling_basis: f32,
        ) -> (f32, f32) {
            let width = if parent_direction.is_row() {
                parent_direction.get_grow_size(1.0, sibling_basis, parent_available_width)
            } else {
                parent_width
            };
            let lines = if width < self.intrinsic_w - 0.5 {
                (self.intrinsic_w / width.max(1.0)).ceil()
            } else {
                1.0
            };
            (width, lines * self.line_h)
        }

        fn get_children(&self) -> Vec<WidgetRef> {
            Vec::new()
        }

        // Grow along the main axis only when that axis is width (Row parent).
        fn get_basis(&self, direction: &Direction) -> f32 {
            if direction.is_row() {
                1.0
            } else {
                0.0
            }
        }

        fn get_children_basis(&self) -> f32 {
            0.0
        }

        fn get_fixed_width(&self) -> Option<f32> {
            None
        }

        fn get_fixed_height(&self) -> Option<f32> {
            None
        }

        fn handle_event(
            &mut self,
            _event: &Event,
            _ctx: &mut EventContext,
            _self_ref: &WidgetRef,
        ) -> bool {
            false
        }

        fn layout(
            &mut self,
            _ctx: &LayoutContext,
            _cursor_x: f32,
            _cursor_y: f32,
            _parent_direction: &Direction,
            _parent_width: f32,
            _parent_available_width: f32,
            _parent_height: f32,
            _parent_available_height: f32,
            _sibling_basis: f32,
        ) {
        }

        fn update(&mut self, _new_widget: WidgetRef) -> UpdateResult {
            UpdateResult::replace()
        }

        fn contains_point(&self, _point: &Point) -> bool {
            false
        }
    }

    #[test]
    fn test_grow_text_in_shrink_row_does_not_balloon() {
        let ctx = test_ctx();
        // A grow-width / shrink-height wrapping child as the lone item of a
        // Shrink-height row, inside a column parent with a real width and a
        // large available height. The row's height must be ~one line (the
        // child measured at the row's full width), NOT the per-glyph-wrapped
        // balloon it became when measured at width 0.
        let textish = Arc::new(Mutex::new(WrapTestWidget {
            intrinsic_w: 200.0,
            line_h: 20.0,
        }));
        let mut row = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Grow(1.0))
                .height(Size::Shrink)
                .direction(Direction::Row)
                .build(),
        );
        row.add_child(textish);

        let (_w, h) = row.get_dimensions(&ctx, &Direction::Column, 300.0, 300.0, 600.0, 600.0, 0.0);
        assert!(
            h <= 40.0,
            "Shrink row ballooned: height {h} (expected ~one 20px line, not a per-glyph wrap)"
        );
    }

    #[test]
    fn test_grow_text_shares_row_width_with_fixed_siblings() {
        let ctx = test_ctx();
        // The grow child sits next to a fixed-width sibling (like a row of
        // icon buttons). Its measured height should still be ~one line — the
        // grow pool (content - fixed) is wide enough to avoid wrapping.
        let textish = Arc::new(Mutex::new(WrapTestWidget {
            intrinsic_w: 200.0,
            line_h: 20.0,
        }));
        let button = Arc::new(Mutex::new(TestWidget::new(24.0, 24.0)));
        let mut row = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Grow(1.0))
                .height(Size::Shrink)
                .direction(Direction::Row)
                .build(),
        );
        row.add_child(textish);
        row.add_child(button);

        let (_w, h) = row.get_dimensions(&ctx, &Direction::Column, 300.0, 300.0, 600.0, 600.0, 0.0);
        assert!(
            h <= 40.0,
            "Shrink row with a fixed sibling ballooned: height {h}"
        );
    }

    #[test]
    fn test_grow_height_child_in_shrink_row_measures_as_content() {
        let ctx = test_ctx();
        // A Shrink-height row holding a full-height accent rule (`height:
        // grow`, no content) beside fixed content — the list-row shape. A
        // grow child has no leftover space to claim in a shrink parent, so
        // it must contribute its content (here: nothing) to the row's
        // measurement instead of the ancestor budget that used to balloon
        // the row to the viewport. The real layout pass then stretches it
        // into the resolved row.
        let rule = Arc::new(Mutex::new(FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Fixed(3.0))
                .height(Size::Grow(1.0))
                .build(),
        )));
        let content = Arc::new(Mutex::new(TestWidget::new(100.0, 40.0)));
        let mut row = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Fixed(300.0))
                .height(Size::Shrink)
                .direction(Direction::Row)
                .build(),
        );
        row.add_child(rule.clone());
        row.add_child(content);

        let (_w, h) = row.get_dimensions(&ctx, &Direction::Column, 300.0, 300.0, 600.0, 600.0, 0.0);
        assert_eq!(
            h, 40.0,
            "the shrink row must be its content's height, not the ancestor budget"
        );

        // And once the row's size is settled, the rule spans it.
        row.layout(
            &ctx,
            0.0,
            0.0,
            &Direction::Column,
            300.0,
            300.0,
            600.0,
            600.0,
            0.0,
        );
        let rule = rule.lock().unwrap();
        let rect = rule.get_layout_rect().expect("rule laid out");
        assert_eq!(
            rect.height, 40.0,
            "the grow rule stretches to the resolved row height"
        );
    }

    #[test]
    fn test_grow_width_child_in_shrink_column_measures_as_content() {
        let ctx = test_ctx();
        // The width twin: a Shrink-width column holding a horizontal rule
        // (`width: grow`) above fixed content.
        let rule = Arc::new(Mutex::new(FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Grow(1.0))
                .height(Size::Fixed(3.0))
                .build(),
        )));
        let content = Arc::new(Mutex::new(TestWidget::new(120.0, 20.0)));
        let mut column = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Shrink)
                .height(Size::Fixed(100.0))
                .direction(Direction::Column)
                .build(),
        );
        column.add_child(rule.clone());
        column.add_child(content);

        let (w, _h) = column.get_dimensions(&ctx, &Direction::Row, 800.0, 800.0, 100.0, 100.0, 0.0);
        assert_eq!(
            w, 120.0,
            "the shrink column must be its content's width, not the ancestor budget"
        );

        column.layout(
            &ctx,
            0.0,
            0.0,
            &Direction::Row,
            800.0,
            800.0,
            100.0,
            100.0,
            0.0,
        );
        let rule = rule.lock().unwrap();
        let rect = rule.get_layout_rect().expect("rule laid out");
        assert_eq!(
            rect.width, 120.0,
            "the grow rule stretches to the resolved column width"
        );
    }

    #[test]
    fn test_scroll_follow_end_pins_to_growing_content() {
        let ctx = test_ctx();
        let mut style = FlexStyle::builder()
            .width(Size::Fixed(100.0))
            .height(Size::Fixed(100.0))
            .direction(Direction::Column)
            .build();
        style.overflow = Overflow::Scroll;
        style.scroll_follow_end = true;
        let mut log = FlexWidget::with_style(style);
        log.add_child(Arc::new(Mutex::new(TestWidget::new(50.0, 300.0))));

        // First layout: land on the end (no ease-in from the top).
        log.layout(
            &ctx,
            0.0,
            0.0,
            &Direction::Column,
            100.0,
            100.0,
            400.0,
            400.0,
            0.0,
        );
        assert_eq!(
            log.scroll_y_target, 200.0,
            "first measure pins the target to the end"
        );
        assert_eq!(log.scroll_y, 200.0, "…and lands there without animating");

        // Content grows while the view sits at the bottom: stay pinned.
        log.add_child(Arc::new(Mutex::new(TestWidget::new(50.0, 100.0))));
        log.layout(
            &ctx,
            0.0,
            0.0,
            &Direction::Column,
            100.0,
            100.0,
            400.0,
            400.0,
            0.0,
        );
        assert_eq!(
            log.scroll_y_target, 300.0,
            "growth at the bottom re-pins to the new end"
        );

        // The reader scrolled up into the history: growth must NOT yank
        // them back down.
        log.scroll_y_target = 40.0;
        log.scroll_y = 40.0;
        log.add_child(Arc::new(Mutex::new(TestWidget::new(50.0, 100.0))));
        log.layout(
            &ctx,
            0.0,
            0.0,
            &Direction::Column,
            100.0,
            100.0,
            400.0,
            400.0,
            0.0,
        );
        assert_eq!(
            log.scroll_y_target, 40.0,
            "a reader up in the history stays put as the log grows"
        );
    }

    #[test]
    fn test_gap_in_row_layout() {
        let ctx = test_ctx();
        // Create a parent with fixed width and gap
        let gap = 10.0;
        let parent_width = 200.0;
        let parent_height = 100.0;

        let mut parent = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Fixed(parent_width))
                .height(Size::Fixed(parent_height))
                .gap(gap)
                .direction(Direction::Row)
                .build(),
        );

        let child_width = 50.0;
        let child_height = 80.0;

        for _ in 0..3 {
            let child = Arc::new(Mutex::new(TestWidget::new(child_width, child_height)));
            parent.add_child(child);
        }

        // Layout the parent
        let parent_ref = Arc::new(Mutex::new(parent));
        {
            let mut parent = parent_ref.lock().expect("widget lock poisoned");
            parent.layout(
                &ctx,
                0.0,
                0.0,
                &Direction::Row,
                parent_width,
                parent_width,
                parent_height,
                parent_height,
                0.0,
            );
        }

        // Verify children don't extend past parent bounds
        let parent = parent_ref.lock().expect("widget lock poisoned");
        let parent_layout = parent.layout.clone().unwrap();
        let content_width = parent_layout.width
            - parent.style.padding.get_left()
            - parent.style.padding.get_right()
            - parent.style.margin.get_left()
            - parent.style.margin.get_right()
            - parent.style.border.get_left()
            - parent.style.border.get_right();
        let content_right = parent_layout.x
            + parent.style.margin.get_left()
            + parent.style.padding.get_left()
            + parent.style.border.get_left()
            + content_width;

        for child_ref in parent.children.iter() {
            let child = child_ref.lock().expect("widget lock poisoned");
            if let Some(child_layout) = child
                .downcast_ref::<TestWidget>()
                .and_then(|w| w.layout.as_ref())
            {
                let child_right = child_layout.x + child_layout.width;
                assert!(
                    child_right <= content_right + 0.001, // Small epsilon for floating point
                    "Child extends past parent content area. Child right: {}, Content right: {}",
                    child_right,
                    content_right
                );
            }
        }
    }

    #[test]
    fn test_gap_in_column_layout() {
        let ctx = test_ctx();
        // Create a parent with fixed height and gap
        let gap = 15.0;
        let parent_width = 100.0;
        let parent_height = 200.0;

        let mut parent = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Fixed(parent_width))
                .height(Size::Fixed(parent_height))
                .gap(gap)
                .direction(Direction::Column)
                .build(),
        );

        let child_width = 80.0;
        let child_height = 50.0;

        for _ in 0..3 {
            let child = Arc::new(Mutex::new(TestWidget::new(child_width, child_height)));
            parent.add_child(child);
        }

        // Layout the parent
        let parent_ref = Arc::new(Mutex::new(parent));
        {
            let mut parent = parent_ref.lock().expect("widget lock poisoned");
            parent.layout(
                &ctx,
                0.0,
                0.0,
                &Direction::Column,
                parent_width,
                parent_width,
                parent_height,
                parent_height,
                0.0,
            );
        }

        // Verify children don't extend past parent bounds
        let parent = parent_ref.lock().expect("widget lock poisoned");
        let parent_layout = parent.layout.clone().unwrap();
        let content_height = parent_layout.height
            - parent.style.padding.get_top()
            - parent.style.padding.get_bottom()
            - parent.style.margin.get_top()
            - parent.style.margin.get_bottom()
            - parent.style.border.get_top()
            - parent.style.border.get_bottom();
        let content_bottom = parent_layout.y
            + parent.style.margin.get_top()
            + parent.style.padding.get_top()
            + parent.style.border.get_top()
            + content_height;

        for child_ref in parent.children.iter() {
            let child = child_ref.lock().expect("widget lock poisoned");
            if let Some(child_layout) = child
                .downcast_ref::<TestWidget>()
                .and_then(|w| w.layout.as_ref())
            {
                let child_bottom = child_layout.y + child_layout.height;
                assert!(
                    child_bottom <= content_bottom + 0.001, // Small epsilon for floating point
                    "Child extends past parent content area. Child bottom: {}, Content bottom: {}",
                    child_bottom,
                    content_bottom
                );
            }
        }
    }

    #[test]
    fn test_gap_with_shrink_width() {
        let ctx = test_ctx();
        // Test that gap is properly accounted for when parent has Shrink width
        let gap = 20.0;
        let parent_height = 100.0;

        let mut parent = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Shrink)
                .height(Size::Fixed(parent_height))
                .gap(gap)
                .direction(Direction::Row)
                .build(),
        );

        let child_width = 100.0;
        let child_height = 80.0;

        for _ in 0..2 {
            let child = Arc::new(Mutex::new(TestWidget::new(child_width, child_height)));
            parent.add_child(child);
        }

        // Layout the parent
        let parent_ref = Arc::new(Mutex::new(parent));
        {
            let mut parent = parent_ref.lock().expect("widget lock poisoned");
            parent.layout(
                &ctx,
                0.0,
                0.0,
                &Direction::Row,
                1000.0, // Large available width
                1000.0,
                parent_height,
                parent_height,
                0.0,
            );
        }

        // Verify parent width accounts for gap
        let parent = parent_ref.lock().expect("widget lock poisoned");
        let parent_layout = parent.layout.clone().unwrap();
        let expected_width = 2.0 * child_width + gap; // 2 children + 1 gap
        assert!(
            (parent_layout.width - expected_width).abs() < 0.001,
            "Parent width should account for gap. Expected: {}, Got: {}",
            expected_width,
            parent_layout.width
        );
    }

    #[test]
    fn test_gap_with_shrink_height() {
        let ctx = test_ctx();
        // Test that gap is properly accounted for when parent has Shrink height
        let gap = 25.0;
        let parent_width = 100.0;

        let mut parent = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Fixed(parent_width))
                .height(Size::Shrink)
                .gap(gap)
                .direction(Direction::Column)
                .build(),
        );

        let child_width = 80.0;
        let child_height = 75.0;

        for _ in 0..2 {
            let child = Arc::new(Mutex::new(TestWidget::new(child_width, child_height)));
            parent.add_child(child);
        }

        // Layout the parent
        let parent_ref = Arc::new(Mutex::new(parent));
        {
            let mut parent = parent_ref.lock().expect("widget lock poisoned");
            parent.layout(
                &ctx,
                0.0,
                0.0,
                &Direction::Column,
                parent_width,
                parent_width,
                1000.0, // Large available height
                1000.0,
                0.0,
            );
        }

        // Verify parent height accounts for gap
        let parent = parent_ref.lock().expect("widget lock poisoned");
        let parent_layout = parent.layout.clone().unwrap();
        let expected_height = 2.0 * child_height + gap; // 2 children + 1 gap
        assert!(
            (parent_layout.height - expected_height).abs() < 0.001,
            "Parent height should account for gap. Expected: {}, Got: {}",
            expected_height,
            parent_layout.height
        );
    }

    #[test]
    fn test_gap_with_single_child() {
        let ctx = test_ctx();
        // Gap should not affect layout when there's only one child
        let gap = 10.0;
        let parent_width = 200.0;
        let parent_height = 100.0;

        let mut parent = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Fixed(parent_width))
                .height(Size::Fixed(parent_height))
                .gap(gap)
                .direction(Direction::Row)
                .build(),
        );

        let child_width = 50.0;
        let child_height = 80.0;
        let child = Arc::new(Mutex::new(TestWidget::new(child_width, child_height)));
        parent.add_child(child);

        // Layout the parent
        let parent_ref = Arc::new(Mutex::new(parent));
        {
            let mut parent = parent_ref.lock().expect("widget lock poisoned");
            parent.layout(
                &ctx,
                0.0,
                0.0,
                &Direction::Row,
                parent_width,
                parent_width,
                parent_height,
                parent_height,
                0.0,
            );
        }

        // With only one child, gap should not be applied
        // Child should still fit within parent
        let parent = parent_ref.lock().expect("widget lock poisoned");
        let parent_layout = parent.layout.clone().unwrap();
        let content_width = parent_layout.width
            - parent.style.padding.get_left()
            - parent.style.padding.get_right()
            - parent.style.margin.get_left()
            - parent.style.margin.get_right()
            - parent.style.border.get_left()
            - parent.style.border.get_right();

        assert!(
            child_width <= content_width,
            "Single child should fit in parent even with gap set"
        );
    }

    #[test]
    fn test_shrink_height_clamped_to_parent() {
        let ctx = test_ctx();
        // A Shrink-height column child whose children total more than the
        // parent's fixed height should be clamped to the parent's content area.
        let parent_height = 200.0;
        let parent_width = 300.0;

        // Inner shrink widget with children totalling 400px (exceeds parent)
        let mut shrink_child = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Grow(1.0))
                .height(Size::Shrink)
                .direction(Direction::Column)
                .build(),
        );
        for _ in 0..4 {
            let item = Arc::new(Mutex::new(TestWidget::new(100.0, 100.0)));
            shrink_child.add_child(item);
        }

        // Parent with fixed height
        let mut parent = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Fixed(parent_width))
                .height(Size::Fixed(parent_height))
                .direction(Direction::Column)
                .build(),
        );
        parent.add_child(Arc::new(Mutex::new(shrink_child)));

        // Layout
        let parent_ref = Arc::new(Mutex::new(parent));
        {
            let mut p = parent_ref.lock().expect("widget lock poisoned");
            p.layout(
                &ctx,
                0.0,
                0.0,
                &Direction::Column,
                parent_width,
                parent_width,
                parent_height,
                parent_height,
                0.0,
            );
        }

        // The shrink child's height should be clamped to the parent's content height
        let p = parent_ref.lock().expect("widget lock poisoned");
        let shrink_ref = &p.children[0];
        let shrink = shrink_ref.lock().expect("widget lock poisoned");
        let (_, shrink_height) = shrink.get_dimensions(
            &ctx,
            &Direction::Column,
            parent_width,
            parent_width,
            parent_height,
            parent_height,
            0.0,
        );
        assert!(
            shrink_height <= parent_height + 0.001,
            "Shrink child height ({}) should be clamped to parent height ({})",
            shrink_height,
            parent_height
        );
    }

    #[test]
    fn test_grow_respects_shrink_sibling_width() {
        let ctx = test_ctx();
        // Row parent containing [Grow, Shrink]. The Grow child must leave
        // room for the Shrink sibling so the Shrink stays inside the
        // parent's right edge.
        let parent_width = 300.0;
        let parent_height = 100.0;
        let shrink_inner_width = 80.0;

        let mut shrink_child = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Shrink)
                .height(Size::Fixed(parent_height))
                .direction(Direction::Row)
                .build(),
        );
        shrink_child.add_child(Arc::new(Mutex::new(TestWidget::new(
            shrink_inner_width,
            parent_height,
        ))));

        let grow_child = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Grow(1.0))
                .height(Size::Fixed(parent_height))
                .direction(Direction::Row)
                .build(),
        );

        let mut parent = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Fixed(parent_width))
                .height(Size::Fixed(parent_height))
                .direction(Direction::Row)
                .build(),
        );
        parent.add_child(Arc::new(Mutex::new(grow_child)));
        parent.add_child(Arc::new(Mutex::new(shrink_child)));

        let parent_ref = Arc::new(Mutex::new(parent));
        {
            let mut p = parent_ref.lock().expect("widget lock poisoned");
            p.layout(
                &ctx,
                0.0,
                0.0,
                &Direction::Row,
                parent_width,
                parent_width,
                parent_height,
                parent_height,
                0.0,
            );
        }

        let p = parent_ref.lock().expect("widget lock poisoned");
        let grow_layout = p.children[0]
            .lock()
            .expect("widget lock poisoned")
            .get_layout_rect()
            .cloned()
            .expect("grow child laid out");
        let shrink_layout = p.children[1]
            .lock()
            .expect("widget lock poisoned")
            .get_layout_rect()
            .cloned()
            .expect("shrink child laid out");

        let expected_grow_width = parent_width - shrink_inner_width;
        assert!(
            (grow_layout.width - expected_grow_width).abs() < 0.001,
            "Grow child width should be {} but was {}",
            expected_grow_width,
            grow_layout.width,
        );
        assert!(
            (shrink_layout.width - shrink_inner_width).abs() < 0.001,
            "Shrink child width should be {} but was {}",
            shrink_inner_width,
            shrink_layout.width,
        );
        let shrink_right = shrink_layout.x + shrink_layout.width;
        assert!(
            shrink_right <= parent_width + 0.001,
            "Shrink sibling overflowed parent. shrink_right={}, parent_width={}",
            shrink_right,
            parent_width,
        );
    }

    #[test]
    fn test_grow_respects_shrink_sibling_height() {
        let ctx = test_ctx();
        // Column parent containing [Grow, Shrink]. Same invariant on the
        // vertical axis.
        let parent_width = 100.0;
        let parent_height = 300.0;
        let shrink_inner_height = 80.0;

        let mut shrink_child = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Fixed(parent_width))
                .height(Size::Shrink)
                .direction(Direction::Column)
                .build(),
        );
        shrink_child.add_child(Arc::new(Mutex::new(TestWidget::new(
            parent_width,
            shrink_inner_height,
        ))));

        let grow_child = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Fixed(parent_width))
                .height(Size::Grow(1.0))
                .direction(Direction::Column)
                .build(),
        );

        let mut parent = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Fixed(parent_width))
                .height(Size::Fixed(parent_height))
                .direction(Direction::Column)
                .build(),
        );
        parent.add_child(Arc::new(Mutex::new(grow_child)));
        parent.add_child(Arc::new(Mutex::new(shrink_child)));

        let parent_ref = Arc::new(Mutex::new(parent));
        {
            let mut p = parent_ref.lock().expect("widget lock poisoned");
            p.layout(
                &ctx,
                0.0,
                0.0,
                &Direction::Column,
                parent_width,
                parent_width,
                parent_height,
                parent_height,
                0.0,
            );
        }

        let p = parent_ref.lock().expect("widget lock poisoned");
        let grow_layout = p.children[0]
            .lock()
            .expect("widget lock poisoned")
            .get_layout_rect()
            .cloned()
            .expect("grow child laid out");
        let shrink_layout = p.children[1]
            .lock()
            .expect("widget lock poisoned")
            .get_layout_rect()
            .cloned()
            .expect("shrink child laid out");

        let expected_grow_height = parent_height - shrink_inner_height;
        assert!(
            (grow_layout.height - expected_grow_height).abs() < 0.001,
            "Grow child height should be {} but was {}",
            expected_grow_height,
            grow_layout.height,
        );
        assert!(
            (shrink_layout.height - shrink_inner_height).abs() < 0.001,
            "Shrink child height should be {} but was {}",
            shrink_inner_height,
            shrink_layout.height,
        );
        let shrink_bottom = shrink_layout.y + shrink_layout.height;
        assert!(
            shrink_bottom <= parent_height + 0.001,
            "Shrink sibling overflowed parent. shrink_bottom={}, parent_height={}",
            shrink_bottom,
            parent_height,
        );
    }

    #[test]
    fn test_grow_respects_shrink_with_fixed_and_gap() {
        let ctx = test_ctx();
        // [Fixed, Grow, Shrink] in a row with a gap. The Grow sibling
        // must leave room for both the fixed and shrink children plus
        // gaps.
        let parent_width = 400.0;
        let parent_height = 100.0;
        let fixed_width = 60.0;
        let shrink_inner_width = 90.0;
        let gap = 10.0;

        let mut shrink_child = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Shrink)
                .height(Size::Fixed(parent_height))
                .direction(Direction::Row)
                .build(),
        );
        shrink_child.add_child(Arc::new(Mutex::new(TestWidget::new(
            shrink_inner_width,
            parent_height,
        ))));

        let mut parent = FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Fixed(parent_width))
                .height(Size::Fixed(parent_height))
                .gap(gap)
                .direction(Direction::Row)
                .build(),
        );
        parent.add_child(Arc::new(Mutex::new(TestWidget::new(
            fixed_width,
            parent_height,
        ))));
        parent.add_child(Arc::new(Mutex::new(FlexWidget::with_style(
            FlexStyle::builder()
                .width(Size::Grow(1.0))
                .height(Size::Fixed(parent_height))
                .direction(Direction::Row)
                .build(),
        ))));
        parent.add_child(Arc::new(Mutex::new(shrink_child)));

        let parent_ref = Arc::new(Mutex::new(parent));
        {
            let mut p = parent_ref.lock().expect("widget lock poisoned");
            p.layout(
                &ctx,
                0.0,
                0.0,
                &Direction::Row,
                parent_width,
                parent_width,
                parent_height,
                parent_height,
                0.0,
            );
        }

        let p = parent_ref.lock().expect("widget lock poisoned");
        let grow_layout = p.children[1]
            .lock()
            .expect("widget lock poisoned")
            .get_layout_rect()
            .cloned()
            .expect("grow child laid out");

        // Available pool = parent - fixed - 2 gaps. Shrink takes 90 of it.
        let expected_grow_width = parent_width - fixed_width - shrink_inner_width - 2.0 * gap;
        assert!(
            (grow_layout.width - expected_grow_width).abs() < 0.001,
            "Grow child width should be {} but was {}",
            expected_grow_width,
            grow_layout.width,
        );
    }

    // ---------------------------------------------------------------------
    // Key-based reconciliation tests
    // ---------------------------------------------------------------------

    /// Returns the Arc identities of `parent`'s children as raw pointer
    /// values, used to prove a child widget survived reconciliation.
    fn child_identities(parent: &FlexWidget) -> Vec<*const ()> {
        parent
            .children
            .iter()
            .map(|c| Arc::as_ptr(c) as *const ())
            .collect()
    }

    fn keyed_flex(key: &str) -> WidgetRef {
        let mut w = FlexWidget::new();
        w.key = Some(key.to_string());
        Arc::new(Mutex::new(w))
    }

    fn unkeyed_flex() -> WidgetRef {
        Arc::new(Mutex::new(FlexWidget::new()))
    }

    fn wrap_parent(children: Vec<WidgetRef>) -> FlexWidget {
        let mut parent = FlexWidget::new();
        for c in children {
            parent.add_child(c);
        }
        parent
    }

    #[test]
    fn keyed_children_survive_reordering() {
        // Old: [A, B, C] — new: [C, A, B]. All three should be reused,
        // just rearranged.
        let a = keyed_flex("a");
        let b = keyed_flex("b");
        let c = keyed_flex("c");
        let mut parent = wrap_parent(vec![a.clone(), b.clone(), c.clone()]);

        let a_id = Arc::as_ptr(&a) as *const ();
        let b_id = Arc::as_ptr(&b) as *const ();
        let c_id = Arc::as_ptr(&c) as *const ();

        let mut new_children = vec![keyed_flex("c"), keyed_flex("a"), keyed_flex("b")];
        parent.reconcile_children(&mut new_children);

        let ids = child_identities(&parent);
        assert_eq!(ids, vec![c_id, a_id, b_id]);
    }

    #[test]
    fn keyed_children_survive_middle_removal() {
        // Old: [A, B, C] — new: [A, C]. B is dropped; A and C survive.
        let a = keyed_flex("a");
        let b = keyed_flex("b");
        let c = keyed_flex("c");
        let mut parent = wrap_parent(vec![a.clone(), b.clone(), c.clone()]);

        let a_id = Arc::as_ptr(&a) as *const ();
        let c_id = Arc::as_ptr(&c) as *const ();

        let mut new_children = vec![keyed_flex("a"), keyed_flex("c")];
        parent.reconcile_children(&mut new_children);

        let ids = child_identities(&parent);
        assert_eq!(ids, vec![a_id, c_id]);
        assert_eq!(parent.children.len(), 2);
    }

    #[test]
    fn unkeyed_children_match_by_position() {
        // Old: [X, Y] unkeyed — new: [X', Y']. Both update in place.
        let x = unkeyed_flex();
        let y = unkeyed_flex();
        let mut parent = wrap_parent(vec![x.clone(), y.clone()]);

        let x_id = Arc::as_ptr(&x) as *const ();
        let y_id = Arc::as_ptr(&y) as *const ();

        let mut new_children = vec![unkeyed_flex(), unkeyed_flex()];
        parent.reconcile_children(&mut new_children);

        let ids = child_identities(&parent);
        assert_eq!(ids, vec![x_id, y_id]);
    }

    #[test]
    fn new_keyed_child_is_inserted_fresh() {
        // Old: [A] — new: [A, B]. A survives by key, B is brand new.
        let a = keyed_flex("a");
        let mut parent = wrap_parent(vec![a.clone()]);

        let a_id = Arc::as_ptr(&a) as *const ();
        let fresh_b = keyed_flex("b");
        let b_id = Arc::as_ptr(&fresh_b) as *const ();

        let mut new_children = vec![keyed_flex("a"), fresh_b];
        parent.reconcile_children(&mut new_children);

        let ids = child_identities(&parent);
        assert_eq!(ids[0], a_id);
        assert_eq!(ids[1], b_id);
    }

    #[test]
    fn mixed_keyed_and_unkeyed_children() {
        // Old: [K1, U, K2] — new: [K2, U, K1]. Keyed children match by
        // key; the single unkeyed child matches by position within the
        // unkeyed subsequence (there's only one, so it survives).
        let k1 = keyed_flex("k1");
        let u = unkeyed_flex();
        let k2 = keyed_flex("k2");
        let mut parent = wrap_parent(vec![k1.clone(), u.clone(), k2.clone()]);

        let k1_id = Arc::as_ptr(&k1) as *const ();
        let u_id = Arc::as_ptr(&u) as *const ();
        let k2_id = Arc::as_ptr(&k2) as *const ();

        let mut new_children = vec![keyed_flex("k2"), unkeyed_flex(), keyed_flex("k1")];
        parent.reconcile_children(&mut new_children);

        let ids = child_identities(&parent);
        assert_eq!(ids, vec![k2_id, u_id, k1_id]);
    }

    #[test]
    fn keyed_child_arc_survives_reorder() {
        // When reordering keyed children, the Arc identities survive —
        // the in-place update happens on the original widget, preserving
        // any state that wasn't overwritten by update().
        let a = keyed_flex("a");
        let b = keyed_flex("b");
        let a_id = Arc::as_ptr(&a) as *const ();
        let mut parent = wrap_parent(vec![a.clone(), b.clone()]);

        let mut new_children = vec![keyed_flex("b"), keyed_flex("a")];
        parent.reconcile_children(&mut new_children);

        // The original `a` Arc is at position 1 after reordering.
        let ids = child_identities(&parent);
        assert_eq!(
            ids[1], a_id,
            "reconciliation must reuse the same widget for matching keys"
        );
    }

    // ---------------------------------------------------------------------
    // Entry / exit lifecycle tests
    // ---------------------------------------------------------------------

    fn make_transition_style(bg: Color) -> FlexStyle {
        let mut s = FlexStyle::default();
        s.background_color = Some(bg);
        s.transitions.background_color = Some(crate::widget::animation::TransitionConfig::DEFAULT);
        s
    }

    #[test]
    fn initial_style_seeds_starting_state() {
        // initial_style: opaque black. declared_style: opaque white.
        // apply_entry_transition puts `self.style` at the initial and
        // kicks off a spring toward the declared target.
        let mut w = FlexWidget::new();
        w.declared_style = make_transition_style(Color::new(255, 255, 255, 255));
        w.initial_style = Some(make_transition_style(Color::new(0, 0, 0, 255)));
        w.style = w.declared_style.clone();

        w.apply_entry_transition();

        assert_eq!(
            w.style.background_color,
            Some(Color::new(0, 0, 0, 255)),
            "style should be seeded from initial_style"
        );
        assert!(
            w.animations.background_color.is_some(),
            "entry animation should be armed"
        );
    }

    #[test]
    fn paint_only_background_change_marks_needs_repaint_not_relayout() {
        // Regression: a widget whose ONLY change is background_color — e.g. a
        // selected chip flipping to its accent highlight, label text unchanged —
        // must report needs_repaint so it actually redraws. `layout_equal`
        // ignores paint fields, so before `paint_equal` this returned
        // needs_repaint:false and the new colour rendered stale until an
        // unrelated layout/text change forced a repaint (the Lorekeeper
        // map-editor mode-switcher "button doesn't highlight" bug).
        let mut old = FlexWidget::new();
        old.declared_style.background_color = Some(Color::new(24, 27, 33, 255));
        old.style = old.declared_style.clone();

        let mut new = FlexWidget::new();
        new.declared_style.background_color = Some(Color::new(70, 100, 150, 255)); // accent
        let new_ref: WidgetRef = Arc::new(Mutex::new(new));

        let result = old.update(new_ref);
        assert!(
            result.needs_repaint,
            "a paint-only background_color change must mark needs_repaint"
        );
        assert!(
            !result.needs_layout,
            "a paint-only change must not force a relayout"
        );
        assert_eq!(
            old.style.background_color,
            Some(Color::new(70, 100, 150, 255)),
            "the new colour is adopted onto the rendered style"
        );
    }

    #[test]
    fn non_transitioned_layout_change_snaps_while_springs_fly() {
        // Regression: a widget with a declared transition (say, on
        // background_color) receives an update that ALSO changes a
        // layout field with no transition of its own (width). The width
        // must snap onto the rendered style during this update — this is
        // the frame whose UpdateResult carries needs_layout — while the
        // color spring keeps interpolating. Holding the entire old
        // rendered style until the next tick left the box at its stale
        // size forever: the tick that finally wrote the new width reports
        // needs_layout only for spring-driven properties (the Ashworth
        // Manor title screen's shrunken invitation card, when the
        // fullscreen resize landed mid entrance animation).
        let mut old = FlexWidget::new();
        old.declared_style = make_transition_style(Color::new(0, 0, 0, 255));
        old.declared_style.width = Size::Fixed(100.0);
        old.style = old.declared_style.clone();

        let mut new = FlexWidget::new();
        new.declared_style = make_transition_style(Color::new(255, 255, 255, 255));
        new.declared_style.width = Size::Fixed(500.0);
        let new_ref: WidgetRef = Arc::new(Mutex::new(new));

        let result = old.update(new_ref);
        assert!(result.needs_layout, "a width change must mark needs_layout");
        assert!(
            !old.animations.is_empty(),
            "the background_color spring must be in flight"
        );
        assert_eq!(
            old.style.width,
            Size::Fixed(500.0),
            "the non-transitioned width must snap to the new target immediately"
        );
        assert_eq!(
            old.style.background_color,
            Some(Color::new(0, 0, 0, 255)),
            "the transitioned colour still renders its spring's current value"
        );
    }

    #[test]
    fn restart_entry_animation_reseeds_style_from_initial() {
        // After a widget has settled at declared_style, restart_entry_animation
        // should push self.style back to initial_style and arm springs to
        // re-target declared.
        let mut w = FlexWidget::new();
        w.declared_style = make_transition_style(Color::new(255, 255, 255, 255));
        w.initial_style = Some(make_transition_style(Color::new(0, 0, 0, 255)));
        w.style = w.declared_style.clone();
        // Simulate "settled at declared": no animations, style == declared.
        assert!(w.animations.is_empty());

        w.restart_entry_animation();

        assert_eq!(
            w.style.background_color,
            Some(Color::new(0, 0, 0, 255)),
            "restart should reseed style from initial_style"
        );
        assert!(
            w.animations.background_color.is_some(),
            "restart should arm the spring back toward declared"
        );
    }

    #[test]
    fn restart_entry_animation_no_op_without_initial_style() {
        // A widget without `initial_style` has no entry to restart;
        // restart_entry_animation should leave its style untouched and
        // not invent springs out of nowhere.
        let mut w = FlexWidget::new();
        w.declared_style = make_transition_style(Color::new(200, 200, 200, 255));
        w.style = w.declared_style.clone();
        let before = w.style.background_color;

        w.restart_entry_animation();

        assert_eq!(w.style.background_color, before);
        assert!(
            w.animations.is_empty(),
            "no springs should be armed when there's no initial_style"
        );
    }

    #[test]
    fn restart_entry_animation_cascades_to_children() {
        // Calling restart on a parent should drive every descendant's
        // initial-style reset so the whole subtree re-plays entry
        // together — this is what makes a route swap feel coherent.
        let child: WidgetRef = {
            let mut c = FlexWidget::new();
            c.declared_style = make_transition_style(Color::new(255, 255, 255, 255));
            c.initial_style = Some(make_transition_style(Color::new(0, 0, 0, 255)));
            c.style = c.declared_style.clone();
            Arc::new(Mutex::new(c))
        };
        let mut parent = FlexWidget::new();
        parent.declared_style = make_transition_style(Color::new(100, 100, 100, 255));
        parent.initial_style = Some(make_transition_style(Color::new(50, 50, 50, 255)));
        parent.style = parent.declared_style.clone();
        parent.children = vec![child.clone()];

        parent.restart_entry_animation();

        // Parent reseeded.
        assert_eq!(
            parent.style.background_color,
            Some(Color::new(50, 50, 50, 255)),
        );
        // Child reseeded.
        let g = child.lock().expect("widget lock poisoned");
        let c = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
        assert_eq!(
            c.style.background_color,
            Some(Color::new(0, 0, 0, 255)),
            "child should be reseeded by the cascade"
        );
    }

    #[test]
    fn restart_entry_animation_clears_mid_exit_spring_values() {
        // Regression: restart called on a widget mid-exit. Without
        // clearing existing springs, `retarget` would keep the spring's
        // current value at the mid-exit position and the next tick
        // would paint there instead of `initial`.
        let mut w = FlexWidget::new();
        w.declared_style = make_transition_style(Color::new(255, 255, 255, 255));
        w.initial_style = Some(make_transition_style(Color::new(0, 0, 0, 255)));
        w.exit_style = Some(make_transition_style(Color::new(50, 50, 50, 0)));
        w.style = w.declared_style.clone();

        // Drive several frames of exit so the spring picks up a
        // mid-flight current value distinct from both initial and declared.
        assert!(w.begin_exit());
        for _ in 0..10 {
            w.tick_own_animations(1.0 / 60.0);
        }
        // Sanity: bg is somewhere between declared and exit (not initial).
        let mid = w.style.background_color.expect("bg present");
        assert!(
            mid.r < 255 && mid.r > 50,
            "test setup: spring should be mid-flight, got r={}",
            mid.r
        );

        w.restart_entry_animation();

        // After restart, style snaps to initial.
        assert_eq!(
            w.style.background_color,
            Some(Color::new(0, 0, 0, 255)),
            "restart must seed style at initial"
        );
        // After ONE tick, the rendered style must still be near initial
        // (the spring should be moving from initial toward declared, not
        // jumping back to the mid-exit value it had before restart).
        w.tick_own_animations(1.0 / 60.0);
        let after = w.style.background_color.expect("bg present");
        assert!(
            after.r < 50,
            "after one tick, rendered red should still be near initial (0); \
             got r={} (would indicate spring carried its mid-exit value)",
            after.r
        );
    }

    #[test]
    fn restart_entry_animation_clears_lingering_exit_state() {
        // If the orchestrator restarts entry on a tree that was mid-exit
        // (rare but possible during a rapid revert), the widget must not
        // keep `exiting = true` — that would leave reconcile treating it
        // as a ghost and the spring targeting exit_style.
        let mut w = FlexWidget::new();
        w.declared_style = make_transition_style(Color::new(255, 255, 255, 255));
        w.initial_style = Some(make_transition_style(Color::new(0, 0, 0, 255)));
        w.exit_style = Some(make_transition_style(Color::new(128, 128, 128, 0)));
        w.style = w.declared_style.clone();
        // Force into exiting state.
        assert!(w.begin_exit(), "widget should accept begin_exit");
        assert!(w.exiting);

        w.restart_entry_animation();

        assert!(!w.exiting, "restart must drop the exiting flag");
        assert_eq!(
            w.style.background_color,
            Some(Color::new(0, 0, 0, 255)),
            "restart should reseed style from initial regardless of prior exit state"
        );
    }

    #[test]
    fn exit_style_drives_disappearing_child() {
        // Parent has one keyed child `a` with an exit_style. When `a`
        // disappears from new_children, it should become a ghost (still
        // in parent.children, with exiting=true) rather than being
        // dropped immediately.
        let a = {
            let mut w = FlexWidget::new();
            w.key = Some("a".to_string());
            w.declared_style = make_transition_style(Color::new(255, 255, 255, 255));
            w.exit_style = Some(make_transition_style(Color::new(0, 0, 0, 0)));
            w.style = w.declared_style.clone();
            Arc::new(Mutex::new(w))
        };
        let a_id = Arc::as_ptr(&a) as *const ();
        let mut parent = wrap_parent(vec![a.clone()]);

        let mut new_children: Vec<WidgetRef> = vec![];
        parent.reconcile_children(&mut new_children);

        assert_eq!(
            parent.children.len(),
            1,
            "child with exit_style should become a ghost, not be dropped"
        );
        assert_eq!(
            Arc::as_ptr(&parent.children[0]) as *const (),
            a_id,
            "ghost must be the same Arc as the original child"
        );
        let g = parent.children[0].lock().expect("widget lock poisoned");
        let flex = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
        assert!(flex.exiting, "ghost should be marked exiting");
        assert!(
            flex.animations.background_color.is_some(),
            "exit animation should be running"
        );
    }

    #[test]
    fn child_without_exit_style_is_dropped_immediately() {
        // Without an exit_style, a disappearing child is removed right
        // away — same as pre-ghost behavior.
        let a = keyed_flex("a");
        let mut parent = wrap_parent(vec![a.clone()]);

        let mut new_children: Vec<WidgetRef> = vec![];
        parent.reconcile_children(&mut new_children);

        assert_eq!(parent.children.len(), 0);
    }

    #[test]
    fn reinsertion_cancels_exit() {
        // Ghost `a` is mid-exit. A new `a` key appears in new_children:
        // the ghost un-exits and gets matched normally, transitioning
        // back toward declared_style.
        let a = {
            let mut w = FlexWidget::new();
            w.key = Some("a".to_string());
            w.declared_style = make_transition_style(Color::new(255, 255, 255, 255));
            w.exit_style = Some(make_transition_style(Color::new(0, 0, 0, 0)));
            w.style = w.declared_style.clone();
            Arc::new(Mutex::new(w))
        };
        let mut parent = wrap_parent(vec![a.clone()]);

        // First reconcile: remove `a` → becomes a ghost.
        let mut empty: Vec<WidgetRef> = vec![];
        parent.reconcile_children(&mut empty);
        assert!(parent.children[0]
            .lock()
            .expect("widget lock poisoned")
            .is_exiting());

        // Second reconcile: `a` reappears with same key.
        let reinserted: WidgetRef = {
            let mut w = FlexWidget::new();
            w.key = Some("a".to_string());
            w.declared_style = make_transition_style(Color::new(255, 255, 255, 255));
            w.exit_style = Some(make_transition_style(Color::new(0, 0, 0, 0)));
            w.style = w.declared_style.clone();
            Arc::new(Mutex::new(w))
        };
        let mut new_children: Vec<WidgetRef> = vec![reinserted];
        parent.reconcile_children(&mut new_children);

        let g = parent.children[0].lock().expect("widget lock poisoned");
        let flex = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
        assert!(!flex.exiting, "reinsertion should clear the exiting flag");
    }

    #[test]
    fn settled_ghost_is_drained_during_tick() {
        // After a ghost's exit springs settle, the next tick_animations
        // call drops it from the parent's children list.
        let a = {
            let mut w = FlexWidget::new();
            w.key = Some("a".to_string());
            w.declared_style = make_transition_style(Color::new(255, 255, 255, 255));
            w.exit_style = Some(make_transition_style(Color::new(0, 0, 0, 0)));
            w.style = w.declared_style.clone();
            Arc::new(Mutex::new(w))
        };
        let mut parent = wrap_parent(vec![a.clone()]);

        // Start exit.
        let mut empty: Vec<WidgetRef> = vec![];
        parent.reconcile_children(&mut empty);
        assert_eq!(parent.children.len(), 1);

        // Tick with enough total time for the spring to settle.
        for _ in 0..120 {
            let mut ctx = crate::widget::event::TickContext::new(1.0 / 60.0);
            parent.tick_animations(&mut ctx);
        }

        assert_eq!(
            parent.children.len(),
            0,
            "settled ghost should have been drained"
        );
    }

    #[test]
    fn drain_records_owned_path_prefix_in_tick_context() {
        // Phase 3 M0 wiring: when a ghost child with an
        // owned_path_prefix drains during tick_animations, the
        // prefix is pushed into the TickContext for the runtime
        // to flush owned hooks at the next render boundary.
        let a = {
            let mut w = FlexWidget::new();
            w.key = Some("a".to_string());
            w.declared_style = make_transition_style(Color::new(255, 255, 255, 255));
            w.exit_style = Some(make_transition_style(Color::new(0, 0, 0, 0)));
            w.style = w.declared_style.clone();
            w.owned_path_prefix = "fn@7".to_string();
            Arc::new(Mutex::new(w))
        };
        let mut parent = wrap_parent(vec![a]);

        let mut empty: Vec<WidgetRef> = vec![];
        parent.reconcile_children(&mut empty);

        let mut ctx = crate::widget::event::TickContext::new(1.0 / 60.0);
        for _ in 0..120 {
            parent.tick_animations(&mut ctx);
        }

        assert_eq!(parent.children.len(), 0);
        assert!(
            ctx.drained_path_prefixes.iter().any(|p| p == "fn@7"),
            "drained owned_path_prefix should be recorded; got {:?}",
            ctx.drained_path_prefixes
        );
    }

    #[test]
    fn cancel_exit_records_owned_path_prefix_in_update_result() {
        // Phase 3 M0 wiring: when reconcile_children cancels an
        // in-flight exit (re-mount during exit animation), the
        // child's owned_path_prefix is pushed into the
        // UpdateResult for the runtime to drop matching pending
        // unmount entries.
        let a = {
            let mut w = FlexWidget::new();
            w.key = Some("a".to_string());
            w.declared_style = make_transition_style(Color::new(255, 255, 255, 255));
            w.exit_style = Some(make_transition_style(Color::new(0, 0, 0, 0)));
            w.style = w.declared_style.clone();
            w.owned_path_prefix = "fn@9".to_string();
            Arc::new(Mutex::new(w))
        };
        let mut parent = wrap_parent(vec![a]);

        // Start an exit.
        let mut empty: Vec<WidgetRef> = vec![];
        parent.reconcile_children(&mut empty);
        assert_eq!(parent.children.len(), 1);

        // Re-insert the same key while the exit is in flight.
        let reinserted: WidgetRef = {
            let mut w = FlexWidget::new();
            w.key = Some("a".to_string());
            w.declared_style = make_transition_style(Color::new(255, 255, 255, 255));
            w.exit_style = Some(make_transition_style(Color::new(0, 0, 0, 0)));
            w.style = w.declared_style.clone();
            w.owned_path_prefix = "fn@9".to_string();
            Arc::new(Mutex::new(w))
        };
        let mut new_children: Vec<WidgetRef> = vec![reinserted];
        let result = parent.reconcile_children(&mut new_children);

        assert!(
            result
                .cancelled_unmount_prefixes
                .iter()
                .any(|p| p == "fn@9"),
            "cancelled exit should report the owned_path_prefix; got {:?}",
            result.cancelled_unmount_prefixes
        );
    }

    #[test]
    fn animation_state_persists_across_reorder_with_transitions() {
        // When a widget has an active background-color transition and it
        // gets reordered into a new position, the update() flows through
        // retarget() and preserves the in-flight spring (velocity +
        // current value) rather than wiping it.
        use crate::widget::animation::TransitionConfig;

        let make_with_transitions = |key: &str, bg: Color| -> WidgetRef {
            let mut w = FlexWidget::new();
            w.key = Some(key.to_string());
            let mut s = FlexStyle::default();
            s.background_color = Some(bg);
            s.transitions.background_color = Some(TransitionConfig::DEFAULT);
            w.declared_style = s.clone();
            w.style = s;
            Arc::new(Mutex::new(w))
        };

        let a = make_with_transitions("a", Color::new(0, 0, 0, 255));
        let b = make_with_transitions("b", Color::new(255, 0, 0, 255));

        // Prime widget `a` with an active spring mid-animation.
        {
            let mut guard = a.lock().expect("widget lock poisoned");
            let flex = guard.downcast_mut::<FlexWidget>().expect("FlexWidget");
            let mut springs = crate::widget::animation::ColorSprings::new(
                Color::new(0, 0, 0, 255),
                TransitionConfig::DEFAULT,
            );
            springs.set_target(Color::new(255, 255, 255, 255));
            springs.tick(1.0 / 60.0);
            flex.animations.background_color = Some(springs);
        }

        let mut parent = wrap_parent(vec![a.clone(), b.clone()]);

        // Reorder to [b, a] — new descriptors carry the same targets
        // and transitions as the originals.
        let mut new_children = vec![
            make_with_transitions("b", Color::new(255, 0, 0, 255)),
            make_with_transitions("a", Color::new(0, 0, 0, 255)),
        ];
        parent.reconcile_children(&mut new_children);

        let a_after = parent.children[1].lock().expect("widget lock poisoned");
        let flex = a_after.downcast_ref::<FlexWidget>().expect("FlexWidget");
        assert!(
            flex.animations.background_color.is_some(),
            "mid-animation spring state should survive reorder + update"
        );
    }

    // ── Stagger: group-moment cascades (`FlexStyle::stagger`) ──────────

    use crate::widget::animation::TransitionConfig;

    /// An entry/exit-capable child: opacity 0→1 on entry, →0 on exit,
    /// with `authored_delay` baked into its own transition config.
    fn stagger_child(authored_delay: f32) -> WidgetRef {
        let mut declared = FlexStyle::default();
        declared.transitions.opacity = Some(TransitionConfig {
            delay: authored_delay,
            ..TransitionConfig::DEFAULT
        });
        let mut initial = declared.clone();
        initial.opacity = Opacity(0.0);
        let mut exit = declared.clone();
        exit.opacity = Opacity(0.0);
        let mut w = FlexWidget::with_style(declared);
        w.initial_style = Some(initial);
        w.exit_style = Some(exit);
        w.apply_entry_transition();
        Arc::new(Mutex::new(w))
    }

    fn stagger_parent(stagger: StaggerConfig, children: Vec<WidgetRef>) -> FlexWidget {
        let mut style = FlexStyle::default();
        style.stagger = Some(stagger);
        let mut parent = FlexWidget::with_style(style);
        for c in children {
            parent.add_child(c);
        }
        parent
    }

    fn opacity_delay(w: &WidgetRef) -> f32 {
        let g = w.lock().expect("widget lock poisoned");
        let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
        f.animations
            .opacity
            .as_ref()
            .expect("opacity spring should be active")
            .delay
    }

    const STEPPED: StaggerConfig = StaggerConfig {
        step: 0.1,
        exit_step: 0.05,
        exit_order: StaggerOrder::Reverse,
    };

    #[test]
    fn stagger_offsets_children_by_index_and_sums_authored_delay() {
        let kids: Vec<WidgetRef> =
            vec![stagger_child(0.0), stagger_child(0.0), stagger_child(0.02)];
        let mut parent = stagger_parent(STEPPED, kids);
        parent.apply_child_stagger_offsets();

        assert_eq!(opacity_delay(&parent.children[0]), 0.0);
        assert!((opacity_delay(&parent.children[1]) - 0.1).abs() < 1e-6);
        // Authored per-child delay survives underneath the group offset.
        assert!((opacity_delay(&parent.children[2]) - 0.22).abs() < 1e-6);
    }

    #[test]
    fn restart_entry_animation_reapplies_stagger_offsets() {
        let kids: Vec<WidgetRef> = vec![stagger_child(0.0), stagger_child(0.0)];
        let mut parent = stagger_parent(STEPPED, kids);
        parent.apply_child_stagger_offsets();

        // Settle everything, then restart — the cascade must re-arm.
        for _ in 0..600 {
            for c in parent.children.clone() {
                let mut g = c.lock().expect("widget lock poisoned");
                let f = g.downcast_mut::<FlexWidget>().expect("FlexWidget");
                f.tick_own_animations(1.0 / 60.0);
            }
        }
        parent.restart_entry_animation();

        assert_eq!(opacity_delay(&parent.children[0]), 0.0);
        assert!((opacity_delay(&parent.children[1]) - 0.1).abs() < 1e-6);
    }

    #[test]
    fn exit_cascade_staggers_in_reverse_by_default() {
        let kids: Vec<WidgetRef> = vec![stagger_child(0.0), stagger_child(0.0), stagger_child(0.0)];
        let mut parent = stagger_parent(STEPPED, kids);
        // Settle entries so begin_exit creates fresh exit springs.
        for c in parent.children.clone() {
            let mut g = c.lock().expect("widget lock poisoned");
            let f = g.downcast_mut::<FlexWidget>().expect("FlexWidget");
            for _ in 0..600 {
                f.tick_own_animations(1.0 / 60.0);
            }
        }

        assert!(parent.begin_exit(), "cascade should start");
        // Reverse order: the LAST child leaves first.
        assert!((opacity_delay(&parent.children[0]) - 0.1).abs() < 1e-6);
        assert!((opacity_delay(&parent.children[1]) - 0.05).abs() < 1e-6);
        assert_eq!(opacity_delay(&parent.children[2]), 0.0);
    }

    #[test]
    fn exit_cascade_respects_forward_order_and_zero_exit_step() {
        let forward = StaggerConfig {
            step: 0.1,
            exit_step: 0.05,
            exit_order: StaggerOrder::Forward,
        };
        let kids: Vec<WidgetRef> = vec![stagger_child(0.0), stagger_child(0.0)];
        let mut parent = stagger_parent(forward, kids);
        for c in parent.children.clone() {
            let mut g = c.lock().expect("widget lock poisoned");
            let f = g.downcast_mut::<FlexWidget>().expect("FlexWidget");
            for _ in 0..600 {
                f.tick_own_animations(1.0 / 60.0);
            }
        }
        assert!(parent.begin_exit());
        assert_eq!(opacity_delay(&parent.children[0]), 0.0);
        assert!((opacity_delay(&parent.children[1]) - 0.05).abs() < 1e-6);

        // exit_step 0 = entry-only stagger: everything leaves together.
        let entry_only = StaggerConfig {
            step: 0.1,
            exit_step: 0.0,
            exit_order: StaggerOrder::Reverse,
        };
        let kids: Vec<WidgetRef> = vec![stagger_child(0.0), stagger_child(0.0)];
        let mut parent = stagger_parent(entry_only, kids);
        for c in parent.children.clone() {
            let mut g = c.lock().expect("widget lock poisoned");
            let f = g.downcast_mut::<FlexWidget>().expect("FlexWidget");
            for _ in 0..600 {
                f.tick_own_animations(1.0 / 60.0);
            }
        }
        assert!(parent.begin_exit());
        assert_eq!(opacity_delay(&parent.children[0]), 0.0);
        assert_eq!(opacity_delay(&parent.children[1]), 0.0);
    }

    #[test]
    fn reconcile_strips_inherited_offset_from_individually_mounted_child() {
        // Live parent with no children; the "new tree" hands it a child
        // whose ancestors injected a cascade offset at construction.
        // The child mounts alone, so the offset must be stripped back to
        // its authored delay.
        let mut live = FlexWidget::new();
        let newcomer = stagger_child(0.02);
        {
            let mut g = newcomer.lock().expect("widget lock poisoned");
            g.add_group_delay(0.27); // simulated new-tree ancestor injection
        }
        let mut new_children = vec![newcomer];
        live.reconcile_children(&mut new_children);

        assert!((opacity_delay(&live.children[0]) - 0.02).abs() < 1e-6);
    }

    #[test]
    fn strip_preserves_cascades_internal_to_the_new_subtree() {
        // A whole staggered list mounting as one new unit keeps its own
        // internal cascade; only the offset inherited from ABOVE the
        // unit is stripped.
        let kids: Vec<WidgetRef> = vec![stagger_child(0.0), stagger_child(0.0)];
        let mut container = stagger_parent(STEPPED, kids);
        container.apply_child_stagger_offsets();
        let container_ref: WidgetRef = Arc::new(Mutex::new(container));
        {
            let mut g = container_ref.lock().expect("widget lock poisoned");
            g.add_group_delay(0.3); // simulated new-tree ancestor injection
        }

        let mut live = FlexWidget::new();
        let mut new_children = vec![container_ref];
        live.reconcile_children(&mut new_children);

        let g = live.children[0].lock().expect("widget lock poisoned");
        let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
        assert_eq!(opacity_delay(&f.children[0]), 0.0, "inherited 0.3 stripped");
        assert!(
            (opacity_delay(&f.children[1]) - 0.1).abs() < 1e-6,
            "internal cascade survives the strip"
        );
    }

    #[test]
    fn stagger_parses_from_source_and_sums_through_nesting() {
        // End-to-end through the parser + builder: a staggered root with a
        // plain leaf and a nested staggered list. Offsets must sum down the
        // tree: leaf 0.0; list children 0.1 + j*0.1.
        let src = r##"
            let item = fn () {
              Flex {
                initial: { opacity: 0 },
                style: {
                  width: "grow", height: "shrink",
                  transition: { opacity: { stiffness: 170, damping: 26 } },
                },
                children: [],
              }
            };
            let main = fn () {
              Flex {
                style: { direction: "column", stagger: { step: 0.1 } },
                children: [
                  item(),
                  Flex {
                    style: { direction: "column", stagger: { step: 0.1 } },
                    children: [item(), item()],
                  },
                ],
              }
            };
        "##;
        let o = crate::Ogham::from_source(src, crate::runtime::config::RuntimeConfig::default())
            .expect("from_source");
        let root = o.get_ui().root.clone();
        let g = root.lock().expect("widget lock poisoned");
        let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget root");

        assert_eq!(opacity_delay(&f.children[0]), 0.0);
        let list = f.children[1].lock().expect("widget lock poisoned");
        let list = list.downcast_ref::<FlexWidget>().expect("FlexWidget list");
        assert!((opacity_delay(&list.children[0]) - 0.1).abs() < 1e-6);
        assert!((opacity_delay(&list.children[1]) - 0.2).abs() < 1e-6);
    }

    /// A scroll column offers its children an unbounded main axis: a
    /// `height: shrink` section inside it measures its natural height —
    /// its rows keep stacking past the viewport, and the next section
    /// starts below ALL of them. The old shrink clamp capped the section
    /// at the viewport's height while its children laid out past it, so
    /// every row past the fold painted over the following section
    /// (regency's almanac bug, 2026-07-08).
    #[test]
    fn scroll_column_children_measure_unclamped() {
        let src = r##"
            let row = fn () {
              Flex { style: { width: "grow", height: 40 } }
            };
            let main = fn () {
              Flex {
                style: { width: 300, height: 100, direction: "column", overflow: "scroll" },
                children: [
                  Flex {
                    style: { width: "grow", height: "shrink", direction: "column" },
                    children: [row(), row(), row(), row(), row()],
                  },
                  Flex {
                    style: { width: "grow", height: "shrink", direction: "column" },
                    children: [row(), row(), row()],
                  },
                ],
              }
            };
        "##;
        let mut o =
            crate::Ogham::from_source(src, crate::runtime::config::RuntimeConfig::default())
                .expect("from_source");
        o.get_ui_mut().layout(300.0, 100.0);
        let root = o.get_ui().root.clone();
        let g = root.lock().expect("widget lock poisoned");
        let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget root");
        assert_eq!(f.style.overflow, Overflow::Scroll);

        let first = f.children[0].lock().expect("widget lock poisoned");
        let first_rect = first
            .get_layout_rect()
            .expect("first section laid out")
            .clone();
        assert_eq!(
            first_rect.height, 200.0,
            "a shrink section in a scroll column keeps its natural height"
        );
        let second = f.children[1].lock().expect("widget lock poisoned");
        let second_rect = second
            .get_layout_rect()
            .expect("second section laid out")
            .clone();
        assert_eq!(
            second_rect.y,
            first_rect.y + first_rect.height,
            "the next section starts below the whole first section"
        );
        assert_eq!(second_rect.height, 120.0);
    }

    // ---- Exiting widgets are hit-test-invisible (PRESENCE_POP.md §6) --

    /// A clickable flex pinned at `rect`: mouse_down listener recording
    /// into `fired`, plus the layout rect contains_point needs. Styled
    /// with a bg transition so begin_exit can arm a spring.
    fn clickable_at(rect: Rect, fired: &Arc<Mutex<bool>>) -> FlexWidget {
        let mut w = FlexWidget::new();
        w.layout = Some(rect);
        w.declared_style = make_transition_style(Color::new(255, 255, 255, 255));
        w.style = w.declared_style.clone();
        let flag = fired.clone();
        w.event_listeners.insert(
            "mouse_down".to_string(),
            vec![Box::new(move |_e: &Event| {
                *flag.lock().unwrap() = true;
            })],
        );
        w
    }

    #[test]
    fn exiting_widget_ignores_pointer_events() {
        let fired = Arc::new(Mutex::new(false));
        let mut w = clickable_at(Rect::new(0.0, 0.0, 100.0, 100.0), &fired);
        w.exit_style = Some(make_transition_style(Color::new(0, 0, 0, 0)));
        assert!(w.begin_exit());

        let event = Event::with_point("mouse_down".to_string(), Point::new(50.0, 50.0));
        let mut ctx = EventContext::new();
        let self_ref: WidgetRef = Arc::new(Mutex::new(FlexWidget::new()));
        let consumed = w.handle_event(&event, &mut ctx, &self_ref);

        assert!(!consumed, "an exiting widget must not consume presses");
        assert!(
            !*fired.lock().unwrap(),
            "listener on exiting widget must not fire"
        );
    }

    #[test]
    fn press_falls_through_ghost_to_widget_beneath() {
        // Two overlapping siblings: an exiting ghost above a live
        // button. The parent's child walk must skip the ghost so the
        // press lands on the live widget.
        let ghost_fired = Arc::new(Mutex::new(false));
        let live_fired = Arc::new(Mutex::new(false));

        let mut ghost = clickable_at(Rect::new(0.0, 0.0, 100.0, 100.0), &ghost_fired);
        ghost.exit_style = Some(make_transition_style(Color::new(0, 0, 0, 0)));
        assert!(ghost.begin_exit());
        let live = clickable_at(Rect::new(0.0, 0.0, 100.0, 100.0), &live_fired);

        let mut parent = FlexWidget::new();
        parent.layout = Some(Rect::new(0.0, 0.0, 200.0, 200.0));
        parent.children = vec![
            Arc::new(Mutex::new(ghost)) as WidgetRef,
            Arc::new(Mutex::new(live)) as WidgetRef,
        ];

        let event = Event::with_point("mouse_down".to_string(), Point::new(50.0, 50.0));
        let mut ctx = EventContext::new();
        let self_ref: WidgetRef = Arc::new(Mutex::new(FlexWidget::new()));
        let consumed = parent.handle_event(&event, &mut ctx, &self_ref);

        assert!(consumed, "the live sibling should consume the press");
        assert!(
            !*ghost_fired.lock().unwrap(),
            "ghost must not see the press"
        );
        assert!(
            *live_fired.lock().unwrap(),
            "live widget beneath must get it"
        );
    }

    #[test]
    fn exiting_widget_does_not_block_point() {
        let mut w = FlexWidget::new();
        w.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
        w.declared_style = make_transition_style(Color::new(255, 255, 255, 255));
        w.style = w.declared_style.clone();
        w.block_interactions = true;
        assert!(w.blocks_point(&Point::new(50.0, 50.0)));

        w.exit_style = Some(make_transition_style(Color::new(0, 0, 0, 0)));
        assert!(w.begin_exit());
        assert!(
            !w.blocks_point(&Point::new(50.0, 50.0)),
            "an exiting widget must not occlude the world beneath it"
        );
    }
}
