//! The `dismiss` backdrop policy: a press outside a popover is swallowed
//! and reported to it, instead of reaching whatever is under it.
//!
//! `popover` defaults to it; a Portal can override with `backdrop:`.
//! The composition this replaces — a full-viewport catch child ahead of
//! the menu — cannot be written for a parent-anchored popover, whose
//! children lay out inside the face.

use std::sync::{Arc, Mutex};

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::skia::SkiaEnv;
use ogham::widget::event::Event;
use ogham::widget::point::Point;
use ogham::widget::Surface;
use ogham::Ogham;

const W: f32 = 800.0;
const H: f32 = 600.0;
const DT: f32 = 1.0 / 60.0;

type Log = Arc<Mutex<Vec<String>>>;

/// A full-width bar at the top with a popover anchored to it, and a
/// full-width target under it. The popover is 200 × 80 at (0, 100).
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
        mouse_down: fn () {{ event("bar_press"); }},
        children: [
          Portal {{
            open: true,
            layer: "popover",
            anchor: "parent",
            dismiss: fn () {{ event("dismissed"); }},
            {portal_extra}
            children: [
              Flex {{
                style: {{ width: 200, height: 80 }},
                mouse_down: fn () {{ event("inner_press"); }},
              }}
            ],
          }}
        ],
      }},
      Flex {{
        style: {{ width: "grow", height: 300 }},
        mouse_down: fn () {{ event("below_press"); }},
      }},
    ],
  }}
}};
"#
    )
}

fn mounted(src: &str) -> (Ogham, Log) {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let mut config = RuntimeConfig::new();
    for name in ["bar_press", "inner_press", "below_press", "dismissed"] {
        let log = log.clone();
        config = config.with_event_handler(name, move |_| {
            log.lock().unwrap().push(name.to_string());
            Ok(Value::Void)
        });
    }
    let mut o = Ogham::from_source(src, config).expect("the document mounts");
    for _ in 0..3 {
        o.frame(W, H, DT).expect("frame");
    }
    let surface =
        ogham::skia_safe::surfaces::raster_n32_premul((256, 256)).expect("raster surface");
    let mut env = SkiaEnv::new_with_dpi_scale(surface, 1.0);
    env.draw(o.get_ui_mut());
    (o, log)
}

fn send(o: &mut Ogham, name: &str, x: f32, y: f32) -> bool {
    o.get_ui_mut()
        .call_event(&Event::with_point(name.to_string(), Point::new(x, y)))
}

/// Outside: the popover is told, the widget under the press is not.
#[test]
fn a_press_outside_fires_dismiss_and_never_reaches_the_widget_under_it() {
    let (mut o, log) = mounted(&document(""));
    assert!(
        send(&mut o, "mouse_down", 600.0, 300.0),
        "the press is consumed"
    );
    assert_eq!(log.lock().unwrap().as_slice(), ["dismissed"]);
}

/// Inside: the popover's own handler, and no dismiss.
#[test]
fn a_press_inside_fires_the_inner_handler_and_no_dismiss() {
    let (mut o, log) = mounted(&document(""));
    assert!(send(&mut o, "mouse_down", 50.0, 120.0));
    assert_eq!(log.lock().unwrap().as_slice(), ["inner_press"]);
}

/// Only a press dismisses. The release that follows an outside press
/// is still swallowed — nothing under the popover gets half a gesture —
/// but it is not a second dismissal.
#[test]
fn a_release_outside_is_swallowed_but_does_not_dismiss() {
    let (mut o, log) = mounted(&document(""));
    assert!(
        !send(&mut o, "mouse_up", 600.0, 300.0),
        "swallowed, nothing fired"
    );
    assert!(log.lock().unwrap().is_empty());
}

/// `backdrop: "none"` opts a popover out: the press falls through.
#[test]
fn a_none_backdrop_lets_the_press_through() {
    let (mut o, log) = mounted(&document(r#"backdrop: "none","#));
    assert!(send(&mut o, "mouse_down", 600.0, 300.0));
    assert_eq!(log.lock().unwrap().as_slice(), ["below_press"]);
}

/// `backdrop: "block"` swallows silently, like a modal layer does.
#[test]
fn a_block_backdrop_swallows_without_reporting() {
    let (mut o, log) = mounted(&document(r#"backdrop: "block","#));
    assert!(!send(&mut o, "mouse_down", 600.0, 300.0));
    assert!(log.lock().unwrap().is_empty());
}

/// While a dismissing popover is open, the world under the chrome gets
/// no vote anywhere: `blocks_point` is true off the popover too.
#[test]
fn a_dismissing_popover_occludes_the_whole_viewport() {
    let (o, _log) = mounted(&document(""));
    assert!(o.get_ui().blocks_point(&Point::new(600.0, 300.0)));
    let (o, _log) = mounted(&document(r#"backdrop: "none","#));
    assert!(
        o.get_ui().blocks_point(&Point::new(600.0, 300.0)),
        "the target under the press blocks on its own"
    );
    assert!(
        !o.get_ui().blocks_point(&Point::new(600.0, 500.0)),
        "past the target, nothing blocks under a pass-through popover"
    );
}

/// An unknown policy name is refused, not defaulted.
#[test]
fn an_unknown_backdrop_name_is_a_build_error() {
    let src = document(r#"backdrop: "modal","#);
    let err = Ogham::from_source(&src, RuntimeConfig::new())
        .err()
        .expect("the build is refused");
    let text = format!("{err:?}");
    assert!(text.contains("backdrop"), "{text}");
    assert!(text.contains("none, dismiss, block"), "{text}");
}
