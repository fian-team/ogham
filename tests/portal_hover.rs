//! Hover resolves through the portal layers the way a press does.
//!
//! A `mouse_move` walks the layers high→low before the base tree: a
//! point inside an open entry's content hovers that content at its
//! painted position, and a layer whose open entries carry a `block` or
//! `dismiss` backdrop stops hover from reaching anything under it — the
//! widget below reads as un-hovered and gets its `mouse_leave`. A `none`
//! backdrop falls through. Pointer capture keeps the hover chain where
//! the press left it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::skia::SkiaEnv;
use ogham::widget::event::Event;
use ogham::widget::point::Point;
use ogham::widget::{Surface, WidgetRef};
use ogham::Ogham;

const W: f32 = 800.0;
const H: f32 = 600.0;
const DT: f32 = 1.0 / 60.0;

type Log = Arc<Mutex<Vec<String>>>;

const EVENTS: [&str; 6] = [
    "bar_enter",
    "bar_leave",
    "inner_enter",
    "inner_leave",
    "below_enter",
    "below_leave",
];

/// A full-width bar at the top with a portal anchored to it, and a
/// full-width target under it. The portal's content is 200 × 80 at
/// (0, 100); the target spans y 100..400. `open` is host state so a
/// test can close the portal. The bar and the target are transparent
/// (`block_interactions: false`) so `hovered_blocks` reports the
/// backdrop's obstruction and not their own; the content blocks, as a
/// menu does.
fn document(portal_extra: &str) -> String {
    format!(
        r#"
let main = fn () {{
  Flex {{
    style: {{ width: "grow", height: "grow", direction: "column" }},
    block_interactions: false,
    children: [
      Flex {{
        style: {{ width: "grow", height: 100 }},
        block_interactions: false,
        mouse_enter: fn () {{ event("bar_enter"); }},
        mouse_leave: fn () {{ event("bar_leave"); }},
        children: [
          Portal {{
            open: open,
            anchor: "parent",
            {portal_extra}
            children: [
              Flex {{
                style: {{ width: 200, height: 80 }},
                mouse_enter: fn () {{ event("inner_enter"); }},
                mouse_leave: fn () {{ event("inner_leave"); }},
              }}
            ],
          }}
        ],
      }},
      Flex {{
        style: {{ width: "grow", height: 300 }},
        block_interactions: false,
        mouse_enter: fn () {{ event("below_enter"); }},
        mouse_leave: fn () {{ event("below_leave"); }},
      }},
    ],
  }}
}};
"#
    )
}

fn draw(o: &mut Ogham) {
    for _ in 0..3 {
        o.frame(W, H, DT).expect("frame");
    }
    let surface =
        ogham::skia_safe::surfaces::raster_n32_premul((256, 256)).expect("raster surface");
    let mut env = SkiaEnv::new_with_dpi_scale(surface, 1.0);
    env.draw(o.get_ui_mut());
}

fn mounted(src: &str) -> (Ogham, Log) {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let mut config = RuntimeConfig::new()
        .with_host_state(HashMap::from([("open".to_string(), Value::Boolean(true))]));
    for name in EVENTS {
        let log = log.clone();
        config = config.with_event_handler(name, move |_| {
            log.lock().unwrap().push(name.to_string());
            Ok(Value::Void)
        });
    }
    let mut o = Ogham::from_source(src, config).expect("the document mounts");
    draw(&mut o);
    (o, log)
}

fn set_open(o: &mut Ogham, open: bool) {
    o.with_runtime_mut(|rt| rt.set_host_state("open", open));
    draw(o);
}

fn hover(o: &mut Ogham, x: f32, y: f32) -> bool {
    o.get_ui_mut().call_event(&Event::with_point(
        "mouse_move".to_string(),
        Point::new(x, y),
    ))
}

fn child(w: &WidgetRef, i: usize) -> WidgetRef {
    w.lock().unwrap().get_children()[i].clone()
}

fn bar(o: &Ogham) -> WidgetRef {
    child(&o.get_ui().root, 0)
}

fn below(o: &Ogham) -> WidgetRef {
    child(&o.get_ui().root, 1)
}

fn inner(o: &Ogham) -> WidgetRef {
    child(&child(&bar(o), 0), 0)
}

fn hovered(w: &WidgetRef) -> bool {
    w.lock().unwrap().is_hovered()
}

fn take(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.lock().unwrap())
}

const POPOVER: &str = r#"layer: "popover","#;
const MODAL: &str = r#"layer: "overlay-modal","#;

/// Off the popover, over the target under it: the target is not hovered
/// and never told it was.
#[test]
fn hover_under_an_open_dismiss_popover_does_not_reach_the_widget_beneath() {
    let (mut o, log) = mounted(&document(POPOVER));
    hover(&mut o, 600.0, 300.0);
    assert!(
        !hovered(&below(&o)),
        "the target under the popover is obstructed"
    );
    assert!(!hovered(&inner(&o)));
    assert!(take(&log).is_empty(), "no enter fired below the backdrop");
    assert!(
        o.get_ui().hovered_blocks(),
        "the world under a dismissing popover gets no vote"
    );
}

/// Inside the popover's content, at its painted position: the content
/// is hovered and told so.
#[test]
fn hover_inside_the_popover_content_reaches_it() {
    let (mut o, log) = mounted(&document(POPOVER));
    assert!(hover(&mut o, 50.0, 120.0), "the hover chain changed");
    assert!(hovered(&inner(&o)));
    assert!(!hovered(&below(&o)), "the target under the content is not");
    assert!(
        !hovered(&bar(&o)),
        "nor the face the popover is declared in"
    );
    assert_eq!(take(&log), ["inner_enter"]);
    // Leaving the content: the leave fires, and nothing under it enters.
    hover(&mut o, 600.0, 300.0);
    assert!(!hovered(&inner(&o)));
    assert_eq!(take(&log), ["inner_leave"]);
}

/// Once the popover closes, the pointer that was parked over the target
/// hovers it on the next move.
#[test]
fn hover_returns_to_the_widget_beneath_after_the_popover_closes() {
    let (mut o, log) = mounted(&document(POPOVER));
    hover(&mut o, 600.0, 300.0);
    assert!(!hovered(&below(&o)));
    set_open(&mut o, false);
    assert!(hover(&mut o, 600.0, 301.0));
    assert!(hovered(&below(&o)));
    assert_eq!(take(&log), ["below_enter"]);
    assert!(!o.get_ui().hovered_blocks());
}

/// The other direction: a hovered target loses its hover — with a
/// `mouse_leave` — when a popover opens over it and the pointer moves.
#[test]
fn a_popover_opening_takes_hover_off_the_widget_beneath() {
    let (mut o, log) = mounted(&document(POPOVER));
    set_open(&mut o, false);
    hover(&mut o, 600.0, 300.0);
    assert!(hovered(&below(&o)));
    assert_eq!(take(&log), ["below_enter"]);
    set_open(&mut o, true);
    assert!(hover(&mut o, 600.0, 301.0), "the hover chain changed");
    assert!(!hovered(&below(&o)));
    assert_eq!(take(&log), ["below_leave"]);
}

/// The same rules under a blocking modal.
#[test]
fn hover_under_a_blocking_modal_is_obstructed_and_inside_it_is_not() {
    let (mut o, log) = mounted(&document(MODAL));
    hover(&mut o, 600.0, 300.0);
    assert!(!hovered(&below(&o)), "the modal's backdrop obstructs");
    assert!(take(&log).is_empty());
    assert!(o.get_ui().hovered_blocks());
    assert!(o.get_ui().blocks_point(&Point::new(600.0, 300.0)));

    hover(&mut o, 50.0, 120.0);
    assert!(hovered(&inner(&o)));
    assert!(!hovered(&below(&o)));
    assert_eq!(take(&log), ["inner_enter"]);
    hover(&mut o, 600.0, 300.0);
    assert_eq!(
        take(&log),
        ["inner_leave"],
        "and still nothing enters below"
    );

    set_open(&mut o, false);
    hover(&mut o, 600.0, 301.0);
    assert!(hovered(&below(&o)), "hover returns once the modal is gone");
    assert_eq!(take(&log), ["below_enter"]);
    assert!(!o.get_ui().hovered_blocks());
}

/// `backdrop: "none"` lets hover fall through to the target under the
/// popover — and the content still takes it where it paints.
#[test]
fn a_none_backdrop_passes_hover_through() {
    let (mut o, log) = mounted(&document(r#"layer: "popover", backdrop: "none","#));
    hover(&mut o, 600.0, 300.0);
    assert!(hovered(&below(&o)));
    assert_eq!(take(&log), ["below_enter"]);
    assert!(
        !o.get_ui().hovered_blocks(),
        "a pass-through popover obstructs nothing"
    );

    hover(&mut o, 50.0, 120.0);
    assert!(
        hovered(&inner(&o)),
        "the content wins over the target it covers"
    );
    assert!(!hovered(&below(&o)));
    // The layer pass runs before the base tree's, so the enter lands
    // before the leave; only the pair is the contract.
    let mut fired = take(&log);
    fired.sort();
    assert_eq!(fired, ["below_leave", "inner_enter"]);
}

/// A widget holding pointer capture keeps the hover chain wherever the
/// pointer goes: no walk runs, so nothing else lights up and the
/// captured widget never gets a `mouse_leave` mid-gesture.
#[test]
fn a_captured_widget_keeps_hover_while_the_pointer_is_elsewhere() {
    let src = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow", direction: "column" },
    block_interactions: false,
    children: [
      Flex {
        style: { width: "grow", height: 100 },
        mouse_enter: fn () { event("bar_enter"); },
        mouse_leave: fn () { event("bar_leave"); },
        children: [
          Slider {
            value: 0, min: 0, max: 1,
            style: { width: 200, height: 20 },
          }
        ],
      },
      Flex {
        style: { width: "grow", height: 300 },
        mouse_enter: fn () { event("below_enter"); },
        mouse_leave: fn () { event("below_leave"); },
      },
    ],
  }
};
"#;
    let (mut o, log) = mounted(src);
    let slider = child(&bar(&o), 0);
    hover(&mut o, 50.0, 10.0);
    assert!(hovered(&slider));
    assert_eq!(take(&log), ["bar_enter"]);

    o.get_ui_mut().call_event(&Event::with_point(
        "mouse_down".to_string(),
        Point::new(50.0, 10.0),
    ));
    assert!(o.get_ui().captured().is_some(), "the press took capture");

    hover(&mut o, 600.0, 300.0);
    assert!(hovered(&slider), "the captured widget keeps hover");
    assert!(hovered(&bar(&o)));
    assert!(!hovered(&below(&o)), "nothing else takes it");
    assert!(take(&log).is_empty());

    o.get_ui_mut().call_event(&Event::with_point(
        "mouse_up".to_string(),
        Point::new(600.0, 300.0),
    ));
    assert!(o.get_ui().captured().is_none());
    hover(&mut o, 600.0, 301.0);
    assert!(
        !hovered(&slider),
        "released, the pointer hovers what it is over"
    );
    assert!(hovered(&below(&o)));
    assert_eq!(take(&log), ["bar_leave", "below_enter"]);
}
