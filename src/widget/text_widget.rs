use std::cell::RefCell;
use std::collections::HashMap;

use crate::widget::event::EventContext;
use crate::widget::{LayoutContext, WidgetRef};

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
use super::{UpdateResult, Widget};

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

    fn build_paragraph(&self, ctx: &LayoutContext) -> skia_safe::textlayout::Paragraph {
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
                cache.skia_text_style.set_font_families(&[family.as_str()]);
            } else if let Some(default) = ctx.default_font {
                cache.skia_text_style.set_font_families(&[default]);
            } else {
                cache.skia_text_style.set_font_families(&[] as &[&str]);
            }

            let font_collection = ctx.font_collection.unwrap_or(&cache.font_collection);

            let mut paragraph_builder =
                ParagraphBuilder::new(&cache.paragraph_style, font_collection);
            paragraph_builder.push_style(&cache.skia_text_style);
            paragraph_builder.add_text(self.text.clone());
            paragraph_builder.build()
        })
    }
}

impl Widget for TextWidget {
    fn update(&mut self, new_widget: WidgetRef) -> UpdateResult {
        let mut new_widget = new_widget.lock().expect("widget lock poisoned");
        if let Some(new_text_widget) = new_widget.downcast_mut::<TextWidget>() {
            // Text content is the dominant trigger for relayout (size/wrap
            // depends on it). Style fields don't have PartialEq across the
            // board, so we conservatively assume they changed if text
            // changed — same outcome in the common HUD case (text update
            // with stable style).
            let text_changed = self.text != new_text_widget.text;

            self.text = new_text_widget.text.clone();
            self.style = new_text_widget.style.clone();
            self.hover_style = new_text_widget.hover_style.clone();
            // Swap event listeners - we can't clone closures, so we swap them
            std::mem::swap(
                &mut self.event_listeners,
                &mut new_text_widget.event_listeners,
            );

            UpdateResult {
                absorbed: true,
                needs_layout: text_changed,
                needs_repaint: text_changed,
                cancelled_unmount_prefixes: Vec::new(),
                drained_path_prefixes: Vec::new(),
            }
        } else {
            UpdateResult::replace()
        }
    }

    fn get_type(&self) -> &str {
        "text"
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
        // If either axis is Shrink, measure the text using Skia's paragraph layout.
        let needs_measurement =
            matches!(self.style.width, Size::Shrink) || matches!(self.style.height, Size::Shrink);

        let mut measured_paragraph = if needs_measurement {
            Some(self.build_paragraph(ctx))
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
        if direction.is_row() {
            self.style.width.grow_basis()
        } else {
            self.style.height.grow_basis()
        }
    }

    fn get_children_basis(&self) -> f32 {
        0.0 // No children so no basis
    }

    fn get_fixed_width(&self) -> Option<f32> {
        self.style.width.as_fixed()
    }

    fn get_fixed_height(&self) -> Option<f32> {
        self.style.height.as_fixed()
    }

    fn handle_event(
        &mut self,
        event: &Event,
        ctx: &mut EventContext,
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
                    ctx.listener_fired = true;
                    return true; // Event was handled
                }
            }
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
        self.layout = Some(Rect::new(cursor_x, cursor_y, width, height));
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
        ctx: &mut dyn crate::widget::RenderContext,
        _focused: bool,
        _image_cache: &mut crate::widget::image::ImageCache,
    ) {
        if let Some(layout) = &self.layout {
            let style = self.effective_style();
            ctx.draw_text(&self.text, style, layout.x, layout.y, layout.width);
        }
    }
}
