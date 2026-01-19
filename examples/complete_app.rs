//! Complete example showing how to integrate Ogham into a real application.
//!
//! This example demonstrates:
//! - Setting up host state
//! - Compiling a UI file
//! - File watching for hot-reloading
//! - Event handling
//! - Error handling
//!
//! In a real application, you would integrate this with your windowing
//! system (like winit), rendering system (like Skia), and event loop.

use ogham::runtime::{FileWatcher, RuntimeConfig, RuntimeError};
use ogham::tree::{UI, Event, Point};
use ogham::vm::Value;
use std::collections::HashMap;

struct App {
    ui: UI,
    watcher: FileWatcher,
    config: RuntimeConfig,
}

impl App {
    /// Initialize the application with an Ogham UI file
    fn new(ui_file: &str) -> Result<Self, RuntimeError> {
        // Step 1: Set up host state - this is data from your application
        // that you want to make available to the Ogham script
        let mut host_state = HashMap::new();
        
        // Example: Pass application configuration
        host_state.insert(
            "app_name".to_string(),
            Value::String("My Application".to_string()),
        );
        host_state.insert(
            "version".to_string(),
            Value::String("1.0.0".to_string()),
        );
        
        // Example: Pass user data
        host_state.insert(
            "user_name".to_string(),
            Value::String("Alice".to_string()),
        );
        host_state.insert(
            "user_id".to_string(),
            Value::Integer(12345),
        );
        host_state.insert(
            "is_premium".to_string(),
            Value::Boolean(true),
        );
        
        // Example: Pass window dimensions (you'd update these on resize)
        host_state.insert(
            "window_width".to_string(),
            Value::Integer(1024),
        );
        host_state.insert(
            "window_height".to_string(),
            Value::Integer(768),
        );
        
        // Step 2: Create runtime configuration
        let config = RuntimeConfig::new()
            .with_host_state(host_state)
            .with_event_handler(|event_name, data| {
                // Handle events from the Ogham UI
                // This callback is called when the UI emits events
                match event_name {
                    "button_clicked" => {
                        println!("Button clicked!");
                        if let Some(Value::String(button_id)) = data {
                            println!("  Button ID: {}", button_id);
                        }
                        true // Event was handled
                    }
                    "text_submitted" => {
                        if let Some(Value::String(text)) = data {
                            println!("Text submitted: {}", text);
                        }
                        true
                    }
                    "menu_item_selected" => {
                        if let Some(Value::Integer(item_id)) = data {
                            println!("Menu item {} selected", item_id);
                        }
                        true
                    }
                    _ => {
                        println!("Unhandled event: {} ({:?})", event_name, data);
                        false // Event not handled
                    }
                }
            });
        
        // Step 3: Watch and compile the UI file
        // This sets up file watching for hot-reloading during development
        let (ui, watcher) = ogham::runtime::watch_and_compile(ui_file, Some(config.clone()))?;
        
        Ok(Self {
            ui,
            watcher,
            config,
        })
    }
    
    /// Update the application (called each frame)
    fn update(&mut self) -> Result<(), RuntimeError> {
        // Check if the UI file has changed (for hot-reloading)
        if self.watcher.check_for_changes() {
            println!("UI file changed, recompiling...");
            
            match self.watcher.recompile(Some(self.config.clone())) {
                Ok(new_ui) => {
                    self.ui = new_ui;
                    println!("Recompilation successful!");
                }
                Err(e) => {
                    eprintln!("Recompilation failed: {}", e);
                    // In a real app, you might want to show an error overlay
                }
            }
        }
        
        Ok(())
    }
    
    /// Handle a mouse click event
    fn handle_mouse_click(&mut self, x: f32, y: f32) {
        let event = Event::with_point("mouse_down".to_string(), Point::new(x, y));
        let handled = self.ui.call_event(&event);
        
        if handled {
            println!("UI handled mouse click at ({}, {})", x, y);
        }
    }
    
    /// Handle a keyboard event
    fn handle_key_press(&mut self, key_code: u32, character: Option<char>) {
        use ogham::tree::event::KeyModifiers;
        
        let event = Event::keypress(key_code, character, KeyModifiers::new());
        let handled = self.ui.call_event(&event);
        
        if handled {
            println!("UI handled key press: {:?}", character);
        }
    }
    
    /// Update UI layout (call when window is resized)
    fn resize(&mut self, width: f32, height: f32) {
        self.ui.layout(width, height);
    }
    
    /// Get a reference to the UI for rendering
    fn ui(&self) -> &UI {
        &self.ui
    }
}

fn main() -> Result<(), RuntimeError> {
    println!("=== Ogham Library Integration Example ===\n");
    
    // Initialize the application
    let mut app = App::new("examples/hello_world.ogh")?;
    
    println!("Application initialized successfully!");
    println!("UI file: examples/hello_world.ogh");
    println!("\nIn a real application, you would now:");
    println!("  1. Set up your windowing system (e.g., winit)");
    println!("  2. Set up your rendering system (e.g., Skia)");
    println!("  3. Enter your main event loop");
    println!("\nExample event loop structure:");
    println!("  loop {{");
    println!("    // Handle window events");
    println!("    for event in window.poll_events() {{");
    println!("      match event {{");
    println!("        WindowEvent::Close => break,");
    println!("        WindowEvent::MouseInput {{ position, .. }} => {{");
    println!("          app.handle_mouse_click(position.x, position.y);");
    println!("        }}");
    println!("        WindowEvent::Resized {{ width, height }} => {{");
    println!("          app.resize(width as f32, height as f32);");
    println!("        }}");
    println!("        _ => {{}}");
    println!("      }}");
    println!("    }}");
    println!("    ");
    println!("    // Check for UI file changes (hot-reload)");
    println!("    app.update()?;");
    println!("    ");
    println!("    // Render the UI");
    println!("    renderer.draw(app.ui());");
    println!("  }}");
    
    // Simulate some interactions
    println!("\n=== Simulating some interactions ===\n");
    
    // Simulate a mouse click
    app.handle_mouse_click(100.0, 200.0);
    
    // Simulate a key press
    app.handle_key_press(65, Some('A')); // 'A' key
    
    // Simulate a window resize
    app.resize(1920.0, 1080.0);
    println!("UI layout updated for 1920x1080");
    
    // Simulate checking for file changes
    println!("\n=== Checking for file changes ===\n");
    app.update()?;
    
    println!("\n=== Example complete ===");
    
    Ok(())
}


