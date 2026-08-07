//! Host-painted `Canvas` leaf — the one place a host application draws
//! its own content *inside* Ogham's flex layout.
//!
//! Everything Ogham can already paint goes through the typed
//! [`RenderContext`] primitives (`fill_rect`, `draw_border`, `draw_text`,
//! …). Some host content is not expressible that way: arcs, wedges,
//! gradients along a path, blend modes, colour matrices. Historically the
//! only answer was "paint it on your own full-screen surface underneath a
//! transparent Ogham root", which works for a *screen* and falls apart for
//! a *widget* — a widget has siblings, and its size is a layout outcome,
//! not a constant. Hosts ended up hardcoding measured Ogham dimensions on
//! the Skia side and defending them with geometry tests.
//!
//! `Canvas` closes that gap. It is a leaf widget that participates in flex
//! layout like any other, and paints by handing a host-registered
//! *painter* a backend-native canvas pre-positioned at its own laid-out
//! rect. See [`docs/internal/CANVAS_LEAF.md`] for the full design and
//! [`INTENT.md`] §6 for the named exception this carves out of the
//! "`Surface` is the only rendering seam" tenet.
//!
//! # The two halves
//!
//! - [`Painter`](canvas_widget::Painter) +
//!   [`CanvasPainter`](canvas_widget::CanvasPainter) +
//!   [`RenderContext::with_local_canvas`] are the *paint escape*: a host
//!   closure, a canvas, and a size.
//! - [`CanvasWidget`](canvas_widget::CanvasWidget) is the *layout
//!   citizen*: sizes itself from a [`FlexStyle`](style::FlexStyle),
//!   hit-tests, reconciles, and delegates paint to the painter named in
//!   its `.ogh` declaration.
//!
//! # Direction of flow
//!
//! Nothing flows *out* of a painter. It receives the `props` map declared
//! in `.ogh` verbatim and reads live host data through whatever
//! `Arc<Mutex<…>>` it captured at registration time, exactly as an event
//! handler does. A painter that reached back into the runtime to push
//! state would violate [`INTENT.md`] §2.
//!
//! [`docs/internal/CANVAS_LEAF.md`]: ../../../docs/internal/CANVAS_LEAF.md
//! [`INTENT.md`]: ../../../docs/internal/INTENT.md

use std::collections::HashMap;
use std::sync::Arc;
#[cfg(debug_assertions)]
use std::sync::{Mutex, OnceLock};

use crate::runtime::value::Value;
use crate::widget::event::{Event, EventContext};
use crate::widget::image::ImageCache;
use crate::widget::point::Point;
use crate::widget::rect::Rect;
use crate::widget::style::{CursorRole, Direction, FlexStyle, Position, Size};
use crate::widget::{LayoutContext, RenderContext, UpdateResult, Widget, WidgetRef};

/// The pointer events that make a widget "eat" a press. Kept identical to
/// `FlexWidget`'s list so `UI::blocks_point` gives the same answer for a
/// listening `Canvas` as it does for a listening `Flex` — hosts gate world
/// picking on that one predicate and must not have to special-case widget
/// types.
const POINTER_LISTENERS: [&str; 3] = ["mouse_down", "mouse_up", "contextmenu"];

/// The drawing handle a [`CanvasPainter`] receives.
///
/// **The coordinate contract is the load-bearing ergonomic decision of this
/// whole feature.** The canvas inside a `Painter` has already been
/// `save()`d, translated to the widget's laid-out origin, and scaled by the
/// backend's DPI factor. A painter therefore draws from `(0, 0)` to
/// `(width(), height())` in *logical* pixels and never learns either its
/// position in the widget tree or the display's DPI. The backend restores
/// the canvas when the painter returns.
///
/// *Why:* the alternative — handing over the raw viewport canvas plus an
/// origin — makes every painter carry a seating constant, which is the
/// exact debt this widget exists to delete. With the pre-translate, a
/// full-screen paint routine becomes a widget paint routine by replacing
/// its centre computation with `width() / 2.0`, `height() / 2.0` and
/// deleting everything else.
///
/// [`dpi_scale`](Self::dpi_scale) is exposed for the rare painter that
/// genuinely wants device pixels (hairlines that should stay crisp, a
/// texture atlas sampled at native resolution). Painters that don't care
/// should ignore it — logical coordinates are the default for a reason.
pub struct Painter<'a> {
    canvas: &'a skia_safe::Canvas,
    width: f32,
    height: f32,
    dpi_scale: f32,
}

impl<'a> Painter<'a> {
    /// Wrap a canvas that the caller has **already** translated to the
    /// widget's origin and scaled by `dpi_scale`. Backends construct this;
    /// painters only ever receive one.
    pub fn new(canvas: &'a skia_safe::Canvas, width: f32, height: f32, dpi_scale: f32) -> Self {
        Self {
            canvas,
            width,
            height,
            dpi_scale,
        }
    }

    /// The backend-native canvas, positioned so that `(0, 0)` is this
    /// widget's top-left corner and one unit is one logical pixel.
    pub fn canvas(&self) -> &'a skia_safe::Canvas {
        self.canvas
    }

    /// Laid-out width in logical pixels (margins excluded — margin is space
    /// *around* the widget, so the painter never owns it).
    pub fn width(&self) -> f32 {
        self.width
    }

    /// Laid-out height in logical pixels (margins excluded).
    pub fn height(&self) -> f32 {
        self.height
    }

    /// The backend's device-pixel ratio, already baked into the canvas
    /// matrix. Only needed by painters that want to reason in device
    /// pixels; drawing in logical coordinates requires no knowledge of it.
    pub fn dpi_scale(&self) -> f32 {
        self.dpi_scale
    }
}

/// A host-registered paint routine, named from `.ogh` by
/// `Canvas { painter: "name" }` and registered with
/// [`RuntimeConfig::with_painter`](crate::runtime::config::RuntimeConfig::with_painter).
///
/// The second argument is the widget's `props:` map, handed over verbatim
/// each frame — the only channel from `.ogh` into the painter. Live host
/// data comes from handles the closure captured at registration.
///
/// `Send + Sync` matches `WidgetFactory` and the event-handler map, so a
/// painter captures `Arc<Mutex<…>>` host state the same way an event
/// handler already does.
///
/// **A panicking painter is not caught.** It is host code running on the
/// host's own render thread; swallowing its panic would hide host bugs
/// behind a blank rectangle.
pub type CanvasPainter = Arc<dyn Fn(&mut Painter, &Value) + Send + Sync>;

/// Report, once per painter name, that the active backend cannot expose a
/// native canvas so this `Canvas` painted nothing.
///
/// A backend without the hatch is not an error — a `Canvas` is opt-in host
/// paint — but "my canvas is blank" needs an answer somewhere other than
/// the reader's memory of this file. Debug-only, like the runaway-layout
/// warning in `UI::layout`.
///
/// The already-warned set is deliberately *not* a field on the widget:
/// `Widget::render` takes `&self`, and `SURFACE.md` names interior
/// mutability inside `render` as a drift indicator. Warned-about-ness is a
/// property of the process anyway — a reconcile that replaces the widget
/// shouldn't restart the spam.
#[cfg(debug_assertions)]
fn warn_blank_canvas_once(painter_name: &str) {
    static WARNED: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    let mut warned = WARNED
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if warned.iter().any(|n| n == painter_name) {
        return;
    }
    warned.push(painter_name.to_string());
    eprintln!(
        "[ogham] Canvas painter {:?} drew nothing: this rendering backend does not \
         implement RenderContext::with_local_canvas",
        painter_name
    );
}

#[cfg(not(debug_assertions))]
fn warn_blank_canvas_once(_painter_name: &str) {}

/// A leaf widget whose pixels are painted by host Rust rather than by
/// Ogham's typed primitives.
///
/// Sizing comes from [`FlexStyle`] (`width` / `height` accept `"grow"`,
/// `"shrink"`, or a number) rather than from bare required numbers, so a
/// `Canvas` can claim a share of a row the same way a `Flex` does. That is
/// a deliberate departure from `ImageWidget`, whose required `width` /
/// `height` f32s make it unable to participate in layout at all.
///
/// A `Canvas` has **no children**. Chrome that must sit over painted
/// content is a sibling with `position: { type: "absolute" }`, or a
/// `Portal`. Admitting children would mean the painter and the widget tree
/// both own the same pixels, and neither could be told which one drew a
/// given one.
///
/// The style is a plain declared style with no transition machinery:
/// per [`INTENT.md`] §4, `Flex` owns springs and hover interpolation and
/// leaves don't grow their own copies.
///
/// [`INTENT.md`]: ../../../docs/internal/INTENT.md
pub struct CanvasWidget {
    /// The painter name exactly as written in `.ogh`. Kept even after the
    /// painter itself is resolved, because reconciliation compares names
    /// (a different painter is a different widget) and the blank-backend
    /// diagnostic needs something to call it.
    pub painter_name: String,
    /// The resolved painter. `None` only when the widget was built without
    /// a registry to resolve against — the builder rejects an unregistered
    /// name outright, so a widget reached through `.ogh` always carries a
    /// painter.
    pub painter: Option<CanvasPainter>,
    /// The `props:` map handed to the painter verbatim each frame.
    /// Defaults to an empty `Value::Map` so painters can pattern-match one
    /// shape whether or not the author declared any props.
    pub props: Value,
    /// Declared layout + paint style. Only the box-model fields
    /// (`width`, `height`, `padding`, `margin`, `border`, `position`) and
    /// `cursor` mean anything here — the painter owns the interior, so
    /// `background_color` and friends are not drawn by the widget.
    pub style: FlexStyle,
    /// Event listeners for handling user interactions.
    pub event_listeners: HashMap<String, Vec<Box<dyn Fn(&Event)>>>,
    /// Whether the pointer is currently inside this widget. Drives the
    /// `mouse_enter` / `mouse_leave` edge that `UI::update_hover` fires;
    /// without it those listeners would silently never run.
    pub hovered: bool,
    /// Layout information computed during the layout phase, in
    /// parent-relative coordinates.
    pub layout: Option<Rect>,
}

impl CanvasWidget {
    /// A `Canvas` naming `painter_name` with no painter resolved yet. The
    /// builder resolves and attaches one; tests use this to exercise
    /// layout and hit-testing without a backend.
    pub fn new(painter_name: impl Into<String>) -> Self {
        Self {
            painter_name: painter_name.into(),
            painter: None,
            props: Value::Map(HashMap::new()),
            style: FlexStyle::default(),
            event_listeners: HashMap::new(),
            hovered: false,
            layout: None,
        }
    }

    /// True when this widget carries at least one listener that consumes a
    /// pointer press. The `Canvas` twin of `FlexWidget`'s
    /// `block_interactions || listener` rule — minus `block_interactions`,
    /// which is a container concern (a leaf has nothing to shield).
    fn has_pointer_listener(&self) -> bool {
        POINTER_LISTENERS
            .iter()
            .any(|name| self.event_listeners.contains_key(*name))
    }

    /// The rect the painter owns: the layout rect minus margins, matching
    /// `FlexWidget`'s border box and this widget's own `contains_point`.
    /// Returns `None` before the first layout pass.
    fn paint_rect(&self) -> Option<Rect> {
        let layout = self.layout.as_ref()?;
        let (left, top) = (self.style.margin.get_left(), self.style.margin.get_top());
        let (right, bottom) = (
            self.style.margin.get_right(),
            self.style.margin.get_bottom(),
        );
        Some(Rect::new(
            layout.x + left,
            layout.y + top,
            (layout.width - left - right).max(0.0),
            (layout.height - top - bottom).max(0.0),
        ))
    }
}

impl Widget for CanvasWidget {
    fn get_type(&self) -> &str {
        "canvas"
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
        // Same resolution rules as `FlexWidget::get_dimensions`, with the
        // children collapsed out: a leaf's shrink size is exactly its own
        // inset. A painter has no intrinsic size to report — it draws
        // whatever rect it is given — so `shrink` on a Canvas means "take
        // no room", not "measure my content".
        let width = match ctx.effective_width(self.style.width) {
            Size::Fixed(w) => w,
            Size::Shrink => {
                let unclamped = self.style.horizontal_inset();
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
                // A child's own content should not affect how its parent
                // allocates it. Width grows along the parent's main axis
                // only when the parent is a row.
                if parent_direction.is_row() {
                    parent_direction.get_grow_size(basis, sibling_basis, parent_available_width)
                } else {
                    parent_width
                }
            }
            // `Percent` has no `.ogh` syntax (`parse_size_value` never
            // produces it) and `FlexWidget` resolves it to 0 as well.
            // Mirrored rather than handled so the two stay in step.
            Size::Percent(_) => 0.0,
        };

        let height = match ctx.effective_height(self.style.height) {
            Size::Fixed(h) => h,
            Size::Shrink => {
                let unclamped = self.style.vertical_inset();
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
                    parent_direction.get_grow_size(basis, sibling_basis, parent_available_height)
                }
            }
            Size::Percent(_) => 0.0,
        };

        (width, height)
    }

    fn get_children(&self) -> Vec<WidgetRef> {
        // A Canvas is a leaf, by design. See the struct docs.
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
        _self_ref: &WidgetRef,
    ) -> bool {
        let Some(point) = &event.point else {
            return false;
        };
        if !self.contains_point(point) {
            return false;
        }
        // `listener_fired` is checked the same way `FlexWidget` checks it:
        // the deepest widget in the hit path that has a matching listener
        // wins, and ancestors don't double-fire.
        if ctx.listener_fired {
            return false;
        }
        let Some(listeners) = self.event_listeners.get(&event.name) else {
            return false;
        };
        for listener in listeners {
            listener(event);
        }
        ctx.listener_fired = true;
        true
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

    fn update(&mut self, new_widget: WidgetRef) -> UpdateResult {
        let mut new_widget = new_widget.lock().expect("widget lock poisoned");
        let Some(new_canvas) = new_widget.downcast_mut::<CanvasWidget>() else {
            return UpdateResult::replace();
        };

        // A different painter is a different widget. The painter owns every
        // pixel inside the rect, so absorbing across a name change would
        // preserve identity for content that shares nothing with what came
        // before — and would quietly keep the old painter alive if the new
        // name failed to resolve.
        if self.painter_name != new_canvas.painter_name {
            return UpdateResult::replace();
        }

        let layout_changed = !self.style.layout_equal(&new_canvas.style);
        // Props are the painter's whole input, so any change repaints —
        // there is no way to tell from here which props are decorative.
        let paint_changed =
            !self.style.paint_equal(&new_canvas.style) || self.props != new_canvas.props;

        self.style = new_canvas.style.clone();
        self.props = std::mem::replace(&mut new_canvas.props, Value::Void);
        // Re-adopt the freshly-resolved painter rather than keeping our
        // own: after a host re-registers under the same name, the newly
        // built widget carries the new closure.
        self.painter = new_canvas.painter.clone();
        std::mem::swap(&mut self.event_listeners, &mut new_canvas.event_listeners);

        UpdateResult {
            absorbed: true,
            needs_layout: layout_changed,
            needs_repaint: layout_changed || paint_changed,
            cancelled_unmount_prefixes: Vec::new(),
            drained_path_prefixes: Vec::new(),
        }
    }

    fn contains_point(&self, point: &Point) -> bool {
        // Margin-aware, mirroring `FlexWidget::contains_point`: a click in
        // a widget's margin belongs to whatever is behind it.
        let Some(layout) = self.layout.as_ref() else {
            return false;
        };
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
    }

    fn blocks_point(&self, point: &Point) -> bool {
        // A bare Canvas is see-through for occlusion purposes: hosts gate
        // world picking on `UI::blocks_point`, and a decorative dial that
        // takes no input must not eat clicks meant for the scene behind it.
        self.contains_point(point) && self.has_pointer_listener()
    }

    fn blocks_interactions(&self) -> bool {
        self.has_pointer_listener()
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
        match self.event_listeners.get(&event.name) {
            Some(listeners) => {
                for listener in listeners {
                    listener(event);
                }
                !listeners.is_empty()
            }
            None => false,
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
        let (Some(rect), Some(painter)) = (self.paint_rect(), self.painter.as_ref()) else {
            return;
        };
        let props = &self.props;
        let painted = ctx.with_local_canvas(&rect, &mut |p: &mut Painter| painter(p, props));
        if !painted {
            warn_blank_canvas_once(&self.painter_name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::style::Spacing;

    fn test_ctx() -> LayoutContext<'static> {
        LayoutContext {
            font_collection: None,
            default_font: None,
            measure_grow_width_as_shrink: false,
            measure_grow_height_as_shrink: false,
        }
    }

    #[test]
    fn shrink_canvas_is_its_own_inset() {
        // A painter reports no intrinsic size, so `shrink` collapses to the
        // box model — padding + margin + border and nothing else.
        let mut c = CanvasWidget::new("dial");
        c.style.width = Size::Shrink;
        c.style.height = Size::Shrink;
        c.style.padding = Spacing::all(6.0);
        let (w, h) = c.get_dimensions(
            &test_ctx(),
            &Direction::Column,
            400.0,
            400.0,
            300.0,
            300.0,
            0.0,
        );
        assert_eq!((w, h), (12.0, 12.0));
    }

    #[test]
    fn absolute_canvas_contributes_no_grow_basis() {
        let mut c = CanvasWidget::new("dial");
        c.style.width = Size::Grow(1.0);
        c.style.position = Position::Absolute(4.0, 8.0);
        assert_eq!(c.get_basis(&Direction::Row), 0.0);
        assert_eq!(c.get_absolute_offset(), Some((4.0, 8.0)));
    }
}
