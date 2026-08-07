//! Host-painted `Canvas` leaf — integration tests for the paint escape
//! (`RenderContext::with_local_canvas`), the widget's layout /
//! reconciliation behaviour, painter registration + hot-reload survival,
//! and the event / occlusion surface.
//!
//! The M0 tests drive a real `SkiaEnv` over an offscreen raster surface:
//! the whole point of the feature is the canvas the painter *actually*
//! receives, and asserting on the canvas matrix is the only way to pin the
//! coordinate contract that hosts are told to rely on.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::runtime::Runtime;
use ogham::skia::SkiaEnv;
use ogham::widget::canvas_widget::{CanvasWidget, Painter};
use ogham::widget::event::Event;
use ogham::widget::image::ImageCache;
use ogham::widget::point::Point;
use ogham::widget::rect::Rect;
use ogham::widget::style::{Border, Color, Corners, CursorRole, TextStyle};
use ogham::widget::{builder, RenderContext, Surface, WidgetRef};
use ogham::Ogham;

const DT: f32 = 1.0 / 60.0;
const W: f32 = 800.0;
const H: f32 = 600.0;

// ── helpers ────────────────────────────────────────────────────────────

// `BridgeError` has no `Debug` for its `WidgetRef`-bearing `VMError`
// variant; flatten the kinds these tests actually reach (same shape as
// tests/portal.rs).
fn describe(err: &builder::BridgeError) -> String {
    match err {
        builder::BridgeError::InvalidWidgetType(s) => format!("InvalidWidgetType({})", s),
        builder::BridgeError::MissingProperty(s) => format!("MissingProperty({})", s),
        builder::BridgeError::InvalidPropertyType(n, m) => {
            format!("InvalidPropertyType({}, {})", n, m)
        }
        builder::BridgeError::VMError(_) => "VMError(...)".to_string(),
    }
}

/// Build a widget tree straight from source, so the builder's accept /
/// reject behaviour is observable without going through `Ogham`.
fn try_build_root(src: &str, config: RuntimeConfig) -> Result<WidgetRef, String> {
    let runtime = Arc::new(Mutex::new(
        Runtime::from_source(src, Some(config)).expect("parse"),
    ));
    let (module, registry) = {
        let rt = runtime.lock().unwrap();
        (
            rt.get_module().expect("module").clone(),
            rt.widget_registry.clone(),
        )
    };
    let widget_value = {
        let mut rt = runtime.lock().unwrap();
        rt.execute_module(&module).expect("execute")
    };
    builder::widget_value_to_widget_ref(&registry, &runtime, &widget_value)
        .map_err(|e| describe(&e))
}

/// The bridge error a source is expected to produce, as a flat string.
fn build_error(src: &str, config: RuntimeConfig) -> String {
    match try_build_root(src, config) {
        Ok(_) => panic!("expected the build to be rejected"),
        Err(err) => err,
    }
}

/// A painter that records nothing — enough to satisfy registration for
/// tests that only care about layout / reconciliation.
fn inert_config(names: &[&str]) -> RuntimeConfig {
    let mut config = RuntimeConfig::new();
    for name in names {
        config = config.with_painter(*name, |_p, _props| {});
    }
    config
}

/// The first `Canvas` in the tree, depth-first. Panics if there isn't one —
/// every test that calls this declares at least one.
fn find_canvas(root: &WidgetRef) -> WidgetRef {
    try_find_canvas(root).expect("no Canvas in the tree")
}

fn try_find_canvas(node: &WidgetRef) -> Option<WidgetRef> {
    let children = {
        let g = node.lock().unwrap();
        if g.downcast_ref::<CanvasWidget>().is_some() {
            return Some(node.clone());
        }
        g.get_children()
    };
    children.iter().find_map(try_find_canvas)
}

/// Read the laid-out rect of the tree's single `Canvas`.
fn canvas_rect(o: &Ogham) -> Rect {
    let canvas = find_canvas(&o.get_ui().root);
    let g = canvas.lock().unwrap();
    g.get_layout_rect().cloned().expect("canvas laid out")
}

/// Run enough frames to lay out and settle.
fn settled(src: &str, config: RuntimeConfig) -> Ogham {
    let mut o = Ogham::from_source(src, config).expect("from_source");
    for _ in 0..8 {
        o.frame(W, H, DT).expect("frame");
    }
    o
}

/// An offscreen Skia environment at the given DPI scale.
fn raster_env(dpi: f32) -> SkiaEnv {
    let surface =
        ogham::skia_safe::surfaces::raster_n32_premul((256, 256)).expect("raster surface");
    SkiaEnv::new_with_dpi_scale(surface, dpi)
}

/// A `RenderContext` that implements only the required primitives, so it
/// inherits the `with_local_canvas` default. Stands in for a future
/// backend that can't hand out a native canvas.
struct NoHatchContext;

impl RenderContext for NoHatchContext {
    fn fill_rect(&mut self, _x: f32, _y: f32, _w: f32, _h: f32, _color: &Color) {}
    fn fill_corners_rect(
        &mut self,
        _x: f32,
        _y: f32,
        _w: f32,
        _h: f32,
        _corners: &Corners,
        _color: &Color,
    ) {
    }
    fn draw_border(
        &mut self,
        _border: &Border,
        _x: f32,
        _y: f32,
        _w: f32,
        _h: f32,
        _corners: &Corners,
    ) {
    }
    fn draw_image(
        &mut self,
        _path: &str,
        _x: f32,
        _y: f32,
        _w: f32,
        _h: f32,
        _cache: &mut ImageCache,
    ) {
    }
    fn draw_text(&mut self, _text: &str, _style: &TextStyle, _x: f32, _y: f32, _width: f32) {}
    fn draw_line(&mut self, _x1: f32, _y1: f32, _x2: f32, _y2: f32, _w: f32, _color: &Color) {}
}

// ── M0 — Painter + the RenderContext escape ────────────────────────────

/// The coordinate contract at dpi 1.0: the canvas arrives translated to the
/// rect's origin and scaled by the DPI factor, and the painter's reported
/// size is the rect's logical size.
#[test]
fn local_canvas_is_translated_and_scaled_at_dpi_1() {
    let mut env = raster_env(1.0);
    let rect = Rect::new(30.0, 50.0, 120.0, 80.0);

    let mut seen: Option<(f32, f32, f32, f32, f32, f32, f32)> = None;
    let ran = env.with_local_canvas(&rect, &mut |p: &mut Painter| {
        let m = p.canvas().local_to_device_as_3x3();
        seen = Some((
            m.translate_x(),
            m.translate_y(),
            m.scale_x(),
            m.scale_y(),
            p.width(),
            p.height(),
            p.dpi_scale(),
        ));
    });

    assert!(ran, "SkiaEnv implements the hatch");
    assert_eq!(seen, Some((30.0, 50.0, 1.0, 1.0, 120.0, 80.0, 1.0)));
}

/// The same contract at dpi 2.0. The translate is in *device* pixels (so
/// the widget lands where the walker put it) but the painter still measures
/// in logical ones — a painter written against `width()` / `height()` is
/// DPI-blind, which is the entire ergonomic claim.
#[test]
fn local_canvas_is_translated_and_scaled_at_dpi_2() {
    let mut env = raster_env(2.0);
    let rect = Rect::new(30.0, 50.0, 120.0, 80.0);

    let mut seen: Option<(f32, f32, f32, f32, f32, f32, f32)> = None;
    env.with_local_canvas(&rect, &mut |p: &mut Painter| {
        let m = p.canvas().local_to_device_as_3x3();
        seen = Some((
            m.translate_x(),
            m.translate_y(),
            m.scale_x(),
            m.scale_y(),
            p.width(),
            p.height(),
            p.dpi_scale(),
        ));
    });

    assert_eq!(seen, Some((60.0, 100.0, 2.0, 2.0, 120.0, 80.0, 2.0)));
}

/// Save/restore semantics, same as `push_clip_rect`: the painter's
/// transform must not leak into whatever paints next.
#[test]
fn local_canvas_restores_the_transform_afterwards() {
    let mut env = raster_env(2.0);
    let before = env.surface.canvas().local_to_device_as_3x3();

    env.with_local_canvas(
        &Rect::new(11.0, 13.0, 40.0, 40.0),
        &mut |p: &mut Painter| {
            // Deliberately dirty the canvas further; restore must undo this too.
            p.canvas().translate((5.0, 5.0));
        },
    );

    let after = env.surface.canvas().local_to_device_as_3x3();
    assert_eq!(before.translate_x(), after.translate_x());
    assert_eq!(before.translate_y(), after.translate_y());
    assert_eq!(before.scale_x(), after.scale_x());
    assert_eq!(before.scale_y(), after.scale_y());
}

/// A backend that keeps the trait default reports `false` and never calls
/// the painter. Blank, not broken — a `Canvas` is opt-in host paint.
#[test]
fn backend_without_the_hatch_returns_false_and_never_paints() {
    let mut ctx = NoHatchContext;
    let mut called = false;
    let ran = ctx.with_local_canvas(&Rect::new(0.0, 0.0, 10.0, 10.0), &mut |_p: &mut Painter| {
        called = true;
    });
    assert!(!ran);
    assert!(!called);
}

// ── M1 — CanvasWidget + builder registration ───────────────────────────

const FIXED_SRC: &str = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow", direction: "row" },
    children: [
      Canvas { painter: "dial", style: { width: 120, height: 80 } },
    ],
  }
};
"#;

#[test]
fn canvas_takes_a_fixed_size_from_style() {
    let o = settled(FIXED_SRC, inert_config(&["dial"]));
    let r = canvas_rect(&o);
    assert_eq!((r.width, r.height), (120.0, 80.0));
}

/// The `Image` mistake not repeated: `"grow"` is legal on a Canvas, so it
/// can claim a share of its parent's axis instead of forcing the host to
/// compute a number and inject it.
#[test]
fn canvas_grows_into_its_parent() {
    let src = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow", direction: "row" },
    children: [
      Canvas { painter: "dial", style: { width: "grow", height: "grow" } },
    ],
  }
};
"#;
    let o = settled(src, inert_config(&["dial"]));
    let r = canvas_rect(&o);
    assert_eq!((r.width, r.height), (W, H));
}

/// `shrink` on a leaf whose content is opaque to the framework collapses to
/// the box model: a painter has no intrinsic size to report.
#[test]
fn canvas_shrinks_to_its_own_inset() {
    let src = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow", direction: "row" },
    children: [
      Canvas { painter: "dial", style: { width: "shrink", height: "shrink", padding: 6 } },
    ],
  }
};
"#;
    let o = settled(src, inert_config(&["dial"]));
    let r = canvas_rect(&o);
    assert_eq!((r.width, r.height), (12.0, 12.0));
}

/// A Canvas is a leaf. Nothing in the tree may hang off it — the painter
/// and the widget tree would otherwise both own the same pixels.
#[test]
fn canvas_is_a_leaf() {
    let root = try_build_root(FIXED_SRC, inert_config(&["dial"])).expect("build");
    let canvas = find_canvas(&root);
    let g = canvas.lock().unwrap();
    assert!(g.get_children().is_empty());
    assert_eq!(g.get_type(), "canvas");
}

/// Reconcile absorbs a re-rendered Canvas in place: the `Arc` identity
/// survives, and a prop change repaints without relayout.
#[test]
fn canvas_absorbs_on_rerender_preserving_identity() {
    let src = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow", direction: "row" },
    children: [
      Canvas { painter: "dial", props: { charge: charge }, style: { width: 120, height: 80 } },
    ],
  }
};
"#;
    let config = inert_config(&["dial"]).with_host_state(
        [("charge".to_string(), Value::Integer(0))]
            .into_iter()
            .collect(),
    );
    let mut o = settled(src, config);
    let before = find_canvas(&o.get_ui().root);

    o.with_runtime_mut(|rt| {
        rt.inject_host_state("charge".to_string(), Value::Integer(7));
        rt.request_rerender();
    });
    o.frame(W, H, DT).expect("frame");

    let after = find_canvas(&o.get_ui().root);
    assert!(
        Arc::ptr_eq(&before, &after),
        "same-painter Canvas should be absorbed, not replaced"
    );
    let g = after.lock().unwrap();
    let c = g.downcast_ref::<CanvasWidget>().expect("canvas");
    match &c.props {
        Value::Map(m) => assert_eq!(m.get("charge"), Some(&Value::Integer(7))),
        other => panic!("expected props map, got {:?}", other),
    }
}

/// A different painter is a different widget: absorbing across the name
/// change would preserve identity for content sharing nothing with what
/// came before.
#[test]
fn canvas_replaces_when_the_painter_name_changes() {
    let src = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow", direction: "row" },
    children: [
      Canvas { painter: which, style: { width: 120, height: 80 } },
    ],
  }
};
"#;
    let config = inert_config(&["dial", "gauge"]).with_host_state(
        [("which".to_string(), Value::String("dial".to_string()))]
            .into_iter()
            .collect(),
    );
    let mut o = settled(src, config);
    let before = find_canvas(&o.get_ui().root);

    o.with_runtime_mut(|rt| {
        rt.inject_host_state("which".to_string(), Value::String("gauge".to_string()));
        rt.request_rerender();
    });
    o.frame(W, H, DT).expect("frame");

    let after = find_canvas(&o.get_ui().root);
    assert!(
        !Arc::ptr_eq(&before, &after),
        "a painter-name change must replace the widget"
    );
    let g = after.lock().unwrap();
    let c = g.downcast_ref::<CanvasWidget>().expect("canvas");
    assert_eq!(c.painter_name, "gauge");
}

/// Missing `painter:` is a loud build-time error, not a blank rectangle.
#[test]
fn missing_painter_property_is_an_error() {
    let src = r#"
let main = fn () {
  Flex { children: [ Canvas { style: { width: 10, height: 10 } } ] }
};
"#;
    let err = build_error(src, inert_config(&["dial"]));
    assert_eq!(err, "MissingProperty(painter)");
}

/// An unregistered painter name is rejected *and* the message lists what is
/// registered — the failure mode this framework keeps getting wrong is the
/// silent one.
#[test]
fn unknown_painter_name_is_an_error_that_lists_the_registered_ones() {
    let src = r#"
let main = fn () {
  Flex { children: [ Canvas { painter: "whel_dail", style: { width: 10, height: 10 } } ] }
};
"#;
    let err = build_error(src, inert_config(&["dial", "gauge"]));
    assert!(
        err.starts_with("InvalidPropertyType(painter,"),
        "got {}",
        err
    );
    assert!(err.contains("whel_dail"), "got {}", err);
    assert!(err.contains("\"dial\""), "got {}", err);
    assert!(err.contains("\"gauge\""), "got {}", err);
}

// ── M2 — registration + hot-reload survival ────────────────────────────

/// End-to-end: a painter registered on the config is invoked by the Skia
/// walker with the widget's laid-out size.
#[test]
fn registered_painter_runs_during_draw_with_the_laid_out_size() {
    let seen: Arc<Mutex<Vec<(f32, f32)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    let config = RuntimeConfig::new().with_painter("dial", move |p: &mut Painter, _props| {
        sink.lock().unwrap().push((p.width(), p.height()));
    });

    let mut o = settled(FIXED_SRC, config);
    let mut env = raster_env(1.0);
    env.draw(o.get_ui_mut());

    assert_eq!(*seen.lock().unwrap(), vec![(120.0, 80.0)]);
}

/// Painters registered on `RuntimeConfig` survive a hot reload for the same
/// reason event handlers do: `Ogham::reload` rebuilds the runtime *from the
/// config*. A painter added post-hoc to a live `Runtime` would vanish here.
#[test]
fn painter_registered_before_watch_still_paints_after_reload() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let config = RuntimeConfig::new().with_painter("dial", move |_p: &mut Painter, _props| {
        counter.fetch_add(1, Ordering::SeqCst);
    });

    let dir = std::env::temp_dir().join(format!("ogham-canvas-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("canvas_reload.ogh");
    std::fs::write(&path, FIXED_SRC).expect("write");

    let mut o = Ogham::watch(path.to_string_lossy().to_string(), config).expect("watch");
    for _ in 0..4 {
        o.frame(W, H, DT).expect("frame");
    }
    let mut env = raster_env(1.0);
    env.draw(o.get_ui_mut());
    assert_eq!(calls.load(Ordering::SeqCst), 1, "painted before reload");

    // Edit the file (a different size, so the reload is observable) and
    // reload explicitly rather than racing the watcher.
    std::fs::write(&path, FIXED_SRC.replace("width: 120", "width: 160")).expect("rewrite");
    o.reload().expect("reload");
    for _ in 0..4 {
        o.frame(W, H, DT).expect("frame");
    }
    env.draw(o.get_ui_mut());

    assert_eq!(calls.load(Ordering::SeqCst), 2, "painted after reload");
    assert_eq!(canvas_rect(&o).width, 160.0, "the reload really took");

    std::fs::remove_file(&path).ok();
}

/// Two `Ogham` instances with separate configs keep separate painter maps:
/// registration is per-config, never process-global.
#[test]
fn painters_do_not_cross_contaminate_between_instances() {
    let a_calls = Arc::new(AtomicUsize::new(0));
    let a_counter = a_calls.clone();
    let a = settled(
        FIXED_SRC,
        RuntimeConfig::new().with_painter("dial", move |_p: &mut Painter, _props| {
            a_counter.fetch_add(1, Ordering::SeqCst);
        }),
    );

    // The second instance registers a *different* name, so `dial` must not
    // resolve there at all.
    let b = try_build_root(FIXED_SRC, inert_config(&["gauge"]));
    assert!(b.is_err(), "instance B should not see instance A's painter");

    let mut a = a;
    let mut env = raster_env(1.0);
    env.draw(a.get_ui_mut());
    assert_eq!(a_calls.load(Ordering::SeqCst), 1);
}

// ── M3 — events + occlusion ────────────────────────────────────────────

const LISTENING_SRC: &str = r#"
let main = fn () {
  Flex {
    block_interactions: false,
    style: { width: "grow", height: "grow" },
    children: [
      Canvas {
        painter: "dial",
        style: { position: { type: "absolute", x: 100, y: 100 }, width: 200, height: 50 },
        mouse_down: fn () { event("dial_press"); },
      },
    ],
  }
};
"#;

const BARE_SRC: &str = r#"
let main = fn () {
  Flex {
    block_interactions: false,
    style: { width: "grow", height: "grow" },
    children: [
      Canvas {
        painter: "dial",
        style: { position: { type: "absolute", x: 100, y: 100 }, width: 200, height: 50 },
      },
    ],
  }
};
"#;

fn listening(counter: Arc<AtomicUsize>) -> Ogham {
    let config = inert_config(&["dial"]).with_event_handler("dial_press", move |_args| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(Value::Boolean(true))
    });
    settled(LISTENING_SRC, config)
}

#[test]
fn click_inside_a_listening_canvas_fires_its_listener() {
    let presses = Arc::new(AtomicUsize::new(0));
    let mut o = listening(presses.clone());
    let handled = o.get_ui_mut().call_event(&Event::with_point(
        "mouse_down".to_string(),
        Point::new(200.0, 125.0),
    ));
    assert!(handled);
    assert_eq!(presses.load(Ordering::SeqCst), 1);
}

#[test]
fn click_outside_a_listening_canvas_does_not_fire() {
    let presses = Arc::new(AtomicUsize::new(0));
    let mut o = listening(presses.clone());
    o.get_ui_mut().call_event(&Event::with_point(
        "mouse_down".to_string(),
        Point::new(500.0, 400.0),
    ));
    assert_eq!(presses.load(Ordering::SeqCst), 0);
}

/// `UI::blocks_point` is the predicate hosts gate world picking on. A
/// listening Canvas eats the press, exactly like a listening Flex.
#[test]
fn listening_canvas_blocks_the_point() {
    let mut o = listening(Arc::new(AtomicUsize::new(0)));
    assert!(o.get_ui_mut().blocks_point(&Point::new(200.0, 125.0)));
}

/// A bare Canvas is see-through: a decorative dial must not swallow clicks
/// meant for the scene behind it.
#[test]
fn bare_canvas_does_not_block_the_point() {
    let mut o = settled(BARE_SRC, inert_config(&["dial"]));
    assert!(!o.get_ui_mut().blocks_point(&Point::new(200.0, 125.0)));
    // ...and outside it, nothing blocks either.
    assert!(!o.get_ui_mut().blocks_point(&Point::new(500.0, 400.0)));
}

/// `cursor:` is authored intent, decoupled from listeners — a Canvas that
/// declares `pointer` gets it whether or not it also takes clicks.
#[test]
fn canvas_declares_its_style_cursor() {
    let src = r#"
let main = fn () {
  Flex {
    block_interactions: false,
    style: { width: "grow", height: "grow" },
    children: [
      Canvas {
        painter: "dial",
        style: {
          position: { type: "absolute", x: 100, y: 100 },
          width: 200, height: 50,
          cursor: "pointer",
        },
      },
    ],
  }
};
"#;
    let mut o = settled(src, inert_config(&["dial"]));
    let ui = o.get_ui_mut();
    ui.call_event(&Event::with_point(
        "mouse_move".to_string(),
        Point::new(200.0, 125.0),
    ));
    assert_eq!(ui.hovered_cursor(), CursorRole::Pointer);
}

// ── M4 — the shipped example ───────────────────────────────────────────

/// `examples/canvas.ogh` plus the `client` binary's `"demo_dial"` painter
/// are the two routes by which anyone discovers this feature. A broken
/// example is a feature nobody finds, so keep it building.
#[test]
fn the_shipped_example_builds_and_declares_canvases() {
    let src = std::fs::read_to_string("examples/canvas.ogh").expect("examples/canvas.ogh");
    let root = try_build_root(&src, inert_config(&["demo_dial"])).expect("example should build");
    let canvas = find_canvas(&root);
    let g = canvas.lock().unwrap();
    let c = g.downcast_ref::<CanvasWidget>().expect("canvas");
    assert_eq!(c.painter_name, "demo_dial");
}
