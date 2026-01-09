use std::collections::HashMap;
use std::sync::Arc;

use super::event::*;
use super::point::*;
use super::rect::*;
use super::style::*;
use super::Widget;
use crate::tree::event::EventContext;
use crate::tree::style::Direction;
use crate::tree::WidgetRef;

/// Flexbox-like layout widget. Supports rendering child elements in either a row or a column with styling applied to the surrounding box.
pub struct FlexWidget {
    pub children: Vec<WidgetRef>,
    pub event_listeners: HashMap<String, Vec<Box<dyn Fn(&Event)>>>,
    pub style: FlexStyle,
    pub block_interactions: bool,
    pub layout: Option<Rect>,
}

impl FlexWidget {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            event_listeners: HashMap::new(),
            style: FlexStyle::default(),
            block_interactions: true,
            layout: None,
        }
    }

    pub fn with_style(style: FlexStyle) -> Self {
        Self {
            children: Vec::new(),
            event_listeners: HashMap::new(),
            style,
            block_interactions: true,
            layout: None,
        }
    }

    pub fn add_child(&mut self, child: WidgetRef) {
        self.children.push(child);
    }
}

impl Widget for FlexWidget {
    fn update(&mut self, new_widget: WidgetRef) -> bool {
        let mut new_widget = new_widget.lock().unwrap();
        if let Some(new_flex_widget) = new_widget.downcast_mut::<FlexWidget>() {
            self.style = new_flex_widget.style.clone();
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
                let mut child = self.children[i].lock().unwrap();
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
        // Absolute positioned children should have a flex basis of 0
        if matches!(self.style.position, Position::Absolute(_, _)) {
            return 0.0;
        }

        match direction {
            Direction::Row | Direction::RowReverse => match self.style.width {
                Size::Fixed(_w) => 0.0,
                Size::Shrink => 0.0,
                Size::Grow(basis) => basis,
                Size::Percent(_p) => 0.0,
            },
            Direction::Column | Direction::ColumnReverse => match self.style.height {
                Size::Fixed(_h) => 0.0,
                Size::Shrink => 0.0,
                Size::Grow(basis) => basis,
                Size::Percent(_p) => 0.0,
            },
        }
    }

    fn get_children_basis(&self) -> f32 {
        let mut basis = 0.0;
        for child in self.children.iter() {
            // Skip absolute positioned children as they don't contribute to flex basis
            let child = child.lock().unwrap();
            if let Some(box_widget) = child.downcast_ref::<FlexWidget>() {
                if matches!(box_widget.style.position, Position::Absolute(_, _)) {
                    continue;
                }
            }
            basis += child.get_basis(&self.style.direction);
        }
        basis
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
        let width = match self.style.width {
            Size::Fixed(w) => w,
            Size::Shrink => {
                let _occupied_width: f32 = self.get_children_fixed_width();
                let occupied_height = self.get_children_fixed_height();
                let get_dimensions = |child: &WidgetRef| {
                    // Skip absolute positioned children in dimension calculations
                    let children_basis = self.get_children_basis();
                    let child = child.lock().unwrap();
                    if let Some(box_widget) = child.downcast_ref::<FlexWidget>() {
                        if matches!(box_widget.style.position, Position::Absolute(_, _)) {
                            return (0.0, 0.0);
                        }
                    }
                    // If parent has Shrink along child's main axis, clamp Grow children by passing 0.0 as available space
                    let child_available_width = if self.style.direction.is_row() {
                        // Parent is shrinking along row axis, so no available space for Grow children
                        0.0
                    } else {
                        // In column direction, width is the cross axis. Don't subtract sibling widths.
                        parent_available_width
                    };
                    let child_available_height = if !self.style.direction.is_row() {
                        // Parent is shrinking along column axis, so no available space for Grow children
                        0.0
                    } else {
                        parent_available_height - occupied_height
                    };
                    child.get_dimensions(
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

                // Calculate gap space between children (gap * (number_of_children - 1))
                let non_absolute_count = self
                    .children
                    .iter()
                    .filter(|child| {
                        let child = child.lock().unwrap();
                        if let Some(box_widget) = child.downcast_ref::<FlexWidget>() {
                            !matches!(box_widget.style.position, Position::Absolute(_, _))
                        } else {
                            true
                        }
                    })
                    .count();
                let gap_size = if non_absolute_count > 1 && self.style.direction.is_row() {
                    self.style.gap * (non_absolute_count - 1) as f32
                } else {
                    0.0
                };

                // Add parent's own padding, margins, borders, and gap to the child size
                child_size
                    + self.style.padding.get_left()
                    + self.style.padding.get_right()
                    + self.style.margin.get_left()
                    + self.style.margin.get_right()
                    + self.style.border.get_left()
                    + self.style.border.get_right()
                    + gap_size
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
                    // Skip absolute positioned children in dimension calculations
                    let child = child.lock().unwrap();
                    if let Some(box_widget) = child.downcast_ref::<FlexWidget>() {
                        if matches!(box_widget.style.position, Position::Absolute(_, _)) {
                            return (0.0, 0.0);
                        }
                    }
                    // If parent has Shrink along child's main axis, clamp Grow children by passing 0.0 as available space
                    let child_available_width = if self.style.direction.is_row() {
                        // Parent is shrinking along row axis, so no available space for Grow children
                        0.0
                    } else {
                        parent_available_width
                    };
                    let child_available_height = if !self.style.direction.is_row() {
                        // Parent is shrinking along column axis, so no available space for Grow children
                        0.0
                    } else {
                        parent_available_height
                    };
                    child.get_dimensions(
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

                // Calculate gap space between children (gap * (number_of_children - 1))
                let non_absolute_count = self
                    .children
                    .iter()
                    .filter(|child| {
                        let child = child.lock().unwrap();
                        if let Some(box_widget) = child.downcast_ref::<FlexWidget>() {
                            !matches!(box_widget.style.position, Position::Absolute(_, _))
                        } else {
                            true
                        }
                    })
                    .count();
                let gap_size = if non_absolute_count > 1 && !self.style.direction.is_row() {
                    self.style.gap * (non_absolute_count - 1) as f32
                } else {
                    0.0
                };

                // Add parent's own padding, margins, borders, and gap to the child size
                child_size
                    + self.style.padding.get_top()
                    + self.style.padding.get_bottom()
                    + self.style.margin.get_top()
                    + self.style.margin.get_bottom()
                    + self.style.border.get_top()
                    + self.style.border.get_bottom()
                    + gap_size
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
                    let mut child = child_ref.lock().unwrap();
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
                let mut child = child_ref.lock().unwrap();
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
                let child = child.lock().unwrap();
                if let Some(box_widget) = child.downcast_ref::<FlexWidget>() {
                    !matches!(box_widget.style.position, Position::Absolute(_, _))
                } else {
                    true
                }
            })
            .count();

        // Calculate gap space that will be used between children
        let gap_space = if non_absolute_count > 1 {
            self.style.gap * (non_absolute_count - 1) as f32
        } else {
            0.0
        };

        // Available space should only subtract fixed-size children along the *main* axis.
        let available_width = width
            - (self.style.margin.get_left() + self.style.margin.get_right())
            - (self.style.padding.get_left() + self.style.padding.get_right())
            - (self.style.border.get_left() + self.style.border.get_right())
            - if self.style.direction.is_row() {
                self.get_children_fixed_width() + gap_space
            } else {
                0.0
            };
        let available_height = height
            - (self.style.margin.get_top() + self.style.margin.get_bottom())
            - (self.style.padding.get_top() + self.style.padding.get_bottom())
            - (self.style.border.get_top() + self.style.border.get_bottom())
            - if self.style.direction.is_row() {
                0.0
            } else {
                self.get_children_fixed_height() + gap_space
            };

        // Calculate the content area by subtracting padding and margin
        let content_width = width
            - (self.style.padding.get_left()
                + self.style.padding.get_right()
                + self.style.margin.get_left()
                + self.style.margin.get_right()
                + self.style.border.get_left()
                + self.style.border.get_right());
        let content_height = height
            - (self.style.padding.get_top()
                + self.style.padding.get_bottom()
                + self.style.margin.get_top()
                + self.style.margin.get_bottom()
                + self.style.border.get_top()
                + self.style.border.get_bottom());

        self.layout = Some(Rect::new(cursor_x, cursor_y, width, height));

        // Calculate total size of children in main axis (excluding absolute positioned)
        let mut total_main_size = 0.0;
        let mut max_cross_size = 0.0;
        for child in self.children.iter() {
            // Skip absolute positioned children in size calculations
            let child = child.lock().unwrap();
            if let Some(box_widget) = child.downcast_ref::<FlexWidget>() {
                if matches!(box_widget.style.position, Position::Absolute(_, _)) {
                    continue;
                }
            }
            let child_dimensions = child.get_dimensions(
                &self.style.direction,
                content_width,
                available_width,
                content_height,
                available_height,
                children_basis,
            );
            if self.style.direction.is_row() {
                total_main_size += child_dimensions.0;
                max_cross_size = f32::max(max_cross_size, child_dimensions.1);
            } else {
                total_main_size += child_dimensions.1;
                max_cross_size = f32::max(max_cross_size, child_dimensions.0);
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
        let mut current_x = cursor_x
            + self.style.margin.get_left() as f32
            + self.style.padding.get_left() as f32
            + self.style.border.get_left();
        let mut current_y = cursor_y
            + self.style.margin.get_top() as f32
            + self.style.padding.get_top() as f32
            + self.style.border.get_top();

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
        for child in self.children.iter_mut() {
            let mut child = child.lock().unwrap();
            // Skip absolute positioned children in normal flow
            if let Some(box_widget) = child.downcast_ref::<FlexWidget>() {
                if matches!(box_widget.style.position, Position::Absolute(_, _)) {
                    continue;
                }
            }

            let child_dimensions = child.get_dimensions(
                &self.style.direction,
                content_width,
                available_width,
                content_height,
                available_height,
                children_basis,
            );

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
            let mut child = child.lock().unwrap();
            if let Some(box_widget) = child.downcast_ref::<FlexWidget>() {
                if let Position::Absolute(offset_x, offset_y) = box_widget.style.position {
                    let _child_dimensions = child.get_dimensions(
                        &self.style.direction,
                        content_width,
                        available_width,
                        content_height,
                        available_height,
                        children_basis,
                    );

                    // Position absolute children at their specified coordinates relative to parent
                    let child_x = cursor_x
                        + self.style.margin.get_left() as f32
                        + self.style.padding.get_left() as f32
                        + self.style.border.get_left()
                        + offset_x;
                    let child_y = cursor_y
                        + self.style.margin.get_top() as f32
                        + self.style.padding.get_top() as f32
                        + self.style.border.get_top()
                        + offset_y;

                    child.layout(
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
    }

    fn get_children_fixed_width(&self) -> f32 {
        self.children
            .iter()
            .filter_map(|child| {
                // Skip absolute positioned children
                let child = child.lock().unwrap();
                if let Some(box_widget) = child.downcast_ref::<FlexWidget>() {
                    if matches!(box_widget.style.position, Position::Absolute(_, _)) {
                        return None;
                    }
                }
                match child.get_fixed_width() {
                    Some(w) => Some(w),
                    None => None,
                }
            })
            .sum()
    }

    fn get_children_fixed_height(&self) -> f32 {
        self.children
            .iter()
            .filter_map(|child| {
                // Skip absolute positioned children
                let child = child.lock().unwrap();
                if let Some(box_widget) = child.downcast_ref::<FlexWidget>() {
                    if matches!(box_widget.style.position, Position::Absolute(_, _)) {
                        return None;
                    }
                }
                match child.get_fixed_height() {
                    Some(h) => Some(h),
                    None => None,
                }
            })
            .sum()
    }

    fn get_fixed_width(&self) -> Option<f32> {
        match self.style.width {
            Size::Fixed(w) => Some(w),
            _ => None,
        }
    }

    fn get_fixed_height(&self) -> Option<f32> {
        match self.style.height {
            Size::Fixed(h) => Some(h),
            _ => None,
        }
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

        fn get_children_fixed_width(&self) -> f32 {
            0.0
        }

        fn get_children_fixed_height(&self) -> f32 {
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

    #[test]
    fn test_gap_in_row_layout() {
        // Create a parent with fixed width and gap
        let gap = 10.0;
        let parent_width = 200.0;
        let parent_height = 100.0;

        let mut parent = FlexWidget::with_style(
            FlexStyle::builder()
                .width_fixed(parent_width)
                .height_fixed(parent_height)
                .gap(gap)
                .row()
                .build(),
        );

        // Add 3 children, each 50px wide
        // Total child width: 3 * 50 = 150px
        // Total gap: 2 * 10 = 20px
        // Total needed: 150 + 20 = 170px (fits in 200px)
        let child_width = 50.0;
        let child_height = 80.0;

        for _ in 0..3 {
            let child = Arc::new(Mutex::new(TestWidget::new(child_width, child_height)));
            parent.add_child(child);
        }

        // Layout the parent
        let parent_ref = Arc::new(Mutex::new(parent));
        {
            let mut parent = parent_ref.lock().unwrap();
            parent.layout(
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
        let parent = parent_ref.lock().unwrap();
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
            let child = child_ref.lock().unwrap();
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
        // Create a parent with fixed height and gap
        let gap = 15.0;
        let parent_width = 100.0;
        let parent_height = 200.0;

        let mut parent = FlexWidget::with_style(
            FlexStyle::builder()
                .width_fixed(parent_width)
                .height_fixed(parent_height)
                .gap(gap)
                .column()
                .build(),
        );

        // Add 3 children, each 50px tall
        // Total child height: 3 * 50 = 150px
        // Total gap: 2 * 15 = 30px
        // Total needed: 150 + 30 = 180px (fits in 200px)
        let child_width = 80.0;
        let child_height = 50.0;

        for _ in 0..3 {
            let child = Arc::new(Mutex::new(TestWidget::new(child_width, child_height)));
            parent.add_child(child);
        }

        // Layout the parent
        let parent_ref = Arc::new(Mutex::new(parent));
        {
            let mut parent = parent_ref.lock().unwrap();
            parent.layout(
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
        let parent = parent_ref.lock().unwrap();
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
            let child = child_ref.lock().unwrap();
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
        // Test that gap is properly accounted for when parent has Shrink width
        let gap = 20.0;
        let parent_height = 100.0;

        let mut parent = FlexWidget::with_style(
            FlexStyle::builder()
                .width_shrink()
                .height_fixed(parent_height)
                .gap(gap)
                .row()
                .build(),
        );

        // Add 2 children, each 100px wide
        // Expected parent width: 2 * 100 + 1 * 20 = 220px
        let child_width = 100.0;
        let child_height = 80.0;

        for _ in 0..2 {
            let child = Arc::new(Mutex::new(TestWidget::new(child_width, child_height)));
            parent.add_child(child);
        }

        // Layout the parent
        let parent_ref = Arc::new(Mutex::new(parent));
        {
            let mut parent = parent_ref.lock().unwrap();
            parent.layout(
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
        let parent = parent_ref.lock().unwrap();
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
        // Test that gap is properly accounted for when parent has Shrink height
        let gap = 25.0;
        let parent_width = 100.0;

        let mut parent = FlexWidget::with_style(
            FlexStyle::builder()
                .width_fixed(parent_width)
                .height_shrink()
                .gap(gap)
                .column()
                .build(),
        );

        // Add 2 children, each 75px tall
        // Expected parent height: 2 * 75 + 1 * 25 = 175px
        let child_width = 80.0;
        let child_height = 75.0;

        for _ in 0..2 {
            let child = Arc::new(Mutex::new(TestWidget::new(child_width, child_height)));
            parent.add_child(child);
        }

        // Layout the parent
        let parent_ref = Arc::new(Mutex::new(parent));
        {
            let mut parent = parent_ref.lock().unwrap();
            parent.layout(
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
        let parent = parent_ref.lock().unwrap();
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
        // Gap should not affect layout when there's only one child
        let gap = 10.0;
        let parent_width = 200.0;
        let parent_height = 100.0;

        let mut parent = FlexWidget::with_style(
            FlexStyle::builder()
                .width_fixed(parent_width)
                .height_fixed(parent_height)
                .gap(gap)
                .row()
                .build(),
        );

        // Add only 1 child
        let child_width = 50.0;
        let child_height = 80.0;
        let child = Arc::new(Mutex::new(TestWidget::new(child_width, child_height)));
        parent.add_child(child);

        // Layout the parent
        let parent_ref = Arc::new(Mutex::new(parent));
        {
            let mut parent = parent_ref.lock().unwrap();
            parent.layout(
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
        let parent = parent_ref.lock().unwrap();
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
}
