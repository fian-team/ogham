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

/// Flexbox-like layout widget.
pub mod flex_widget;
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
    style::{Border, Color, CornerRadii, Direction, TextStyle},
};

/// Context passed through the layout tree during a layout pass.
/// Carries the font collection and default font so that text widgets
/// can measure text without relying on thread-locals.
pub struct LayoutContext<'a> {
    pub font_collection: Option<&'a FontCollection>,
    pub default_font: Option<&'a str>,
}

/// The UI root containing the widget tree and global state.
pub struct UI {
    /// The root element in the widget hierarchy.
    pub root: WidgetRef,
    /// Cached images to prevent reloading on render.
    pub image_cache: ImageCache,
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
}

impl UI {
    pub fn new(root: WidgetRef) -> Self {
        Self {
            root,
            image_cache: ImageCache::new(),
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

            // Process focus request from context
            if let Some(focus_target) = ctx.take_focus_request() {
                self.focused = Some(focus_target);
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

            // Process focus request from context
            if let Some(focus_target) = ctx.take_focus_request() {
                self.focused = Some(focus_target);
            }

            if handled {
                self.mark_dirty();
            }
            handled
        }
    }

    fn handle_click_event(&mut self, event: &Event, point: &Point, ctx: &mut EventContext) -> bool {
        // First, check if the root widget contains the point
        let mut root = self.root.lock().expect("widget lock poisoned");
        if root.contains_point(point) {
            // If it does, handle the event on the root
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
        }

        let children = widget.get_children_mut();
        drop(widget);

        for child in &children {
            changed |= Self::update_hover_recursive(child, point);
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
        self.needs_layout = false;
        self.needs_repaint = false;
        self.last_layout_width = width;
        self.last_layout_height = height;

        #[cfg(debug_assertions)]
        {
            let now = std::time::Instant::now();
            let start = self.layout_window_start.get_or_insert(now);
            self.layout_count += 1;
            if now.duration_since(*start).as_secs_f32() >= 1.0 {
                if self.layout_count > 5 {
                    eprintln!(
                        "[ogham] WARNING: layout() called {} times in the last second \
                         — check for unnecessary dirty-marking",
                        self.layout_count,
                    );
                }
                self.layout_count = 0;
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
    /// Does **not** trigger a layout pass. Call `layout()` separately after
    /// reconciliation when you are ready to recompute element bounds.
    pub fn reconcile(&mut self, new_root: WidgetRef) {
        {
            // Check if the root references are the same Arc to avoid deadlock
            if Arc::ptr_eq(&self.root, &new_root) {
                // Same widget reference, skip reconciliation to avoid deadlock
            } else {
                let mut root = self.root.lock().expect("widget lock poisoned");
                root.update(new_root);
            }
        }
        if let Some(focused_widget) = self.focused.as_ref() {
            let focused_ref_count = Arc::strong_count(focused_widget);
            // If there's only one reference to the focused widget, it must have
            // been removed from the hierarchy and is therefore no longer a valid
            // focus target.
            if focused_ref_count == 1 {
                self.focused = None;
            }
        }
    }

    /// Mark the UI as needing a full layout pass (structural / content change).
    /// Also implies a repaint is needed.
    pub fn mark_needs_layout(&mut self) {
        self.needs_layout = true;
        self.needs_repaint = true;
    }

    /// Mark the UI as needing a visual refresh only (e.g. hover state change).
    /// Does **not** trigger a layout pass.
    pub fn mark_needs_repaint(&mut self) {
        self.needs_repaint = true;
    }

    /// Backward-compatible alias for [`mark_needs_layout`].
    pub fn mark_dirty(&mut self) {
        self.mark_needs_layout();
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
}

use downcast_rs::{impl_downcast, Downcast};
use rect::Rect;

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
    /// widget will be replaced along with all of its descendants. Returns
    /// true if the widget was successfully updated.
    fn update(&mut self, new_widget: WidgetRef) -> bool;
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
    fn get_layout_rect(&self) -> Option<&Rect> { None }

    /// Offset the widget's layout Y position (used for scroll offset).
    fn set_layout_y(&mut self, _y: f32) {}

    /// Returns true if this widget needs post_render called after children render.
    fn needs_post_render(&self) -> bool { false }

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
