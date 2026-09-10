//! `Portal { anchor: "parent" }` — a popover seated against the widget it
//! was declared inside, with no host coordinates.
//!
//! The face + portal composition is what a dropdown is: the Portal is a
//! child of the face, so the walk that reaches it is standing on the
//! face's laid-out box and hands it down as the frame. The entry's
//! `viewport_rect` lands where an `anchor: "<host id>"` entry's would,
//! so paint, hit-testing and occlusion follow with no further changes —
//! which the click tests here verify against a real `SkiaEnv` walk
//! rather than assume.

use std::sync::{Arc, Mutex};

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::skia::SkiaEnv;
use ogham::widget::event::Event;
use ogham::widget::point::Point;
use ogham::widget::{PortalEntry, Surface};
use ogham::Ogham;

const W: f32 = 800.0;
const H: f32 = 600.0;
const DT: f32 = 1.0 / 60.0;

type Log = Arc<Mutex<Vec<String>>>;

/// A face (120 × 32) inside a padded column, with a 200 × 100 popover
/// anchored to it, and a full-width sibling under the row that the
/// popover overlaps. `portal_extra` is spliced into the Portal's
/// properties; `before_row` before the row (a spacer, to push the face
/// to the bottom); `sibling_height` sizes the sibling.
fn document(before_row: &str, portal_extra: &str, sibling_height: f32) -> String {
    format!(
        r#"
let main = fn () {{
  Flex {{
    style: {{ width: "grow", height: "grow", direction: "column", padding: 20 }},
    children: [
      {before_row}
      Flex {{
        style: {{ width: "grow", height: "shrink", direction: "row" }},
        children: [
          Flex {{
            key: "face",
            style: {{ width: 120, height: 32 }},
            mouse_down: fn () {{ event("face_press"); }},
            children: [
              Portal {{
                open: true,
                layer: "popover",
                anchor: "parent",
                {portal_extra}
                children: [
                  Flex {{
                    style: {{ width: 200, height: 100 }},
                    mouse_down: fn () {{ event("popover_press"); }},
                  }}
                ],
              }}
            ],
          }}
        ],
      }},
      Flex {{
        style: {{ width: "grow", height: {sibling_height} }},
        mouse_down: fn () {{ event("sibling_press"); }},
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
    for name in ["face_press", "popover_press", "sibling_press"] {
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
    (o, log)
}

fn draw(o: &mut Ogham) {
    let surface =
        ogham::skia_safe::surfaces::raster_n32_premul((256, 256)).expect("raster surface");
    let mut env = SkiaEnv::new_with_dpi_scale(surface, 1.0);
    env.draw(o.get_ui_mut());
}

fn only_entry(o: &Ogham) -> PortalEntry {
    let layers = &o.get_ui().portal_layers;
    assert_eq!(layers.len(), 1, "one portal entry");
    layers.iter_paint_order().next().cloned().unwrap()
}

fn press(o: &mut Ogham, x: f32, y: f32) -> bool {
    o.get_ui_mut().call_event(&Event::with_point(
        "mouse_down".to_string(),
        Point::new(x, y),
    ))
}

/// The face lays out at (20, 20) — the column's padding — so its
/// bottom-left is (20, 52), and that is where the popover's entry seats.
#[test]
fn a_parent_anchored_popover_seats_at_the_faces_bottom_left() {
    let (mut o, _log) = mounted(&document("", "", 200.0));
    draw(&mut o);
    let entry = only_entry(&o);
    assert_eq!((entry.viewport_rect.x, entry.viewport_rect.y), (20.0, 52.0));
    assert_eq!(
        (entry.viewport_rect.width, entry.viewport_rect.height),
        (200.0, 100.0),
        "the entry is the size of what it paints, not of the face"
    );
}

/// `anchor_offset` nudges from the bottom-left, as it does from a host
/// point.
#[test]
fn the_offset_applies_to_a_parent_anchor_too() {
    let (mut o, _log) = mounted(&document("", r#"anchor_offset: { x: 4, y: 6 },"#, 200.0));
    draw(&mut o);
    let entry = only_entry(&o);
    assert_eq!((entry.viewport_rect.x, entry.viewport_rect.y), (24.0, 58.0));
}

/// A press inside the popover is the popover's; the sibling it overlaps
/// never sees it. The popover occupies (20..220, 52..152) and the
/// sibling starts at y = 52, so (60, 100) is inside both.
#[test]
fn a_press_in_the_popover_does_not_reach_the_sibling_beneath() {
    let (mut o, log) = mounted(&document("", "", 200.0));
    draw(&mut o);
    assert!(press(&mut o, 60.0, 100.0), "the popover consumed the press");
    assert_eq!(log.lock().unwrap().as_slice(), ["popover_press"]);
}

/// And the face itself still takes its own press: the Portal inside it
/// contributes nothing to the base tree's hit test. `backdrop: "none"`
/// here, because under the popover layer's default a press on the face
/// while its menu is open is an *outside* press and closes the menu
/// (`tests/popover_dismiss.rs`).
#[test]
fn the_face_still_takes_its_own_press() {
    let (mut o, log) = mounted(&document("", r#"backdrop: "none","#, 200.0));
    draw(&mut o);
    assert!(press(&mut o, 60.0, 30.0));
    assert_eq!(log.lock().unwrap().as_slice(), ["face_press"]);
}

/// Under the default policy that same press is the popover's to
/// dismiss: the face's own handler does not run, so a face that toggles
/// `open` cannot re-open the menu it just closed.
#[test]
fn a_press_on_the_face_while_open_is_an_outside_press() {
    let (mut o, log) = mounted(&document("", "", 200.0));
    draw(&mut o);
    assert!(
        !press(&mut o, 60.0, 30.0),
        "swallowed; no dismiss listener here"
    );
    assert!(log.lock().unwrap().is_empty());
}

/// With `flip`, a popover that would overrun the bottom goes above the
/// face — its bottom edge at the face's *top*, not at the anchor point.
/// A grow spacer pushes the row down: with a 40 px sibling under it the
/// row lands at y = 600 − 20 − 40 − 32 = 508, so below the face the
/// popover would end at 640 > 592.
#[test]
fn flip_puts_the_popover_above_the_whole_face() {
    let spacer = r#"Flex { style: { width: "grow", height: "grow" } },"#;
    let (mut o, _log) = mounted(&document(spacer, r#"anchor_policy: "flip","#, 40.0));
    draw(&mut o);
    let entry = only_entry(&o);
    assert_eq!(
        entry.viewport_rect.y,
        508.0 - 100.0,
        "bottom edge on the face's top"
    );
    assert_eq!(entry.viewport_rect.x, 20.0);
}

/// The default policy is `clamp`, so the same popover is pulled back
/// inside the bottom inset rather than flipped.
#[test]
fn clamp_is_the_default_for_a_parent_anchor() {
    let spacer = r#"Flex { style: { width: "grow", height: "grow" } },"#;
    let (mut o, _log) = mounted(&document(spacer, "", 40.0));
    draw(&mut o);
    let entry = only_entry(&o);
    assert_eq!(
        entry.viewport_rect.y,
        H - 100.0 - 8.0,
        "clamped to the bottom inset"
    );
}

/// `focus_trap` is legal with a parent anchor: the frame is the walk's
/// own and cannot go missing, which was the reason it is refused with a
/// host anchor.
#[test]
fn a_parent_anchor_may_trap_focus() {
    let (mut o, _log) = mounted(&document("", "focus_trap: true,", 200.0));
    draw(&mut o);
    assert!(only_entry(&o).focus_trap);
}
