use std::collections::HashMap;

use crate::event::EventContext;
use crate::WidgetRef;

use super::event::*;
use super::point::*;
use super::rect::*;
use super::style::*;
use super::Widget;

pub struct TextWidget {
    pub text: String,
    pub event_listeners: HashMap<String, Vec<Box<dyn Fn(&Event)>>>,
    pub style: TextStyle,
    pub layout: Option<Rect>,
}

impl TextWidget {
    pub fn new(text: String) -> Self {
        Self {
            text,
            event_listeners: HashMap::new(),
            style: TextStyle::default(),
            layout: None,
        }
    }

    pub fn with_color(text: String, color: Color) -> Self {
        Self {
            text,
            event_listeners: HashMap::new(),
            style: TextStyle::default().with_color(color),
            layout: None,
        }
    }
}

impl Widget for TextWidget {
    fn update(&mut self, new_widget: WidgetRef) -> bool {
        let mut new_widget = new_widget.lock().unwrap();
        if let Some(new_text_widget) = new_widget.downcast_mut::<TextWidget>() {
            self.text = new_text_widget.text.clone();
            self.style = new_text_widget.style.clone();
            self.layout = new_text_widget.layout.clone();
            // Swap event listeners - we can't clone closures, so we swap them
            std::mem::swap(
                &mut self.event_listeners,
                &mut new_text_widget.event_listeners,
            );
            true
        } else {
            false
        }
    }

    fn get_type(&self) -> &str {
        "text"
    }

    fn get_dimensions(
        &self,
        parent_direction: &Direction,
        parent_width: f32,
        parent_available_width: f32,
        parent_height: f32,
        parent_available_height: f32,
        sibling_basis: f32,
    ) -> (f32, f32) {
        let basis = self.get_basis(parent_direction);
        match parent_direction {
            Direction::Row | Direction::RowReverse => {
                let width = if sibling_basis > 0.0 {
                    (basis / sibling_basis) * parent_available_width
                } else {
                    parent_available_width
                };
                (width, parent_height)
            }
            Direction::Column | Direction::ColumnReverse => {
                let height = if sibling_basis > 0.0 {
                    (basis / sibling_basis) * parent_available_height
                } else {
                    parent_available_height
                };
                (parent_width, height)
            }
        }
    }

    fn get_children(&self) -> Vec<WidgetRef> {
        Vec::new() // Text widgets have no children
    }

    fn get_basis(&self, _direction: &Direction) -> f32 {
        1.0 // Always grow with basis 1.0
    }

    fn get_children_basis(&self) -> f32 {
        0.0 // No children so no basis
    }

    fn get_children_fixed_width(&self) -> f32 {
        0.0 // No children so no fixed width
    }

    fn get_children_fixed_height(&self) -> f32 {
        0.0 // No children so no fixed height
    }

    fn get_fixed_width(&self) -> Option<f32> {
        None // Text widgets never have fixed width
    }

    fn get_fixed_height(&self) -> Option<f32> {
        None // Text widgets never have fixed height
    }

    fn handle_event(
        &mut self,
        event: &Event,
        _ctx: &mut EventContext,
        _self_ref: &WidgetRef,
    ) -> bool {
        if let Some(point) = &event.point {
            // For click events, check if this widget contains the point
            if self.contains_point(point) {
                // If it does, call any registered event listeners
                if let Some(listeners) = self.event_listeners.get(&event.name) {
                    for listener in listeners {
                        listener(event);
                    }
                    println!("Event handled: {}", event.name);
                    return true; // Event was handled
                }
            }
        }
        false
    }

    fn layout(
        &mut self,
        cursor_x: f32,
        cursor_y: f32,
        _parent_direction: &Direction,
        _parent_width: f32,
        parent_available_width: f32,
        _parent_height: f32,
        parent_available_height: f32,
        _sibling_basis: f32,
    ) {
        self.layout = Some(Rect::new(
            cursor_x,
            cursor_y,
            parent_available_width,
            parent_available_height,
        ));
    }

    fn contains_point(&self, point: &Point) -> bool {
        if let Some(layout) = &self.layout {
            // Text widgets don't have margins, so we just check if the point is within the layout bounds
            point.x() >= layout.x
                && point.x() <= layout.x + layout.width
                && point.y() >= layout.y
                && point.y() <= layout.y + layout.height
        } else {
            false
        }
    }

    fn get_children_mut(&mut self) -> Vec<WidgetRef> {
        Vec::new()
    }
}
