use crate::app::{ClientUI, ClientUpdate};
use glow::Context as GlowContext;
use input::Input;
use skia_safe::Surface;
use std::{
    fs,
    rc::Rc,
    sync::{Arc, Mutex},
};
use ui::{
    ast_bridge,
    ast_vm::VM,
    event::Event,
    flex_widget::FlexWidget,
    parser::Parser,
    scanner::Scanner,
    style::{Color, FlexStyle, Size},
    text_widget::TextWidget,
    WidgetRef, UI,
};
use winit::window::Window;

pub struct Client {
    width: u32,
    height: u32,
    dpi_scale: f32,
    dirty: bool,
    ui: UI,
    gl: Option<Rc<GlowContext>>,
    src: String,
}

impl Client {
    pub fn new(width: u32, height: u32) -> Self {
        let src =
            fs::read_to_string("./ui/examples/hello_world.ogh").unwrap_or_else(|_| String::new());
        let mut scanner = Scanner::new(src.clone());
        let tokens = scanner.scan();
        println!("{:?}", tokens);
        let mut parser = Parser::new(tokens);
        let module = parser.parse().unwrap();
        println!("{:?}", module);
        let mut vm = VM::new();
        let value = vm.execute_module(&module).unwrap();
        let widget = ast_bridge::widget_value_to_widget_ref(&mut vm, &value).unwrap();
        // let mut container = FlexWidget::with_style(
        //     FlexStyle::builder()
        //         .width(Size::Grow(1.0))
        //         .height(Size::Grow(1.0))
        //         .background_color(Color::new(255, 255, 255, 255))
        //         .build(),
        // );
        // let text_widget: WidgetRef =
        //     Arc::new(Mutex::new(TextWidget::new("Hello, world!".to_string())));
        // container.add_child(text_widget);
        let ui: UI = UI::new(widget);
        Self {
            width,
            height,
            dpi_scale: 1.0,
            dirty: false,
            ui,
            gl: None,
            src,
        }
    }

    pub fn set_src(&mut self, src: String) {
        self.src = src;
        self.recompile();
        self.dirty = true;
    }

    fn recompile(&mut self) {
        // TODO: Implement recompile
    }

    pub fn set_gl_context(&mut self, gl: Rc<GlowContext>) {
        self.gl = Some(gl.clone());
        // Initialize sandbox with glow context
        // if self.sandbox.is_none() {
        //     self.sandbox = Some(Sandbox::new(&gl));
        // }
    }

    pub fn set_dpi_scale(&mut self, dpi_scale: f32) {
        self.dpi_scale = dpi_scale;
    }

    pub fn set_screen_size(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    fn update_impl(&mut self, input: &mut Input, frame_length: f32) {}

    pub fn render(&mut self, surface: &mut Surface) {}

    pub fn render_ui(&mut self) -> WidgetRef {
        self.ui.root.clone()
    }

    pub fn on_close_requested(&mut self) {}

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn clean(&mut self) {
        self.dirty = false;
    }

    /// Handle a UI event and return whether it was handled
    pub fn handle_ui_event(&mut self, event: &Event) -> bool {
        self.ui.call_event(event)
    }

    /// Check if UI is dirty
    pub fn is_ui_dirty(&self) -> bool {
        self.ui.is_dirty()
    }

    /// Update UI if dirty, then layout with the given dimensions
    pub fn update_ui_layout(&mut self, width: f32, height: f32) {
        let dirty = self.is_dirty();
        if dirty {
            self.clean();
            let new_root = self.render_ui();
            self.ui.update(new_root, width, height);
        }
        self.ui.layout(width, height);
    }

    /// Get mutable reference to UI for drawing
    pub fn get_ui_mut(&mut self) -> &mut UI {
        &mut self.ui
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
    fn handle_ui_event(&mut self, event: &ui::event::Event) -> bool {
        self.handle_ui_event(event)
    }

    fn is_ui_dirty(&self) -> bool {
        self.is_ui_dirty()
    }

    fn update_ui_layout(&mut self, width: f32, height: f32) {
        self.update_ui_layout(width, height)
    }

    fn get_ui_mut(&mut self) -> &mut ui::UI {
        self.get_ui_mut()
    }

    fn render(&mut self, surface: &mut skia_safe::Surface) {
        self.render(surface)
    }
}
