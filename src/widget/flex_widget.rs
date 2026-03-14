use std::collections::HashMap;
use std::sync::Arc;

use super::event::*;
use super::point::*;
use super::rect::*;
use super::style::*;
use super::Widget;
use crate::widget::event::EventContext;
use crate::widget::style::Direction;
use crate::widget::{LayoutContext, WidgetRef};

/// Flexbox-like layout widget. Supports rendering child elements in either a row or a column with styling applied to the surrounding box.
pub struct FlexWidget {
    pub children: Vec<WidgetRef>,
    pub event_listeners: HashMap<String, Vec<Box<dyn Fn(&Event)>>>,
    pub style: FlexStyle,
    pub hover_style: Option<FlexStyle>,
    pub hovered: bool,
    pub block_interactions: bool,
    pub layout: Option<Rect>,
}

impl FlexWidget {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            event_listeners: HashMap::new(),
            style: FlexStyle::default(),
            hover_style: None,
            hovered: false,
            block_interactions: true,
            layout: None,
        }
    }

    pub fn with_style(style: FlexStyle) -> Self {
        Self {
            children: Vec::new(),
            event_listeners: HashMap::new(),
            style,
            hover_style: None,
            hovered: false,
            block_interactions: true,
            layout: None,
        }
    }

    /// Returns the style to use for layout and rendering. When the widget is
    /// hovered and a pre-merged `hover_style` is set, returns it; otherwise
    /// returns the base style.
    pub fn effective_style(&self) -> &FlexStyle {
        if self.hovered {
            if let Some(ref s) = self.hover_style {
                return s;
            }
        }
        &self.style
    }

    pub fn add_child(&mut self, child: WidgetRef) {
        self.children.push(child);
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
    fn update(&mut self, new_widget: WidgetRef) -> bool {
        let mut new_widget = new_widget.lock().expect("widget lock poisoned");
        if let Some(new_flex_widget) = new_widget.downcast_mut::<FlexWidget>() {
            self.style = new_flex_widget.style.clone();
            self.hover_style = new_flex_widget.hover_style.clone();
            self.block_interactions = new_flex_widget.block_interactions;
            self.layout = new_flex_widget.layout.clone();
            // Swap event listeners - we can't clone closures, so we swap them
            std::mem::swap(
                &mut self.event_listeners,
                &mut new_flex_widget.event_listeners,
            );

            // Update existing children and remove children without corresponding new_child
            let new_child_count = new_flex_widget.children.len();
            for i in 0..self.children.len().min(new_child_count) {
                // Check if the child references are the same Arc to avoid deadlock
                if Arc::ptr_eq(&self.children[i], &new_flex_widget.children[i]) {
                    // Same widget reference, skip update to avoid deadlock
                    continue;
                }
                let mut child = self.children[i].lock().expect("widget lock poisoned");
                if !child.update(new_flex_widget.children[i].clone()) {
                    drop(child); // Drop the lock before replacing the child
                    self.children[i] = new_flex_widget.children[i].clone();
                }
            }

            // Remove old children that don't have corresponding new children
            self.children.truncate(new_child_count);

            // Add new children that don't have corresponding old children
            for i in self.children.len()..new_child_count {
                self.children.push(new_flex_widget.children[i].clone());
            }

            true
        } else {
            false
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
                        parent_available_height
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
                let mut event_handled = false;

                // If it does, check children
                for child_ref in self.children.iter() {
                    let mut child = child_ref.lock().expect("widget lock poisoned");
                    if child.handle_event(event, ctx, child_ref) {
                        event_handled = true;
                        break; // Stop propagating once a child handles the event
                    }
                }

                // If no child handled the event, check if this widget has event listeners
                if !event_handled && self.event_listeners.contains_key(&event.name) {
                    for listener in self.event_listeners.get(&event.name).unwrap() {
                        listener(event);
                    }
                    event_handled = true;
                }

                return self.block_interactions || event_handled;
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

        // Compute dimensions for every child once and cache the results.
        // Absolute-positioned children get None so they are skipped in the
        // normal-flow calculations below.
        let child_dims: Vec<Option<(f32, f32)>> = self
            .children
            .iter()
            .map(|child| {
                let child = child.lock().expect("widget lock poisoned");
                if child.is_absolute_positioned() {
                    return None;
                }
                Some(child.get_dimensions(
                    ctx,
                    &self.style.direction,
                    content_width,
                    available_width,
                    content_height,
                    available_height,
                    children_basis,
                ))
            })
            .collect();

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

        // Start positioning children from the content area with initial offset
        let mut current_x = cursor_x + self.style.inset_left();
        let mut current_y = cursor_y + self.style.inset_top();

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
                let child_x = cursor_x + self.style.inset_left() + offset_x;
                let child_y = cursor_y + self.style.inset_top() + offset_y;

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

    fn get_children_mut(&mut self) -> Vec<WidgetRef> {
        self.children.clone()
    }

    fn get_type(&self) -> &str {
        "box"
    }

    fn set_hovered(&mut self, hovered: bool) {
        self.hovered = hovered;
    }

    fn is_hovered(&self) -> bool {
        self.hovered
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

        fn update(&mut self, _new_widget: WidgetRef) -> bool {
            false
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
}
