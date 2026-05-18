use std::collections::HashMap;

use super::event::*;
use super::point::*;
use super::rect::*;
use super::Widget;
use crate::widget::event::EventContext;
use crate::widget::image::ImageCache;
use crate::widget::style::{Color, Corners, Direction, Spacing};
use crate::widget::{LayoutContext, RenderContext, UpdateResult, WidgetRef};

/// Grid placement metadata for a child within a GridWidget.
#[derive(Debug, Clone)]
pub struct GridPlacement {
    pub col: u32,
    pub row: u32,
    pub col_span: u32,
    pub row_span: u32,
}

impl Default for GridPlacement {
    fn default() -> Self {
        Self {
            col: 0,
            row: 0,
            col_span: 1,
            row_span: 1,
        }
    }
}

/// Style properties for a grid layout.
#[derive(Debug, Clone)]
pub struct GridStyle {
    pub columns: u32,
    pub rows: u32,
    pub cell_width: f32,
    pub cell_height: f32,
    pub gap: f32,
    pub padding: Spacing,
    pub margin: Spacing,
    pub background_color: Option<Color>,
    pub cell_color: Option<Color>,
    pub corners: Corners,
}

impl Default for GridStyle {
    fn default() -> Self {
        Self {
            columns: 1,
            rows: 1,
            cell_width: 40.0,
            cell_height: 40.0,
            gap: 0.0,
            padding: Spacing::identity(),
            margin: Spacing::identity(),
            background_color: None,
            cell_color: None,
            corners: Corners::identity(),
        }
    }
}

/// A grid layout widget that places children into a fixed-size cell grid.
///
/// Each child has a `GridPlacement` that determines which cells it occupies.
/// Grid lines are achieved by setting a `gap` and `background_color` — the
/// background shows through the gaps between cells.
pub struct GridWidget {
    pub style: GridStyle,
    pub children: Vec<(GridPlacement, WidgetRef)>,
    pub event_listeners: HashMap<String, Vec<Box<dyn Fn(&Event)>>>,
    pub layout: Option<Rect>,
    pub hovered: bool,
}

impl GridWidget {
    pub fn new() -> Self {
        Self {
            style: GridStyle::default(),
            children: Vec::new(),
            event_listeners: HashMap::new(),
            layout: None,
            hovered: false,
        }
    }

    /// Total width of the grid including padding and margin.
    fn total_width(&self) -> f32 {
        let content = self.style.columns as f32 * self.style.cell_width
            + (self.style.columns.saturating_sub(1)) as f32 * self.style.gap;
        content + self.style.padding.left + self.style.padding.right
            + self.style.margin.left + self.style.margin.right
    }

    /// Total height of the grid including padding and margin.
    fn total_height(&self) -> f32 {
        let content = self.style.rows as f32 * self.style.cell_height
            + (self.style.rows.saturating_sub(1)) as f32 * self.style.gap;
        content + self.style.padding.top + self.style.padding.bottom
            + self.style.margin.top + self.style.margin.bottom
    }

    /// Compute the pixel position and size for a given grid placement.
    fn cell_rect(&self, placement: &GridPlacement, origin_x: f32, origin_y: f32) -> Rect {
        let x = origin_x
            + self.style.margin.left
            + self.style.padding.left
            + placement.col as f32 * (self.style.cell_width + self.style.gap);
        let y = origin_y
            + self.style.margin.top
            + self.style.padding.top
            + placement.row as f32 * (self.style.cell_height + self.style.gap);
        let w = placement.col_span as f32 * self.style.cell_width
            + (placement.col_span.saturating_sub(1)) as f32 * self.style.gap;
        let h = placement.row_span as f32 * self.style.cell_height
            + (placement.row_span.saturating_sub(1)) as f32 * self.style.gap;
        Rect::new(x, y, w, h)
    }
}

impl Widget for GridWidget {
    fn get_type(&self) -> &str {
        "grid"
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
        (self.total_width(), self.total_height())
    }

    fn get_children(&self) -> Vec<WidgetRef> {
        self.children.iter().map(|(_, r)| r.clone()).collect()
    }

    fn get_children_mut(&mut self) -> Vec<WidgetRef> {
        self.children.iter().map(|(_, r)| r.clone()).collect()
    }

    fn get_basis(&self, _direction: &Direction) -> f32 {
        0.0
    }

    fn get_children_basis(&self) -> f32 {
        0.0
    }

    fn get_fixed_width(&self) -> Option<f32> {
        Some(self.total_width())
    }

    fn get_fixed_height(&self) -> Option<f32> {
        Some(self.total_height())
    }

    fn handle_event(
        &mut self,
        event: &Event,
        ctx: &mut EventContext,
        _self_ref: &WidgetRef,
    ) -> bool {
        if let Some(point) = &event.point {
            if !self.contains_point(point) {
                return false;
            }
            // Shift the point into this widget's own coord space before
            // recursing (child rects are parent-relative).
            let origin = self.layout.as_ref().map(|r| (r.x, r.y)).unwrap_or((0.0, 0.0));
            let local_event = event.shift_point(-origin.0, -origin.1);
            // Propagate to children in reverse order (topmost first)
            for (_, child_ref) in self.children.iter().rev() {
                let mut child = child_ref.lock().expect("widget lock poisoned");
                if child.handle_event(&local_event, ctx, child_ref) {
                    return true;
                }
            }
            // Check own listeners
            if let Some(listeners) = self.event_listeners.get(&event.name) {
                for listener in listeners {
                    listener(event);
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
        _parent_direction: &Direction,
        _parent_width: f32,
        _parent_available_width: f32,
        _parent_height: f32,
        _parent_available_height: f32,
        _sibling_basis: f32,
    ) {
        let w = self.total_width();
        let h = self.total_height();
        self.layout = Some(Rect::new(cursor_x, cursor_y, w, h));

        // Layout each child at its grid-determined position, relative to
        // this widget's own origin.
        for (placement, child_ref) in &self.children {
            let cell = self.cell_rect(placement, 0.0, 0.0);
            let mut child = child_ref.lock().expect("widget lock poisoned");
            child.layout(
                ctx,
                cell.x,
                cell.y,
                &Direction::Column,
                cell.width,
                cell.width,
                cell.height,
                cell.height,
                0.0,
            );
        }
    }

    fn update(&mut self, new_widget: WidgetRef) -> UpdateResult {
        let mut new_widget = new_widget.lock().expect("widget lock poisoned");
        if let Some(new_grid) = new_widget.downcast_mut::<GridWidget>() {
            self.style = new_grid.style.clone();
            std::mem::swap(&mut self.event_listeners, &mut new_grid.event_listeners);

            // Reconcile children
            let new_children = std::mem::take(&mut new_grid.children);
            let old_iter = self.children.drain(..).collect::<Vec<_>>();

            let mut result = Vec::new();
            let mut agg = UpdateResult::layout_changed();
            for (i, (placement, new_child)) in new_children.into_iter().enumerate() {
                if i < old_iter.len() {
                    let (_, old_child) = &old_iter[i];
                    let mut child_result = {
                        let mut old = old_child.lock().expect("widget lock poisoned");
                        old.update(new_child)
                    };
                    agg.cancelled_unmount_prefixes
                        .append(&mut child_result.cancelled_unmount_prefixes);
                    agg.drained_path_prefixes
                        .append(&mut child_result.drained_path_prefixes);
                    result.push((placement, old_iter[i].1.clone()));
                } else {
                    result.push((placement, new_child));
                }
            }
            self.children = result;
            agg
        } else {
            UpdateResult::replace()
        }
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

    fn contains_point(&self, point: &Point) -> bool {
        self.layout.as_ref().is_some_and(|r| r.contains(point))
    }

    /// The skia render walker uses this to translate the canvas into the
    /// widget's own coordinate space before rendering its children.
    /// Without it, Grid children (whose cell rects are grid-local) draw
    /// against the parent's origin and stack at the top-left.
    fn get_layout_rect(&self) -> Option<&Rect> {
        self.layout.as_ref()
    }

    fn render(
        &self,
        ctx: &mut dyn RenderContext,
        _focused: bool,
        _image_cache: &mut ImageCache,
    ) {
        let layout = match &self.layout {
            Some(l) => l,
            None => return,
        };

        let content_x = layout.x + self.style.margin.left;
        let content_y = layout.y + self.style.margin.top;
        let content_w = layout.width - self.style.margin.left - self.style.margin.right;
        let content_h = layout.height - self.style.margin.top - self.style.margin.bottom;

        // Draw grid background (shows through gaps as "grid lines")
        if let Some(ref bg) = self.style.background_color {
            if self.style.corners.is_all_sharp() {
                ctx.fill_rect(content_x, content_y, content_w, content_h, bg);
            } else {
                ctx.fill_corners_rect(
                    content_x, content_y, content_w, content_h,
                    &self.style.corners, bg,
                );
            }
        }

        // Draw individual cell backgrounds
        if let Some(ref cell_color) = self.style.cell_color {
            for row in 0..self.style.rows {
                for col in 0..self.style.columns {
                    let placement = GridPlacement {
                        col, row, col_span: 1, row_span: 1,
                    };
                    let cell = self.cell_rect(&placement, layout.x, layout.y);
                    ctx.fill_rect(cell.x, cell.y, cell.width, cell.height, cell_color);
                }
            }
        }
    }
}
