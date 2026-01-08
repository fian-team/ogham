use std::ops::{Deref, DerefMut};

use crate::app::{create_application, Application};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::EventLoop;

use crate::client::Client;
mod app;
mod ast_vm;
mod browser;
mod client;
mod parser;
mod scanner;

mod input;

struct UIStandalone(Application<Client>);

fn main() {
    let el = EventLoop::new().expect("Failed to create event loop");
    let client = Client::new(1024, 768);
    let mut application = UIStandalone(create_application(client, &el));

    // Pass glow context to client
    application
        .0
        .client
        .set_gl_context(application.0.gl.clone());

    // Exit fullscreen (if any) and maximize the window to fullscreen with borders (window decorations)
    application.0.window.set_fullscreen(None);
    application.0.window.set_maximized(true);
    application.0.window.set_title("UIStandalone");

    el.run_app(&mut application).expect("run() failed");
}

impl Deref for UIStandalone {
    type Target = Application<Client>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for UIStandalone {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ApplicationHandler for UIStandalone {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        Application::<Client>::resumed(&mut self.0, event_loop);
    }

    fn new_events(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        cause: winit::event::StartCause,
    ) {
        Application::<Client>::new_events(&mut self.0, event_loop, cause);
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        // Handle app-specific close event
        if let WindowEvent::CloseRequested = &event {
            self.client.on_close_requested();
            event_loop.exit();
            return;
        }

        // Handle app-specific events that need client notification
        match &event {
            WindowEvent::Resized(physical_size) => {
                let (width, height): (u32, u32) = (*physical_size).into();
                self.client.set_screen_size(width, height);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.client.set_dpi_scale(*scale_factor as f32);
            }
            _ => {}
        }

        // Delegate all events to Application's implementation
        Application::<Client>::window_event(&mut self.0, event_loop, window_id, event);

        // Update cursor state based on current view (lock in sandbox, unlock in main menu)
        self.0
            .client
            .update_cursor_state(&mut self.0.input, &self.0.window);
    }
}
