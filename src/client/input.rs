use std::collections::HashMap;
use std::time::Instant;

use nalgebra_glm as glm;
use winit::{event::MouseButton, keyboard::Key, window::Window};

pub struct KeyState {
    held: bool,
    pressed: bool,
    released: bool,
}

impl Default for KeyState {
    fn default() -> Self {
        KeyState {
            held: false,
            pressed: false,
            released: false,
        }
    }
}

struct MouseButtonState {
    key_state: KeyState,
    drag_start_position: Option<glm::Vec2>,
    drag_threshold: f32,
    should_clear_drag_start: bool,
    last_click_time: Option<Instant>,
    last_click_position: Option<glm::Vec2>,
    double_click_detected: bool,
    double_click_threshold_ms: u128,
    double_click_distance_threshold: f32,
}

impl Default for MouseButtonState {
    fn default() -> Self {
        MouseButtonState {
            key_state: KeyState::default(),
            drag_start_position: None,
            drag_threshold: 5.0,
            should_clear_drag_start: false,
            last_click_time: None,
            last_click_position: None,
            double_click_detected: false,
            double_click_threshold_ms: 400,
            double_click_distance_threshold: 5.0,
        }
    }
}

pub struct Input {
    key_map: HashMap<Key, KeyState>,
    cursor_delta: glm::Vec2,
    cursor_position: glm::Vec2,
    pending_cursor_position: glm::Vec2,
    mouse_button_map: HashMap<MouseButton, MouseButtonState>,
    cursor_locked: bool,
    cursor_visible: bool,
    scroll_delta: glm::Vec2,
}

impl Input {
    pub fn new() -> Self {
        Input {
            key_map: HashMap::new(),
            cursor_delta: glm::Vec2::zeros(),
            cursor_position: glm::Vec2::zeros(),
            pending_cursor_position: glm::Vec2::zeros(),
            mouse_button_map: HashMap::new(),
            cursor_locked: false,
            cursor_visible: true,
            scroll_delta: glm::Vec2::zeros(),
        }
    }

    pub fn update(&mut self, window: &Window) {
        // Update key states
        for (_, state) in self.key_map.iter_mut() {
            if state.pressed {
                state.held = true;
                state.pressed = false;
                state.released = false;
            } else if state.released {
                state.held = false;
                state.pressed = false;
                state.released = false;
            }
        }

        // Update mouse button states
        for (_, state) in self.mouse_button_map.iter_mut() {
            if state.key_state.pressed {
                state.key_state.held = true;
                state.key_state.pressed = false;
                state.key_state.released = false;
            } else if state.key_state.released {
                state.key_state.held = false;
                state.key_state.pressed = false;
                state.key_state.released = false;
                // Don't clear drag start position here - it will be cleared in the next update
                // after the drag detection methods have been called
            }

            // Clear drag start position if it was marked for clearing
            if state.should_clear_drag_start {
                state.drag_start_position = None;
                state.should_clear_drag_start = false;
            }

            // Clear double click detected flag
            state.double_click_detected = false;
        }

        self.cursor_delta = self.pending_cursor_position - self.cursor_position;
        self.cursor_position = self.pending_cursor_position;

        // Clear scroll delta each frame
        self.scroll_delta = glm::Vec2::zeros();

        if self.is_cursor_locked() {
            self.center_window_cursor(window);
        }
    }

    pub fn mouse_button_pressed(&self, button: MouseButton) -> bool {
        let state = self.mouse_button_map.get(&button);
        if let Some(state) = state {
            state.key_state.pressed
        } else {
            false
        }
    }

    pub fn mouse_button_held(&self, button: MouseButton) -> bool {
        let state = self.mouse_button_map.get(&button);
        if let Some(state) = state {
            state.key_state.held
        } else {
            false
        }
    }

    pub fn mouse_button_pressed_or_held(&self, button: MouseButton) -> bool {
        let state = self.mouse_button_map.get(&button);
        if let Some(state) = state {
            state.key_state.pressed || state.key_state.held
        } else {
            false
        }
    }

    pub fn mouse_button_released(&self, button: MouseButton) -> bool {
        let state = self.mouse_button_map.get(&button);
        if let Some(state) = state {
            state.key_state.released
        } else {
            false
        }
    }

    /// Check if a mouse button was released after a drag (not a click)
    pub fn mouse_button_released_after_drag(&mut self, button: MouseButton) -> bool {
        let state = self.mouse_button_map.get_mut(&button);
        if let Some(state) = state {
            // Only return true if the button was actually released this frame
            if !state.key_state.released {
                return false;
            }

            if let Some(drag_start) = state.drag_start_position {
                let drag_distance = (self.cursor_position - drag_start).norm();
                let is_drag = drag_distance >= state.drag_threshold;
                if is_drag {
                    state.should_clear_drag_start = true;
                }
                is_drag
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Check if a mouse button was released as a click (not a drag)
    pub fn mouse_button_released_as_click(&mut self, button: MouseButton) -> bool {
        let state = self.mouse_button_map.get_mut(&button);
        if let Some(state) = state {
            // Only return true if the button was actually released this frame
            if !state.key_state.released {
                return false;
            }

            if let Some(drag_start) = state.drag_start_position {
                let drag_distance = (self.cursor_position - drag_start).norm();
                let is_click = drag_distance < state.drag_threshold;
                if is_click {
                    state.should_clear_drag_start = true;

                    // Check for double-click
                    let now = Instant::now();
                    if let (Some(last_time), Some(last_pos)) =
                        (state.last_click_time, state.last_click_position)
                    {
                        let time_diff = now.duration_since(last_time).as_millis();
                        let pos_diff = (self.cursor_position - last_pos).norm();

                        if time_diff <= state.double_click_threshold_ms
                            && pos_diff <= state.double_click_distance_threshold
                        {
                            state.double_click_detected = true;
                            // Reset click tracking after double-click
                            state.last_click_time = None;
                            state.last_click_position = None;
                        } else {
                            // Record this click for potential double-click
                            state.last_click_time = Some(now);
                            state.last_click_position = Some(self.cursor_position);
                        }
                    } else {
                        // First click, record it
                        state.last_click_time = Some(now);
                        state.last_click_position = Some(self.cursor_position);
                    }
                }
                is_click
            } else {
                // If no drag start position and button was released, treat as click
                state.should_clear_drag_start = true;

                // Check for double-click even without drag_start
                let now = Instant::now();
                if let (Some(last_time), Some(last_pos)) =
                    (state.last_click_time, state.last_click_position)
                {
                    let time_diff = now.duration_since(last_time).as_millis();
                    let pos_diff = (self.cursor_position - last_pos).norm();

                    if time_diff <= state.double_click_threshold_ms
                        && pos_diff <= state.double_click_distance_threshold
                    {
                        state.double_click_detected = true;
                        state.last_click_time = None;
                        state.last_click_position = None;
                    } else {
                        state.last_click_time = Some(now);
                        state.last_click_position = Some(self.cursor_position);
                    }
                } else {
                    state.last_click_time = Some(now);
                    state.last_click_position = Some(self.cursor_position);
                }

                true
            }
        } else {
            false
        }
    }

    /// Check if a mouse button was double-clicked
    pub fn mouse_button_double_clicked(&self, button: MouseButton) -> bool {
        let state = self.mouse_button_map.get(&button);
        if let Some(state) = state {
            state.double_click_detected
        } else {
            false
        }
    }

    /// Set the drag threshold for a specific mouse button
    pub fn set_mouse_button_drag_threshold(&mut self, button: MouseButton, threshold: f32) {
        let state = self
            .mouse_button_map
            .entry(button)
            .or_insert_with(MouseButtonState::default);
        state.drag_threshold = threshold;
    }

    fn normalize_key(key: Key) -> Key {
        match key {
            Key::Character(c) => {
                let lower = c.to_lowercase();
                Key::Character(lower.into())
            }
            _ => key,
        }
    }

    pub fn pressed(&self, key: Key) -> bool {
        let normalized_key = Self::normalize_key(key);
        let state = self.key_map.get(&normalized_key);
        if let Some(state) = state {
            state.pressed
        } else {
            false
        }
    }

    pub fn pressed_or_held(&self, key: Key) -> bool {
        let normalized_key = Self::normalize_key(key);
        let state = self.key_map.get(&normalized_key);
        if let Some(state) = state {
            state.pressed || state.held
        } else {
            false
        }
    }

    pub fn released(&self, key: Key) -> bool {
        let normalized_key = Self::normalize_key(key);
        let state = self.key_map.get(&normalized_key);
        if let Some(state) = state {
            state.released
        } else {
            false
        }
    }

    pub fn held(&self, key: Key) -> bool {
        let normalized_key = Self::normalize_key(key);
        let state = self.key_map.get(&normalized_key);
        if let Some(state) = state {
            state.held
        } else {
            false
        }
    }

    pub fn press(&mut self, key: Key) {
        let normalized_key = Self::normalize_key(key.clone());
        let state = self
            .key_map
            .entry(normalized_key)
            .or_insert_with(KeyState::default);
        if !state.held {
            state.pressed = true;
            state.held = false;
            state.released = false;
        }
    }

    pub fn release(&mut self, key: Key) {
        let normalized_key = Self::normalize_key(key.clone());
        let state = self
            .key_map
            .entry(normalized_key)
            .or_insert_with(KeyState::default);
        state.pressed = false;
        state.held = false;
        state.released = true;
    }

    pub fn press_mouse_button(&mut self, button: MouseButton) {
        let state = self
            .mouse_button_map
            .entry(button)
            .or_insert_with(MouseButtonState::default);
        state.key_state.pressed = true;
        state.key_state.held = false;
        state.key_state.released = false;
        // Record the drag start position when button is pressed
        state.drag_start_position = Some(self.cursor_position);
    }

    pub fn release_mouse_button(&mut self, button: MouseButton) {
        let state = self
            .mouse_button_map
            .entry(button)
            .or_insert_with(MouseButtonState::default);
        state.key_state.pressed = false;
        state.key_state.held = false;
        state.key_state.released = true;
        // Don't clear drag_start_position here - it will be cleared in update() after checking for drag
    }

    pub fn has_input(&self) -> bool {
        self.key_map
            .values()
            .any(|state| state.pressed || state.held)
            || self
                .mouse_button_map
                .values()
                .any(|state| state.key_state.pressed || state.key_state.held)
    }

    pub fn lock_cursor(&mut self, window: &Window) {
        if !self.is_cursor_locked() {
            // Try Locked mode first, fall back to Confined if it fails
            let grab_result = window
                .set_cursor_grab(winit::window::CursorGrabMode::Locked)
                .or_else(|_| {
                    println!("Locked cursor mode failed, trying Confined mode");
                    window.set_cursor_grab(winit::window::CursorGrabMode::Confined)
                });

            if let Err(e) = grab_result {
                eprintln!("Failed to lock cursor: {}", e);
            } else {
                self.set_cursor_locked(true);
                self.center_window_cursor(window);
            }
        }

        if self.is_cursor_visible() {
            window.set_cursor_visible(false);
            self.set_cursor_visible(false);
        }
    }

    pub fn unlock_cursor(&mut self, window: &Window) {
        if self.is_cursor_locked() {
            if let Err(e) = window.set_cursor_grab(winit::window::CursorGrabMode::None) {
                eprintln!("Failed to unlock cursor: {}", e);
            } else {
                self.set_cursor_locked(false);
            }
        }

        if !self.is_cursor_visible() {
            window.set_cursor_visible(true);
            self.set_cursor_visible(true);
        }
    }

    pub fn move_cursor(&mut self, position: glm::Vec2) {
        self.pending_cursor_position = position;
    }

    pub fn cursor_delta(&self) -> &glm::Vec2 {
        &self.cursor_delta
    }

    pub fn cursor_position(&self) -> &glm::Vec2 {
        &self.cursor_position
    }

    pub fn is_cursor_locked(&self) -> bool {
        self.cursor_locked
    }

    pub fn is_cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    pub fn set_cursor_locked(&mut self, locked: bool) {
        self.cursor_locked = locked;
    }

    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor_visible = visible;
    }

    pub fn set_cursor_delta(&mut self, delta: glm::Vec2) {
        self.cursor_delta = delta;
    }

    pub fn scroll_delta(&self) -> &glm::Vec2 {
        &self.scroll_delta
    }

    pub fn set_scroll_delta(&mut self, delta: glm::Vec2) {
        self.scroll_delta = delta;
    }

    pub fn clear_scroll_delta(&mut self) {
        self.scroll_delta = glm::Vec2::zeros();
    }

    fn center_window_cursor(&mut self, window: &Window) {
        let center_x = window.inner_size().width as f32 / 2.0;
        let center_y = window.inner_size().height as f32 / 2.0;
        self.cursor_position = glm::vec2(center_x, center_y);
        self.pending_cursor_position = glm::vec2(center_x, center_y);
        if let Err(e) = window.set_cursor_position(winit::dpi::PhysicalPosition::new(
            center_x as f64,
            center_y as f64,
        )) {
            eprintln!("Failed to set cursor position: {}", e);
        }
    }
}
