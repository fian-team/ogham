//! `TextInput`: `on_blur`, an honoured `align`, `placeholder`, and
//! drag-select through pointer capture.
//!
//! Blur is asserted through the document — a raise with the value — for
//! each way focus can leave: a press elsewhere, Tab, Escape. The caret
//! and the placeholder are read off the widget and off a recording
//! `RenderContext`, so nothing here needs a window.

use std::sync::{Arc, Mutex};

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::widget::event::{Event, KeyModifiers};
use ogham::widget::image::ImageCache;
use ogham::widget::point::Point;
use ogham::widget::style::{Border, Color, Corners, TextStyle};
use ogham::widget::text_input_widget::TextInputWidget;
use ogham::widget::{RenderContext, Widget, WidgetRef};
use ogham::Ogham;

const W: f32 = 800.0;
const H: f32 = 600.0;
const DT: f32 = 1.0 / 60.0;

type Log = Arc<Mutex<Vec<(String, String)>>>;

/// Two fields in a column, each 200 × 30, and a button under them.
const TWO_FIELDS: &str = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow", direction: "column" },
    children: [
      TextInput {
        value: "first",
        style: { width: 200, height: 30 },
        on_blur: fn (v: string) { event("blur_a", v); },
      },
      TextInput {
        value: "second",
        style: { width: 200, height: 30 },
        on_blur: fn (v: string) { event("blur_b", v); },
      },
      Flex {
        style: { width: 200, height: 40 },
        mouse_down: fn () { event("button"); },
      },
    ],
  }
};
"#;

fn mounted(src: &str) -> (Ogham, Log) {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let mut config = RuntimeConfig::new();
    for name in ["blur_a", "blur_b", "button"] {
        let log = log.clone();
        config = config.with_event_handler(name, move |args| {
            let value = match args.first() {
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            };
            log.lock().unwrap().push((name.to_string(), value));
            Ok(Value::Void)
        });
    }
    let mut o = Ogham::from_source(src, config).expect("the document mounts");
    o.frame(W, H, DT).expect("frame");
    (o, log)
}

fn press(o: &mut Ogham, x: f32, y: f32) {
    o.get_ui_mut().call_event(&Event::with_point(
        "mouse_down".to_string(),
        Point::new(x, y),
    ));
    o.get_ui_mut()
        .call_event(&Event::with_point("mouse_up".to_string(), Point::new(x, y)));
}

fn keydown(o: &mut Ogham, code: u32) -> bool {
    o.get_ui_mut()
        .call_event(&Event::keydown(code, None, KeyModifiers::default()))
}

fn field(o: &Ogham, index: usize) -> WidgetRef {
    o.get_ui().root.lock().unwrap().get_children()[index].clone()
}

#[test]
fn a_press_elsewhere_blurs_with_the_value() {
    let (mut o, log) = mounted(TWO_FIELDS);
    press(&mut o, 10.0, 10.0);
    assert!(
        log.lock().unwrap().is_empty(),
        "gaining focus is not a blur"
    );
    // Pressing the same field again is not a blur either.
    press(&mut o, 20.0, 15.0);
    assert!(log.lock().unwrap().is_empty());
    // The press's own handler runs during dispatch; the blur is reported
    // once the dispatch has settled where focus went — so the button
    // hears the press before the field hears it lost focus.
    press(&mut o, 10.0, 80.0);
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            ("button".to_string(), String::new()),
            ("blur_a".to_string(), "first".to_string())
        ]
    );
}

#[test]
fn tab_blurs_the_field_it_leaves() {
    let (mut o, log) = mounted(TWO_FIELDS);
    press(&mut o, 10.0, 10.0);
    assert!(
        keydown(&mut o, 9),
        "Tab is consumed while a field is focused"
    );
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [("blur_a".to_string(), "first".to_string())]
    );
    assert!(std::sync::Arc::ptr_eq(
        o.get_ui().get_focused().unwrap(),
        &field(&o, 1)
    ));
    // And back: Shift-Tab blurs the second.
    o.get_ui_mut().call_event(&Event::keydown(
        9,
        None,
        KeyModifiers {
            shift: true,
            ..KeyModifiers::default()
        },
    ));
    assert_eq!(
        log.lock().unwrap()[1],
        ("blur_b".to_string(), "second".to_string())
    );
}

#[test]
fn escape_blurs() {
    let (mut o, log) = mounted(TWO_FIELDS);
    press(&mut o, 10.0, 40.0);
    assert!(keydown(&mut o, 27));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [("blur_b".to_string(), "second".to_string())]
    );
    assert!(o.get_ui().get_focused().is_none());
}

/// A right-aligned single-line field puts the caret at the right edge
/// of its content box for a short value: the input is 200 wide with 10
/// of padding, so the caret sits at x = 190.
#[test]
fn a_right_aligned_caret_sits_at_the_right_edge_minus_padding() {
    let src = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow" },
    children: [
      TextInput { value: "ab", style: { width: 200, height: 30, padding: 10, align: "right", size: 14 } },
    ],
  }
};
"#;
    let (mut o, _log) = mounted(src);
    press(&mut o, 100.0, 15.0);
    // Caret to the end regardless of where the press landed.
    keydown(&mut o, 35);
    let input = field(&o, 0);
    let g = input.lock().unwrap();
    let input = g.downcast_ref::<TextInputWidget>().expect("a TextInput");
    let (x, _top, _height) = input.caret_rect().expect("laid out");
    assert!((x - 190.0).abs() < 0.5, "caret x = {x}, expected 190");

    // And at the start of the value the caret is the run's width back
    // from that edge — the run is seated, not stretched.
    drop(g);
    keydown(&mut o, 36);
    let input = field(&o, 0);
    let g = input.lock().unwrap();
    let input = g.downcast_ref::<TextInputWidget>().expect("a TextInput");
    let (x0, _, _) = input.caret_rect().unwrap();
    assert!(x0 < 190.0 && x0 > 150.0, "caret x = {x0}");
}

/// An empty right-aligned field parks the caret at the right edge too.
#[test]
fn an_empty_right_aligned_field_carets_at_the_right_edge() {
    let src = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow" },
    children: [
      TextInput { value: "", style: { width: 200, height: 30, padding: 10, align: "right" } },
    ],
  }
};
"#;
    let (o, _log) = mounted(src);
    let input = field(&o, 0);
    let g = input.lock().unwrap();
    let input = g.downcast_ref::<TextInputWidget>().unwrap();
    let (x, _, _) = input.caret_rect().unwrap();
    assert!((x - 190.0).abs() < 0.5, "caret x = {x}");
}

/// Records every `draw_text`.
#[derive(Default)]
struct Recorder {
    texts: Vec<(String, u8, f32)>,
}

impl RenderContext for Recorder {
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
    fn draw_text(&mut self, text: &str, style: &TextStyle, x: f32, _y: f32, _width: f32) {
        self.texts.push((text.to_string(), style.get_color().a, x));
    }
    fn draw_line(&mut self, _x1: f32, _y1: f32, _x2: f32, _y2: f32, _w: f32, _color: &Color) {}
}

fn painted(input: &WidgetRef, focused: bool) -> Vec<(String, u8, f32)> {
    let mut rec = Recorder::default();
    let mut cache = ImageCache::new();
    input.lock().unwrap().render(&mut rec, focused, &mut cache);
    rec.texts
}

#[test]
fn the_placeholder_paints_only_while_empty_and_unfocused() {
    let src = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow" },
    children: [
      TextInput { value: "", placeholder: "Type here", style: { width: 200, height: 30, padding: 10, color: { r: 0, g: 0, b: 0, a: 200 } } },
    ],
  }
};
"#;
    let (o, _log) = mounted(src);
    let input = field(&o, 0);
    let unfocused = painted(&input, false);
    assert_eq!(unfocused.len(), 1);
    assert_eq!(unfocused[0].0, "Type here");
    assert_eq!(unfocused[0].1, 100, "the text style at half alpha");
    assert_eq!(unfocused[0].2, 10.0, "at the text origin");
    assert!(
        painted(&input, true).is_empty(),
        "focused: caret only, no placeholder"
    );

    // Typed into: the value paints, the placeholder does not.
    let mut o = o;
    press(&mut o, 20.0, 15.0);
    o.get_ui_mut().call_event(&Event::keypress(
        'h' as u32,
        Some('h'),
        KeyModifiers::default(),
    ));
    let input = field(&o, 0);
    let typed = painted(&input, false);
    assert_eq!(typed.len(), 1);
    assert_eq!(typed[0].0, "h");
    assert_eq!(typed[0].1, 200);
}

/// A press takes capture, so moves past the box's edge keep extending
/// the selection from the press: drag-select fell out of capture.
#[test]
fn dragging_from_a_press_selects() {
    let src = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow" },
    children: [
      TextInput { value: "hello", style: { width: 200, height: 30, padding: 10, size: 14 } },
    ],
  }
};
"#;
    let (mut o, _log) = mounted(src);
    o.get_ui_mut().call_event(&Event::with_point(
        "mouse_down".to_string(),
        Point::new(11.0, 15.0),
    ));
    assert!(o.get_ui().captured().is_some(), "the press took capture");
    o.get_ui_mut().call_event(&Event::with_point(
        "mouse_move".to_string(),
        Point::new(700.0, 400.0),
    ));
    o.get_ui_mut().call_event(&Event::with_point(
        "mouse_up".to_string(),
        Point::new(700.0, 400.0),
    ));
    assert!(o.get_ui().captured().is_none(), "the release ended it");
    let input = field(&o, 0);
    let g = input.lock().unwrap();
    let input = g.downcast_ref::<TextInputWidget>().unwrap();
    assert_eq!(input.selection.start(), 0);
    assert_eq!(
        input.selection.end(),
        5,
        "dragged past the end selects to the end"
    );
}
