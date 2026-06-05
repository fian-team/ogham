use std::collections::HashMap;

use super::event::*;
use super::point::*;
use super::rect::*;
use super::Widget;
use crate::widget::event::EventContext;
use crate::widget::image::ImageCache;
use crate::widget::style::Direction;
use crate::widget::{LayoutContext, RenderContext, UpdateResult, WidgetRef};

/// Widget for rendering raster images (PNG).
/// Uses the shared `ImageCache` to avoid reloading images every frame.
pub struct ImageWidget {
    /// Path to the image file (relative to `data/assets/`)
    pub path: String,
    /// Fixed width for the widget
    pub width: f32,
    /// Fixed height for the widget
    pub height: f32,
    /// Event listeners for handling user interactions
    pub event_listeners: HashMap<String, Vec<Box<dyn Fn(&Event)>>>,
    /// Layout information computed during layout phase
    pub layout: Option<Rect>,
}

impl ImageWidget {
    pub fn new(path: String, width: f32, height: f32) -> Self {
        Self {
            path,
            width,
            height,
            event_listeners: HashMap::new(),
            layout: None,
        }
    }
}

impl Widget for ImageWidget {
    fn get_type(&self) -> &str {
        "image"
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
        event: &Event,
        _ctx: &mut EventContext,
        _self_ref: &WidgetRef,
    ) -> bool {
        if let Some(point) = &event.point {
            if self.contains_point(point) {
                if let Some(listeners) = self.event_listeners.get(&event.name) {
                    for listener in listeners {
                        listener(event);
                    }
                    return true;
                }
            }
        }
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

    fn update(&mut self, new_widget: WidgetRef) -> UpdateResult {
        let mut new_widget = new_widget.lock().expect("widget lock poisoned");
        if let Some(new_image) = new_widget.downcast_mut::<ImageWidget>() {
            let dims_changed = self.width != new_image.width || self.height != new_image.height;
            let path_changed = self.path != new_image.path;
            self.path = new_image.path.clone();
            self.width = new_image.width;
            self.height = new_image.height;
            std::mem::swap(&mut self.event_listeners, &mut new_image.event_listeners);
            UpdateResult {
                absorbed: true,
                needs_layout: dims_changed,
                needs_repaint: dims_changed || path_changed,
                cancelled_unmount_prefixes: Vec::new(),
                drained_path_prefixes: Vec::new(),
            }
        } else {
            UpdateResult::replace()
        }
    }

    fn contains_point(&self, point: &Point) -> bool {
        self.layout.as_ref().is_some_and(|r| r.contains(point))
    }

    fn render(&self, ctx: &mut dyn RenderContext, _focused: bool, image_cache: &mut ImageCache) {
        if let Some(layout) = &self.layout {
            ctx.draw_image(
                &self.path,
                layout.x,
                layout.y,
                layout.width,
                layout.height,
                image_cache,
            );
        }
    }
}
