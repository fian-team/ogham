use std::collections::HashMap;

use skia_safe::{svg, FontMgr};

use super::event::*;
use super::point::*;
use super::rect::*;
use super::Widget;
use crate::event::EventContext;
use crate::style::{Color, Direction};
use crate::WidgetRef;

/// Widget for rendering SVG files.
/// Loads the SVG at creation time. If the SVG file doesn't exist or fails to load,
/// the widget will render as a blank space with the specified dimensions.
pub struct SvgWidget {
    /// Path to the SVG file
    pub path: String,
    /// Loaded SVG DOM, or None if loading failed
    pub svg_dom: Option<svg::Dom>,
    /// Fixed width for the widget
    pub width: f32,
    /// Fixed height for the widget
    pub height: f32,
    /// Optional color to apply to the SVG (for recoloring)
    pub color: Option<Color>,
    /// Event listeners for handling user interactions
    pub event_listeners: HashMap<String, Vec<Box<dyn Fn(&Event)>>>,
    /// Layout information computed during layout phase
    pub layout: Option<Rect>,
}

impl SvgWidget {
    /// Creates a new SVG widget with the specified path, width, and height.
    /// The SVG is loaded immediately at creation time.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the SVG file (relative to the data directory or absolute)
    /// * `width` - Width of the widget in logical pixels
    /// * `height` - Height of the widget in logical pixels
    /// * `color` - Optional color to apply to the SVG (for recoloring)
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use ui::style::Color;
    /// use ui::svg_widget::SvgWidget;
    ///
    /// let svg_widget = SvgWidget::new("assets/icons/select.svg".to_string(), 24.0, 24.0, None);
    /// let red_svg = SvgWidget::new(
    ///     "assets/icons/select.svg".to_string(),
    ///     24.0,
    ///     24.0,
    ///     Some(Color::new(255, 0, 0, 255)),
    /// );
    /// ```
    pub fn new(path: String, width: f32, height: f32, color: Option<Color>) -> Self {
        // Try to load the SVG file with the specified color
        let svg_dom = Self::load_svg_with_color(&path, color.as_ref());

        Self {
            path,
            svg_dom,
            width,
            height,
            color,
            event_listeners: HashMap::new(),
            layout: None,
        }
    }

    /// Attempts to load an SVG file from the given path.
    /// Returns Some(Dom) if successful, None otherwise.
    fn load_svg(path: &str) -> Option<svg::Dom> {
        Self::load_svg_with_color(path, None)
    }

    /// Attempts to load an SVG file from the given path, optionally applying a color.
    /// If a color is provided, the SVG XML is modified to use that color for fill and stroke.
    /// Returns Some(Dom) if successful, None otherwise.
    fn load_svg_with_color(path: &str, color: Option<&Color>) -> Option<svg::Dom> {
        // Read the file from the exact path provided
        let mut data = match std::fs::read(path) {
            Ok(data) => data,
            Err(_) => return None,
        };

        // If a color is specified, modify the SVG XML to apply the color
        if let Some(color) = color {
            // Convert the bytes to a string (assuming UTF-8)
            if let Ok(svg_string) = String::from_utf8(data.clone()) {
                // Convert color to hex format (RRGGBB)
                let color_hex = format!("{:02x}{:02x}{:02x}", color.r, color.g, color.b);

                // Modify the SVG to set fill and stroke colors
                // We'll add a style attribute to the root <svg> element
                let modified_svg = Self::apply_color_to_svg(&svg_string, &color_hex);

                // Convert back to bytes
                data = modified_svg.into_bytes();
            }
        }

        // Create a font manager for SVG rendering
        // Using default() to allow system fonts, or new_empty() if fonts aren't needed
        let font_mgr = FontMgr::default();

        // Parse the SVG data into a DOM
        svg::Dom::from_bytes(&data, font_mgr).ok()
    }

    /// Modifies SVG XML to apply a color to fill and stroke attributes.
    /// This function adds a style attribute to the root <svg> element that sets
    /// the color for all elements that use currentColor or don't have explicit colors.
    /// It also replaces fill="currentColor" and stroke="currentColor" with the actual color.
    fn apply_color_to_svg(svg_content: &str, color_hex: &str) -> String {
        let mut modified = svg_content.to_string();

        // Replace currentColor references with the actual color
        // This handles both fill="currentColor" and stroke="currentColor"
        modified = modified.replace("fill=\"currentColor\"", &format!("fill=\"#{}\"", color_hex));
        modified = modified.replace("fill='currentColor'", &format!("fill='#{}'", color_hex));
        modified = modified.replace(
            "stroke=\"currentColor\"",
            &format!("stroke=\"#{}\"", color_hex),
        );
        modified = modified.replace("stroke='currentColor'", &format!("stroke='#{}'", color_hex));

        // Also replace in style attributes
        modified = modified.replace("fill:currentColor", &format!("fill:#{}", color_hex));
        modified = modified.replace("stroke:currentColor", &format!("stroke:#{}", color_hex));

        // Try to find the root <svg> element and add/update style attribute
        if let Some(svg_start) = modified.find("<svg") {
            if let Some(svg_end) = modified[svg_start..].find('>') {
                let svg_tag_end = svg_start + svg_end;
                let svg_tag = &modified[svg_start..svg_tag_end];

                // Check if style attribute already exists
                if let Some(style_start) = svg_tag.find("style=\"") {
                    // Find the end of the style attribute value
                    if let Some(style_value_end) = modified[svg_start + style_start + 7..].find('"')
                    {
                        let style_attr_start = svg_start + style_start + 7;
                        let style_attr_end = style_attr_start + style_value_end;
                        let existing_style = &modified[style_attr_start..style_attr_end];

                        // Update or add color, fill, and stroke to the style
                        let mut new_style = existing_style.to_string();
                        if !new_style.contains("color:") {
                            new_style = format!("color:#{};{}", color_hex, new_style);
                        }
                        if !new_style.contains("fill:") {
                            new_style = format!("{};fill:#{}", new_style, color_hex);
                        }
                        if !new_style.contains("stroke:") {
                            new_style = format!("{};stroke:#{}", new_style, color_hex);
                        }

                        // Replace the style attribute value
                        modified.replace_range(style_attr_start..style_attr_end, &new_style);
                    }
                } else {
                    // Add style attribute before the closing >
                    let style_attr = format!(
                        " style=\"color:#{};fill:#{};stroke:#{}\"",
                        color_hex, color_hex, color_hex
                    );
                    modified.insert_str(svg_tag_end, &style_attr);
                }
            }
        }

        modified
    }
}

impl Widget for SvgWidget {
    fn update(&mut self, new_widget: WidgetRef) -> bool {
        let mut new_widget = new_widget.lock().unwrap();
        if let Some(new_svg_widget) = new_widget.downcast_mut::<SvgWidget>() {
            // Update path and reload SVG if path or color changed
            let path_changed = self.path != new_svg_widget.path;
            let color_changed = match (&self.color, &new_svg_widget.color) {
                (None, None) => false,
                (Some(a), Some(b)) => a.r != b.r || a.g != b.g || a.b != b.b || a.a != b.a,
                _ => true, // One is Some, one is None
            };

            if path_changed || color_changed {
                self.path = new_svg_widget.path.clone();
                self.color = new_svg_widget.color;
                self.svg_dom = Self::load_svg_with_color(&self.path, self.color.as_ref());
            }
            self.width = new_svg_widget.width;
            self.height = new_svg_widget.height;
            self.layout = new_svg_widget.layout.clone();
            // Swap event listeners - we can't clone closures, so we swap them
            std::mem::swap(
                &mut self.event_listeners,
                &mut new_svg_widget.event_listeners,
            );
            true
        } else {
            false
        }
    }

    fn get_type(&self) -> &str {
        "svg"
    }

    fn get_dimensions(
        &self,
        _parent_direction: &Direction,
        _parent_width: f32,
        _parent_available_width: f32,
        _parent_height: f32,
        _parent_available_height: f32,
        _sibling_basis: f32,
    ) -> (f32, f32) {
        // SVG widgets always use their fixed dimensions
        (self.width, self.height)
    }

    fn get_children(&self) -> Vec<WidgetRef> {
        Vec::new() // SVG widgets have no children
    }

    fn get_basis(&self, _direction: &Direction) -> f32 {
        0.0 // Fixed size widgets don't grow
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
            // For click events, check if this widget contains the point
            if self.contains_point(point) {
                // If it does, call any registered event listeners
                if let Some(listeners) = self.event_listeners.get(&event.name) {
                    for listener in listeners {
                        listener(event);
                    }
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
        _parent_available_width: f32,
        _parent_height: f32,
        _parent_available_height: f32,
        _sibling_basis: f32,
    ) {
        self.layout = Some(Rect::new(cursor_x, cursor_y, self.width, self.height));
    }

    fn contains_point(&self, point: &Point) -> bool {
        if let Some(layout) = &self.layout {
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
