use crate::app::{ClientUI, ClientUpdate};
use crate::home_page::HOME_PAGE;
use crate::input::Input;
use ogham::runtime::config::RuntimeConfig;
use ogham::tree::event::Event;
use ogham::tree::UI;
use ogham::Ogham;
use skia_safe::Surface as SkiaSurface;
use winit::keyboard::NamedKey;
use winit::{keyboard::Key, window::Window};

pub struct Client {
    width: u32,
    height: u32,
    dpi_scale: f32,
    dirty: bool,
    ogham: Ogham,
    path: Option<String>,
}

impl Client {
    pub fn new(width: u32, height: u32) -> Self {
        let src = HOME_PAGE.to_string();
        let ogham = Ogham::from_source(&src, RuntimeConfig::new())
            .expect("Failed to create Ogham from HOME_PAGE");
        Self {
            width,
            height,
            dpi_scale: 1.0,
            dirty: false,
            ogham,
            path: None,
        }
    }

    pub fn set_dpi_scale(&mut self, dpi_scale: f32) {
        self.dpi_scale = dpi_scale;
    }

    pub fn set_screen_size(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    fn update_impl(&mut self, input: &mut Input, _frame_length: f32) {
        // Check for Ctrl+O to open file dialog
        if input.pressed(Key::Character("o".into())) && input.held(Key::Named(NamedKey::Control)) {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Ogham files", &["ogh"])
                .add_filter("All files", &["*"])
                .pick_file()
            {
                let path_str = path.to_string_lossy().to_string();
                if let Err(err) = self.ogham.load_file(path_str.clone()) {
                    eprintln!("[ogham] Error loading {}: {}", path_str, err);
                } else {
                    self.path = Some(path_str);
                    self.dirty = true;
                }
            }
        }

        // Check for file change events
        if self.ogham.check_for_changes() {
            if let Err(err) = self.ogham.reload() {
                if let Some(path) = self.path.as_ref() {
                    eprintln!("[ogham] Error reloading {}: {}", path, err);
                } else {
                    eprintln!("[ogham] Error reloading: {}", err);
                }
            } else {
                self.dirty = true;
            }
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn clean(&mut self) {
        self.dirty = false;
    }

    /// Handle a UI event and return whether it was handled
    pub fn handle_ui_event(&mut self, event: &Event) -> bool {
        self.ogham.get_ui_mut().call_event(event)
    }

    /// Check if UI is dirty
    pub fn is_ui_dirty(&self) -> bool {
        self.ogham.get_ui().is_dirty()
    }

    /// True when the UI needs to be updated (tree dirty or runtime state changed).
    pub fn needs_ui_update(&self) -> bool {
        self.ogham.get_ui().is_dirty() || self.ogham.get_runtime().lock().expect("runtime lock poisoned").needs_rerender()
    }

    /// Update UI if dirty, then layout with the given dimensions
    pub fn update_ui_layout(&mut self, width: f32, height: f32) {
        // Check if runtime needs a rerender due to state updates
        let needs_rerender = {
            let runtime = self.ogham.get_runtime();
            runtime.lock().expect("runtime lock poisoned").needs_rerender()
        };

        if needs_rerender {
            // Rerender the module to get updated widget tree
            let runtime = self.ogham.get_runtime().clone();
            let widget_value = {
                let mut rt = runtime.lock().expect("runtime lock poisoned");
                rt.rerender()
                    .map_err(|e| {
                        eprintln!("[ogham] Rerender error: {:?}", e);
                        e
                    })
                    .ok()
            };

            if let Some(widget_value) = widget_value {
                // Convert the widget value to a WidgetRef
                let new_root =
                    ogham::tree::ast_bridge::widget_value_to_widget_ref(&runtime, &widget_value)
                        .map_err(|e| {
                            eprintln!("[ogham] Bridge error during rerender: {:?}", e);
                            e
                        })
                        .ok();

                if let Some(new_root) = new_root {
                    self.ogham.get_ui_mut().reconcile(new_root);
                }
            }
        }

        self.ogham.get_ui_mut().layout(width, height);
    }

    /// Get mutable reference to UI for drawing
    pub fn get_ui_mut(&mut self) -> &mut UI {
        self.ogham.get_ui_mut()
    }

    /// Update cursor state based on current view
    /// Locks and hides cursor in sandbox mode, unlocks and shows it in main menu
    pub fn update_cursor_state(&mut self, input: &mut Input, window: &Window) {
        input.unlock_cursor(window);
    }
}

impl ClientUpdate for Client {
    fn update(&mut self, input: &mut Input, frame_length: f32) {
        self.update_impl(input, frame_length);
    }
}

impl ClientUI for Client {
    fn handle_ui_event(&mut self, event: &Event) -> bool {
        self.handle_ui_event(event)
    }

    fn is_ui_dirty(&self) -> bool {
        self.is_ui_dirty()
    }

    fn needs_ui_update(&self) -> bool {
        self.needs_ui_update()
    }

    fn update_ui_layout(&mut self, width: f32, height: f32) {
        self.update_ui_layout(width, height)
    }

    fn get_ui_mut(&mut self) -> &mut UI {
        self.get_ui_mut()
    }

    fn render(&mut self, _surface: &mut SkiaSurface) {}
}
