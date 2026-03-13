use std::collections::HashMap;

use super::event::*;
use super::point::*;
use super::rect::*;
use super::style::*;
use super::Widget;
use crate::tree::event::EventContext;
use crate::tree::style::Direction;
use crate::tree::{LayoutContext, WidgetRef};

pub struct TextInputWidget {
    pub value: String,
    pub cursor_position: usize,
    pub event_listeners: HashMap<String, Vec<Box<dyn Fn(&Event)>>>,
    pub style: FlexStyle,
    pub hover_style: Option<FlexStyle>,
    pub text_style: TextStyle,
    pub hover_text_style: Option<TextStyle>,
    pub hovered: bool,
    pub layout: Option<Rect>,
}

impl TextInputWidget {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            cursor_position: 0,
            event_listeners: HashMap::new(),
            style: FlexStyle::default(),
            hover_style: None,
            text_style: TextStyle::default(),
            hover_text_style: None,
            hovered: false,
            layout: None,
        }
    }

    pub fn with_style(style: FlexStyle) -> Self {
        Self {
            value: String::new(),
            cursor_position: 0,
            event_listeners: HashMap::new(),
            style,
            hover_style: None,
            text_style: TextStyle::default(),
            hover_text_style: None,
            hovered: false,
            layout: None,
        }
    }

    pub fn with_value(value: String) -> Self {
        let cursor_position = value.len();
        Self {
            value,
            cursor_position,
            event_listeners: HashMap::new(),
            style: FlexStyle::default(),
            hover_style: None,
            text_style: TextStyle::default(),
            hover_text_style: None,
            hovered: false,
            layout: None,
        }
    }

    /// Returns the flex style to use for rendering. When hovered and a
    /// pre-merged `hover_style` is set, returns it; otherwise returns the
    /// base style.
    pub fn effective_style(&self) -> &FlexStyle {
        if self.hovered {
            if let Some(ref s) = self.hover_style {
                return s;
            }
        }
        &self.style
    }

    /// Returns the text style to use for rendering. When hovered and a
    /// pre-merged `hover_text_style` is set, returns it; otherwise returns
    /// the base text style.
    pub fn effective_text_style(&self) -> &TextStyle {
        if self.hovered {
            if let Some(ref s) = self.hover_text_style {
                return s;
            }
        }
        &self.text_style
    }

    pub fn get_value(&self) -> &str {
        &self.value
    }

    pub fn set_value(&mut self, value: String) {
        self.value = value;
        self.cursor_position = self.value.len().min(self.cursor_position);
    }

    pub fn insert_char(&mut self, ch: char) {
        if self.cursor_position <= self.value.len() {
            self.value.insert(self.cursor_position, ch);
            self.cursor_position += 1;
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 && self.cursor_position <= self.value.len() {
            self.value.remove(self.cursor_position - 1);
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.value.len() {
            self.cursor_position += 1;
        }
    }

    pub fn move_cursor_to_start(&mut self) {
        self.cursor_position = 0;
    }

    pub fn move_cursor_to_end(&mut self) {
        self.cursor_position = self.value.len();
    }
}

impl Widget for TextInputWidget {
    fn update(&mut self, new_widget: WidgetRef) -> bool {
        let mut new_widget = new_widget.lock().expect("widget lock poisoned");
        if let Some(new_text_input_widget) = new_widget.downcast_mut::<TextInputWidget>() {
            self.style = new_text_input_widget.style.clone();
            self.hover_style = new_text_input_widget.hover_style.clone();
            self.text_style = new_text_input_widget.text_style.clone();
            self.hover_text_style = new_text_input_widget.hover_text_style.clone();
            self.layout = new_text_input_widget.layout.clone();
            // Swap event listeners - we can't clone closures, so we swap them
            std::mem::swap(
                &mut self.event_listeners,
                &mut new_text_input_widget.event_listeners,
            );
            let new_value = new_text_input_widget.value.clone();
            let value_unchanged = new_value == self.value;
            self.value = new_value;
            self.cursor_position = if value_unchanged {
                self.cursor_position.min(self.value.len())
            } else {
                self.value.len()
            };
            true
        } else {
            false
        }
    }

    fn get_type(&self) -> &str {
        "text_input"
    }

    fn get_basis(&self, direction: &Direction) -> f32 {
        if direction.is_row() {
            self.style.width.grow_basis()
        } else {
            self.style.height.grow_basis()
        }
    }

    fn get_children_basis(&self) -> f32 {
        0.0 // Text input widgets have no children
    }

    fn get_dimensions(
        &self,
        _ctx: &LayoutContext,
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
                // For text input, use a reasonable default width if no text
                let text_width = if self.value.is_empty() {
                    100.0 // Default minimum width
                } else {
                    // Estimate text width (this is a simple estimation)
                    self.value.len() as f32 * 8.0 + 20.0 // 8px per char + padding
                };
                text_width
            }
            Size::Grow(basis) => {
                if parent_direction.is_row() {
                    self.style
                        .direction
                        .get_grow_size(basis, sibling_basis, parent_available_width)
                } else {
                    parent_width
                }
            }
            Size::Percent(_) => 0.0, // Will be calculated during layout based on parent
        };

        let height = match self.style.height {
            Size::Fixed(h) => h,
            Size::Shrink => {
                // Default height for text input
                30.0
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
        Vec::new() // Text input widgets have no children
    }

    fn handle_event(
        &mut self,
        event: &Event,
        ctx: &mut EventContext,
        self_ref: &WidgetRef,
    ) -> bool {
        let mut event_handled = false;

        // Handle click events for focus
        if let Some(point) = &event.point {
            if self.contains_point(point) {
                if event.name == "mouse_up" {
                    self.cursor_position = self.value.len();
                    ctx.request_focus(self_ref.clone());
                    event_handled = true;
                }
            }
        }

        // Handle keyboard events if focused
        if event.name.starts_with("key") && ctx.is_focused(self_ref) {
            if let Some(keyboard_data) = &event.keyboard_data {
                match event.name.as_str() {
                    "keydown" => {
                        if let Some(key_code) = keyboard_data.key_code {
                            match key_code {
                                8 => {
                                    // Backspace
                                    self.delete_char();
                                    event_handled = true;
                                }
                                37 => {
                                    // Left arrow
                                    self.move_cursor_left();
                                    event_handled = true;
                                }
                                38 => {
                                    // Up arrow
                                    self.move_cursor_to_start();
                                    event_handled = true;
                                }
                                39 => {
                                    // Right arrow
                                    self.move_cursor_right();
                                    event_handled = true;
                                }
                                40 => {
                                    // Down arrow
                                    self.move_cursor_to_end();
                                    event_handled = true;
                                }
                                36 => {
                                    // Home
                                    self.move_cursor_to_start();
                                    event_handled = true;
                                }
                                35 => {
                                    // End
                                    self.move_cursor_to_end();
                                    event_handled = true;
                                }
                                46 => {
                                    // Delete
                                    if self.cursor_position < self.value.len() {
                                        self.value.remove(self.cursor_position);
                                    }
                                    event_handled = true;
                                }
                                _ => {
                                    // Unknown keydown code - ignore
                                }
                            }
                        }
                    }
                    "keypress" => {
                        if let Some(character) = keyboard_data.character {
                            // Only insert printable characters
                            if character.is_ascii_graphic() || character.is_ascii_whitespace() {
                                self.insert_char(character);
                                event_handled = true;
                            }
                        }
                    }
                    _ => {}
                }
            }
            if event_handled {
                let mut dummy_ctx = EventContext::new();
                self.handle_event(
                    &Event::with_value("on_change".to_string(), self.value.clone()),
                    &mut dummy_ctx,
                    self_ref,
                );
            }
        }

        // Call registered event listeners
        if let Some(listeners) = self.event_listeners.get(&event.name) {
            for listener in listeners {
                listener(event);
            }
            event_handled = true;
        }

        event_handled
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

    fn get_fixed_width(&self) -> Option<f32> {
        self.style.width.as_fixed()
    }

    fn get_fixed_height(&self) -> Option<f32> {
        self.style.height.as_fixed()
    }

    fn contains_point(&self, point: &Point) -> bool {
        self.layout.as_ref().is_some_and(|r| r.contains(point))
    }

    fn set_hovered(&mut self, hovered: bool) {
        self.hovered = hovered;
    }

    fn is_hovered(&self) -> bool {
        self.hovered
    }

    fn render(
        &self,
        ctx: &mut dyn crate::tree::RenderContext,
        focused: bool,
        _image_cache: &mut crate::tree::image::ImageCache,
    ) {
        if let Some(layout) = &self.layout {
            let style = self.effective_style();
            let text_style = self.effective_text_style();

            let box_x = layout.x + style.margin.get_left();
            let box_y = layout.y + style.margin.get_top();
            let box_width = layout.width - style.margin.get_left() - style.margin.get_right();
            let box_height = layout.height - style.margin.get_top() - style.margin.get_bottom();

            // Background
            let bg = style
                .background_color
                .unwrap_or(crate::tree::style::Color::new(255, 255, 255, 255));
            ctx.fill_rect(box_x, box_y, box_width, box_height, &bg);

            // Borders
            ctx.draw_border(
                &style.border,
                box_x,
                box_y,
                box_width,
                box_height,
                &style.corner_radii,
            );

            // Text
            let padding_left = style.padding.get_left();
            let padding_right = style.padding.get_right();
            let padding_top = style.padding.get_top();
            let font_size = text_style.get_size();
            let text_x = box_x + padding_left;
            let text_y = box_y + padding_top - font_size * 0.2;
            let text_width = box_width - padding_left - padding_right;

            let display_text = if self.value.is_empty() {
                ""
            } else {
                &self.value
            };
            ctx.draw_text(display_text, text_style, text_x, text_y, text_width);

            // Cursor
            if focused {
                let char_width = font_size * 0.55;
                let cursor_x = text_x + (self.cursor_position as f32 * char_width);
                let cursor_y1 = text_y;
                let cursor_y2 = text_y + font_size;
                ctx.draw_line(
                    cursor_x,
                    cursor_y1,
                    cursor_x,
                    cursor_y2,
                    1.0,
                    &crate::tree::style::Color::new(0, 0, 0, 255),
                );
            }
        }
    }
}
