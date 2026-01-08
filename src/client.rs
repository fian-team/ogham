use crate::app::{ClientUI, ClientUpdate};
use glow::Context as GlowContext;
use notify::{Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use skia_safe::Surface;
use std::{fs, path::PathBuf, rc::Rc, sync::mpsc};
use ui::{
    ast_bridge, ast_vm::VM, event::Event, input::Input, parser::Parser, scanner::Scanner,
    WidgetRef, UI,
};
use winit::{
    keyboard::{Key, NamedKey},
    window::Window,
};

pub struct Client {
    width: u32,
    height: u32,
    dpi_scale: f32,
    dirty: bool,
    ui: UI,
    gl: Option<Rc<GlowContext>>,
    src: String,
    path: Option<String>,
    file_watcher: Option<RecommendedWatcher>,
    file_watcher_receiver: mpsc::Receiver<Result<NotifyEvent, notify::Error>>,
}

impl Client {
    pub fn new(width: u32, height: u32) -> Self {
        let src =
            fs::read_to_string("./examples/hello_world.ogh").unwrap_or_else(|_| String::new());
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
        let initial_path = "./examples/hello_world.ogh".to_string();
        let (tx, rx) = mpsc::channel();
        let mut watcher_opt = None;
        if let Ok(mut watcher) = notify::recommended_watcher(tx) {
            let path_buf = PathBuf::from(&initial_path);
            if let Some(parent) = path_buf.parent() {
                let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
            }
            watcher_opt = Some(watcher);
        }

        Self {
            width,
            height,
            dpi_scale: 1.0,
            dirty: false,
            ui,
            gl: None,
            src,
            path: Some(initial_path),
            file_watcher: watcher_opt,
            file_watcher_receiver: rx,
        }
    }

    pub fn set_src(&mut self, src: String) {
        self.src = src;
        self.recompile();
        self.dirty = true;
    }

    fn recompile(&mut self) {
        let mut scanner = Scanner::new(self.src.clone());
        let tokens = scanner.scan();
        let mut parser = Parser::new(tokens);
        if let Ok(module) = parser.parse() {
            let mut vm = VM::new();
            if let Ok(value) = vm.execute_module(&module) {
                if let Ok(widget) = ast_bridge::widget_value_to_widget_ref(&mut vm, &value) {
                    self.ui = UI::new(widget);
                }
            }
        }
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

    fn update_impl(&mut self, input: &mut Input, frame_length: f32) {
        // Check for Ctrl+O to open file dialog
        // Note: For now, we check for 'o' key press
        // TODO: Add proper modifier tracking to Input struct to detect Control key
        // The file dialog will work when 'o' is pressed (user should hold Ctrl)
        if input.pressed(Key::Character("o".into())) {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Ogham files", &["ogh"])
                .add_filter("All files", &["*"])
                .pick_file()
            {
                println!("Selected file: {:?}", path);
                self.path = Some(path.to_string_lossy().to_string());
                self.load();
            }
        }

        // Check for file change events
        while let Ok(Ok(event)) = self.file_watcher_receiver.try_recv() {
            if let EventKind::Modify(_) | EventKind::Create(_) = event.kind {
                let path_to_check = self.path.as_ref().map(|p| PathBuf::from(p));
                if let Some(ref path_buf) = path_to_check {
                    // Check if the changed file matches our watched file
                    if event.paths.iter().any(|p| p == path_buf) {
                        self.load();
                    }
                }
            }
        }
    }

    fn load(&mut self) {
        let path_clone = self.path.clone();
        if let Some(ref path) = path_clone {
            if let Ok(src) = fs::read_to_string(path) {
                self.src = src;
                self.recompile();
                self.dirty = true;
            }
        }

        // Set up file watching for the new file (after loading)
        if let Some(ref path) = path_clone {
            self.setup_file_watcher(path);
        }
    }

    fn setup_file_watcher(&mut self, path: &str) {
        // Unwatch the previous file if any
        if let Some(ref mut watcher) = self.file_watcher {
            let path_buf = PathBuf::from(path);
            if let Some(parent) = path_buf.parent() {
                let _ = watcher.unwatch(parent);
            }
        }

        // Create a new watcher for the new file
        let (tx, rx) = mpsc::channel();
        if let Ok(mut watcher) = notify::recommended_watcher(tx) {
            let path_buf = PathBuf::from(path);
            if let Some(parent) = path_buf.parent() {
                let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
            }
            self.file_watcher = Some(watcher);
            self.file_watcher_receiver = rx;
        }
    }

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
