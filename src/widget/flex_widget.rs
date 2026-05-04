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
    /// Debug-only: consecutive frames where this widget reported
    /// `layout_effects && still_moving` from `tick_own_animations`.
    /// Used to identify a stuck spring by emitting a single warning
    /// after a threshold of frames.
    #[cfg(debug_assertions)]
    pub layout_anim_frames: u32,
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
            #[cfg(debug_assertions)]
            layout_anim_frames: 0,
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
            #[cfg(debug_assertions)]
            layout_anim_frames: 0,
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
                return TickResult { needs_repaint: true, ..TickResult::NONE };
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
                         border={} padding={} margin={} corner_radius={} gap={} text_size={} \
                         ({} frames)",
                        self.key,
                        self.exiting,
                        self.hovered,
                        self.animations.border.is_some(),
                        self.animations.padding.is_some(),
                        self.animations.margin.is_some(),
                        self.animations.corner_radius.is_some(),
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
        };
        // Pre-pass: if a new child's key matches a currently-exiting
        // ghost, cancel the exit so the ghost re-enters normal matching.
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
                g.cancel_exit();
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
                    let updated_in_place = {
                        let mut child = self.children[idx]
                            .lock()
                            .expect("widget lock poisoned");
                        child.update(new_child.clone())
                    };
                    agg.needs_layout |= updated_in_place.needs_layout;
                    agg.needs_repaint |= updated_in_place.needs_repaint;
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
                        let can_ghost = {
                            let mut g = self.children[idx]
                                .lock()
                                .expect("widget lock poisoned");
                            g.begin_exit()
                        };
                        if can_ghost {
                            next.push(self.children[idx].clone());
                        }
                        next.push(new_child.clone());
                    }
                }
            } else {
                // Brand-new keyed child or a fresh tail entry — structural
                // change, layout has to re-flow.
                agg.needs_layout = true;
                agg.needs_repaint = true;
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
            let (is_exiting, begin_ok) = {
                let mut g = old_child.lock().expect("widget lock poisoned");
                let already = g.is_exiting();
                let started = if already { true } else { g.begin_exit() };
                (already, started)
            };
            if !begin_ok {
                // No exit capability and not already exiting — drop.
                // Dropping a child shifts siblings, so layout needs to re-flow.
                agg.needs_layout = true;
                agg.needs_repaint = true;
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
    /// reconcile pass from above.
    fn drain_exited_children(&mut self) {
        self.children.retain(|child| {
            let g = child.lock().expect("widget lock poisoned");
            !g.is_exit_complete()
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

            let new_target = self.target_style().clone();

            if new_target.transitions.any_enabled() {
                self.animations.retarget(&old_rendered, &new_target);
                if self.animations.is_empty() {
                    // No property actually changed — snap to target.
                    self.style = new_target;
                }
                // Otherwise leave self.style as old_rendered; the next
                // tick will interpolate from there toward the new target.
            } else {
                // No transitions declared — snap immediately.
                self.animations = AnimationState::default();
                self.style = new_target;
            }

            let children_result = self.reconcile_children(&mut new_flex_widget.children);

            UpdateResult {
                absorbed: true,
                needs_layout: own_layout_changed || children_result.needs_layout,
                needs_repaint: own_layout_changed || children_result.needs_repaint,
            }
        } else {
            UpdateResult::REPLACE
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
        let width = match self.style.width {
            Size::Fixed(w) => w,
            Size::Shrink => {
                let _occupied_width: f32 = self.get_children_fixed_width();
                let occupied_height = self.get_children_fixed_height();
                let children_basis = self.get_children_basis();
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
                        0.0
                    } else {
                        parent_available_height - occupied_height
                    };
                    child.get_dimensions(
                        ctx,
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
                if max_width > 0.0 { unclamped.min(max_width) } else { unclamped }
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

        let height = match self.style.height {
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
                let get_dimensions = |child: &WidgetRef| {
                    let child = child.lock().expect("widget lock poisoned");
                    if child.is_absolute_positioned() {
                        return (0.0, 0.0);
                    }
                    let child_available_width = if self.style.direction.is_row() {
                        0.0
                    } else {
                        self_content_width
                    };
                    let child_available_height = if !self.style.direction.is_row() {
                        0.0
                    } else {
                        parent_available_height
                    };
                    child.get_dimensions(
                        ctx,
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
                        let projected = if is_first { cursor + cw } else { cursor + self.style.gap + cw };
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
                        if max_height > 0.0 { unclamped.min(max_height) } else { unclamped },
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
                if max_height > 0.0 { unclamped.min(max_height) } else { unclamped }
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
            // For click events, first check if this widget contains the point
            if self.contains_point(point) {
                // Handle scroll events for scrollable containers
                if let Some((_, dy)) = event.scroll_delta {
                    if self.style.overflow == Overflow::Scroll {
                        let max_scroll = (self.content_height - self.viewport_height).max(0.0);
                        self.scroll_y_target =
                            (self.scroll_y_target - dy).clamp(0.0, max_scroll);
                        return true;
                    }
                }

                // Build an event whose point is in this widget's own content
                // coordinate space, so children (which store parent-relative
                // rects) can hit-test without knowing the ancestor chain.
                let origin = self.layout.as_ref().map(|r| (r.x, r.y)).unwrap_or((0.0, 0.0));
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
                available_height,
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
                    (available_width, available_height)
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
                (available_width, available_height)
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
                if child.is_absolute_positioned() { continue; }
                if let Some(r) = child.get_layout_rect() {
                    max_bottom = max_bottom.max(r.y + r.height);
                }
            }
            self.content_height = max_bottom - content_top;
            self.viewport_height = content_height;

            let max_scroll = (self.content_height - self.viewport_height).max(0.0);
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

    fn tick_animations(&mut self, dt: f32) -> TickResult {
        let mut result = self.tick_own_animations(dt);
        result = result.merge(self.tick_smooth_scroll(dt));
        let children = self.children.clone();
        for child in &children {
            let child_result = {
                let mut guard = child.lock().expect("widget lock poisoned");
                guard.tick_animations(dt)
            };
            result = result.merge(child_result);
        }
        // A ghost whose exit animation settled this frame is now safe
        // to drop. Removing it triggers one more layout pass so its
        // former slot collapses.
        let before = self.children.len();
        self.drain_exited_children();
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
        let mut any_descendant_exiting = false;
        let children = self.children.clone();
        for child in &children {
            let started = {
                let mut g = child.lock().expect("widget lock poisoned");
                g.begin_exit()
            };
            if started {
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
                if style.corner_radii.top_left > 0.0
                    || style.corner_radii.top_right > 0.0
                    || style.corner_radii.bottom_left > 0.0
                    || style.corner_radii.bottom_right > 0.0
                {
                    ctx.fill_rounded_rect(
                        border_box_x,
                        border_box_y,
                        border_box_width,
                        border_box_height,
                        &style.corner_radii,
                        background_color,
                    );
                } else {
                    ctx.fill_rect(
                        border_box_x,
                        border_box_y,
                        border_box_width,
                        border_box_height,
                        background_color,
                    );
                }
            }

            ctx.draw_border(
                &style.border,
                border_box_x,
                border_box_y,
                border_box_width,
                border_box_height,
                &style.corner_radii,
            );

            // For scrollable/hidden overflow, clip children to the border box
            if self.style.overflow != Overflow::Visible {
                ctx.push_clip_rect(border_box_x, border_box_y, border_box_width, border_box_height);
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
            UpdateResult::REPLACE
        }

        fn contains_point(&self, _point: &Point) -> bool {
            false
        }
    }

    fn test_ctx() -> LayoutContext<'static> {
        LayoutContext {
            font_collection: None,
            default_font: None,
        }
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
        s.transitions.background_color =
            Some(crate::widget::animation::TransitionConfig::DEFAULT);
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
        assert!(
            parent.children[0]
                .lock()
                .expect("widget lock poisoned")
                .is_exiting()
        );

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
        assert!(
            !flex.exiting,
            "reinsertion should clear the exiting flag"
        );
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
            parent.tick_animations(1.0 / 60.0);
        }

        assert_eq!(
            parent.children.len(),
            0,
            "settled ghost should have been drained"
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
}
