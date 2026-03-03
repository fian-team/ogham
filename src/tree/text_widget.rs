use std::cell::RefCell;
use std::collections::HashMap;

use crate::tree::event::EventContext;
use crate::tree::WidgetRef;

use skia_safe::{
    font_style::{Slant, Weight, Width},
    textlayout::{
        FontCollection, ParagraphBuilder, ParagraphStyle, TextAlign as SkiaTextAlign,
        TextStyle as SkiaTextStyle,
    },
    FontMgr, FontStyle, Paint,
};

use super::event::*;
use super::point::*;
use super::rect::*;
use super::style::*;
use super::Widget;

struct TextLayoutCache {
    font_collection: FontCollection,
    paragraph_style: ParagraphStyle,
    skia_text_style: SkiaTextStyle,
}

thread_local! {
    static TEXT_LAYOUT_CACHE: RefCell<TextLayoutCache> = RefCell::new({
        let mut font_collection = FontCollection::new();
        font_collection.set_default_font_manager(FontMgr::new(), None);
        TextLayoutCache {
            font_collection,
            paragraph_style: ParagraphStyle::new(),
            skia_text_style: SkiaTextStyle::new(),
        }
    });
}

pub struct TextWidget {
    pub text: String,
    pub event_listeners: HashMap<String, Vec<Box<dyn Fn(&Event)>>>,
    pub style: TextStyle,
    pub hover_style: Option<TextStyle>,
    pub hovered: bool,
    pub layout: Option<Rect>,
}

impl TextWidget {
    pub fn new(text: String) -> Self {
        Self {
            text,
            event_listeners: HashMap::new(),
            style: TextStyle::default(),
            hover_style: None,
            hovered: false,
            layout: None,
        }
    }

    pub fn with_color(text: String, color: Color) -> Self {
        Self {
            text,
            event_listeners: HashMap::new(),
            style: TextStyle::default().with_color(color),
            hover_style: None,
            hovered: false,
            layout: None,
        }
    }

    /// Returns the style to use for rendering. When the widget is hovered and
    /// a pre-merged `hover_style` is set, returns it; otherwise returns the
    /// base style.
    pub fn effective_style(&self) -> &TextStyle {
        if self.hovered {
            if let Some(ref s) = self.hover_style {
                return s;
            }
        }
        &self.style
    }

    fn build_paragraph(&self) -> skia_safe::textlayout::Paragraph {
        use crate::tree::{with_active_default_font, with_active_font_collection};

        TEXT_LAYOUT_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();

            cache
                .paragraph_style
                .set_text_align(match self.style.get_align() {
                    TextAlign::Left => SkiaTextAlign::Left,
                    TextAlign::Center => SkiaTextAlign::Center,
                    TextAlign::Right => SkiaTextAlign::Right,
                });

            let c = self.style.get_color();
            let mut paint = Paint::default();
            paint.set_color(skia_safe::Color::from_argb(c.a, c.r, c.g, c.b));
            cache.skia_text_style.set_foreground_paint(&paint);
            cache.skia_text_style.set_font_size(self.style.get_size());
            cache.skia_text_style.set_font_style(FontStyle::new(
                match self.style.get_weight() {
                    FontWeight::Normal => Weight::NORMAL,
                    FontWeight::SemiBold => Weight::SEMI_BOLD,
                    FontWeight::Bold => Weight::BOLD,
                    FontWeight::Light => Weight::LIGHT,
                },
                Width::NORMAL,
                Slant::Upright,
            ));

            if let Some(ref family) = self.style.font {
                cache
                    .skia_text_style
                    .set_font_families(&[family.as_str()]);
            } else if let Some(default) = with_active_default_font(|f| f.to_string()) {
                cache
                    .skia_text_style
                    .set_font_families(&[default.as_str()]);
            } else {
                cache.skia_text_style.set_font_families(&[] as &[&str]);
            }

            let fc = with_active_font_collection(|fc| fc.clone());
            let font_collection = fc.as_ref().unwrap_or(&cache.font_collection);

            let mut paragraph_builder =
                ParagraphBuilder::new(&cache.paragraph_style, font_collection);
            paragraph_builder.push_style(&cache.skia_text_style);
            paragraph_builder.add_text(self.text.clone());
            paragraph_builder.build()
        })
    }
}

impl Widget for TextWidget {
    fn update(&mut self, new_widget: WidgetRef) -> bool {
        let mut new_widget = new_widget.lock().expect("widget lock poisoned");
        if let Some(new_text_widget) = new_widget.downcast_mut::<TextWidget>() {
            self.text = new_text_widget.text.clone();
            self.style = new_text_widget.style.clone();
            self.hover_style = new_text_widget.hover_style.clone();
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
        // If either axis is Shrink, measure the text using Skia's paragraph layout.
        let needs_measurement =
            matches!(self.style.width, Size::Shrink) || matches!(self.style.height, Size::Shrink);

        let mut measured_paragraph = if needs_measurement {
            Some(self.build_paragraph())
        } else {
            None
        };

        // Skia populates max_intrinsic_width during layout(); without this, max_intrinsic_width() returns 0.
        if let Some(ref mut p) = measured_paragraph {
            p.layout(f32::INFINITY);
        }

        // First compute the width this widget is allowed to use.
        let width = match self.style.width {
            Size::Fixed(w) => w,
            Size::Shrink => {
                if needs_measurement {
                    let intrinsic = measured_paragraph
                        .as_ref()
                        .expect("paragraph must exist when measuring")
                        .max_intrinsic_width();
                    if parent_available_width > 0.0 {
                        intrinsic.min(parent_available_width)
                    } else {
                        intrinsic
                    }
                } else {
                    0.0
                }
            }
            Size::Grow(basis) => {
                if parent_direction.is_row() {
                    parent_direction.get_grow_size(basis, sibling_basis, parent_available_width)
                } else {
                    parent_width
                }
            }
            Size::Percent(_) => 0.0, // Calculated during layout (not currently supported for Text)
        };

        let height = match self.style.height {
            Size::Fixed(h) => h,
            Size::Shrink => {
                if needs_measurement {
                    let paragraph = measured_paragraph
                        .as_mut()
                        .expect("paragraph must exist when measuring");
                    // Height depends on layout width due to wrapping. We already laid out with
                    // INFINITY above, which gives a single line. Only re-layout when we're
                    // constrained by the parent (width < intrinsic); otherwise max_intrinsic_width
                    // can be slightly under the true one-line width and layout(width) would wrap the
                    // last character.
                    let intrinsic = paragraph.max_intrinsic_width();
                    if width < intrinsic - 0.5 {
                        let layout_width = width.max(0.0);
                        paragraph.layout(layout_width);
                    }
                    paragraph.height()
                } else {
                    0.0
                }
            }
            Size::Grow(basis) => {
                if parent_direction.is_row() {
                    parent_height
                } else {
                    parent_direction.get_grow_size(basis, sibling_basis, parent_available_height)
                }
            }
            Size::Percent(_) => 0.0, // Calculated during layout (not currently supported for Text)
        };

        (width, height)
    }

    fn get_children(&self) -> Vec<WidgetRef> {
        Vec::new() // Text widgets have no children
    }

    fn get_basis(&self, direction: &Direction) -> f32 {
        match direction {
            Direction::Row | Direction::RowReverse => match self.style.width {
                Size::Fixed(_) => 0.0,
                Size::Shrink => 0.0,
                Size::Grow(basis) => basis,
                Size::Percent(_) => 0.0,
            },
            Direction::Column | Direction::ColumnReverse => match self.style.height {
                Size::Fixed(_) => 0.0,
                Size::Shrink => 0.0,
                Size::Grow(basis) => basis,
                Size::Percent(_) => 0.0,
            },
        }
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
        self.layout = Some(Rect::new(cursor_x, cursor_y, width, height));
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

    fn set_hovered(&mut self, hovered: bool) {
        self.hovered = hovered;
    }

    fn is_hovered(&self) -> bool {
        self.hovered
    }
}
