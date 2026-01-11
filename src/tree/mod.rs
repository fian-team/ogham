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

pub mod event;
/// Flexbox-like layout widget.
pub mod flex_widget;
pub mod image;
/// Convenience macros for working with widgets.
#[macro_use]
pub mod macros;
pub mod point;
pub mod rect;
pub mod style;
/// SVG rendering widget.
pub mod svg_widget;
/// Text input field widget.
pub mod text_input_widget;
/// Text rendering widget.
pub mod text_widget;

/// Bridge between the AST and the UI tree.
pub mod ast_bridge;

use std::sync::{Arc, Mutex};

use crate::tree::{
    event::{Event, EventContext},
    flex_widget::FlexWidget,
    image::ImageCache,
    point::Point,
    style::Direction,
    svg_widget::SvgWidget,
    text_input_widget::TextInputWidget,
    text_widget::TextWidget,
};

/// The UI root containing the widget tree and global state.
pub struct UI {
    /// The root element in the widget hierarchy.
    pub root: WidgetRef,
    /// Cached images to prevent reloading on render.
    pub image_cache: ImageCache,
    /// Flag to indicate whether interactions have occurred since the last render.
    dirty: bool,
    /// Currently-focused widget, if any.
    focused: Option<WidgetRef>,
}

impl UI {
    pub fn new(root: WidgetRef) -> Self {
        Self {
            root,
            image_cache: ImageCache::new(),
            dirty: true,
            focused: None,
        }
    }

    pub fn call_event(&mut self, event: &Event) -> bool {
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
            let mut root = self.root.lock().unwrap();
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
        let mut root = self.root.lock().unwrap();
        if root.contains_point(point) {
            // If it does, handle the event on the root
            return root.handle_event(event, ctx, &self.root.clone());
        }
        false
    }

    /// Updates the bounds of widgets in the hierarchy within the constraints provided (typically the screen size).
    pub fn layout(&mut self, width: f32, height: f32) {
        let mut root = self.root.lock().unwrap();
        root.layout(
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
    /// Subsequently triggers a layout to update element bounds.
    pub fn update(&mut self, new_root: WidgetRef, width: f32, height: f32) {
        {
            // Check if the root references are the same Arc to avoid deadlock
            if Arc::ptr_eq(&self.root, &new_root) {
                // Same widget reference, skip update to avoid deadlock
            } else {
                let mut root = self.root.lock().unwrap();
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
        self.layout(width, height);
    }

    /// Mark the UI as having had interactions since the last render.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Retrieve whether the UI has had interactions since the last render.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn get_focused(&self) -> Option<&WidgetRef> {
        self.focused.as_ref()
    }
}

/// The Surface trait must be implemented for a given renderer (such as Skia) to draw the widget tree to a bitmap.
pub trait Surface {
    fn draw(&mut self, ui: &mut UI);
    fn draw_widget(
        &mut self,
        widget: &WidgetRef,
        focused: Option<&WidgetRef>,
        image_cache: &mut ImageCache,
    );
    fn draw_box(&mut self, widget: &FlexWidget, image_cache: &mut ImageCache);
    fn draw_borders(&mut self, widget: &FlexWidget, x: f32, y: f32, width: f32, height: f32);
    fn draw_text(&mut self, widget: &TextWidget);
    fn draw_text_input(&mut self, widget: &TextInputWidget);
    fn draw_svg(&mut self, widget: &SvgWidget);
}

use downcast_rs::{impl_downcast, Downcast};

/// All widgets (boxes, text inputs, etc) must implement the Widget trait.
/// Can be used to implement custom rendering systems (e.g. grid instead of
/// flexbox).
pub trait Widget: Downcast {
    fn get_type(&self) -> &str;
    fn get_dimensions(
        &self,
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
    fn get_children_fixed_width(&self) -> f32;
    fn get_children_fixed_height(&self) -> f32;
    fn get_fixed_width(&self) -> Option<f32>;
    fn get_fixed_height(&self) -> Option<f32>;
    fn handle_event(&mut self, event: &Event, ctx: &mut EventContext, self_ref: &WidgetRef)
        -> bool;
    fn layout(
        &mut self,
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
}
impl_downcast!(Widget);

/// Utility type alias for a widget reference. Widget are almost always
/// wrapped in an Arc and Mutex to support references and mutability
/// across the entire tree.
pub type WidgetRef = Arc<Mutex<dyn Widget>>;
