use super::point::Point;
use std::sync::{Arc, Mutex};

use crate::widget::Widget;

/// EventContext is used to communicate UI-level state changes from widgets
/// back to the root UI during event handling. This allows widgets to request
/// actions that require coordination at the UI level (like focus management,
/// cursor style changes, etc.) without needing direct access to the UI struct.
pub struct EventContext {
    /// Widget that should receive focus (if any)
    focus_request: Option<Arc<Mutex<dyn Widget>>>,
    /// Currently focused widget (if any)
    pub focused_widget: Option<Arc<Mutex<dyn Widget>>>,
    /// Set true by any widget that fires a pointer listener during the
    /// current dispatch. Lets ancestors distinguish "a descendant fired a
    /// real listener" from "a descendant returned `true` only because its
    /// `block_interactions` was set". Reset by the dispatcher between
    /// top-level events.
    pub listener_fired: bool,
}

impl EventContext {
    pub fn new() -> Self {
        Self {
            focus_request: None,
            focused_widget: None,
            listener_fired: false,
        }
    }

    /// Create a new EventContext with a focused widget reference
    pub fn with_focused(focused_widget: Option<Arc<Mutex<dyn Widget>>>) -> Self {
        Self {
            focus_request: None,
            focused_widget,
            listener_fired: false,
        }
    }

    /// Request that a widget receives focus
    pub fn request_focus(&mut self, widget: Arc<Mutex<dyn Widget>>) {
        self.focus_request = Some(widget);
    }

    /// Take the focus request, consuming it
    pub fn take_focus_request(&mut self) -> Option<Arc<Mutex<dyn Widget>>> {
        self.focus_request.take()
    }

    /// Check if the given widget reference is the currently focused widget
    /// This compares the inner pointers of the Arc, not the Arc instances themselves,
    /// since cloned Arcs are different instances but point to the same data.
    pub fn is_focused(&self, widget_ref: &Arc<Mutex<dyn Widget>>) -> bool {
        if let Some(ref focused) = self.focused_widget {
            let focused_ptr = Arc::as_ptr(focused);
            let widget_ptr = Arc::as_ptr(widget_ref);
            let is_eq = std::ptr::eq(focused_ptr, widget_ptr);
            is_eq
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
pub struct KeyboardData {
    pub key_code: Option<u32>,
    pub character: Option<char>,
    pub modifiers: KeyModifiers,
}

#[derive(Debug, Clone, Default)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl KeyModifiers {}

pub struct Event {
    pub name: String,
    pub point: Option<Point>,
    pub keyboard_data: Option<KeyboardData>,
    pub callback: Option<Box<dyn Fn(&Event)>>,
    pub value: Option<String>,
    /// Scroll wheel delta (dx, dy) in logical pixels.
    pub scroll_delta: Option<(f32, f32)>,
}

impl Event {
    pub fn new(name: String) -> Self {
        Self {
            name,
            point: None,
            keyboard_data: None,
            callback: None,
            value: None,
            scroll_delta: None,
        }
    }

    pub fn with_point(name: String, point: Point) -> Self {
        Self {
            name,
            point: Some(point),
            keyboard_data: None,
            callback: None,
            value: None,
            scroll_delta: None,
        }
    }

    pub fn with_keyboard(
        name: String,
        key_code: u32,
        character: Option<char>,
        modifiers: KeyModifiers,
    ) -> Self {
        Self {
            name,
            point: None,
            keyboard_data: Some(KeyboardData {
                key_code: Some(key_code),
                character,
                modifiers,
            }),
            callback: None,
            value: None,
            scroll_delta: None,
        }
    }

    pub fn with_value(name: String, value: String) -> Self {
        Self {
            name,
            point: None,
            keyboard_data: None,
            callback: None,
            value: Some(value),
            scroll_delta: None,
        }
    }

    pub fn scroll(point: Point, dx: f32, dy: f32) -> Self {
        Self {
            name: "scroll".to_string(),
            point: Some(point),
            keyboard_data: None,
            callback: None,
            value: None,
            scroll_delta: Some((dx, dy)),
        }
    }

    pub fn keydown(key_code: u32, character: Option<char>, modifiers: KeyModifiers) -> Self {
        Self::with_keyboard("keydown".to_string(), key_code, character, modifiers)
    }

    pub fn keypress(key_code: u32, character: Option<char>, modifiers: KeyModifiers) -> Self {
        Self::with_keyboard("keypress".to_string(), key_code, character, modifiers)
    }

    pub fn keyup(key_code: u32, character: Option<char>, modifiers: KeyModifiers) -> Self {
        Self::with_keyboard("keyup".to_string(), key_code, character, modifiers)
    }

    /// Returns a copy of this event with its point shifted by `(dx, dy)`.
    /// The hit-test walker uses this to translate the event point into a
    /// child widget's local coordinate space before recursing. The
    /// `callback` field (not cloneable) is dropped.
    pub fn shift_point(&self, dx: f32, dy: f32) -> Event {
        Self {
            name: self.name.clone(),
            point: self.point.as_ref().map(|p| Point::new(p.x() + dx, p.y() + dy)),
            keyboard_data: self.keyboard_data.clone(),
            callback: None,
            value: self.value.clone(),
            scroll_delta: self.scroll_delta,
        }
    }
}
