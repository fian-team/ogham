use std::collections::HashMap;

use skia_safe::textlayout::FontCollection;

use super::event::*;
use super::point::*;
use super::rect::*;
use super::style::*;
use super::Widget;
use crate::widget::event::EventContext;
use crate::widget::style::Direction;
use crate::widget::{text_layout, LayoutContext, UpdateResult, WidgetRef};

/// Minimum content width (logical px) reserved for an empty `Shrink`-width
/// input so it stays visible and clickable before anything is typed.
const EMPTY_MIN_CONTENT_WIDTH: f32 = 100.0;

/// A text selection / caret. Byte offsets into the input's `value`, always kept
/// on char boundaries. `anchor` is the fixed end (where a shift-extend started
/// or a drag began); `caret` is the moving end and where the blinking cursor is
/// drawn. Collapsed (`anchor == caret`) is an ordinary caret with no selection.
#[derive(Debug, Clone, Copy, Default)]
pub struct Selection {
    pub anchor: usize,
    pub caret: usize,
}

impl Selection {
    pub fn collapsed(pos: usize) -> Self {
        Self {
            anchor: pos,
            caret: pos,
        }
    }

    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.caret
    }

    pub fn start(&self) -> usize {
        self.anchor.min(self.caret)
    }

    pub fn end(&self) -> usize {
        self.anchor.max(self.caret)
    }
}

pub struct TextInputWidget {
    pub value: String,
    /// Current selection / caret. Replaces the old single `cursor_position`.
    pub selection: Selection,
    pub event_listeners: HashMap<String, Vec<Box<dyn Fn(&Event)>>>,
    pub style: FlexStyle,
    pub hover_style: Option<FlexStyle>,
    /// Applied when the widget is the currently-focused input. Takes
    /// precedence over hover when both apply — if a user is mousing over
    /// their own focused field, focus wins since it's the more
    /// load-bearing state.
    pub focus_style: Option<FlexStyle>,
    pub text_style: TextStyle,
    pub hover_text_style: Option<TextStyle>,
    pub focus_text_style: Option<TextStyle>,
    pub hovered: bool,
    pub layout: Option<Rect>,
    /// Font collection + default font cached at `layout()` time so `render` and
    /// `handle_event` (which don't receive a `LayoutContext`) can measure text
    /// with the same fonts layout used. See `text_layout`.
    measure_fc: Option<FontCollection>,
    measure_font: Option<String>,
    /// Horizontal scroll offset (logical px) keeping the caret in view for a
    /// single-line input whose text overflows the box. Always 0 in the
    /// wrapping (multi-line) case.
    scroll_x: f32,
}

impl TextInputWidget {
    pub fn new() -> Self {
        Self {
            value: String::new(),
            selection: Selection::default(),
            event_listeners: HashMap::new(),
            style: FlexStyle::default(),
            hover_style: None,
            focus_style: None,
            text_style: TextStyle::default(),
            hover_text_style: None,
            focus_text_style: None,
            hovered: false,
            layout: None,
            measure_fc: None,
            measure_font: None,
            scroll_x: 0.0,
        }
    }

    pub fn with_style(style: FlexStyle) -> Self {
        Self {
            style,
            ..Self::new()
        }
    }

    pub fn with_value(value: String) -> Self {
        let caret = value.len();
        Self {
            value,
            selection: Selection::collapsed(caret),
            ..Self::new()
        }
    }

    /// True when this input wraps long text onto multiple lines and grows its
    /// height to fit. The contract: `height: Shrink` is the multi-line / grow
    /// case; any other height is a single-line input that scrolls horizontally.
    fn is_multiline(&self) -> bool {
        matches!(self.style.height, Size::Shrink)
    }

    /// Returns the flex style to use for rendering. Focus wins over hover
    /// when both apply — a user hovering their own already-focused field
    /// should still see the focus affordance, not just the hover.
    pub fn effective_style(&self, focused: bool) -> &FlexStyle {
        if focused {
            if let Some(ref s) = self.focus_style {
                return s;
            }
        }
        if self.hovered {
            if let Some(ref s) = self.hover_style {
                return s;
            }
        }
        &self.style
    }

    /// Returns the text style to use for rendering. Focus wins over hover
    /// for the same reason `effective_style` does.
    pub fn effective_text_style(&self, focused: bool) -> &TextStyle {
        if focused {
            if let Some(ref s) = self.focus_text_style {
                return s;
            }
        }
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
        self.clamp_selection();
    }

    /// Re-snap both selection ends into `[0, len]` on char boundaries (after an
    /// external value change).
    fn clamp_selection(&mut self) {
        let len = self.value.len();
        self.selection.anchor = text_layout::snap_to_boundary(&self.value, self.selection.anchor.min(len));
        self.selection.caret = text_layout::snap_to_boundary(&self.value, self.selection.caret.min(len));
    }

    // ---- editing primitives (E2: char-boundary safe, selection aware) ----

    /// Replace the current selection (or insert at the caret when collapsed)
    /// with `s`, leaving the caret collapsed just after the inserted text.
    fn replace_selection(&mut self, s: &str) {
        let start = self.selection.start();
        let end = self.selection.end();
        self.value.replace_range(start..end, s);
        self.selection = Selection::collapsed(start + s.len());
    }

    fn insert_char(&mut self, ch: char) {
        let mut buf = [0u8; 4];
        self.replace_selection(ch.encode_utf8(&mut buf));
    }

    /// Backspace. Returns whether `value` changed.
    fn delete_backward(&mut self) -> bool {
        if !self.selection.is_collapsed() {
            self.replace_selection("");
            return true;
        }
        let caret = self.selection.caret;
        if caret == 0 {
            return false;
        }
        let prev = text_layout::prev_boundary(&self.value, caret);
        self.value.replace_range(prev..caret, "");
        self.selection = Selection::collapsed(prev);
        true
    }

    /// Forward delete (Delete key). Returns whether `value` changed.
    fn delete_forward(&mut self) -> bool {
        if !self.selection.is_collapsed() {
            self.replace_selection("");
            return true;
        }
        let caret = self.selection.caret;
        if caret >= self.value.len() {
            return false;
        }
        let next = text_layout::next_boundary(&self.value, caret);
        self.value.replace_range(caret..next, "");
        true
    }

    /// Move the caret to `pos` (snapped to a char boundary). When `extend` is
    /// false the selection collapses there; when true the anchor is kept so the
    /// selection grows/shrinks.
    fn set_caret(&mut self, pos: usize, extend: bool) {
        let pos = text_layout::snap_to_boundary(&self.value, pos.min(self.value.len()));
        self.selection.caret = pos;
        if !extend {
            self.selection.anchor = pos;
        }
    }

    fn move_left(&mut self, extend: bool) {
        let pos = if !extend && !self.selection.is_collapsed() {
            self.selection.start()
        } else {
            text_layout::prev_boundary(&self.value, self.selection.caret)
        };
        self.set_caret(pos, extend);
    }

    fn move_right(&mut self, extend: bool) {
        let pos = if !extend && !self.selection.is_collapsed() {
            self.selection.end()
        } else {
            text_layout::next_boundary(&self.value, self.selection.caret)
        };
        self.set_caret(pos, extend);
    }

    fn move_to_start(&mut self, extend: bool) {
        self.set_caret(0, extend);
    }

    fn move_to_end(&mut self, extend: bool) {
        self.set_caret(self.value.len(), extend);
    }

    fn select_all(&mut self) {
        self.selection = Selection {
            anchor: 0,
            caret: self.value.len(),
        };
    }

    /// Fire `on_change` listeners with the current value. Called only when an
    /// edit actually mutated `value` — cursor-only movement no longer fires it
    /// (Stage 3 on_change fix).
    fn fire_change(&self) {
        if let Some(listeners) = self.event_listeners.get("on_change") {
            let ev = Event::with_value("on_change".to_string(), self.value.clone());
            for listener in listeners {
                listener(&ev);
            }
        }
    }

    /// Fire `on_submit` listeners with the current value (Enter pressed).
    fn fire_submit(&self) {
        if let Some(listeners) = self.event_listeners.get("on_submit") {
            let ev = Event::with_value("on_submit".to_string(), self.value.clone());
            for listener in listeners {
                listener(&ev);
            }
        }
    }

    // ---- measurement geometry helpers (E1 consumers) ----

    /// `(text_origin_x, text_origin_y, content_width, content_height, wrap_width)`
    /// in the widget's parent-relative coordinate space, for the given focus
    /// state. The text origin is the box inset by margin + padding; `wrap_width`
    /// is the content width when multi-line, else infinite (single line).
    fn text_geometry(&self, focused: bool) -> Option<(f32, f32, f32, f32, f32)> {
        let layout = self.layout.as_ref()?;
        let s = self.effective_style(focused);
        let box_x = layout.x + s.margin.get_left();
        let box_y = layout.y + s.margin.get_top();
        let box_w = layout.width - s.margin.get_left() - s.margin.get_right();
        let box_h = layout.height - s.margin.get_top() - s.margin.get_bottom();
        let text_x = box_x + s.padding.get_left();
        let text_y = box_y + s.padding.get_top();
        let content_w = (box_w - s.padding.get_left() - s.padding.get_right()).max(0.0);
        let content_h = (box_h - s.padding.get_top() - s.padding.get_bottom()).max(0.0);
        let wrap_w = if self.is_multiline() {
            content_w
        } else {
            f32::INFINITY
        };
        Some((text_x, text_y, content_w, content_h, wrap_w))
    }

    /// Map a point in the widget's parent-relative space to a byte offset.
    fn index_at_point(&self, point: &Point) -> usize {
        let Some((text_x, text_y, _cw, _ch, wrap_w)) = self.text_geometry(true) else {
            return 0;
        };
        let local_x = point.x() - text_x + self.scroll_x;
        let local_y = point.y() - text_y;
        text_layout::glyph_index_at(
            self.measure_fc.as_ref(),
            self.measure_font.as_deref(),
            self.effective_text_style(true),
            &self.value,
            wrap_w,
            (local_x, local_y),
        )
    }

    /// Recompute `scroll_x` so the caret stays inside the content box. No-op for
    /// the multi-line (wrapping) case, which never scrolls horizontally.
    fn update_scroll(&mut self) {
        if self.is_multiline() {
            self.scroll_x = 0.0;
            return;
        }
        let Some((_tx, _ty, content_w, _ch, wrap_w)) = self.text_geometry(true) else {
            return;
        };
        let caret = text_layout::caret_geometry(
            self.measure_fc.as_ref(),
            self.measure_font.as_deref(),
            self.effective_text_style(true),
            &self.value,
            wrap_w,
            self.selection.caret,
        );
        // Keep the caret within [0, content_w] of the visible window.
        if caret.x - self.scroll_x > content_w {
            self.scroll_x = caret.x - content_w;
        }
        if caret.x - self.scroll_x < 0.0 {
            self.scroll_x = caret.x;
        }
        // Don't scroll past the start.
        if self.scroll_x < 0.0 {
            self.scroll_x = 0.0;
        }
    }
}

impl Widget for TextInputWidget {
    fn cursor_preference_when_focused(
        &self,
    ) -> Option<crate::widget::portal_layer::CursorPreference> {
        // Phase 2.5 M1: a focused TextInput needs a visible
        // cursor so the user can see what they're typing.
        Some(crate::widget::portal_layer::CursorPreference::Free)
    }

    fn claims_character_keys(&self) -> bool {
        // Phase 2.5 M2: focused TextInput consumes character
        // keys so the user can type into the field without
        // game hotkeys firing.
        true
    }

    fn is_focusable(&self) -> bool {
        true
    }

    fn update(&mut self, new_widget: WidgetRef) -> UpdateResult {
        let mut new_widget = new_widget.lock().expect("widget lock poisoned");
        if let Some(new_text_input_widget) = new_widget.downcast_mut::<TextInputWidget>() {
            let style_changed = !self.style.layout_equal(&new_text_input_widget.style);
            self.style = new_text_input_widget.style.clone();
            self.hover_style = new_text_input_widget.hover_style.clone();
            self.focus_style = new_text_input_widget.focus_style.clone();
            self.text_style = new_text_input_widget.text_style.clone();
            self.hover_text_style = new_text_input_widget.hover_text_style.clone();
            self.focus_text_style = new_text_input_widget.focus_text_style.clone();
            // Swap event listeners - we can't clone closures, so we swap them
            std::mem::swap(
                &mut self.event_listeners,
                &mut new_text_input_widget.event_listeners,
            );
            let new_value = new_text_input_widget.value.clone();
            let value_unchanged = new_value == self.value;
            self.value = new_value;
            if value_unchanged {
                self.clamp_selection();
            } else {
                // Host pushed a new value — collapse the caret at the end.
                self.selection = Selection::collapsed(self.value.len());
            }
            let value_changed = !value_unchanged;
            // Value only feeds into layout when width is `Shrink` —
            // `get_dimensions` derives the box width from the text in that
            // case. For `Fixed` / `Grow` / `Percent` the box is unaffected and
            // the new text is paint-only. A `Shrink` height also re-flows.
            let value_affects_layout = value_changed
                && (matches!(self.style.width, Size::Shrink)
                    || matches!(self.style.height, Size::Shrink));
            UpdateResult {
                absorbed: true,
                needs_layout: style_changed || value_affects_layout,
                needs_repaint: style_changed || value_changed,
                cancelled_unmount_prefixes: Vec::new(),
                drained_path_prefixes: Vec::new(),
            }
        } else {
            UpdateResult::replace()
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
        ctx: &LayoutContext,
        parent_direction: &Direction,
        parent_width: f32,
        parent_available_width: f32,
        parent_height: f32,
        parent_available_height: f32,
        sibling_basis: f32,
    ) -> (f32, f32) {
        let fc = ctx.font_collection;
        let font = ctx.default_font;
        let chrome_w = self.style.margin.get_left()
            + self.style.margin.get_right()
            + self.style.padding.get_left()
            + self.style.padding.get_right();
        let chrome_h = self.style.margin.get_top()
            + self.style.margin.get_bottom()
            + self.style.padding.get_top()
            + self.style.padding.get_bottom();

        let width = match self.style.width {
            Size::Fixed(w) => w,
            Size::Shrink => {
                let intrinsic = text_layout::measure(
                    fc,
                    font,
                    &self.text_style,
                    &self.value,
                    f32::INFINITY,
                )
                .intrinsic_width;
                let min = if self.value.is_empty() {
                    EMPTY_MIN_CONTENT_WIDTH
                } else {
                    0.0
                };
                let want = intrinsic.max(min) + chrome_w;
                if parent_available_width > 0.0 {
                    want.min(parent_available_width)
                } else {
                    want
                }
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
                // Multi-line: grow to fit the text wrapped at the content width.
                let content_w = (width - chrome_w).max(0.0);
                let wrap_w = if content_w > 0.0 {
                    content_w
                } else {
                    f32::INFINITY
                };
                text_layout::measure(fc, font, &self.text_style, &self.value, wrap_w).height
                    + chrome_h
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

        // Pointer: focus the field and place the caret where it was clicked.
        if let Some(point) = &event.point {
            if self.contains_point(point)
                && (event.name == "mouse_down" || event.name == "mouse_up")
            {
                ctx.request_focus(self_ref.clone());
                ctx.listener_fired = true;
                let idx = self.index_at_point(point);
                self.set_caret(idx, false);
                if let Some(listeners) = self.event_listeners.get(&event.name) {
                    for listener in listeners {
                        listener(event);
                    }
                }
                event_handled = true;
            }
        }

        // Keyboard, only when focused.
        if event.name.starts_with("key") && ctx.is_focused(self_ref) {
            let mut value_mutated = false;
            if let Some(keyboard_data) = &event.keyboard_data {
                let shift = keyboard_data.modifiers.shift;
                let cmd = keyboard_data.modifiers.ctrl || keyboard_data.modifiers.meta;
                match event.name.as_str() {
                    "keydown" => {
                        if let Some(key_code) = keyboard_data.key_code {
                            match key_code {
                                8 => {
                                    value_mutated = self.delete_backward();
                                    event_handled = true;
                                }
                                46 => {
                                    value_mutated = self.delete_forward();
                                    event_handled = true;
                                }
                                37 => {
                                    self.move_left(shift);
                                    event_handled = true;
                                }
                                39 => {
                                    self.move_right(shift);
                                    event_handled = true;
                                }
                                // Home / Up → start of field; End / Down → end.
                                36 | 38 => {
                                    self.move_to_start(shift);
                                    event_handled = true;
                                }
                                35 | 40 => {
                                    self.move_to_end(shift);
                                    event_handled = true;
                                }
                                // Enter → submit.
                                13 => {
                                    self.fire_submit();
                                    event_handled = true;
                                }
                                // Ctrl/Cmd+A → select all.
                                97 if cmd => {
                                    self.select_all();
                                    event_handled = true;
                                }
                                _ => {}
                            }
                        }
                    }
                    "keypress" => {
                        if let Some(character) = keyboard_data.character {
                            // Insert printable characters only. Skip control
                            // chars (Tab/Enter/etc.) and anything chorded with
                            // Ctrl/Cmd (those are shortcuts, not text).
                            if !cmd && !character.is_control() {
                                self.insert_char(character);
                                value_mutated = true;
                                event_handled = true;
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Layout only depends on value when an axis is `Shrink`.
            if value_mutated
                && (matches!(self.style.width, Size::Shrink)
                    || matches!(self.style.height, Size::Shrink))
            {
                ctx.request_layout();
            }
            if value_mutated {
                self.fire_change();
            }
            // Caret may have moved even without a value change — keep it in view.
            if event_handled {
                self.update_scroll();
            }
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
        // Cache the fonts so render/events can measure with the same collection.
        self.measure_fc = ctx.font_collection.cloned();
        self.measure_font = ctx.default_font.map(|s| s.to_string());

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
        self.update_scroll();
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
        ctx: &mut dyn crate::widget::RenderContext,
        focused: bool,
        _image_cache: &mut crate::widget::image::ImageCache,
    ) {
        let Some(layout) = &self.layout else {
            return;
        };
        let style = self.effective_style(focused);
        let text_style = self.effective_text_style(focused);

        let box_x = layout.x + style.margin.get_left();
        let box_y = layout.y + style.margin.get_top();
        let box_width = layout.width - style.margin.get_left() - style.margin.get_right();
        let box_height = layout.height - style.margin.get_top() - style.margin.get_bottom();

        // Background
        let bg = style
            .background_color
            .unwrap_or(crate::widget::style::Color::new(255, 255, 255, 255));
        if style.corners.is_all_sharp() {
            ctx.fill_rect(box_x, box_y, box_width, box_height, &bg);
        } else {
            ctx.fill_corners_rect(box_x, box_y, box_width, box_height, &style.corners, &bg);
        }

        // Borders
        ctx.draw_border(
            &style.border,
            box_x,
            box_y,
            box_width,
            box_height,
            &style.corners,
        );

        let padding_left = style.padding.get_left();
        let padding_right = style.padding.get_right();
        let padding_top = style.padding.get_top();
        let padding_bottom = style.padding.get_bottom();
        let text_x = box_x + padding_left;
        let text_y = box_y + padding_top;
        let content_w = (box_width - padding_left - padding_right).max(0.0);
        let content_h = (box_height - padding_top - padding_bottom).max(0.0);
        let wrap_w = if self.is_multiline() {
            content_w
        } else {
            f32::INFINITY
        };

        // Clip text / caret / selection to the content box so overflow (and the
        // horizontally-scrolled single-line case) doesn't spill past the box.
        ctx.push_clip_rect(text_x, box_y, content_w, box_height);

        let origin_x = text_x - self.scroll_x;
        let origin_y = text_y;

        // Selection highlight (painted under the text).
        if focused && !self.selection.is_collapsed() {
            let sel = crate::widget::style::Color::new(51, 153, 255, 90);
            for (rx, ry, rw, rh) in text_layout::selection_rects(
                self.measure_fc.as_ref(),
                self.measure_font.as_deref(),
                text_style,
                &self.value,
                wrap_w,
                self.selection.start(),
                self.selection.end(),
            ) {
                ctx.fill_rect(origin_x + rx, origin_y + ry, rw, rh, &sel);
            }
        }

        // Text
        if !self.value.is_empty() {
            ctx.draw_text(&self.value, text_style, origin_x, origin_y, wrap_w);
        }

        // Caret
        if focused {
            let caret = text_layout::caret_geometry(
                self.measure_fc.as_ref(),
                self.measure_font.as_deref(),
                text_style,
                &self.value,
                wrap_w,
                self.selection.caret,
            );
            let caret_x = origin_x + caret.x;
            let caret_y1 = origin_y + caret.top;
            let caret_y2 = caret_y1 + caret.height;
            ctx.draw_line(
                caret_x,
                caret_y1,
                caret_x,
                caret_y2,
                1.0,
                &crate::widget::style::Color::new(0, 0, 0, 255),
            );
        }

        ctx.pop_clip_rect();
        let _ = content_h;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(value: &str) -> TextInputWidget {
        TextInputWidget::with_value(value.to_string())
    }

    #[test]
    fn with_value_places_caret_at_end_collapsed() {
        let w = input("hello");
        assert_eq!(w.selection.caret, 5);
        assert!(w.selection.is_collapsed());
    }

    #[test]
    fn insert_ascii_at_caret() {
        let mut w = input("ac");
        w.set_caret(1, false);
        w.insert_char('b');
        assert_eq!(w.value, "abc");
        assert_eq!(w.selection.caret, 2);
    }

    #[test]
    fn insert_and_navigate_multibyte_does_not_panic() {
        // 'é' is 2 bytes, '🦀' is 4 bytes — arrow movement must land on char
        // boundaries so the following edits never split a codepoint.
        let mut w = input("é🦀z");
        w.move_to_start(false);
        assert_eq!(w.selection.caret, 0);
        w.move_right(false); // past 'é'
        assert_eq!(w.selection.caret, 2);
        w.move_right(false); // past '🦀'
        assert_eq!(w.selection.caret, 6);
        w.move_right(false); // past 'z'
        assert_eq!(w.selection.caret, 7);
        w.move_right(false); // clamp at end
        assert_eq!(w.selection.caret, 7);
    }

    #[test]
    fn backspace_removes_whole_codepoint() {
        let mut w = input("a🦀");
        // caret at end (byte 5). Backspace must delete the 4-byte crab cleanly.
        assert!(w.delete_backward());
        assert_eq!(w.value, "a");
        assert_eq!(w.selection.caret, 1);
        assert!(w.delete_backward());
        assert_eq!(w.value, "");
        assert!(!w.delete_backward()); // empty → no change
    }

    #[test]
    fn forward_delete_removes_whole_codepoint() {
        let mut w = input("é!");
        w.move_to_start(false);
        assert!(w.delete_forward());
        assert_eq!(w.value, "!");
        assert_eq!(w.selection.caret, 0);
    }

    #[test]
    fn typing_replaces_selection() {
        let mut w = input("hello");
        w.selection = Selection { anchor: 0, caret: 5 };
        w.insert_char('y');
        assert_eq!(w.value, "y");
        assert_eq!(w.selection.caret, 1);
        assert!(w.selection.is_collapsed());
    }

    #[test]
    fn backspace_deletes_selection_not_just_one_char() {
        let mut w = input("hello");
        w.selection = Selection { anchor: 1, caret: 4 }; // "ell"
        assert!(w.delete_backward());
        assert_eq!(w.value, "ho");
        assert_eq!(w.selection.caret, 1);
    }

    #[test]
    fn shift_arrow_extends_then_unshifted_collapses() {
        let mut w = input("hello");
        w.move_to_start(false);
        w.move_right(true); // extend over 'h'
        w.move_right(true); // extend over 'e'
        assert_eq!(w.selection.anchor, 0);
        assert_eq!(w.selection.caret, 2);
        // Unshifted left collapses to the selection start, not caret-1.
        w.move_left(false);
        assert!(w.selection.is_collapsed());
        assert_eq!(w.selection.caret, 0);
    }

    #[test]
    fn select_all_spans_value() {
        let mut w = input("abc");
        w.select_all();
        assert_eq!(w.selection.start(), 0);
        assert_eq!(w.selection.end(), 3);
    }

    #[test]
    fn set_value_clamps_selection_to_boundary() {
        let mut w = input("hello world");
        w.set_caret(11, false);
        w.set_value("hi".to_string());
        assert!(w.selection.caret <= 2);
        assert!(w.value.is_char_boundary(w.selection.caret));
    }
}
