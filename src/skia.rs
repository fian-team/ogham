//! Skia rendering backend: draws the widget tree to a Skia surface.

use skia_safe::{
    font_style::{Slant, Weight, Width},
    textlayout::{
        FontCollection, ParagraphBuilder, ParagraphStyle, TextAlign as SkiaTextAlign, TextStyle,
    },
    Color, FontMgr, FontStyle, Paint, PaintStyle, PathBuilder, Point, RRect, Rect,
    Surface as SkiaSurface,
};
use std::sync::Arc;

use crate::widget::{
    flex_widget::FlexWidget,
    image::ImageCache,
    style::{Border, BorderSide, CornerRadii, FontWeight, TextAlign},
    RenderContext, Surface, WidgetRef, UI,
};

pub struct SkiaEnv {
    pub surface: SkiaSurface,
    pub path: PathBuilder,
    pub paint: Paint,
    pub paragraph_style: ParagraphStyle,
    pub font_collection: FontCollection,
    pub text_style: TextStyle,
    pub dpi_scale: f32,
    pub default_font: Option<String>,
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
            default_font: None,
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

    /// Configures `self.paint` and `self.text_style` from an Ogham `TextStyle`,
    /// and sets the paragraph text alignment. Returns the DPI-scaled font size.
    fn apply_text_style(&mut self, style: &crate::widget::style::TextStyle) -> f32 {
        self.paint.set_style(PaintStyle::Fill);
        let color = style.get_color();
        self.paint
            .set_color(Color::from_argb(color.a, color.r, color.g, color.b));
        self.text_style.set_foreground_paint(&self.paint);
        let scaled_font_size = self.scale_font_size(style.get_size());
        self.text_style.set_font_size(scaled_font_size);
        self.text_style.set_font_style(FontStyle::new(
            match style.get_weight() {
                FontWeight::Normal => Weight::NORMAL,
                FontWeight::SemiBold => Weight::SEMI_BOLD,
                FontWeight::Bold => Weight::BOLD,
                FontWeight::Light => Weight::LIGHT,
            },
            Width::NORMAL,
            Slant::Upright,
        ));
        if let Some(ref family) = style.font {
            self.text_style.set_font_families(&[family.as_str()]);
        } else if let Some(ref default) = self.default_font {
            self.text_style.set_font_families(&[default.as_str()]);
        } else {
            self.text_style.set_font_families(&[] as &[&str]);
        }
        self.paragraph_style
            .set_text_align(match style.get_align() {
                TextAlign::Left => SkiaTextAlign::Left,
                TextAlign::Center => SkiaTextAlign::Center,
                TextAlign::Right => SkiaTextAlign::Right,
            });
        scaled_font_size
    }

    /// Constructs an `RRect` with DPI-scaled corner radii.
    fn make_scaled_rrect(&self, rect: Rect, radii: &CornerRadii) -> RRect {
        RRect::new_rect_radii(
            rect,
            &[
                Point::new(
                    self.scale_dim(radii.top_left),
                    self.scale_dim(radii.top_left),
                ),
                Point::new(
                    self.scale_dim(radii.top_right),
                    self.scale_dim(radii.top_right),
                ),
                Point::new(
                    self.scale_dim(radii.bottom_right),
                    self.scale_dim(radii.bottom_right),
                ),
                Point::new(
                    self.scale_dim(radii.bottom_left),
                    self.scale_dim(radii.bottom_left),
                ),
            ],
        )
    }

    /// Draws a single border line if the side has a non-zero width.
    fn draw_border_line(&mut self, side: &BorderSide, x1: f32, y1: f32, x2: f32, y2: f32) {
        if side.width > 0.0 {
            self.paint.set_style(PaintStyle::Stroke);
            self.paint
                .set_stroke_width(self.scale_stroke(side.width));
            self.paint.set_color(Color::from_argb(
                side.color.a,
                side.color.r,
                side.color.g,
                side.color.b,
            ));
            self.move_to(x1, y1);
            self.line_to(x2, y2);
            self.stroke();
        }
    }

    /// Draws rectangular borders (top, right, bottom, left) inset by half their stroke widths.
    fn draw_border_sides(&mut self, border: &Border, x: f32, y: f32, width: f32, height: f32) {
        let half_top = self.scale_stroke(border.top.width) / 2.0;
        let half_right = self.scale_stroke(border.right.width) / 2.0;
        let half_bottom = self.scale_stroke(border.bottom.width) / 2.0;
        let half_left = self.scale_stroke(border.left.width) / 2.0;

        self.draw_border_line(
            &border.top,
            x + half_left,
            y + half_top,
            x + width - half_right,
            y + half_top,
        );
        self.draw_border_line(
            &border.right,
            x + width - half_right,
            y + half_top,
            x + width - half_right,
            y + height - half_bottom,
        );
        self.draw_border_line(
            &border.bottom,
            x + width - half_right,
            y + height - half_bottom,
            x + half_left,
            y + height - half_bottom,
        );
        self.draw_border_line(
            &border.left,
            x + half_left,
            y + height - half_bottom,
            x + half_left,
            y + half_top,
        );
    }

}

impl RenderContext for SkiaEnv {
    fn fill_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: &crate::widget::style::Color,
    ) {
        let sx = self.scale_coord(x);
        let sy = self.scale_coord(y);
        let sw = self.scale_dim(w);
        let sh = self.scale_dim(h);
        self.paint.set_style(PaintStyle::Fill);
        self.paint
            .set_color(Color::from_argb(color.a, color.r, color.g, color.b));
        self.surface
            .canvas()
            .draw_rect(Rect::new(sx, sy, sx + sw, sy + sh), &self.paint);
    }

    fn fill_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: &CornerRadii,
        color: &crate::widget::style::Color,
    ) {
        let sx = self.scale_coord(x);
        let sy = self.scale_coord(y);
        let sw = self.scale_dim(w);
        let sh = self.scale_dim(h);
        self.paint.set_style(PaintStyle::Fill);
        self.paint
            .set_color(Color::from_argb(color.a, color.r, color.g, color.b));
        let rect = Rect::new(sx, sy, sx + sw, sy + sh);
        let rrect = self.make_scaled_rrect(rect, radii);
        self.surface.canvas().draw_rrect(rrect, &self.paint);
    }

    fn draw_border(
        &mut self,
        border: &Border,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: &CornerRadii,
    ) {
        let has_border = border.top.width > 0.0
            || border.right.width > 0.0
            || border.bottom.width > 0.0
            || border.left.width > 0.0;
        if !has_border {
            return;
        }

        let sx = self.scale_coord(x);
        let sy = self.scale_coord(y);
        let sw = self.scale_dim(w);
        let sh = self.scale_dim(h);

        let has_radii = radii.top_left > 0.0
            || radii.top_right > 0.0
            || radii.bottom_left > 0.0
            || radii.bottom_right > 0.0;

        if has_radii {
            let border_width = border.top.width;
            let border_color = border.top.color;
            self.paint.set_style(PaintStyle::Stroke);
            self.paint
                .set_stroke_width(self.scale_stroke(border_width));
            self.paint.set_color(Color::from_argb(
                border_color.a,
                border_color.r,
                border_color.g,
                border_color.b,
            ));
            let half = self.scale_stroke(border_width) / 2.0;
            let rect = Rect::new(sx + half, sy + half, sx + sw - half, sy + sh - half);
            let rrect = self.make_scaled_rrect(rect, radii);
            self.surface.canvas().draw_rrect(rrect, &self.paint);
        } else {
            self.draw_border_sides(border, sx, sy, sw, sh);
        }
    }

    fn draw_image(
        &mut self,
        path: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        image_cache: &mut ImageCache,
    ) {
        if let Some(image) = image_cache.get(path) {
            let sx = self.scale_coord(x);
            let sy = self.scale_coord(y);
            let sw = self.scale_dim(w);
            let sh = self.scale_dim(h);
            let mut image_paint = Paint::default();
            image_paint.set_anti_alias(true);
            let image_rect = Rect::new(sx, sy, sx + sw, sy + sh);
            self.surface
                .canvas()
                .draw_image_rect(image, None, image_rect, &image_paint);
        }
    }

    fn draw_text(
        &mut self,
        text: &str,
        style: &crate::widget::style::TextStyle,
        x: f32,
        y: f32,
        width: f32,
    ) {
        self.apply_text_style(style);
        let mut paragraph_builder =
            ParagraphBuilder::new(&self.paragraph_style, &self.font_collection);
        paragraph_builder.push_style(&self.text_style);
        paragraph_builder.add_text(text);
        let mut paragraph = paragraph_builder.build();
        let scaled_width = self.scale_dim(width);
        paragraph.layout(f32::INFINITY);
        let intrinsic = paragraph.max_intrinsic_width();
        if scaled_width < intrinsic - 0.5 {
            paragraph.layout(scaled_width);
        }
        let scaled_x = self.scale_coord(x);
        let scaled_y = self.scale_coord(y);
        paragraph.paint(self.canvas(), Point::new(scaled_x, scaled_y));
    }

    fn draw_line(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        width: f32,
        color: &crate::widget::style::Color,
    ) {
        self.paint.set_style(PaintStyle::Stroke);
        self.paint
            .set_color(Color::from_argb(color.a, color.r, color.g, color.b));
        self.paint.set_stroke_width(self.scale_stroke(width));
        self.move_to(self.scale_coord(x1), self.scale_coord(y1));
        self.line_to(self.scale_coord(x2), self.scale_coord(y2));
        self.stroke();
    }

    fn draw_svg_dom(
        &mut self,
        dom: &skia_safe::svg::Dom,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        let sx = self.scale_coord(x);
        let sy = self.scale_coord(y);
        let sw = self.scale_dim(w);
        let sh = self.scale_dim(h);

        self.save();
        self.translate(sx, sy);

        let mut dom_mut = dom.clone();
        dom_mut.set_container_size((sw, sh));
        dom_mut.render(self.canvas());

        self.canvas().restore();
    }

    fn push_clip_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let sx = self.scale_coord(x);
        let sy = self.scale_coord(y);
        let sw = self.scale_dim(w);
        let sh = self.scale_dim(h);
        self.surface.canvas().save();
        self.surface.canvas().clip_rect(
            skia_safe::Rect::from_xywh(sx, sy, sw, sh),
            skia_safe::ClipOp::Intersect,
            true,
        );
    }

    fn pop_clip_rect(&mut self) {
        self.surface.canvas().restore();
    }

    fn push_effects(
        &mut self,
        opacity: f32,
        transform: &crate::widget::style::Transform,
        pivot_x: f32,
        pivot_y: f32,
    ) {
        let clamped = opacity.clamp(0.0, 1.0);
        // save_layer_alpha is an expensive offscreen composite — only use
        // it when actually needed. A plain save suffices for identity
        // opacity and lets us still push a matrix transform.
        if clamped < 1.0 {
            let alpha_u8 = (clamped * 255.0).round() as u8;
            // Bounds=None lets Skia infer from subsequent draws.
            self.surface
                .canvas()
                .save_layer_alpha(None, alpha_u8 as u32);
        } else {
            self.surface.canvas().save();
        }

        if !transform.is_identity() {
            let spx = self.scale_coord(pivot_x);
            let spy = self.scale_coord(pivot_y);
            let tx = self.scale_coord(transform.translate_x);
            let ty = self.scale_coord(transform.translate_y);

            // Skia composes transforms with later calls closer to the
            // drawing side. To get "rotate and scale around pivot, then
            // translate by (tx, ty)" we emit, outer-first:
            //   translate(tx, ty) -> translate(pivot) -> rotate/scale -> translate(-pivot)
            let canvas = self.surface.canvas();
            if tx != 0.0 || ty != 0.0 {
                canvas.translate((tx, ty));
            }
            canvas.translate((spx, spy));
            if transform.rotate != 0.0 {
                canvas.rotate(transform.rotate, None);
            }
            if transform.scale_x != 1.0 || transform.scale_y != 1.0 {
                canvas.scale((transform.scale_x, transform.scale_y));
            }
            canvas.translate((-spx, -spy));
        }
    }

    fn pop_effects(&mut self) {
        self.surface.canvas().restore();
    }

    fn push_backdrop_blur(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: &crate::widget::style::CornerRadii,
        sigma: f32,
    ) {
        let sx = self.scale_coord(x);
        let sy = self.scale_coord(y);
        let sw = self.scale_dim(w);
        let sh = self.scale_dim(h);
        let s_sigma = self.scale_dim(sigma);
        let rect = skia_safe::Rect::from_xywh(sx, sy, sw, sh);
        // Build a rounded-rect mask so the blurred capture is clipped to
        // the panel shape — without this, a plain rect clip leaves
        // blurred halos at the corners.
        let tl = self.scale_dim(radii.top_left);
        let tr = self.scale_dim(radii.top_right);
        let br = self.scale_dim(radii.bottom_right);
        let bl = self.scale_dim(radii.bottom_left);
        let rrect = skia_safe::RRect::new_rect_radii(
            rect,
            &[
                skia_safe::Point::new(tl, tl),
                skia_safe::Point::new(tr, tr),
                skia_safe::Point::new(br, br),
                skia_safe::Point::new(bl, bl),
            ],
        );

        let canvas = self.surface.canvas();
        // Outer save: lets pop_backdrop_blur restore() back past the
        // clip in addition to the save_layer.
        canvas.save();
        canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);

        let filter = skia_safe::image_filters::blur(
            (s_sigma, s_sigma),
            skia_safe::TileMode::Clamp,
            None,
            None,
        );
        if let Some(filter) = filter.as_ref() {
            let rec = skia_safe::canvas::SaveLayerRec::default()
                .bounds(&rect)
                .backdrop(filter);
            canvas.save_layer(&rec);
        } else {
            // Filter creation failed (zero sigma rounds to a no-op,
            // GPU-context oddities, etc.). Match the push count so the
            // paired pop stays balanced.
            canvas.save();
        }
    }

    fn pop_backdrop_blur(&mut self) {
        let canvas = self.surface.canvas();
        // Pop the save_layer (or the fallback save), then the outer
        // save that owns the clip.
        canvas.restore();
        canvas.restore();
    }
}

impl Surface for SkiaEnv {
    fn draw(&mut self, ui: &mut UI) {
        let saved_fc = if let Some(ref fc) = ui.font_collection {
            let old = std::mem::replace(&mut self.font_collection, fc.clone());
            Some(old)
        } else {
            None
        };
        let saved_default_font = if ui.default_font.is_some() {
            std::mem::replace(&mut self.default_font, ui.default_font.clone())
        } else {
            None
        };

        // Phase 2.5 M0: clear per-frame portal_layers; the
        // main render walk repopulates them. Pass B walks
        // layers in priority order (low → high) so higher-
        // priority layers paint on top.
        ui.portal_layers.clear();

        let focused = ui.get_focused().cloned();
        Self::draw_widget_recursive(
            self,
            &ui.root,
            focused.as_ref(),
            &mut ui.image_cache,
            &mut ui.portal_layers,
            (0.0, 0.0), // accumulated_translate — viewport origin at root
        );

        // Phase 3 M2: synthesize a CursorAttached layer entry
        // for the in-flight drag preview, if any. Positioned
        // at the cursor; the preview's own layout rect is
        // drawn by paint_portal_entry like any other layer
        // entry.
        if let Some(preview_state) = ui.active_drag_preview().cloned() {
            let preview_rect = {
                let g = preview_state.preview.lock().expect("widget lock poisoned");
                g.get_layout_rect().cloned().unwrap_or_else(|| {
                    crate::widget::rect::Rect::new(0.0, 0.0, 0.0, 0.0)
                })
            };
            ui.portal_layers.push(crate::widget::PortalEntry {
                widget: preview_state.preview.clone(),
                focus_trap: false,
                viewport_rect: crate::widget::rect::Rect::new(
                    preview_state.cursor.x(),
                    preview_state.cursor.y(),
                    preview_rect.width.max(0.0),
                    preview_rect.height.max(0.0),
                ),
                layer: crate::widget::portal_layer::PortalLayer::CursorAttached,
                cursor: crate::widget::portal_layer::CursorPreference::Inherit,
            });
        }

        // Pass B: walk layers in priority order. For each
        // layer with `Block` backdrop policy and any open
        // entry, paint a viewport-sized translucent backdrop
        // first (the runtime backdrop). Then paint the
        // entries in mount order.
        //
        // We need the viewport size for the backdrop rect —
        // pull from the root's layout rect.
        let viewport_size = {
            let root = ui.root.lock().expect("widget lock poisoned");
            root.get_layout_rect()
                .map(|r| (r.width, r.height))
                .unwrap_or((0.0, 0.0))
        };
        for layer in crate::widget::portal_layer::PortalLayer::ALL {
            let entries: Vec<crate::widget::PortalEntry> =
                ui.portal_layers.entries_in(layer).to_vec();
            if entries.is_empty() {
                continue;
            }
            // Layer-policy backdrop. Block policy gets a
            // translucent backdrop drawn before any entry.
            // Dim is currently treated as None (TODO; no
            // layer defaults to Dim).
            if layer.default_backdrop()
                == crate::widget::portal_layer::BackdropPolicy::Block
            {
                Self::paint_layer_backdrop(self, viewport_size);
            }
            for entry in &entries {
                Self::paint_portal_entry(
                    self,
                    entry,
                    focused.as_ref(),
                    &mut ui.image_cache,
                );
            }
        }

        // Phase 2 M4: reconcile the focus stack now that
        // portal_layers reflects this frame's open portals.
        // Pushes new focus_trap entries; pops + restores
        // closed ones.
        ui.sync_focus_stack();

        if let Some(old) = saved_fc {
            self.font_collection = old;
        }
        self.default_font = saved_default_font;
    }
}

impl SkiaEnv {
    fn draw_widget_recursive(
        env: &mut SkiaEnv,
        widget_ref: &WidgetRef,
        focused: Option<&WidgetRef>,
        image_cache: &mut ImageCache,
        portal_layers: &mut crate::widget::PortalLayers,
        accumulated_translate: (f32, f32),
    ) {
        use crate::widget::RenderContext;

        let is_focused = focused
            .map(|f| std::ptr::eq(Arc::as_ptr(f), Arc::as_ptr(widget_ref)))
            .unwrap_or(false);

        // Phase 2 Portal (extended in P25-M0): detect a Portal
        // node and defer to the portal layer instead of
        // recursing. The portal node itself paints nothing;
        // its children render in Pass B with viewport bounds.
        //
        // Push to portal_layers when the portal is open OR
        // when it has ghost children mid-exit-animation (so
        // the close transition still paints). focus_trap is
        // only honored when actually open — ghosting portals
        // don't continue to trap focus.
        //
        // The captured viewport_rect uses accumulated_translate
        // (parent-chain translates accumulated through the
        // recursion) so Pass B can translate from viewport
        // origin without re-walking the parent chain.
        {
            let widget = widget_ref.lock().expect("widget lock poisoned");
            if let Some(info) = widget.as_portal() {
                let children = widget.get_children();
                let has_ghosts = children.iter().any(|c| {
                    let g = c.lock().expect("widget lock poisoned");
                    g.is_exiting()
                });
                if info.open || has_ghosts {
                    let local_rect = widget
                        .get_layout_rect()
                        .cloned()
                        .unwrap_or_else(crate::widget::rect::Rect::zero);
                    drop(widget);
                    let viewport_rect = crate::widget::rect::Rect::new(
                        local_rect.x + accumulated_translate.0,
                        local_rect.y + accumulated_translate.1,
                        local_rect.width,
                        local_rect.height,
                    );
                    portal_layers.push(crate::widget::PortalEntry {
                        widget: widget_ref.clone(),
                        viewport_rect,
                        layer: info.layer,
                        // Only trap focus when actually open;
                        // a closed portal with ghosts shouldn't
                        // continue to trap.
                        focus_trap: info.open && info.focus_trap,
                        // Cursor preference is the portal's
                        // resolved effective_cursor (explicit
                        // override or layer default). Closed
                        // ghosting portals don't influence
                        // cursor either — treat as Inherit.
                        cursor: if info.open {
                            info.cursor
                        } else {
                            crate::widget::portal_layer::CursorPreference::Inherit
                        },
                    });
                }
                return;
            }
        }

        let widget = widget_ref.lock().expect("widget lock poisoned");
        let effects = widget.render_effects();
        drop(widget);

        // Push opacity/transform BEFORE drawing the widget itself so
        // background/border are affected, and pop AFTER children so the
        // whole subtree composites under the effect.
        if let Some(ref fx) = effects {
            env.push_effects(fx.opacity, &fx.transform, fx.pivot_x, fx.pivot_y);
        }

        let widget = widget_ref.lock().expect("widget lock poisoned");
        // render() runs with the canvas positioned at this widget's parent's
        // coordinate origin, so the widget draws itself (and pushes any clip)
        // in parent-relative coordinates.
        widget.render(env, is_focused, image_cache);
        let needs_post = widget.needs_post_render();
        let children = widget.get_children();
        let child_origin = widget
            .get_layout_rect()
            .map(|r| (r.x, r.y))
            .unwrap_or((0.0, 0.0));
        let (scroll_x, scroll_y) = widget.scroll_offset();
        drop(widget);

        // Step into this widget's own content coordinate space before
        // rendering its children: translate by its origin (so the child's
        // parent-relative layout.x/y becomes the right physical position)
        // and subtract any scroll offset so descendants shift within the
        // clipped viewport.
        let tx = child_origin.0 - scroll_x;
        let ty = child_origin.1 - scroll_y;
        let needs_transform = tx != 0.0 || ty != 0.0;
        if needs_transform {
            env.save();
            let dpi = env.dpi_scale;
            env.translate(tx * dpi, ty * dpi);
        }

        // Phase 2.5 M0: thread the accumulated translate
        // through the recursion. Each level's child translate
        // (origin minus scroll) adds to the cumulative
        // translate, so when a portal is encountered deep in
        // the tree its viewport_rect is captured correctly.
        let child_accumulated = (
            accumulated_translate.0 + tx,
            accumulated_translate.1 + ty,
        );
        for child in &children {
            Self::draw_widget_recursive(
                env,
                child,
                focused,
                image_cache,
                portal_layers,
                child_accumulated,
            );
        }

        if needs_transform {
            env.surface.canvas().restore();
        }

        if needs_post {
            let widget = widget_ref.lock().expect("widget lock poisoned");
            widget.post_render(env, image_cache);
        }

        if effects.is_some() {
            env.pop_effects();
        }
    }

    /// Phase 2.5 M0: paint a single portal entry's children
    /// in Pass B. The entry's `viewport_rect` is now
    /// viewport-absolute (captured during Pass A by
    /// accumulating parent translates), so we translate from
    /// the viewport origin without re-walking the parent chain.
    ///
    /// Nested portals encountered in the children's recursion
    /// land in the same `portal_layers` storage and get
    /// painted by the outer Pass B loop. We pass an empty
    /// throwaway PortalLayers here to satisfy the recursion
    /// signature; in M3 use cases (root-level portals) this
    /// path doesn't recursively encounter portals.
    fn paint_portal_entry(
        env: &mut SkiaEnv,
        entry: &crate::widget::PortalEntry,
        focused: Option<&WidgetRef>,
        image_cache: &mut ImageCache,
    ) {
        // Translate from the viewport origin to the portal's
        // viewport-absolute slot.
        let dpi = env.dpi_scale;
        env.save();
        env.translate(entry.viewport_rect.x * dpi, entry.viewport_rect.y * dpi);

        let children = {
            let widget = entry.widget.lock().expect("widget lock poisoned");
            widget.get_children()
        };
        // Nested portals encountered inside this portal's
        // children push to `nested` and get painted after the
        // main child render. Their viewport_rect is captured
        // with accumulated_translate = entry.viewport_rect.{x,y}
        // because we've already translated into the portal's
        // slot — children's local coords add to that.
        let mut nested = crate::widget::PortalLayers::new();
        for child in &children {
            Self::draw_widget_recursive(
                env,
                child,
                focused,
                image_cache,
                &mut nested,
                (entry.viewport_rect.x, entry.viewport_rect.y),
            );
        }
        // Paint nested portals if any. Pass-through to the
        // outer iteration order isn't preserved here — nested
        // portals are rare and stacking among them follows
        // the order they appeared in this entry's subtree.
        for nested_entry in nested.iter_paint_order() {
            Self::paint_portal_entry(env, nested_entry, focused, image_cache);
        }

        env.surface.canvas().restore();
    }

    /// Phase 2.5 M0: paint the runtime-side backdrop for a
    /// layer with `Block` policy. A viewport-sized translucent
    /// black rect drawn before the layer's first entry; lower
    /// layers stay visible underneath but pointer events to
    /// them are gated by the hit-test path.
    ///
    /// Consumers can also render their own backdrop into the
    /// layer (the canonical Modal() example fn does this for
    /// styling control). Both render; the consumer backdrop
    /// appears on top of the runtime backdrop. Slightly
    /// wasteful but visually correct.
    fn paint_layer_backdrop(env: &mut SkiaEnv, viewport_size: (f32, f32)) {
        use crate::widget::style::Color;
        use crate::widget::RenderContext;
        let backdrop = Color::new(0, 0, 0, 96);
        env.fill_rect(0.0, 0.0, viewport_size.0, viewport_size.1, &backdrop);
    }
}

impl SkiaEnv {
    /// Draw debug outlines for FlexWidget boundaries
    pub fn draw_debug_outlines(&mut self, widget: &FlexWidget, layout: &crate::widget::rect::Rect) {
        let es = widget.effective_style();

        let margin_left = self.scale_coord(es.margin.get_left());
        let margin_top = self.scale_coord(es.margin.get_top());
        let margin_right = self.scale_coord(es.margin.get_right());
        let margin_bottom = self.scale_coord(es.margin.get_bottom());

        let border_left = self.scale_coord(es.border.get_left());
        let border_top = self.scale_coord(es.border.get_top());
        let border_right = self.scale_coord(es.border.get_right());
        let border_bottom = self.scale_coord(es.border.get_bottom());

        let padding_left = self.scale_coord(es.padding.get_left());
        let padding_top = self.scale_coord(es.padding.get_top());
        let padding_right = self.scale_coord(es.padding.get_right());
        let padding_bottom = self.scale_coord(es.padding.get_bottom());

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
