//! `Portal { anchor: "press" }` — a popover seated at the last pointer
//! press, and the right-click that opens one.
//!
//! A context menu is the consumer: a row's `contextmenu:` listener flips
//! host state, the document re-renders with the menu's Portal open, and
//! the menu has to stand where the hand was — a frame *after* the press,
//! with no host coordinates in the `.ogh`. The runtime records every
//! `mouse_down` and `contextmenu` point under `PRESS_ANCHOR`, and the
//! builder spells it `"press"`.
//!
//! The second half is dismissal. A left press outside a dismissing
//! popover is swallowed and reported (`popover_dismiss.rs`); a
//! *right*-click outside one is reported and then **falls through**, so
//! right-clicking another row while a menu is up closes the menu and
//! opens the row's in one gesture.

use std::sync::{Arc, Mutex};

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::skia::SkiaEnv;
use ogham::widget::event::Event;
use ogham::widget::point::Point;
use ogham::widget::{PortalEntry, Surface, PRESS_ANCHOR};
use ogham::Ogham;

const W: f32 = 800.0;
const H: f32 = 600.0;
const DT: f32 = 1.0 / 60.0;

type Log = Arc<Mutex<Vec<String>>>;

/// Two full-width rows, each raising `row_menu(i)` on a right-click,
/// and one press-anchored 160 × 60 menu whose `open` is host state.
const SRC: &str = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow", direction: "column" },
    block_interactions: false,
    children: [
      Flex {
        style: { width: "grow", height: 100 },
        contextmenu: fn () { event("row_menu", 0); },
        mouse_down: fn () { event("row_press", 0); },
        children: [],
      },
      Flex {
        style: { width: "grow", height: 100 },
        contextmenu: fn () { event("row_menu", 1); },
        mouse_down: fn () { event("row_press", 1); },
        children: [],
      },
      Portal {
        open: menu.open,
        layer: "popover",
        anchor: "press",
        dismiss: fn () { event("menu_close"); },
        children: [
          Flex {
            style: { width: 160, height: 60 },
            mouse_down: fn () { event("menu_press"); },
            children: [],
          }
        ],
      },
    ],
  }
};
"#;

fn config(open: bool, log: &Log) -> RuntimeConfig {
    let mut config = RuntimeConfig::new();
    for name in ["row_menu", "row_press", "menu_close", "menu_press"] {
        let log = log.clone();
        config = config.with_event_handler(name, move |args| {
            let arg = match args.first() {
                Some(Value::Integer(i)) => format!("({i})"),
                _ => String::new(),
            };
            log.lock().unwrap().push(format!("{name}{arg}"));
            Ok(Value::Void)
        });
    }
    let mut menu = std::collections::HashMap::new();
    menu.insert("open".to_string(), Value::Boolean(open));
    config.with_host_state([("menu".to_string(), Value::Map(menu.into()))].into_iter().collect())
}

fn mounted(open: bool) -> (Ogham, Log) {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let mut o = Ogham::from_source(SRC, config(open, &log)).expect("the document mounts");
    frame(&mut o);
    (o, log)
}

fn frame(o: &mut Ogham) {
    for _ in 0..3 {
        o.frame(W, H, DT).expect("frame");
    }
    let surface =
        ogham::skia_safe::surfaces::raster_n32_premul((256, 256)).expect("raster surface");
    let mut env = SkiaEnv::new_with_dpi_scale(surface, 1.0);
    env.draw(o.get_ui_mut());
}

fn send(o: &mut Ogham, name: &str, x: f32, y: f32) -> bool {
    o.get_ui_mut()
        .call_event(&Event::with_point(name.to_string(), Point::new(x, y)))
}

fn set_open(o: &mut Ogham, open: bool) {
    o.with_runtime_mut(|rt| {
        let mut menu = std::collections::HashMap::new();
        menu.insert("open".to_string(), Value::Boolean(open));
        rt.set_host_state("menu", Value::Map(menu.into()));
    });
    frame(o);
}

fn press_at(o: &Ogham) -> Option<(f32, f32)> {
    o.get_ui().anchor(PRESS_ANCHOR).map(|p| (p.x(), p.y()))
}

fn only_entry(o: &Ogham) -> Option<PortalEntry> {
    let entries: Vec<PortalEntry> = o.get_ui().portal_layers.iter_hit_test_order().cloned().collect();
    assert!(entries.len() <= 1, "one entry at most");
    entries.into_iter().next()
}

/// The press is recorded before it is answered, so a menu the answer
/// opens stands at the press on the next frame.
#[test]
fn a_right_click_records_the_press_and_the_menu_opens_there() {
    let (mut o, log) = mounted(false);
    assert!(o.get_ui().anchor(PRESS_ANCHOR).is_none(), "nothing pressed yet");
    assert!(only_entry(&o).is_none(), "a press-anchored portal with no press is not painted");
    assert!(send(&mut o, "contextmenu", 300.0, 150.0), "the row takes the right-click");
    assert_eq!(log.lock().unwrap().as_slice(), ["row_menu(1)"]);
    assert_eq!(press_at(&o), Some((300.0, 150.0)));
    set_open(&mut o, true);
    let entry = only_entry(&o).expect("the menu is painted");
    assert_eq!((entry.viewport_rect.x, entry.viewport_rect.y), (300.0, 150.0));
    assert_eq!((entry.viewport_rect.width, entry.viewport_rect.height), (160.0, 60.0));
    // And a press on it is the menu's.
    log.lock().unwrap().clear();
    assert!(send(&mut o, "mouse_down", 320.0, 170.0));
    assert_eq!(log.lock().unwrap().as_slice(), ["menu_press"]);
}

/// A left press records the point too — a popover may open at a click.
#[test]
fn a_left_press_is_a_press() {
    let (mut o, _log) = mounted(false);
    assert!(send(&mut o, "mouse_down", 40.0, 40.0));
    assert_eq!(press_at(&o), Some((40.0, 40.0)));
}

/// The menu seats where the press was, not where the pointer is now:
/// the anchor is the press's and a move does not touch it.
#[test]
fn the_pointer_moving_does_not_move_the_menu() {
    let (mut o, _log) = mounted(false);
    send(&mut o, "contextmenu", 300.0, 150.0);
    send(&mut o, "mouse_move", 500.0, 400.0);
    assert_eq!(press_at(&o), Some((300.0, 150.0)));
}

/// A right-click outside the open menu closes it *and* reaches the row
/// under it: one gesture moves the menu from row to row.
#[test]
fn a_right_click_outside_the_menu_dismisses_it_and_reaches_the_row_beneath() {
    let (mut o, log) = mounted(false);
    send(&mut o, "contextmenu", 300.0, 150.0);
    set_open(&mut o, true);
    log.lock().unwrap().clear();
    assert!(send(&mut o, "contextmenu", 300.0, 50.0), "handled: dismissed and re-targeted");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["menu_close", "row_menu(0)"],
        "the menu is told first, then the row under the hand"
    );
    assert_eq!(press_at(&o), Some((300.0, 50.0)), "the new press");
}

/// A left press outside is the ordinary dismiss: swallowed and reported,
/// nothing under it told (`popover_dismiss.rs`'s rule, unchanged).
#[test]
fn a_left_press_outside_the_menu_dismisses_it_and_stops() {
    let (mut o, log) = mounted(false);
    send(&mut o, "contextmenu", 300.0, 150.0);
    set_open(&mut o, true);
    log.lock().unwrap().clear();
    assert!(send(&mut o, "mouse_down", 300.0, 50.0));
    assert_eq!(log.lock().unwrap().as_slice(), ["menu_close"]);
}

/// A right-click outside the menu on nothing at all still closes it, and
/// reports handled — the chrome owned the gesture.
#[test]
fn a_right_click_on_the_page_closes_the_menu() {
    let (mut o, log) = mounted(false);
    send(&mut o, "contextmenu", 300.0, 150.0);
    set_open(&mut o, true);
    log.lock().unwrap().clear();
    assert!(send(&mut o, "contextmenu", 300.0, 500.0));
    assert_eq!(log.lock().unwrap().as_slice(), ["menu_close"]);
}

/// `"press"` is a reserved word the way `"parent"` is: a press anchor
/// cannot trap focus, because a trap over a point that predates the
/// document is a trap over nothing.
#[test]
fn a_press_anchor_cannot_trap_focus() {
    let src = SRC.replace(r#"anchor: "press","#, r#"anchor: "press", focus_trap: true,"#);
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let err = Ogham::from_source(&src, config(true, &log)).err().expect("refused");
    assert!(format!("{err:?}").contains("focus_trap"), "{err:?}");
}
