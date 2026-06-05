use super::point::Point;
use std::sync::{Arc, Mutex};

use crate::widget::Widget;

/// Phase 3 M0: per-tick context threaded through
/// `Widget::tick_animations`. Widgets push side-effects
/// (drained widgets' path prefixes, cancelled exits) into
/// the context vecs; `UI::tick_animations` drains them after
/// the recursion completes and stores them on UI's pending
/// fields for the next render cycle to consume.
///
/// This replaces the bare `dt: f32` parameter so drag-related
/// side channels (M1) and drain-time unmount machinery (M3)
/// can share infrastructure without growing TickResult into
/// a Vec-bearing struct that loses Copy.
pub struct TickContext {
    pub dt: f32,
    /// Path prefixes whose owning widgets drained this tick.
    /// Populated by `FlexWidget::drain_exited_children` (and
    /// any other widget that drains owned-path-prefix
    /// children). Consumed by `UI::tick_animations` →
    /// `UI.pending_drained_prefixes` → next frame's
    /// `Runtime::process_drain_queues`.
    pub drained_path_prefixes: Vec<String>,
    /// Path prefixes whose pending unmount was cancelled
    /// this tick (re-mount during exit animation). Mirror
    /// of `drained_path_prefixes` for the cancel side.
    pub cancelled_unmount_prefixes: Vec<String>,
}

impl TickContext {
    pub fn new(dt: f32) -> Self {
        Self {
            dt,
            drained_path_prefixes: Vec::new(),
            cancelled_unmount_prefixes: Vec::new(),
        }
    }
}

/// Phase 3 M1: per-frame drag state. The lorekeeper input
/// pump constructs this when the dead-zone state machine
/// (`src/client/input.rs`) detects a drag start; the dispatch
/// path stashes it in `EventContext.drag_state` for widgets
/// to read (e.g. for hover highlighting based on
/// `accepts_drop`).
///
/// M0 declares the struct as a placeholder (`Default::default`
/// produces an empty/sentinel value); M1 wires the actual
/// fields and dispatch.
#[derive(Clone, Default)]
pub struct DragState {
    /// The widget where the drag originated. Set when
    /// `mouse_down` past the dead-zone fires `drag_start`.
    pub origin_widget: Option<Arc<Mutex<dyn Widget>>>,
    /// The payload value declared by the originator. Untyped
    /// `Value` per design decision #4 — consumer convention
    /// (e.g. `kind: "inventory_item"` field) handles
    /// discrimination.
    pub payload: Option<crate::runtime::value::Value>,
    /// Cursor position when drag_start fired. Widget-local to
    /// the originator at start; reused as the anchor for
    /// past-dead-zone calculations.
    pub start_position: Point,
    /// Cursor position at the most recent drag_move dispatch.
    pub current_position: Point,
    /// True once the cursor has moved past the dead-zone (4px
    /// default; per-widget override). Drag events only fire
    /// after this flips true.
    pub past_dead_zone: bool,
}

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
    /// Phase 3 M0: drag state if a drag is in flight when
    /// this event was dispatched. Set by `UI::dispatch_drag_*`
    /// (M1 wires the actual dispatch); widgets read for hover
    /// highlighting, accepts-drop checks, etc. M0 ships the
    /// field as a placeholder so M1 can fill the dispatch
    /// path without re-touching every event call site.
    pub drag_state: Option<DragState>,
    /// Set true by a widget whose event handling mutated its own
    /// state in a way that affects the layout this frame (e.g. a
    /// `TextInput` with `width: Shrink` ingesting a character).
    /// Most handlers just fire host events — the resulting state
    /// flows back through reconcile, so a same-frame layout pass
    /// is unnecessary. `UI::call_event` checks this flag on
    /// handled events to choose between `mark_needs_layout` and
    /// the cheaper `mark_needs_repaint`.
    pub needs_layout: bool,
}

impl EventContext {
    pub fn new() -> Self {
        Self {
            focus_request: None,
            focused_widget: None,
            listener_fired: false,
            drag_state: None,
            needs_layout: false,
        }
    }

    /// Create a new EventContext with a focused widget reference
    pub fn with_focused(focused_widget: Option<Arc<Mutex<dyn Widget>>>) -> Self {
        Self {
            focus_request: None,
            focused_widget,
            listener_fired: false,
            drag_state: None,
            needs_layout: false,
        }
    }

    /// Signal that the just-processed event mutated widget state in a way
    /// that requires a layout pass this frame. See [`needs_layout`] for
    /// the contract. Idempotent.
    pub fn request_layout(&mut self) {
        self.needs_layout = true;
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
    /// Phase 3 M1: drag-event payload. Carries the
    /// `drag_payload` declared on the originating widget
    /// through `drag_start` / `drag_move` / `drag_end`
    /// dispatches so listener closures can read it.
    pub payload: Option<crate::runtime::value::Value>,
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
            payload: None,
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
            payload: None,
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
            payload: None,
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
            payload: None,
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
            payload: None,
        }
    }

    /// Phase 3 M1: construct a drag-event (`drag_start`,
    /// `drag_move`, `drag_end`) carrying the current cursor
    /// point and the originator's `drag_payload` Value.
    pub fn drag(name: &str, point: Point, payload: Option<crate::runtime::value::Value>) -> Self {
        Self {
            name: name.to_string(),
            point: Some(point),
            keyboard_data: None,
            callback: None,
            value: None,
            scroll_delta: None,
            payload,
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
            point: self
                .point
                .as_ref()
                .map(|p| Point::new(p.x() + dx, p.y() + dy)),
            keyboard_data: self.keyboard_data.clone(),
            callback: None,
            value: self.value.clone(),
            scroll_delta: self.scroll_delta,
            payload: self.payload.clone(),
        }
    }
}
