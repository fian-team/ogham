use skia_safe::{
    font_style::{Slant, Weight, Width},
    textlayout::{
        FontCollection, ParagraphBuilder, ParagraphStyle, TextAlign as SkiaTextAlign, TextStyle,
    },
    Color, FontMgr, FontStyle, Paint, PaintStyle, PathBuilder, Point, RRect, Rect,
    Surface as SkiaSurface,
};
use std::sync::Arc;

use crate::tree::{
    flex_widget::FlexWidget,
    image::ImageCache,
    style::{FontWeight, TextAlign},
    svg_widget::SvgWidget,
    text_input_widget::TextInputWidget,
    text_widget::TextWidget,
    Surface, WidgetRef, UI,
};

pub struct SkiaEnv {
    pub surface: SkiaSurface,
    pub path: PathBuilder,
    pub paint: Paint,
    pub paragraph_style: ParagraphStyle,
    pub font_collection: FontCollection,
    pub text_style: TextStyle,
    pub dpi_scale: f32,
}

impl SkiaEnv {
    pub fn new(surface: SkiaSurface) -> SkiaEnv {
        Self::new_with_dpi_scale(surface, 1.0)
    }

    pub fn new_with_dpi_scale(surface: SkiaSurface, dpi_scale: f32) -> SkiaEnv {
        let path = PathBuilder::new();
        let mut paint = Paint::default();
        paint.set_color(Color::BLACK);
        paint.set_anti_alias(true);
        paint.set_stroke_width(1.0 * dpi_scale);
        let mut font_collection = FontCollection::new();
        font_collection.set_default_font_manager(FontMgr::new(), None);
        let paragraph_style = ParagraphStyle::new();
        let text_style = TextStyle::new();
        SkiaEnv {
            surface,
            path,
            paint,
            paragraph_style,
            font_collection,
            text_style,
            dpi_scale,
        }
    }

    /// Update the DPI scale factor (useful when the window is moved between displays)
    pub fn set_dpi_scale(&mut self, dpi_scale: f32) {
        self.dpi_scale = dpi_scale;
        // Update the default stroke width to account for new DPI scale
        self.paint.set_stroke_width(1.0 * dpi_scale);
    }

    /// Scale a logical coordinate to physical pixels
    #[inline]
    fn scale_coord(&self, coord: f32) -> f32 {
        coord * self.dpi_scale
    }

    /// Scale a logical dimension to physical pixels
    #[inline]
    fn scale_dim(&self, dim: f32) -> f32 {
        dim * self.dpi_scale
    }

    /// Scale a logical stroke width to physical pixels
    #[inline]
    fn scale_stroke(&self, width: f32) -> f32 {
        width * self.dpi_scale
    }

    /// Scale a logical font size to physical pixels
    #[inline]
    fn scale_font_size(&self, size: f32) -> f32 {
        size * self.dpi_scale
    }

    #[inline]
    pub fn save(&mut self) {
        self.canvas().save();
    }

    #[inline]
    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.canvas().translate((dx, dy));
    }

    #[inline]
    pub fn scale(&mut self, sx: f32, sy: f32) {
        self.canvas().scale((sx, sy));
    }

    #[inline]
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.begin_path();
        self.path.move_to((x, y));
    }

    #[inline]
    pub fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to((x, y));
    }

    #[inline]
    pub fn quad_to(&mut self, cpx: f32, cpy: f32, x: f32, y: f32) {
        self.path.quad_to((cpx, cpy), (x, y));
    }

    #[allow(dead_code)]
    #[inline]
    pub fn bezier_curve_to(&mut self, cp1x: f32, cp1y: f32, cp2x: f32, cp2y: f32, x: f32, y: f32) {
        self.path.cubic_to((cp1x, cp1y), (cp2x, cp2y), (x, y));
    }

    #[allow(dead_code)]
    #[inline]
    pub fn close_path(&mut self) {
        self.path.close();
    }

    #[inline]
    pub fn begin_path(&mut self) {
        let path = self.path.detach();
        self.surface.canvas().draw_path(&path, &self.paint);
        self.path = PathBuilder::new();
    }

    #[inline]
    pub fn stroke(&mut self) {
        self.paint.set_style(PaintStyle::Stroke);
        let path = self.path.detach();
        self.surface.canvas().draw_path(&path, &self.paint);
        self.path = PathBuilder::new();
    }

    #[inline]
    pub fn fill(&mut self) {
        self.paint.set_style(PaintStyle::Fill);
        let path = self.path.detach();
        self.surface.canvas().draw_path(&path, &self.paint);
        self.path = PathBuilder::new();
    }

    #[inline]
    pub fn set_line_width(&mut self, width: f32) {
        self.paint.set_stroke_width(self.scale_stroke(width));
    }

    // #[inline]
    // pub fn data(&mut self) -> Data {
    //     let image = self.surface.image_snapshot();
    //     let mut context = self.surface.direct_context();
    //     image
    //         .encode(context.as_mut(), EncodedImageFormat::PNG, None)
    //         .unwrap()
    // }

    #[inline]
    fn canvas(&mut self) -> &skia_safe::Canvas {
        self.surface.canvas()
    }

    fn draw_text_input_borders(
        &mut self,
        widget: &TextInputWidget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        // Draw top border
        if widget.style.border.top.width > 0.0 {
            self.paint.set_style(PaintStyle::Stroke);
            self.paint
                .set_stroke_width(self.scale_stroke(widget.style.border.top.width as f32));
            self.paint.set_color(Color::from_argb(
                widget.style.border.top.color.a,
                widget.style.border.top.color.r,
                widget.style.border.top.color.g,
                widget.style.border.top.color.b,
            ));
            self.move_to(x, y);
            self.line_to(x + width, y);
            self.stroke();
        }

        // Draw right border
        if widget.style.border.right.width > 0.0 {
            self.paint.set_style(PaintStyle::Stroke);
            self.paint
                .set_stroke_width(self.scale_stroke(widget.style.border.right.width as f32));
            self.paint.set_color(Color::from_argb(
                widget.style.border.right.color.a,
                widget.style.border.right.color.r,
                widget.style.border.right.color.g,
                widget.style.border.right.color.b,
            ));
            self.move_to(x + width, y);
            self.line_to(x + width, y + height);
            self.stroke();
        }

        // Draw bottom border
        if widget.style.border.bottom.width > 0.0 {
            self.paint.set_style(PaintStyle::Stroke);
            self.paint
                .set_stroke_width(self.scale_stroke(widget.style.border.bottom.width as f32));
            self.paint.set_color(Color::from_argb(
                widget.style.border.bottom.color.a,
                widget.style.border.bottom.color.r,
                widget.style.border.bottom.color.g,
                widget.style.border.bottom.color.b,
            ));
            self.move_to(x + width, y + height);
            self.line_to(x, y + height);
            self.stroke();
        }

        // Draw left border
        if widget.style.border.left.width > 0.0 {
            self.paint.set_style(PaintStyle::Stroke);
            self.paint
                .set_stroke_width(self.scale_stroke(widget.style.border.left.width as f32));
            self.paint.set_color(Color::from_argb(
                widget.style.border.left.color.a,
                widget.style.border.left.color.r,
                widget.style.border.left.color.g,
                widget.style.border.left.color.b,
            ));
            self.move_to(x, y + height);
            self.line_to(x, y);
            self.stroke();
        }
    }
}

impl Surface for SkiaEnv {
    fn draw(&mut self, ui: &mut UI) {
        let focused = ui.get_focused().map(|f| f.clone());
        self.draw_widget(&ui.root, focused.as_ref(), &mut ui.image_cache);
    }

    fn draw_widget(
        &mut self,
        widget: &WidgetRef,
        focused: Option<&WidgetRef>,
        image_cache: &mut ImageCache,
    ) {
        let is_focused = if let Some(ref focused) = focused {
            let focused_ptr = Arc::as_ptr(focused);
            let widget_ptr = Arc::as_ptr(widget);
            let is_eq = std::ptr::eq(focused_ptr, widget_ptr);
            is_eq
        } else {
            false
        };
        let widget = widget.lock().unwrap();
        if let Some(box_widget) = widget.downcast_ref::<FlexWidget>() {
            self.draw_box(box_widget, image_cache);
            for child in box_widget.children.iter() {
                self.draw_widget(child, focused, image_cache);
            }
        } else if let Some(text_widget) = widget.downcast_ref::<TextWidget>() {
            self.draw_text(text_widget);
        } else if let Some(text_input_widget) = widget.downcast_ref::<TextInputWidget>() {
            self.draw_text_input(text_input_widget, is_focused);
        } else if let Some(svg_widget) = widget.downcast_ref::<SvgWidget>() {
            self.draw_svg(svg_widget);
        } else {
            println!("Failed to match downcast.");
        }
    }

    fn draw_box(&mut self, widget: &FlexWidget, image_cache: &mut ImageCache) {
        if let Some(layout) = &widget.layout {
            // Inset by margin so background and borders are drawn in the border box only (margin stays transparent)
            let border_box_x = layout.x + widget.style.margin.get_left();
            let border_box_y = layout.y + widget.style.margin.get_top();
            let border_box_width =
                layout.width - widget.style.margin.get_left() - widget.style.margin.get_right();
            let border_box_height =
                layout.height - widget.style.margin.get_top() - widget.style.margin.get_bottom();

            let box_x = self.scale_coord(border_box_x);
            let box_y = self.scale_coord(border_box_y);
            let box_width = self.scale_dim(border_box_width);
            let box_height = self.scale_dim(border_box_height);

            // Draw background image if specified
            if let Some(background_image_path) = &widget.style.background_image {
                if let Some(image) = image_cache.get(background_image_path) {
                    // Create a paint for the image
                    let mut image_paint = Paint::default();
                    image_paint.set_anti_alias(true);

                    // Create a rect for the image destination
                    let image_rect = Rect::new(box_x, box_y, box_x + box_width, box_y + box_height);

                    // Draw the image
                    self.surface.canvas().draw_image_rect(
                        image,
                        None, // Use the entire source image
                        image_rect,
                        &image_paint,
                    );
                }
            }

            // Draw background color if specified (will be drawn on top of image if both are present)
            if let Some(background_color) = &widget.style.background_color {
                self.paint.set_style(PaintStyle::Fill);
                self.paint.set_color(Color::from_argb(
                    background_color.a,
                    background_color.r,
                    background_color.g,
                    background_color.b,
                ));

                // Use rounded rectangle if corner radii are specified
                if widget.style.corner_radii.top_left > 0.0
                    || widget.style.corner_radii.top_right > 0.0
                    || widget.style.corner_radii.bottom_left > 0.0
                    || widget.style.corner_radii.bottom_right > 0.0
                {
                    let rect = Rect::new(box_x, box_y, box_x + box_width, box_y + box_height);
                    let rounded_rect = RRect::new_rect_radii(
                        rect,
                        &[
                            Point::new(
                                self.scale_dim(widget.style.corner_radii.top_left),
                                self.scale_dim(widget.style.corner_radii.top_left),
                            ),
                            Point::new(
                                self.scale_dim(widget.style.corner_radii.top_right),
                                self.scale_dim(widget.style.corner_radii.top_right),
                            ),
                            Point::new(
                                self.scale_dim(widget.style.corner_radii.bottom_right),
                                self.scale_dim(widget.style.corner_radii.bottom_right),
                            ),
                            Point::new(
                                self.scale_dim(widget.style.corner_radii.bottom_left),
                                self.scale_dim(widget.style.corner_radii.bottom_left),
                            ),
                        ],
                    );
                    self.surface.canvas().draw_rrect(rounded_rect, &self.paint);
                } else {
                    self.surface.canvas().draw_rect(
                        Rect::new(box_x, box_y, box_x + box_width, box_y + box_height),
                        &self.paint,
                    );
                }
            }

            // Draw borders
            self.draw_borders(widget, box_x, box_y, box_width, box_height);

            // Draw debug outlines
            // self.draw_debug_outlines(widget, layout);
        }
    }

    fn draw_borders(&mut self, widget: &FlexWidget, x: f32, y: f32, width: f32, height: f32) {
        // Check if any border has width > 0
        let has_border = widget.style.border.top.width > 0.0
            || widget.style.border.right.width > 0.0
            || widget.style.border.bottom.width > 0.0
            || widget.style.border.left.width > 0.0;

        if !has_border {
            return;
        }

        // For now, use the top border properties for the entire border
        // TODO: Support different border styles per side
        let border_width = widget.style.border.top.width;
        let border_color = widget.style.border.top.color;

        self.paint.set_style(PaintStyle::Stroke);
        self.paint.set_stroke_width(self.scale_stroke(border_width));
        self.paint.set_color(Color::from_argb(
            border_color.a,
            border_color.r,
            border_color.g,
            border_color.b,
        ));

        // Use rounded rectangle if corner radii are specified
        if widget.style.corner_radii.top_left > 0.0
            || widget.style.corner_radii.top_right > 0.0
            || widget.style.corner_radii.bottom_left > 0.0
            || widget.style.corner_radii.bottom_right > 0.0
        {
            let rect = Rect::new(x, y, x + width, y + height);
            let rounded_rect = RRect::new_rect_radii(
                rect,
                &[
                    Point::new(
                        self.scale_dim(widget.style.corner_radii.top_left),
                        self.scale_dim(widget.style.corner_radii.top_left),
                    ),
                    Point::new(
                        self.scale_dim(widget.style.corner_radii.top_right),
                        self.scale_dim(widget.style.corner_radii.top_right),
                    ),
                    Point::new(
                        self.scale_dim(widget.style.corner_radii.bottom_right),
                        self.scale_dim(widget.style.corner_radii.bottom_right),
                    ),
                    Point::new(
                        self.scale_dim(widget.style.corner_radii.bottom_left),
                        self.scale_dim(widget.style.corner_radii.bottom_left),
                    ),
                ],
            );
            self.surface.canvas().draw_rrect(rounded_rect, &self.paint);
        } else {
            // Draw individual border sides for rectangular borders
            self.draw_rectangular_borders(widget, x, y, width, height);
        }
    }

    fn draw_text(&mut self, widget: &TextWidget) {
        if let Some(layout) = &widget.layout {
            self.paint.set_style(PaintStyle::Fill);
            // Use the text color from the style if available, otherwise default to black
            let color = widget.style.get_color();
            self.paint
                .set_color(Color::from_argb(color.a, color.r, color.g, color.b));
            self.text_style.set_foreground_paint(&self.paint);
            self.text_style
                .set_font_size(self.scale_font_size(widget.style.get_size()));
            self.text_style.set_font_style(FontStyle::new(
                match widget.style.get_weight() {
                    FontWeight::Normal => Weight::NORMAL,
                    FontWeight::SemiBold => Weight::SEMI_BOLD,
                    FontWeight::Bold => Weight::BOLD,
                    FontWeight::Light => Weight::LIGHT,
                },
                Width::NORMAL,
                Slant::Upright,
            ));
            self.paragraph_style
                .set_text_align(match widget.style.get_align() {
                    TextAlign::Left => SkiaTextAlign::Left,
                    TextAlign::Center => SkiaTextAlign::Center,
                    TextAlign::Right => SkiaTextAlign::Right,
                });
            let mut paragraph_builder =
                ParagraphBuilder::new(&self.paragraph_style, &self.font_collection);
            paragraph_builder.push_style(&self.text_style);
            paragraph_builder.add_text(widget.text.clone());
            let mut paragraph = paragraph_builder.build();
            paragraph.layout(self.scale_dim(layout.width));
            let scaled_x = self.scale_coord(layout.x);
            let scaled_y = self.scale_coord(layout.y);
            paragraph.paint(self.canvas(), Point::new(scaled_x, scaled_y));
        }
    }

    fn draw_text_input(&mut self, widget: &TextInputWidget) {
        if let Some(layout) = &widget.layout {
            let x = self.scale_coord(layout.x);
            let y = self.scale_coord(layout.y);
            let width = self.scale_dim(layout.width);
            let height = self.scale_dim(layout.height);

            // Draw background if specified
            if let Some(background_color) = &widget.style.background_color {
                self.paint.set_style(PaintStyle::Fill);
                self.paint.set_color(Color::from_argb(
                    background_color.a,
                    background_color.r,
                    background_color.g,
                    background_color.b,
                ));
                self.surface
                    .canvas()
                    .draw_rect(Rect::new(x, y, x + width, y + height), &self.paint);
            }

            // Draw borders
            self.draw_text_input_borders(widget, x, y, width, height);

            // Draw the text content
            self.paint.set_style(PaintStyle::Fill);
            let text_color = widget
                .style
                .text_color
                .unwrap_or(crate::tree::style::Color::new(0, 0, 0, 255));
            self.paint.set_color(Color::from_argb(
                text_color.a,
                text_color.r,
                text_color.g,
                text_color.b,
            ));
            self.text_style.set_foreground_paint(&self.paint);

            let text_size = widget.style.text_size.unwrap_or(16.0);
            self.text_style.set_font_size(text_size);
            self.text_style.set_font_style(FontStyle::new(
                Weight::NORMAL,
                Width::NORMAL,
                Slant::Upright,
            ));

            self.paragraph_style.set_text_align(SkiaTextAlign::Left);
            let mut paragraph_builder =
                ParagraphBuilder::new(&self.paragraph_style, &self.font_collection);
            paragraph_builder.push_style(&self.text_style);

            // Display the text value
            let display_text = if widget.value.is_empty() {
                "".to_string()
            } else {
                widget.value.clone()
            };

            paragraph_builder.add_text(display_text);
            let mut paragraph = paragraph_builder.build();

            paragraph.layout(self.scale_dim(layout.width) - 8.0); // Account for padding

            let text_x = self.scale_coord(layout.x) + 4.0; // Left padding
            let text_y = self.scale_coord(layout.y) + text_size * 0.8; // Adjust for baseline
            paragraph.paint(self.canvas(), Point::new(text_x, text_y - text_size));

            // Draw cursor if focused
            // if widget.is_focused() {
            //     let cursor_x = text_x + paragraph.longest_line();
            //     let cursor_y = self.scale_coord(layout.y);
            //     let cursor_height = self.scale_dim(layout.height);

            //     self.paint.set_style(PaintStyle::Fill);
            //     self.paint.set_color(Color::from_argb(255, 0, 0, 0)); // Black cursor
            //     self.surface.canvas().draw_rect(
            //         Rect::new(cursor_x, cursor_y, cursor_x + 1.0, cursor_y + cursor_height),
            //         &self.paint,
            //     );
            // }
        }
    }

    fn draw_svg(&mut self, widget: &SvgWidget) {
        if let Some(layout) = &widget.layout {
            let x = self.scale_coord(layout.x);
            let y = self.scale_coord(layout.y);
            let width = self.scale_dim(layout.width);
            let height = self.scale_dim(layout.height);

            // Save the canvas state
            self.save();

            // Translate to the widget position
            self.translate(x, y);

            // Scale the SVG to fit the widget dimensions
            // The SVG will be rendered in its own coordinate space
            if let Some(dom) = &widget.svg_dom {
                // Set the container size to the widget dimensions
                // This tells the SVG how large its viewport should be
                // Note: We need to clone to get a mutable reference since the widget's dom is immutable
                let mut dom_mut = dom.clone();
                dom_mut.set_container_size((width, height));

                // Render the SVG (color is already applied to the DOM during loading)
                dom_mut.render(self.canvas());
            }
            // If svg_dom is None, we just leave it blank (no rendering)

            // Restore the canvas state
            self.canvas().restore();
        }
    }
}

impl SkiaEnv {
    fn draw_rectangular_borders(
        &mut self,
        widget: &FlexWidget,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) {
        // Draw top border
        if widget.style.border.top.width > 0.0 {
            self.paint.set_style(PaintStyle::Stroke);
            self.paint
                .set_stroke_width(self.scale_stroke(widget.style.border.top.width as f32));
            self.paint.set_color(Color::from_argb(
                widget.style.border.top.color.a,
                widget.style.border.top.color.r,
                widget.style.border.top.color.g,
                widget.style.border.top.color.b,
            ));
            self.move_to(x, y);
            self.line_to(x + width, y);
            self.stroke();
        }

        // Draw right border
        if widget.style.border.right.width > 0.0 {
            self.paint.set_style(PaintStyle::Stroke);
            self.paint
                .set_stroke_width(self.scale_stroke(widget.style.border.right.width as f32));
            self.paint.set_color(Color::from_argb(
                widget.style.border.right.color.a,
                widget.style.border.right.color.r,
                widget.style.border.right.color.g,
                widget.style.border.right.color.b,
            ));
            self.move_to(x + width, y);
            self.line_to(x + width, y + height);
            self.stroke();
        }

        // Draw bottom border
        if widget.style.border.bottom.width > 0.0 {
            self.paint.set_style(PaintStyle::Stroke);
            self.paint
                .set_stroke_width(self.scale_stroke(widget.style.border.bottom.width as f32));
            self.paint.set_color(Color::from_argb(
                widget.style.border.bottom.color.a,
                widget.style.border.bottom.color.r,
                widget.style.border.bottom.color.g,
                widget.style.border.bottom.color.b,
            ));
            self.move_to(x + width, y + height);
            self.line_to(x, y + height);
            self.stroke();
        }

        // Draw left border
        if widget.style.border.left.width > 0.0 {
            self.paint.set_style(PaintStyle::Stroke);
            self.paint
                .set_stroke_width(self.scale_stroke(widget.style.border.left.width as f32));
            self.paint.set_color(Color::from_argb(
                widget.style.border.left.color.a,
                widget.style.border.left.color.r,
                widget.style.border.left.color.g,
                widget.style.border.left.color.b,
            ));
            self.move_to(x, y + height);
            self.line_to(x, y);
            self.stroke();
        }
    }

    fn draw_text(&mut self, widget: &TextWidget) {
        if let Some(layout) = &widget.layout {
            self.paint.set_style(PaintStyle::Fill);
            // Use the text color from the style if available, otherwise default to black
            let color = widget.style.get_color();
            self.paint
                .set_color(Color::from_argb(color.a, color.r, color.g, color.b));
            self.text_style.set_foreground_paint(&self.paint);
            self.text_style
                .set_font_size(self.scale_font_size(widget.style.get_size()));
            self.text_style.set_font_style(FontStyle::new(
                match widget.style.get_weight() {
                    FontWeight::Normal => Weight::NORMAL,
                    FontWeight::SemiBold => Weight::SEMI_BOLD,
                    FontWeight::Bold => Weight::BOLD,
                    FontWeight::Light => Weight::LIGHT,
                },
                Width::NORMAL,
                Slant::Upright,
            ));
            self.paragraph_style
                .set_text_align(match widget.style.get_align() {
                    TextAlign::Left => SkiaTextAlign::Left,
                    TextAlign::Center => SkiaTextAlign::Center,
                    TextAlign::Right => SkiaTextAlign::Right,
                });
            let mut paragraph_builder =
                ParagraphBuilder::new(&self.paragraph_style, &self.font_collection);
            paragraph_builder.push_style(&self.text_style);
            paragraph_builder.add_text(widget.text.clone());
            let mut paragraph = paragraph_builder.build();
            let scaled_width = self.scale_dim(layout.width);
            paragraph.layout(f32::INFINITY);
            let intrinsic = paragraph.max_intrinsic_width();
            // If layout width is effectively unconstrained, keep single-line layout to avoid
            // last-character wrap (max_intrinsic_width can be slightly under the true width).
            if scaled_width < intrinsic - 0.5 {
                paragraph.layout(scaled_width);
            }
            let scaled_x = self.scale_coord(layout.x);
            let scaled_y = self.scale_coord(layout.y);
            paragraph.paint(self.canvas(), Point::new(scaled_x, scaled_y));
        }
    }

    fn draw_text_input(&mut self, widget: &TextInputWidget, is_focused: bool) {
        if let Some(layout) = &widget.layout {
            // Draw background if specified
            if let Some(background_color) = &widget.style.background_color {
                self.paint.set_style(PaintStyle::Fill);
                self.paint.set_color(Color::from_argb(
                    background_color.a,
                    background_color.r,
                    background_color.g,
                    background_color.b,
                ));
            } else {
                // Default background for text input
                self.paint.set_style(PaintStyle::Fill);
                self.paint.set_color(Color::WHITE);
            }

            let box_x = self.scale_coord(layout.x + widget.style.margin.get_left());
            let box_y = self.scale_coord(layout.y + widget.style.margin.get_top());
            let box_width = self.scale_dim(
                layout.width - widget.style.margin.get_left() - widget.style.margin.get_right(),
            );
            let box_height = self.scale_dim(
                layout.height - widget.style.margin.get_top() - widget.style.margin.get_bottom(),
            );

            // Pre-calculate all scaled values to avoid borrowing conflicts
            let text_size = self.scale_font_size(widget.text_style.size);
            let padding_left = self.scale_dim(widget.style.padding.get_left());
            let padding_right = self.scale_dim(widget.style.padding.get_right());
            let padding_top = self.scale_dim(widget.style.padding.get_top());

            self.surface.canvas().draw_rect(
                Rect::new(box_x, box_y, box_x + box_width, box_y + box_height),
                &self.paint,
            );

            // Draw borders (similar to box widget)
            self.draw_text_input_borders(widget, box_x, box_y, box_width, box_height);

            // Draw the text content
            self.paint.set_style(PaintStyle::Fill);
            let text_color = widget.text_style.color;
            self.paint.set_color(Color::from_argb(
                text_color.a,
                text_color.r,
                text_color.g,
                text_color.b,
            ));
            self.text_style.set_foreground_paint(&self.paint);

            self.text_style.set_font_size(text_size);
            self.text_style.set_font_style(FontStyle::new(
                match widget.text_style.weight {
                    crate::tree::style::FontWeight::Normal => Weight::NORMAL,
                    crate::tree::style::FontWeight::SemiBold => Weight::SEMI_BOLD,
                    crate::tree::style::FontWeight::Bold => Weight::BOLD,
                    crate::tree::style::FontWeight::Light => Weight::LIGHT,
                },
                Width::NORMAL,
                Slant::Upright,
            ));

            self.paragraph_style
                .set_text_align(match widget.text_style.align {
                    crate::tree::style::TextAlign::Left => SkiaTextAlign::Left,
                    crate::tree::style::TextAlign::Center => SkiaTextAlign::Center,
                    crate::tree::style::TextAlign::Right => SkiaTextAlign::Right,
                });
            let mut paragraph_builder =
                ParagraphBuilder::new(&self.paragraph_style, &self.font_collection);
            paragraph_builder.push_style(&self.text_style);

            // Display the text value
            let display_text = if widget.value.is_empty() {
                "".to_string()
            } else {
                widget.value.clone()
            };

            paragraph_builder.add_text(display_text);
            let mut paragraph = paragraph_builder.build();

            paragraph.layout(box_width - padding_left - padding_right);

            let text_x = box_x + padding_left;
            let text_y = box_y + padding_top + text_size * 0.8; // Adjust for baseline
            paragraph.paint(self.canvas(), Point::new(text_x, text_y - text_size));

            // Draw cursor if focused
            if is_focused {
                self.paint.set_style(PaintStyle::Stroke);
                self.paint.set_color(Color::BLACK);
                self.paint.set_stroke_width(self.scale_stroke(1.0));

                // Calculate cursor position (simplified - assumes monospace font)
                let char_width = text_size * 0.55; // Approximate character width
                let cursor_x = text_x + (widget.cursor_position as f32 * char_width);
                let cursor_y1 = text_y - text_size;
                let cursor_y2 = text_y;

                self.move_to(cursor_x, cursor_y1);
                self.line_to(cursor_x, cursor_y2);
                self.stroke();
            }
        }
    }
}

impl SkiaEnv {
    /// Draw debug outlines for FlexWidget boundaries
    /// - Red outline: margin area
    /// - Blue outline: border area  
    /// - Green outline: padding area
    /// - Purple outline: content area
    pub fn draw_debug_outlines(&mut self, widget: &FlexWidget, layout: &crate::tree::rect::Rect) {
        // Calculate all the boundary coordinates
        let margin_left = self.scale_coord(widget.style.margin.get_left());
        let margin_top = self.scale_coord(widget.style.margin.get_top());
        let margin_right = self.scale_coord(widget.style.margin.get_right());
        let margin_bottom = self.scale_coord(widget.style.margin.get_bottom());

        let border_left = self.scale_coord(widget.style.border.get_left());
        let border_top = self.scale_coord(widget.style.border.get_top());
        let border_right = self.scale_coord(widget.style.border.get_right());
        let border_bottom = self.scale_coord(widget.style.border.get_bottom());

        let padding_left = self.scale_coord(widget.style.padding.get_left());
        let padding_top = self.scale_coord(widget.style.padding.get_top());
        let padding_right = self.scale_coord(widget.style.padding.get_right());
        let padding_bottom = self.scale_coord(widget.style.padding.get_bottom());

        // Calculate boundary rectangles
        let layout_x = self.scale_coord(layout.x);
        let layout_y = self.scale_coord(layout.y);
        let layout_width = self.scale_dim(layout.width);
        let layout_height = self.scale_dim(layout.height);

        // Margin area (red outline) - outermost boundary
        let margin_x = layout_x;
        let margin_y = layout_y;
        let margin_width = layout_width;
        let margin_height = layout_height;

        // Border area (blue outline) - inside margin
        let border_x = margin_x + margin_left;
        let border_y = margin_y + margin_top;
        let border_width = margin_width - margin_left - margin_right;
        let border_height = margin_height - margin_top - margin_bottom;

        // Padding area (green outline) - inside border
        let padding_x = border_x + border_left;
        let padding_y = border_y + border_top;
        let padding_width = border_width - border_left - border_right;
        let padding_height = border_height - border_top - border_bottom;

        // Content area (purple outline) - inside padding
        let content_x = padding_x + padding_left;
        let content_y = padding_y + padding_top;
        let content_width = padding_width - padding_left - padding_right;
        let content_height = padding_height - padding_top - padding_bottom;

        // Set up stroke style for debug outlines
        self.paint.set_style(PaintStyle::Stroke);
        self.paint.set_anti_alias(true);
        self.paint.set_stroke_width(self.scale_stroke(2.0));

        // Draw margin outline (red)
        if margin_left > 0.0 || margin_top > 0.0 || margin_right > 0.0 || margin_bottom > 0.0 {
            self.paint.set_color(Color::RED);
            self.move_to(margin_x, margin_y);
            self.line_to(margin_x + margin_width, margin_y);
            self.line_to(margin_x + margin_width, margin_y + margin_height);
            self.line_to(margin_x, margin_y + margin_height);
            self.line_to(margin_x, margin_y);
            self.stroke();
        }

        // Draw border outline (blue)
        if border_left > 0.0 || border_top > 0.0 || border_right > 0.0 || border_bottom > 0.0 {
            self.paint.set_color(Color::BLUE);
            self.move_to(border_x, border_y);
            self.line_to(border_x + border_width, border_y);
            self.line_to(border_x + border_width, border_y + border_height);
            self.line_to(border_x, border_y + border_height);
            self.line_to(border_x, border_y);
            self.stroke();
        }

        // Draw padding outline (green)
        if padding_left > 0.0 || padding_top > 0.0 || padding_right > 0.0 || padding_bottom > 0.0 {
            self.paint.set_color(Color::GREEN);
            self.move_to(padding_x, padding_y);
            self.line_to(padding_x + padding_width, padding_y);
            self.line_to(padding_x + padding_width, padding_y + padding_height);
            self.line_to(padding_x, padding_y + padding_height);
            self.line_to(padding_x, padding_y);
            self.stroke();
        }

        // Draw content outline (purple)
        if content_width > 0.0 && content_height > 0.0 {
            self.paint.set_color(Color::from_argb(255, 128, 0, 128)); // Purple
            self.move_to(content_x, content_y);
            self.line_to(content_x + content_width, content_y);
            self.line_to(content_x + content_width, content_y + content_height);
            self.line_to(content_x, content_y + content_height);
            self.line_to(content_x, content_y);
            self.stroke();
        }
    }
}
