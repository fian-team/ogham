use crate::app::{ClientUI, ClientUpdate};
use crate::home_page::HOME_PAGE;
use crate::input::Input;
use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::widget::canvas_widget::Painter;
use ogham::widget::event::Event;
use ogham::widget::UI;
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

/// Demo painter for `examples/canvas.ogh`, registered under the name
/// `"demo_dial"`. Draws a ring, a sweep whose extent comes from
/// `props.charge`, and a needle.
///
/// Everything here is in **local logical coordinates**: `(0, 0)` is the
/// widget's top-left, `p.width()` / `p.height()` are its laid-out size, and
/// the 6px stroke is 6 logical pixels on any display. The painter has no
/// idea where in the tree the widget sits, and — because it derives every
/// dimension from `p.width()` / `p.height()` — it works unchanged at the
/// 180×180 dial and the `width: "grow"` × 28 meter in the same file.
///
/// This lives in the `client` binary rather than the library on purpose: a
/// painter is *host* code. The library ships the seam, not the art.
fn demo_dial(p: &mut Painter, props: &Value) {
    use skia_safe::{Paint, PaintStyle, Rect as SkRect};

    let charge = match props {
        Value::Map(map) => match map.get("charge") {
            Some(Value::Integer(i)) => *i,
            Some(Value::Float(f)) => *f as i32,
            _ => 0,
        },
        _ => 0,
    };

    let (w, h) = (p.width(), p.height());
    let (cx, cy) = (w / 2.0, h / 2.0);
    let radius = (w.min(h) / 2.0 - 6.0).max(1.0);
    let oval = SkRect::new(cx - radius, cy - radius, cx + radius, cy + radius);
    let stroke = (radius * 0.16).clamp(2.0, 8.0);

    let mut track = Paint::default();
    track.set_anti_alias(true);
    track.set_style(PaintStyle::Stroke);
    track.set_stroke_width(stroke);
    track.set_argb(255, 62, 66, 74);
    p.canvas().draw_arc(oval, 0.0, 360.0, false, &track);

    // Twelve clicks fill the ring; the modulo keeps the demo looping.
    let sweep_degrees = (charge.rem_euclid(12) as f32) * 30.0;
    let mut sweep = Paint::default();
    sweep.set_anti_alias(true);
    sweep.set_style(PaintStyle::Stroke);
    sweep.set_stroke_width(stroke);
    sweep.set_argb(255, 122, 178, 224);
    p.canvas()
        .draw_arc(oval, -90.0, sweep_degrees, false, &sweep);

    let angle = (sweep_degrees - 90.0).to_radians();
    let mut needle = Paint::default();
    needle.set_anti_alias(true);
    needle.set_style(PaintStyle::Stroke);
    needle.set_stroke_width((stroke * 0.5).max(1.0));
    needle.set_argb(255, 232, 230, 226);
    p.canvas().draw_line(
        (cx, cy),
        (
            cx + angle.cos() * radius * 0.78,
            cy + angle.sin() * radius * 0.78,
        ),
        &needle,
    );
}

impl Client {
    pub fn new(width: u32, height: u32) -> Self {
        let src = HOME_PAGE.to_string();
        // Registered on the config, not on the live Runtime: `load_file`
        // and hot reload both rebuild the runtime from this config, so a
        // painter registered here survives both. See AGENTS.md,
        // "Host-painted `Canvas`".
        let config = RuntimeConfig::new().with_painter("demo_dial", demo_dial);
        let ogham =
            Ogham::from_source(&src, config).expect("Failed to create Ogham from HOME_PAGE");
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

    /// Handle a UI event and return whether it was handled.
    ///
    /// The previewer also republishes every pointer move as the
    /// `"cursor"` anchor, so `examples/portals/anchored_tooltip.ogh`
    /// runs here without a bespoke host. A real host sets whichever
    /// anchors its own chrome needs; this is the one-liner shape that
    /// does it.
    pub fn handle_ui_event(&mut self, event: &Event) -> bool {
        if event.name == "mouse_move" {
            if let Some(point) = event.point.clone() {
                self.ogham.set_anchor("cursor", point);
            }
        }
        self.ogham.get_ui_mut().call_event(event)
    }

    /// Check if UI is dirty
    pub fn is_ui_dirty(&self) -> bool {
        self.ogham.get_ui().is_dirty()
    }

    /// True when the UI needs to be updated (tree dirty or runtime state changed).
    pub fn needs_ui_update(&self) -> bool {
        self.ogham.get_ui().is_dirty()
            || self
                .ogham
                .get_runtime()
                .lock()
                .expect("runtime lock poisoned")
                .needs_rerender()
    }

    /// Update UI if dirty, then layout with the given dimensions.
    /// `dt` is the real elapsed time since the last frame, used to advance
    /// any active style transitions before layout.
    pub fn update_ui_layout(&mut self, width: f32, height: f32, dt: f32) {
        // Check if runtime needs a rerender due to state updates
        let needs_rerender = {
            let runtime = self.ogham.get_runtime();
            runtime
                .lock()
                .expect("runtime lock poisoned")
                .needs_rerender()
        };

        if needs_rerender {
            let runtime = self.ogham.get_runtime().clone();
            let result = {
                let mut rt = runtime.lock().expect("runtime lock poisoned");
                rt.rerender()
                    .map(|v| (v, rt.widget_registry.clone()))
                    .map_err(|e| {
                        eprintln!("[ogham] Rerender error: {:?}", e);
                        e
                    })
                    .ok()
            };

            if let Some((widget_value, registry)) = result {
                let new_root = ogham::widget::builder::widget_value_to_widget_ref(
                    &registry,
                    &runtime,
                    &widget_value,
                )
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

        // Advance any in-flight style transitions before laying out, so the
        // layout pass observes the interpolated values.
        self.ogham.get_ui_mut().tick_animations(dt);

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

    fn update_ui_layout(&mut self, width: f32, height: f32, dt: f32) {
        self.update_ui_layout(width, height, dt)
    }

    fn get_ui_mut(&mut self) -> &mut UI {
        self.get_ui_mut()
    }

    fn render(&mut self, _surface: &mut SkiaSurface, _width: f32, _height: f32) {}
}
