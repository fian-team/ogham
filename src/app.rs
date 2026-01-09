use crate::skia::SkiaEnv;
use crate::tree::event::KeyModifiers;
use crate::tree::point::Point;
use crate::tree::{Surface, UI};
use crate::{input::Input, tree::event::Event};
use glow::Context as GlowContext;
use glutin::{
    context::PossiblyCurrentContext,
    surface::{GlSurface, Surface as GlutinSurface, WindowSurface},
};
use skia_safe::Surface as SkiaSurface;
use skia_safe::{
    gpu::{self, backend_render_targets, gl::FramebufferInfo, SurfaceOrigin},
    ColorType,
};
use std::{
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, KeyEvent, Modifiers, MouseButton, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::Key,
    window::Window,
};

use nalgebra_glm as glm;
use std::num::NonZeroU32;

pub fn create_surface(
    size: (i32, i32),
    fb_info: FramebufferInfo,
    gr_context: &mut skia_safe::gpu::DirectContext,
    num_samples: usize,
    stencil_size: usize,
) -> SkiaSurface {
    let backend_render_target =
        backend_render_targets::make_gl(size, num_samples, stencil_size, fb_info);

    gpu::surfaces::wrap_backend_render_target(
        gr_context,
        &backend_render_target,
        SurfaceOrigin::BottomLeft,
        ColorType::RGBA8888,
        None,
        None,
    )
    .expect("Could not create skia surface")
}

pub struct Application<Client> {
    pub ui_skia: SkiaEnv,
    pub fb_info: FramebufferInfo,
    pub num_samples: usize,
    pub stencil_size: usize,
    pub modifiers: Modifiers,
    pub frame: usize,
    pub previous_frame_start: Instant,
    pub gl_surface: GlutinSurface<WindowSurface>,
    pub gr_context: skia_safe::gpu::DirectContext,
    pub gl_context: PossiblyCurrentContext,
    pub gl: Rc<GlowContext>,
    pub window: Window,
    pub input: Input,
    pub client: Client,
}

impl<Client> Application<Client> {
    /// Update the client with the current input state and frame length.
    /// This helper method avoids borrowing issues when accessing both
    /// `client` and `input` fields through `DerefMut`.
    pub fn update_client(&mut self, frame_length: f32)
    where
        Client: ClientUpdate,
    {
        self.client.update(&mut self.input, frame_length);
    }
}

impl<Client: ClientUpdate + ClientUI> ApplicationHandler for Application<Client> {
    fn resumed(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {}

    fn new_events(
        &mut self,
        _event_loop: &winit::event_loop::ActiveEventLoop,
        cause: winit::event::StartCause,
    ) {
        if let winit::event::StartCause::ResumeTimeReached { .. } = cause {
            self.window.request_redraw()
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let mut draw_frame = false;
        let frame_start = Instant::now();

        // Handle UI events first for MouseInput events
        let ui_handled_event = match &event {
            WindowEvent::MouseInput { state, button, .. } => {
                if *state == ElementState::Pressed && *button == MouseButton::Left {
                    let cursor_pos = self.input.cursor_position();
                    let ui_event = Event::with_point(
                        "mouse_down".to_string(),
                        Point::new(cursor_pos.x, cursor_pos.y),
                    );
                    let handled = self.client.handle_ui_event(&ui_event);
                    handled
                } else if *state == ElementState::Released && *button == MouseButton::Left {
                    let cursor_pos = self.input.cursor_position();
                    let ui_event = Event::with_point(
                        "mouse_up".to_string(),
                        Point::new(cursor_pos.x, cursor_pos.y),
                    );
                    let handled = self.client.handle_ui_event(&ui_event);
                    handled
                } else {
                    false
                }
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    logical_key, state, ..
                },
                ..
            } => {
                // Only process key events on key up (Released)
                if *state == ElementState::Released {
                    // Create a simple key modifiers struct
                    let key_modifiers = KeyModifiers::new();

                    // Handle both keydown and keypress events
                    let mut ui_handled = false;

                    // Convert key to a numeric representation for our event system
                    let key_code = match logical_key {
                        Key::Character(c) => c.chars().next().unwrap_or(' ') as u32,
                        _ => {
                            // Convert the key to a string using debug formatting and map it to key codes
                            let key_str = format!("{:?}", logical_key);
                            match key_str.as_str() {
                                "Backspace" => 8,
                                "Tab" => 9,
                                "Enter" => 13,
                                "Escape" => 27,
                                "Space" => 32,
                                "ArrowLeft" => 37,
                                "ArrowUp" => 38,
                                "ArrowRight" => 39,
                                "ArrowDown" => 40,
                                "Delete" => 46,
                                "Home" => 36,
                                "End" => 35,
                                _ => {
                                    // For other named keys, try to extract the key name and map it
                                    if key_str.starts_with("Named(") && key_str.ends_with(")") {
                                        let key_name = &key_str[6..key_str.len() - 1];
                                        match key_name {
                                            "ArrowLeft" => 37,
                                            "ArrowUp" => 38,
                                            "ArrowRight" => 39,
                                            "ArrowDown" => 40,
                                            "Backspace" => 8,
                                            "Tab" => 9,
                                            "Enter" => 13,
                                            "Escape" => 27,
                                            "Space" => 32,
                                            "Delete" => 46,
                                            "Home" => 36,
                                            "End" => 35,
                                            _ => 0, // Unknown named key
                                        }
                                    } else {
                                        0 // Unknown key
                                    }
                                }
                            }
                        }
                    };

                    // Always send keydown event
                    let keydown_event = Event::keydown(
                        key_code,
                        None, // Character will be handled in keypress
                        key_modifiers.clone(),
                    );
                    ui_handled |= self.client.handle_ui_event(&keydown_event);

                    // Send keypress event for printable characters
                    if let Key::Character(c) = logical_key {
                        if let Some(character) = c.chars().next() {
                            let keypress_event =
                                Event::keypress(key_code, Some(character), key_modifiers);
                            ui_handled |= self.client.handle_ui_event(&keypress_event);
                        }
                    } else if key_code == 32 {
                        // Handle space key (which comes as NamedKey::Space, not Character)
                        let keypress_event = Event::keypress(key_code, Some(' '), key_modifiers);
                        ui_handled |= self.client.handle_ui_event(&keypress_event);
                    }

                    if self.client.is_ui_dirty() {
                        let logical_size: LogicalSize<f64> = self
                            .window
                            .inner_size()
                            .to_logical(self.window.scale_factor());
                        self.client.update_ui_layout(
                            logical_size.width as f32,
                            logical_size.height as f32,
                        );
                    }

                    ui_handled
                } else {
                    false
                }
            }
            _ => false,
        };

        // Only call Client's event method if UI didn't handle the event
        // or if it's not a MouseInput event
        match &event {
            WindowEvent::MouseInput { state, button, .. } => {
                if !ui_handled_event {
                    if *state == ElementState::Pressed {
                        self.input.press_mouse_button(*button);
                    } else {
                        self.input.release_mouse_button(*button);
                    }
                }
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    logical_key, state, ..
                },
                ..
            } => {
                if !ui_handled_event {
                    if *state == ElementState::Pressed {
                        self.input.press(logical_key.clone());
                    } else {
                        self.input.release(logical_key.clone());
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale_factor = self.window.scale_factor();
                let x = position.x as f32 / scale_factor as f32;
                let y = position.y as f32 / scale_factor as f32;
                self.input.move_cursor(glm::vec2(x, y));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => {
                        // Convert line deltas to pixel deltas (typical line height is ~15 pixels)
                        let pixel_delta_x = x * 15.0;
                        let pixel_delta_y = y * 15.0;
                        self.input
                            .set_scroll_delta(glm::vec2(pixel_delta_x, pixel_delta_y));
                    }
                    winit::event::MouseScrollDelta::PixelDelta(position) => {
                        let scale_factor = self.window.scale_factor();
                        let x = position.x as f32 / scale_factor as f32;
                        let y = position.y as f32 / scale_factor as f32;
                        self.input.set_scroll_delta(glm::vec2(x, y));
                    }
                }
            }
            _ => (),
        }

        match event {
            WindowEvent::Resized(physical_size) => {
                let size = self.window.inner_size();
                let size = (
                    size.width.try_into().expect("Could not convert width"),
                    size.height.try_into().expect("Could not convert height"),
                );
                let fb_info = self.fb_info;
                let num_samples = self.num_samples;
                let stencil_size = self.stencil_size;
                let gr_context = &mut self.gr_context;
                self.ui_skia.surface =
                    create_surface(size, fb_info, gr_context, num_samples, stencil_size);
                /* First resize the opengl drawable */
                let (width, height): (u32, u32) = physical_size.into();

                // Use logical size for UI layout
                let logical_size: LogicalSize<f64> = self
                    .window
                    .inner_size()
                    .to_logical(self.window.scale_factor());
                self.client
                    .update_ui_layout(logical_size.width as f32, logical_size.height as f32);

                self.gl_surface.resize(
                    &self.gl_context,
                    NonZeroU32::new(width.max(1)).unwrap(),
                    NonZeroU32::new(height.max(1)).unwrap(),
                );
            }
            WindowEvent::ModifiersChanged(new_modifiers) => self.modifiers = new_modifiers,
            WindowEvent::RedrawRequested => {
                // draw_frame = true;
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // Update DPI scale factor when moving between displays
                self.ui_skia.set_dpi_scale(scale_factor as f32);
            }
            _ => (),
        }

        let expected_frame_length_seconds = 1.0 / 60.0;
        let frame_duration = Duration::from_secs_f32(expected_frame_length_seconds);

        if frame_start - self.previous_frame_start > frame_duration {
            draw_frame = true;
            self.previous_frame_start = frame_start;
        }
        if draw_frame {
            let frame_length = frame_duration;
            self.update_client(frame_length.as_secs_f32());

            {
                let logical_size: LogicalSize<f64> = self
                    .window
                    .inner_size()
                    .to_logical(self.window.scale_factor());
                self.client
                    .update_ui_layout(logical_size.width as f32, logical_size.height as f32);
            }

            {
                self.frame += 1;
                self.gr_context.reset(None);
                self.client.render(&mut self.ui_skia.surface);
                self.ui_skia.draw(self.client.get_ui_mut());

                self.gr_context.flush_and_submit();
                self.gl_surface.swap_buffers(&self.gl_context).unwrap();

                self.input.update(&self.window);
            }
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(
            self.previous_frame_start + frame_duration,
        ));
    }
}

/// Trait for types that can be updated with input and frame length.
pub trait ClientUpdate {
    fn update(&mut self, input: &mut Input, frame_length: f32);
}

/// Trait for types that can handle UI operations.
pub trait ClientUI {
    fn handle_ui_event(&mut self, event: &Event) -> bool;
    fn is_ui_dirty(&self) -> bool;
    fn update_ui_layout(&mut self, width: f32, height: f32);
    fn get_ui_mut(&mut self) -> &mut UI;
    fn render(&mut self, surface: &mut SkiaSurface);
}

pub fn create_application<T: ClientUpdate + ClientUI>(
    client: T,
    el: &EventLoop<()>,
) -> Application<T> {
    use std::{ffi::CString, num::NonZeroU32, time::Instant};

    use gl::types::*;
    use gl_rs as gl;
    use glutin::{
        config::{ConfigTemplateBuilder, GlConfig},
        context::{ContextApi, ContextAttributesBuilder},
        display::{GetGlDisplay, GlDisplay},
        prelude::NotCurrentGlContext,
        surface::{SurfaceAttributesBuilder, WindowSurface},
    };
    use glutin_winit::DisplayBuilder;
    #[allow(deprecated)]
    use raw_window_handle::HasRawWindowHandle;
    use winit::{
        event::Modifiers,
        window::{Fullscreen, WindowAttributes},
    };

    use skia_safe::gpu::gl::FramebufferInfo;

    let window_attributes = WindowAttributes::default()
        .with_title("Untold Lore")
        .with_fullscreen(Some(Fullscreen::Borderless(None)));

    let template = ConfigTemplateBuilder::new()
        .with_alpha_size(8)
        .with_depth_size(24)
        .with_transparency(true)
        .with_multisampling(4); // Request 4x MSAA for better antialiasing

    let display_builder = DisplayBuilder::new().with_window_attributes(window_attributes.into());
    let (window, gl_config) = display_builder
        .build(el, template, |configs| {
            // Find the config with the maximum number of samples for better antialiasing.
            // Prefer configs with more samples, and prioritize transparency support.
            configs
                .reduce(|accum, config| {
                    let transparency_check = config.supports_transparency().unwrap_or(false)
                        & !accum.supports_transparency().unwrap_or(false);

                    if transparency_check || config.num_samples() > accum.num_samples() {
                        config
                    } else {
                        accum
                    }
                })
                .unwrap()
        })
        .unwrap();

    let window = window.expect("Could not create window with OpenGL context");
    #[allow(deprecated)]
    let raw_window_handle = window
        .raw_window_handle()
        .expect("Failed to retrieve RawWindowHandle");

    // The context creation part. It can be created before surface and that's how
    // it's expected in multithreaded + multiwindow operation mode, since you
    // can send NotCurrentContext, but not Surface.
    let context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::OpenGl(Some(glutin::context::Version {
            major: 4,
            minor: 1,
        })))
        .build(Some(raw_window_handle));

    // Since glutin by default tries to create OpenGL core context, which may not be
    // present we should try gles.
    let fallback_context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::Gles(None))
        .build(Some(raw_window_handle));
    let not_current_gl_context = unsafe {
        gl_config
            .display()
            .create_context(&gl_config, &context_attributes)
            .unwrap_or_else(|_| {
                gl_config
                    .display()
                    .create_context(&gl_config, &fallback_context_attributes)
                    .expect("failed to create context")
            })
    };

    let (width, height): (u32, u32) = window.inner_size().into();

    let attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
        raw_window_handle,
        NonZeroU32::new(width).unwrap(),
        NonZeroU32::new(height).unwrap(),
    );

    let gl_surface = unsafe {
        gl_config
            .display()
            .create_window_surface(&gl_config, &attrs)
            .expect("Could not create gl window surface")
    };

    let gl_context = not_current_gl_context
        .make_current(&gl_surface)
        .expect("Could not make GL context current when setting up skia renderer");

    gl::load_with(|s| {
        gl_config
            .display()
            .get_proc_address(CString::new(s).unwrap().as_c_str())
    });
    let interface = skia_safe::gpu::gl::Interface::new_load_with(|name| {
        if name == "eglGetCurrentDisplay" {
            return std::ptr::null();
        }
        gl_config
            .display()
            .get_proc_address(CString::new(name).unwrap().as_c_str())
    })
    .expect("Could not create interface");

    let mut gr_context = skia_safe::gpu::direct_contexts::make_gl(interface, None)
        .expect("Could not create direct context");

    let fb_info = {
        let mut fboid: GLint = 0;
        unsafe { gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut fboid) };
        FramebufferInfo {
            fboid: fboid.try_into().unwrap(),
            format: skia_safe::gpu::gl::Format::RGBA8.into(),
            ..Default::default()
        }
    };

    let num_samples = gl_config.num_samples() as usize;
    let stencil_size = gl_config.stencil_size() as usize;

    let size = (
        width.try_into().expect("Could not convert width"),
        height.try_into().expect("Could not convert height"),
    );
    let surface = create_surface(size, fb_info, &mut gr_context, num_samples, stencil_size);

    let ui_skia = SkiaEnv::new_with_dpi_scale(surface, window.scale_factor() as f32);

    // Create glow context for OpenGL rendering
    let gl_display = Arc::new(gl_config.display());
    let gl = Rc::new(unsafe {
        glow::Context::from_loader_function_cstr(|s| gl_display.get_proc_address(s))
    });

    Application::<T> {
        ui_skia,
        fb_info,
        num_samples,
        stencil_size,
        modifiers: Modifiers::default(),
        frame: 0,
        previous_frame_start: Instant::now(),
        gl_surface,
        gr_context,
        gl_context,
        gl,
        window,
        input: Input::new(),
        client,
    }
}
