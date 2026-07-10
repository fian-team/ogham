//! Backend-agnostic text measurement (E1 of the text-input ergonomics work).
//!
//! The Skia paint path ([`crate::skia`]) and the layout/event/render paths in
//! the widget tree both need to agree on where glyphs land: layout needs the
//! wrapped height, the renderer needs the caret rect, and click/drag-select
//! need to map a coordinate back to a byte offset. This module is the single
//! source of that geometry.
//!
//! Everything here works in **logical** pixels. The Skia backend paints text
//! in logical space too — the paragraph is laid out at the logical font size
//! and wrap width under a canvas DPI transform — so a caret x computed here
//! lines up with what `draw_text` paints, and paint can never re-derive a
//! line break the layout pass didn't measure. The shared
//! [`configure_geometry`] mapping keeps the two from drifting on font family
//! / size / weight / spacing / alignment.
//!
//! Skia's paragraph `TextIndex` is a **UTF-8 byte offset** (the UTF-16 calls
//! are explicitly suffixed `_utf16_`), so [`Selection`](super::text_input_widget)
//! byte offsets pass straight through with no conversion.

use std::cell::RefCell;

use skia_safe::{
    font_style::{Slant, Weight, Width},
    textlayout::{
        FontCollection, ParagraphBuilder, ParagraphStyle, RectHeightStyle, RectWidthStyle,
        TextAlign as SkiaTextAlign, TextStyle as SkiaTextStyle,
    },
    FontMgr, FontStyle, Point,
};

use crate::widget::style::{FontWeight, TextAlign, TextStyle};

thread_local! {
    /// Fallback font collection for measurement when no caller-supplied one is
    /// available (e.g. tests, or an event before the first layout pass cached
    /// the UI's collection). Mirrors `TextWidget`'s thread-local cache.
    static DEFAULT_FONT_COLLECTION: RefCell<FontCollection> = RefCell::new({
        let mut fc = FontCollection::new();
        fc.set_default_font_manager(FontMgr::new(), None);
        fc
    });
}

/// Wrapped-text metrics, logical px.
#[derive(Debug, Clone, Copy)]
pub struct TextMetrics {
    /// Natural unwrapped width (longest the content wants to be). Drives
    /// `Size::Shrink` width sizing.
    pub intrinsic_width: f32,
    /// Height after laying the content out at the requested `max_width`.
    /// Drives `Size::Shrink` height (auto-grow with wrapped lines).
    pub height: f32,
    /// Number of laid-out lines at `max_width`.
    pub line_count: usize,
}

/// Caret placement, logical px, relative to the text origin passed to
/// `draw_text` (i.e. before the input's own padding/scroll offset).
#[derive(Debug, Clone, Copy)]
pub struct CaretRect {
    pub x: f32,
    pub top: f32,
    pub height: f32,
}

/// Map an Ogham [`TextStyle`] onto a Skia paragraph + text style — the
/// geometry-affecting fields only (family, size, weight, alignment). Paint
/// (color / outline) is the caller's concern, so this is safe to share between
/// the measurement path (no paint) and `skia.rs`'s `apply_text_style` (which
/// layers paint on afterwards). `font_size` is passed in already in the
/// caller's target space — logical here, DPI-scaled from `skia.rs`.
pub fn configure_geometry(
    text_style: &mut SkiaTextStyle,
    paragraph_style: &mut ParagraphStyle,
    style: &TextStyle,
    font_size: f32,
    default_font: Option<&str>,
) {
    text_style.set_font_size(font_size);
    // Letter spacing is authored in logical px; scale it by the same factor
    // the caller applied to the font size (1.0 here, the DPI scale from
    // `skia.rs`) so tracking widens with the glyphs.
    let spacing_scale = if style.get_size() > f32::EPSILON {
        font_size / style.get_size()
    } else {
        1.0
    };
    text_style.set_letter_spacing(style.get_letter_spacing() * spacing_scale);
    text_style.set_font_style(FontStyle::new(
        match style.get_weight() {
            FontWeight::Normal => Weight::NORMAL,
            FontWeight::SemiBold => Weight::SEMI_BOLD,
            FontWeight::Bold => Weight::BOLD,
            FontWeight::Light => Weight::LIGHT,
        },
        Width::NORMAL,
        Slant::Upright,
    ));
    if let Some(family) = style.get_font().or(default_font) {
        text_style.set_font_families(&[family]);
    } else {
        // "No families" intent, expressed with a single unresolvable name:
        // skia-safe 0.91's `set_font_families` aborts the process on an
        // empty slice (strict `slice::from_raw_parts` preconditions), and
        // the style must still be overwritten because it is reused across
        // widgets. An unknown family falls through to the collection's
        // default font manager — the same behavior "no families" produced.
        text_style.set_font_families(&[""]);
    }
    paragraph_style.set_text_align(match style.get_align() {
        TextAlign::Left => SkiaTextAlign::Left,
        TextAlign::Center => SkiaTextAlign::Center,
        TextAlign::Right => SkiaTextAlign::Right,
    });
}

/// Build a laid-out paragraph in logical px. `max_width` is the wrap width;
/// pass `f32::INFINITY` for no wrapping. Mirrors `skia.rs`'s wrap heuristic
/// (only re-lay-out at `max_width` when the content actually overflows) so the
/// measured geometry matches the painted geometry.
fn build_paragraph(
    font_collection: Option<&FontCollection>,
    default_font: Option<&str>,
    style: &TextStyle,
    text: &str,
    max_width: f32,
) -> skia_safe::textlayout::Paragraph {
    match font_collection {
        Some(fc) => build_with(fc, default_font, style, text, max_width),
        None => DEFAULT_FONT_COLLECTION
            .with(|fc| build_with(&fc.borrow(), default_font, style, text, max_width)),
    }
}

fn build_with(
    font_collection: &FontCollection,
    default_font: Option<&str>,
    style: &TextStyle,
    text: &str,
    max_width: f32,
) -> skia_safe::textlayout::Paragraph {
    let mut paragraph_style = ParagraphStyle::new();
    let mut text_style = SkiaTextStyle::new();
    configure_geometry(
        &mut text_style,
        &mut paragraph_style,
        style,
        style.get_size(),
        default_font,
    );
    let mut builder = ParagraphBuilder::new(&paragraph_style, font_collection);
    builder.push_style(&text_style);
    builder.add_text(text);
    let mut paragraph = builder.build();
    paragraph.layout(f32::INFINITY);
    if max_width.is_finite() && max_width < paragraph.max_intrinsic_width() - 0.5 {
        paragraph.layout(max_width);
    }
    paragraph
}

/// Measure `text` wrapped at `max_width` (logical px).
pub fn measure(
    font_collection: Option<&FontCollection>,
    default_font: Option<&str>,
    style: &TextStyle,
    text: &str,
    max_width: f32,
) -> TextMetrics {
    if text.is_empty() {
        // An empty paragraph reports zero height; fall back to one font-sized
        // line so an empty input still reserves a sensible box.
        return TextMetrics {
            intrinsic_width: 0.0,
            height: style.get_size(),
            line_count: 1,
        };
    }
    let paragraph = build_paragraph(font_collection, default_font, style, text, max_width);
    TextMetrics {
        intrinsic_width: paragraph.max_intrinsic_width(),
        height: paragraph.height(),
        line_count: paragraph.line_number().max(1),
    }
}

/// Map a point (logical px, relative to the text origin) to the nearest byte
/// offset in `text`. Used for click-to-caret and drag-select. The returned
/// offset is always a char boundary.
pub fn glyph_index_at(
    font_collection: Option<&FontCollection>,
    default_font: Option<&str>,
    style: &TextStyle,
    text: &str,
    max_width: f32,
    point: (f32, f32),
) -> usize {
    if text.is_empty() {
        return 0;
    }
    let paragraph = build_paragraph(font_collection, default_font, style, text, max_width);
    let pos = paragraph.get_glyph_position_at_coordinate(Point::new(point.0, point.1));
    let idx = (pos.position.max(0) as usize).min(text.len());
    snap_to_boundary(text, idx)
}

/// Caret geometry for the caret sitting *before* the byte at `byte_index`
/// (logical px, relative to the text origin). `byte_index` must be a char
/// boundary in `[0, text.len()]`.
pub fn caret_geometry(
    font_collection: Option<&FontCollection>,
    default_font: Option<&str>,
    style: &TextStyle,
    text: &str,
    max_width: f32,
    byte_index: usize,
) -> CaretRect {
    let line_height = style.get_size();
    if text.is_empty() {
        return CaretRect {
            x: 0.0,
            top: 0.0,
            height: line_height,
        };
    }
    let byte_index = snap_to_boundary(text, byte_index.min(text.len()));
    let paragraph = build_paragraph(font_collection, default_font, style, text, max_width);

    // Caret before the first glyph: take the leading edge of the first char.
    // Otherwise take the trailing edge of the preceding char so the caret sits
    // at the boundary even at end-of-text / end-of-line.
    let (range, leading_edge) = if byte_index == 0 {
        let next = next_boundary(text, 0);
        (0..next, true)
    } else {
        let prev = prev_boundary(text, byte_index);
        (prev..byte_index, false)
    };
    let boxes = paragraph.get_rects_for_range(range, RectHeightStyle::Max, RectWidthStyle::Tight);
    match boxes.first() {
        Some(tb) => CaretRect {
            x: if leading_edge {
                tb.rect.left
            } else {
                tb.rect.right
            },
            top: tb.rect.top,
            height: (tb.rect.bottom - tb.rect.top).max(line_height),
        },
        None => CaretRect {
            x: 0.0,
            top: 0.0,
            height: line_height,
        },
    }
}

/// Rects (logical px, relative to the text origin) covering the byte range
/// `start..end` — one per visual line the range spans. Used to paint the
/// selection highlight. Empty when the range is empty.
pub fn selection_rects(
    font_collection: Option<&FontCollection>,
    default_font: Option<&str>,
    style: &TextStyle,
    text: &str,
    max_width: f32,
    start: usize,
    end: usize,
) -> Vec<(f32, f32, f32, f32)> {
    if text.is_empty() || start >= end {
        return Vec::new();
    }
    let start = snap_to_boundary(text, start.min(text.len()));
    let end = snap_to_boundary(text, end.min(text.len()));
    if start >= end {
        return Vec::new();
    }
    let paragraph = build_paragraph(font_collection, default_font, style, text, max_width);
    paragraph
        .get_rects_for_range(start..end, RectHeightStyle::Max, RectWidthStyle::Tight)
        .into_iter()
        .map(|tb| {
            (
                tb.rect.left,
                tb.rect.top,
                tb.rect.right - tb.rect.left,
                tb.rect.bottom - tb.rect.top,
            )
        })
        .collect()
}

/// Round `idx` down to the nearest char boundary in `text`.
pub fn snap_to_boundary(text: &str, idx: usize) -> usize {
    if idx >= text.len() {
        return text.len();
    }
    let mut i = idx;
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// First char boundary strictly after `idx` (clamped to `text.len()`).
pub fn next_boundary(text: &str, idx: usize) -> usize {
    if idx >= text.len() {
        return text.len();
    }
    let mut i = idx + 1;
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Last char boundary strictly before `idx` (clamped to `0`).
pub fn prev_boundary(text: &str, idx: usize) -> usize {
    if idx == 0 {
        return 0;
    }
    let mut i = idx - 1;
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `letter_spacing` participates in measurement: tracked text is wider
    /// than the same text at natural fit, by roughly spacing × glyph count.
    #[test]
    fn letter_spacing_widens_measurement() {
        let text = "INVITATION";
        let mut style = TextStyle::default();
        let plain = measure(None, None, &style, text, f32::INFINITY);
        style.letter_spacing = 4.0;
        let tracked = measure(None, None, &style, text, f32::INFINITY);
        let widened = tracked.intrinsic_width - plain.intrinsic_width;
        assert!(
            widened > 4.0 * (text.len() as f32 - 1.0) - 1.0,
            "tracking should widen the line (got +{widened}px)"
        );
    }
}
/// A `width: "shrink"` Text inside a centered column: its box is exactly
/// its intrinsic width (single line, no wrap) and the column centers it.
/// The engraved-card layout regressed here once — a fitting line's last
/// word wrapped at paint time because paint re-measured at the DPI-scaled
/// font size (see `SkiaEnv::draw_text`: text now paints in logical space).
#[cfg(test)]
#[test]
fn shrink_text_gets_its_intrinsic_width_and_centers() {
    use crate::runtime::config::RuntimeConfig;
    use crate::widget::Widget;
    let src = r##"
let main = fn () {
  Flex {
    style: { width: 400, height: 300, direction: "column", cross_alignment: "center" },
    children: [
      Text { text: "LORD ASHWORTH", style: { width: "shrink", size: 18.6, letter_spacing: 5.95 } },
    ],
  }
};
"##;
    let mut ui = crate::Ogham::from_source(src, RuntimeConfig::default()).unwrap();
    let ui = ui.get_ui_mut();
    ui.layout(400.0, 300.0);

    let root = ui.root.lock().unwrap();
    let child = root.get_children()[0].clone();
    drop(root);
    let child = child.lock().unwrap();
    let text = child
        .downcast_ref::<crate::widget::text_widget::TextWidget>()
        .expect("the column's child is the Text");
    let rect = text.layout.as_ref().expect("laid out").clone();

    let mut style = TextStyle::default();
    style.size = 18.6;
    style.letter_spacing = 5.95;
    let m = measure(None, None, &style, "LORD ASHWORTH", f32::INFINITY);
    assert!(
        (rect.width - m.intrinsic_width).abs() < 0.01,
        "shrink box ({}) hugs the intrinsic width ({})",
        rect.width,
        m.intrinsic_width
    );
    assert_eq!(m.line_count, 1, "a fitting line never wraps");
    assert!(
        (rect.x - (400.0 - m.intrinsic_width) / 2.0).abs() < 0.51,
        "cross-center places the box mid-column (x = {})",
        rect.x
    );
}
