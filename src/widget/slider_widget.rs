//! A native horizontal slider — the first widget that needs the pointer
//! after the cursor has left it.
//!
//! `Slider { value, min, max, step, on_change, on_commit, style,
//! track_color, fill_color, thumb_color }`. The value is **controlled**:
//! the document supplies it every render and the widget reports what
//! the pointer asked for through `on_change(value)` — once per change
//! while the button is held — and `on_commit(value)` on the release.
//! A document that does not write the reported value back sees the
//! thumb snap to what it did supply, which is what "controlled" means.
//!
//! The track is the content box (the style's box less margin and
//! padding); a press at a fraction of its width is that fraction of
//! `min..max`, snapped to `step` when one is declared. The thumb is
//! drawn at that position, held inside the box at the ends. A press
//! takes pointer capture ([`crate::widget::UI`]'s), so the drag keeps
//! tracking wherever the cursor goes until the release.
//!
//! Left / Right while focused move one `step` (a hundredth of the
//! range without one) and commit at once — a key is not a drag.
//!
//! Horizontal only. A vertical slider is a second orientation on the
//! same math and nobody has asked.

use std::collections::HashMap;

use super::event::{Event, EventContext};
use super::point::Point;
use super::rect::Rect;
use super::style::{Color, Corners, CursorRole, Direction, FlexStyle, Position, Size};
use super::{LayoutContext, RenderContext, UpdateResult, Widget, WidgetRef};
use crate::runtime::value::Value;
use crate::widget::image::ImageCache;

/// The box a `shrink`-sized slider takes: enough track to drag along.
const DEFAULT_WIDTH: f32 = 160.0;
const DEFAULT_HEIGHT: f32 = 16.0;
/// The track's thickness, capped to the content height.
const TRACK_THICKNESS: f32 = 6.0;

pub struct SliderWidget {
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// `None` is continuous. A declared step snaps to `min + n·step`.
    pub step: Option<f32>,
    pub style: FlexStyle,
    pub track_color: Color,
    pub fill_color: Color,
    pub thumb_color: Color,
    /// `on_change` / `on_commit`, fired with the value on
    /// `Event::payload`.
    pub event_listeners: HashMap<String, Vec<Box<dyn Fn(&Event)>>>,
    pub hovered: bool,
    pub layout: Option<Rect>,
}

impl SliderWidget {
    pub fn new() -> Self {
        Self {
            value: 0.0,
            min: 0.0,
            max: 1.0,
            step: None,
            style: FlexStyle::default(),
            track_color: Color::new(120, 120, 128, 255),
            fill_color: Color::new(90, 140, 210, 255),
            thumb_color: Color::new(245, 245, 245, 255),
            event_listeners: HashMap::new(),
            hovered: false,
            layout: None,
        }
    }

    /// The range a value is snapped into: `max` may be authored below
    /// `min`; both ends are honoured whichever way round.
    fn bounds(&self) -> (f32, f32) {
        if self.max >= self.min {
            (self.min, self.max)
        } else {
            (self.max, self.min)
        }
    }

    /// `raw` snapped to the step grid and clamped into range.
    pub fn snap(&self, raw: f32) -> f32 {
        let (lo, hi) = self.bounds();
        let snapped = match self.step {
            Some(step) if step > 0.0 => self.min + ((raw - self.min) / step).round() * step,
            _ => raw,
        };
        snapped.clamp(lo, hi)
    }

    /// `value` as a fraction of the track, 0 at `min` and 1 at `max`.
    pub fn fraction(&self) -> f32 {
        let span = self.max - self.min;
        if span.abs() <= f32::EPSILON {
            return 0.0;
        }
        ((self.value - self.min) / span).clamp(0.0, 1.0)
    }

    /// The content box (`x, y, w, h`) in parent-relative space: the
    /// laid-out rect less margin and padding. This is the track.
    fn content_box(&self) -> Option<(f32, f32, f32, f32)> {
        let layout = self.layout.as_ref()?;
        let s = &self.style;
        let x = layout.x + s.margin.get_left() + s.padding.get_left();
        let y = layout.y + s.margin.get_top() + s.padding.get_top();
        let w = layout.width - s.horizontal_inset();
        let h = layout.height - s.vertical_inset();
        Some((x, y, w.max(0.0), h.max(0.0)))
    }

    /// The value a pointer at parent-relative `x` asks for.
    pub fn value_at(&self, x: f32) -> f32 {
        let Some((cx, _cy, cw, _ch)) = self.content_box() else {
            return self.value;
        };
        let fraction = if cw > 0.0 {
            ((x - cx) / cw).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.snap(self.min + fraction * (self.max - self.min))
    }

    fn fire(&self, name: &str) {
        if let Some(listeners) = self.event_listeners.get(name) {
            let mut event = Event::new(name.to_string());
            event.payload = Some(Value::Float(self.value as f64));
            for listener in listeners {
                listener(&event);
            }
        }
    }

    /// Move to `next`, reporting `on_change` if it differs.
    fn set_value(&mut self, next: f32) -> bool {
        if (next - self.value).abs() <= f32::EPSILON {
            return false;
        }
        self.value = next;
        self.fire("on_change");
        true
    }

    fn has_listener(&self, name: &str) -> bool {
        self.event_listeners
            .get(name)
            .is_some_and(|l| !l.is_empty())
    }
}

impl Default for SliderWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for SliderWidget {
    fn get_type(&self) -> &str {
        "slider"
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn update(&mut self, new_widget: WidgetRef) -> UpdateResult {
        let mut new_widget = new_widget.lock().expect("widget lock poisoned");
        let Some(new) = new_widget.downcast_mut::<SliderWidget>() else {
            return UpdateResult::replace();
        };
        let layout_changed = !self.style.layout_equal(&new.style);
        let paint_changed = !self.style.paint_equal(&new.style)
            || self.value != new.value
            || self.min != new.min
            || self.max != new.max
            || self.track_color != new.track_color
            || self.fill_color != new.fill_color
            || self.thumb_color != new.thumb_color;
        // Controlled: the document's value is the value, drag or no drag.
        self.value = new.value;
        self.min = new.min;
        self.max = new.max;
        self.step = new.step;
        self.style = new.style.clone();
        self.track_color = new.track_color;
        self.fill_color = new.fill_color;
        self.thumb_color = new.thumb_color;
        std::mem::swap(&mut self.event_listeners, &mut new.event_listeners);
        UpdateResult {
            absorbed: true,
            needs_layout: layout_changed,
            needs_repaint: layout_changed || paint_changed,
            cancelled_unmount_prefixes: Vec::new(),
            drained_path_prefixes: Vec::new(),
        }
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
        // Same resolution as `CanvasWidget`, with a real intrinsic size:
        // a slider is a control with a natural length, not a painter.
        let width = match ctx.effective_width(self.style.width) {
            Size::Fixed(w) => w,
            Size::Shrink => {
                let want = DEFAULT_WIDTH + self.style.horizontal_inset();
                let max_width = if parent_direction.is_row() {
                    parent_available_width
                } else {
                    parent_width
                };
                if max_width > 0.0 {
                    want.min(max_width)
                } else {
                    want
                }
            }
            Size::Grow(basis) => {
                if parent_direction.is_row() {
                    parent_direction.get_grow_size(basis, sibling_basis, parent_available_width)
                } else {
                    parent_width
                }
            }
            Size::Percent(_) => 0.0,
        };
        let height = match ctx.effective_height(self.style.height) {
            Size::Fixed(h) => h,
            Size::Shrink => {
                let want = DEFAULT_HEIGHT + self.style.vertical_inset();
                let max_height = if parent_direction.is_row() {
                    parent_height
                } else {
                    parent_available_height
                };
                if max_height > 0.0 {
                    want.min(max_height)
                } else {
                    want
                }
            }
            Size::Grow(basis) => {
                if parent_direction.is_row() {
                    parent_height
                } else {
                    parent_direction.get_grow_size(basis, sibling_basis, parent_available_height)
                }
            }
            Size::Percent(_) => 0.0,
        };
        (width, height)
    }

    fn get_children(&self) -> Vec<WidgetRef> {
        Vec::new()
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
        0.0
    }

    fn get_fixed_width(&self) -> Option<f32> {
        self.style.width.as_fixed()
    }

    fn get_fixed_height(&self) -> Option<f32> {
        self.style.height.as_fixed()
    }

    fn handle_event(
        &mut self,
        event: &Event,
        ctx: &mut EventContext,
        self_ref: &WidgetRef,
    ) -> bool {
        if let Some(point) = &event.point {
            let captured = ctx.is_captured(self_ref);
            return match event.name.as_str() {
                "mouse_down" if captured || self.contains_point(point) => {
                    ctx.request_focus(self_ref.clone());
                    ctx.request_capture(self_ref.clone(), point.clone());
                    ctx.listener_fired = true;
                    let next = self.value_at(point.x());
                    self.set_value(next);
                    true
                }
                "mouse_move" if captured => {
                    let next = self.value_at(point.x());
                    self.set_value(next);
                    true
                }
                "mouse_up" if captured => {
                    let next = self.value_at(point.x());
                    self.set_value(next);
                    self.fire("on_commit");
                    true
                }
                _ => false,
            };
        }
        // Keyboard, only when focused: one step per arrow, committed at
        // once.
        if event.name == "keydown" && ctx.is_focused(self_ref) {
            if let Some(code) = event.keyboard_data.as_ref().and_then(|k| k.key_code) {
                let step = match self.step {
                    Some(s) if s > 0.0 => s,
                    _ => (self.max - self.min).abs() / 100.0,
                };
                let direction = match code {
                    37 => -1.0,
                    39 => 1.0,
                    _ => return false,
                };
                let next = self.snap(self.value + direction * step);
                if self.set_value(next) {
                    self.fire("on_commit");
                }
                return true;
            }
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
        self.layout = Some(Rect::new(cursor_x, cursor_y, width, height));
    }

    fn contains_point(&self, point: &Point) -> bool {
        // Margin-aware, like `FlexWidget::contains_point`.
        let Some(layout) = self.layout.as_ref() else {
            return false;
        };
        let m = &self.style.margin;
        let x = layout.x + m.get_left();
        let y = layout.y + m.get_top();
        let w = layout.width - m.get_left() - m.get_right();
        let h = layout.height - m.get_top() - m.get_bottom();
        point.x() >= x && point.x() <= x + w && point.y() >= y && point.y() <= y + h
    }

    fn blocks_point(&self, point: &Point) -> bool {
        // A slider always takes the press: there is no such thing as a
        // decorative one.
        self.contains_point(point)
    }

    fn blocks_interactions(&self) -> bool {
        true
    }

    fn declared_cursor(&self) -> CursorRole {
        self.style.cursor
    }

    fn set_hovered(&mut self, hovered: bool) {
        self.hovered = hovered;
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

    fn fire_event_listener(&self, event: &Event) -> bool {
        if self.has_listener(&event.name) {
            self.fire_listeners(&event.name, event);
            true
        } else {
            false
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

    fn get_layout_rect(&self) -> Option<&Rect> {
        self.layout.as_ref()
    }

    fn render(&self, ctx: &mut dyn RenderContext, _focused: bool, _image_cache: &mut ImageCache) {
        let Some(layout) = self.layout.as_ref() else {
            return;
        };
        let s = &self.style;
        let box_x = layout.x + s.margin.get_left();
        let box_y = layout.y + s.margin.get_top();
        let box_w = layout.width - s.margin.get_left() - s.margin.get_right();
        let box_h = layout.height - s.margin.get_top() - s.margin.get_bottom();
        if let Some(bg) = s.background_color {
            if s.corners.is_all_sharp() {
                ctx.fill_rect(box_x, box_y, box_w, box_h, &bg);
            } else {
                ctx.fill_corners_rect(box_x, box_y, box_w, box_h, &s.corners, &bg);
            }
        }
        ctx.draw_border(&s.border, box_x, box_y, box_w, box_h, &s.corners);

        let Some((cx, cy, cw, ch)) = self.content_box() else {
            return;
        };
        if cw <= 0.0 || ch <= 0.0 {
            return;
        }
        let track_h = TRACK_THICKNESS.min(ch);
        let track_y = cy + (ch - track_h) / 2.0;
        let track_corners = Corners::all_round(track_h / 2.0);
        ctx.fill_corners_rect(cx, track_y, cw, track_h, &track_corners, &self.track_color);
        let fraction = self.fraction();
        let fill_w = cw * fraction;
        if fill_w > 0.0 {
            ctx.fill_corners_rect(
                cx,
                track_y,
                fill_w,
                track_h,
                &track_corners,
                &self.fill_color,
            );
        }
        // The thumb rides the value but is held inside the box at the
        // ends: the track maps the whole content width, the thumb only
        // draws where it fits.
        let thumb = ch.min(cw);
        let centre = (cx + fraction * cw).clamp(cx + thumb / 2.0, cx + cw - thumb / 2.0);
        ctx.fill_corners_rect(
            centre - thumb / 2.0,
            cy,
            thumb,
            thumb,
            &Corners::all_round(thumb / 2.0),
            &self.thumb_color,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slider(min: f32, max: f32, step: Option<f32>) -> SliderWidget {
        let mut s = SliderWidget::new();
        s.min = min;
        s.max = max;
        s.step = step;
        s.layout = Some(Rect::new(10.0, 0.0, 200.0, 16.0));
        s
    }

    #[test]
    fn the_track_is_the_content_box() {
        let s = slider(0.0, 100.0, None);
        assert_eq!(s.value_at(10.0), 0.0);
        assert_eq!(s.value_at(60.0), 25.0);
        assert_eq!(s.value_at(210.0), 100.0);
        assert_eq!(s.value_at(-40.0), 0.0, "clamped at the ends");
        assert_eq!(s.value_at(900.0), 100.0);
    }

    #[test]
    fn a_step_snaps_to_its_grid() {
        let s = slider(0.0, 100.0, Some(25.0));
        assert_eq!(s.value_at(66.0), 25.0); // 28% → 25
        assert_eq!(s.value_at(90.0), 50.0); // 40% → 50
        assert_eq!(s.snap(101.0), 100.0);
        assert_eq!(s.snap(-3.0), 0.0);
    }

    #[test]
    fn a_reversed_range_still_clamps() {
        let s = slider(10.0, 0.0, None);
        assert_eq!(s.value_at(10.0), 10.0);
        assert_eq!(s.value_at(210.0), 0.0);
    }
}
