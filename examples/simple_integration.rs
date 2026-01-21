//! A simple, focused example showing how to integrate Ogham into your application.
//!
//! This demonstrates the most common use case: loading a UI file, passing
//! host state, and handling file changes for hot-reloading.

use ogham::runtime::{RuntimeConfig, RuntimeError};
use ogham::vm::Value;
use std::collections::HashMap;

fn main() -> Result<(), RuntimeError> {
    // Step 1: Prepare host state (data from your application)
    let mut host_state = HashMap::new();
    host_state.insert("app_title".to_string(), Value::String("My App".to_string()));
    host_state.insert("user_count".to_string(), Value::Integer(42));
    
    // Step 2: Create runtime configuration
    let config = RuntimeConfig::new()
        .with_host_state(host_state)
        .with_event_handler("ui_event", |args| {
            println!("UI Event: ui_event -> {:?}", args);
            true // Event handled
        });
    
    // Step 3: Compile the UI file with configuration
    let ui = ogham::runtime::from_file("examples/hello_world.ogh", Some(config))?;
    
    println!("UI compiled successfully!");
    println!("You can now use this UI in your rendering system.");
    
    // Step 4: In a real application, you would:
    // - Pass the UI to your renderer
    // - Call ui.layout(width, height) when window resizes
    // - Call ui.call_event(event) when handling user input
    // - Use a FileWatcher for hot-reloading during development
    
    Ok(())
}

