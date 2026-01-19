//! Example demonstrating how to use Ogham as a library in another Rust application.
//!
//! This example shows:
//! - Basic compilation from a file
//! - Using host state to pass data from the host application
//! - File watching for hot-reloading during development
//! - Error handling

use ogham::runtime::{RuntimeConfig, RuntimeError};
use ogham::tree::UI;
use ogham::vm::Value;
use std::collections::HashMap;

fn main() -> Result<(), RuntimeError> {
    // Example 1: Simple compilation
    println!("Example 1: Simple compilation");
    simple_compilation()?;

    // Example 2: Compilation with host state
    println!("\nExample 2: Compilation with host state");
    compilation_with_host_state()?;

    // Example 3: File watching for hot-reloading
    println!("\nExample 3: File watching");
    file_watching_example()?;

    Ok(())
}

/// Example 1: Simple compilation from a file
fn simple_compilation() -> Result<(), RuntimeError> {
    // The simplest way to use Ogham - just provide a file path
    let ui = ogham::compile_file("examples/hello_world.ogh")?;
    
    println!("Successfully compiled UI with {} widgets", count_widgets(&ui));
    
    // Now you can use the UI in your application
    // For example, you might pass it to your rendering system:
    // renderer.draw(&ui);
    
    Ok(())
}

/// Example 2: Compilation with host state
/// 
/// Host state allows you to pass data from your Rust application
/// to the Ogham script. The script can read these values but cannot modify them.
fn compilation_with_host_state() -> Result<(), RuntimeError> {
    // Create host state with data from your application
    let mut host_state = HashMap::new();
    
    // Inject application data
    host_state.insert(
        "app_name".to_string(),
        Value::String("My Awesome App".to_string()),
    );
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
    
    // You can also pass complex data structures
    let mut user_preferences = HashMap::new();
    user_preferences.insert(
        "theme".to_string(),
        Value::String("dark".to_string()),
    );
    user_preferences.insert(
        "language".to_string(),
        Value::String("en".to_string()),
    );
    host_state.insert(
        "preferences".to_string(),
        Value::Map(user_preferences),
    );
    
    // Create configuration with host state
    let config = RuntimeConfig::new()
        .with_host_state(host_state);
    
    // Compile with host state
    // In your Ogham script, you can now access:
    // - app_name
    // - user_name
    // - user_id
    // - is_premium
    // - preferences.theme
    // - preferences.language
    let ui = ogham::runtime::from_file("examples/hello_world.ogh", Some(config))?;
    
    println!("Compiled UI with host state injected");
    
    Ok(())
}

/// Example 3: File watching for hot-reloading during development
///
/// This is useful for development - you can edit the Ogham file
/// and see changes immediately without restarting your application.
fn file_watching_example() -> Result<(), RuntimeError> {
    use ogham::runtime::FileWatcher;
    
    // Watch a file and compile it initially
    let (mut ui, watcher) = ogham::watch_file("examples/hello_world.ogh")?;
    
    println!("Watching file: {}", watcher.path().display());
    println!("Initial compilation complete");
    
    // In a real application, you would check for changes in your event loop:
    /*
    loop {
        // Your application's event loop
        handle_events();
        
        // Check if the Ogham file has changed
        if watcher.check_for_changes() {
            println!("File changed! Recompiling...");
            
            // Recompile with the same configuration (if you had one)
            match watcher.recompile(None) {
                Ok(new_ui) => {
                    ui = new_ui;
                    println!("Recompilation successful");
                    // Trigger a redraw in your application
                    request_redraw();
                }
                Err(e) => {
                    eprintln!("Recompilation failed: {}", e);
                    // You might want to show an error in your UI
                }
            }
        }
        
        // Render your UI
        renderer.draw(&ui);
        
        // Sleep or yield to avoid busy-waiting
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
    */
    
    // For this example, just demonstrate checking once
    if watcher.check_for_changes() {
        println!("File changed detected!");
    } else {
        println!("No changes detected (this is expected in a short example)");
    }
    
    Ok(())
}

/// Example 4: Complete integration with a windowing system
/// 
/// This shows how you might integrate Ogham into a real application
/// with a window, event loop, and rendering.
#[allow(dead_code)]
fn complete_integration_example() -> Result<(), RuntimeError> {
    use ogham::runtime::{FileWatcher, RuntimeConfig};
    use ogham::vm::Value;
    
    // Set up host state
    let mut host_state = HashMap::new();
    host_state.insert(
        "window_width".to_string(),
        Value::Integer(1024),
    );
    host_state.insert(
        "window_height".to_string(),
        Value::Integer(768),
    );
    
    let config = RuntimeConfig::new()
        .with_host_state(host_state)
        .with_event_handler(|event_name, data| {
            // Handle events from the Ogham UI
            println!("Received event: {} with data: {:?}", event_name, data);
            
            // You can handle different event types
            match event_name {
                "button_clicked" => {
                    println!("Button was clicked!");
                    // Perform some action in your application
                    true // Event was handled
                }
                "text_changed" => {
                    if let Some(Value::String(text)) = data {
                        println!("Text changed to: {}", text);
                    }
                    true
                }
                _ => {
                    println!("Unhandled event: {}", event_name);
                    false // Event was not handled
                }
            }
        });
    
    // Watch and compile the UI
    let (mut ui, watcher) = ogham::runtime::watch_and_compile(
        "ui/main.ogh",
        Some(config.clone()),
    )?;
    
    // In your application's main loop:
    /*
    loop {
        // Handle window events
        for event in window.poll_events() {
            match event {
                WindowEvent::Close => break,
                WindowEvent::Resized { width, height } => {
                    // Update host state with new dimensions
                    // (You'd need to recompile with updated state)
                }
                _ => {}
            }
        }
        
        // Check for file changes (hot-reload)
        if watcher.check_for_changes() {
            match watcher.recompile(Some(config.clone())) {
                Ok(new_ui) => {
                    ui = new_ui;
                    println!("UI reloaded");
                }
                Err(e) => {
                    eprintln!("Reload failed: {}", e);
                }
            }
        }
        
        // Update UI layout with current window size
        let (width, height) = window.size();
        ui.layout(width as f32, height as f32);
        
        // Render the UI
        renderer.begin_frame();
        renderer.draw_ui(&ui);
        renderer.end_frame();
        
        window.swap_buffers();
    }
    */
    
    Ok(())
}

/// Helper function to count widgets (for demonstration)
fn count_widgets(ui: &UI) -> usize {
    // This is a simplified example - in reality you'd traverse the widget tree
    1 // Placeholder
}


