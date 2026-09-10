//! `Slider` and the pointer capture it rides on.
//!
//! A press at a fraction of the track reports that fraction of the
//! range; the moves that follow keep reporting wherever the cursor goes,
//! because the press took capture; the release commits. The value is the
//! document's: what it re-supplies on the next render is what the thumb
//! shows.

use std::sync::{Arc, Mutex};

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::widget::event::{Event, KeyModifiers};
use ogham::widget::point::Point;
use ogham::widget::slider_widget::SliderWidget;
use ogham::widget::Widget;
use ogham::Ogham;

const W: f32 = 800.0;
const H: f32 = 600.0;
const DT: f32 = 1.0 / 60.0;

type Log = Arc<Mutex<Vec<(String, f64)>>>;

/// A 200 × 20 slider at the origin, `min..max` with `extra` spliced in.
fn document(min: f32, max: f32, extra: &str) -> String {
    format!(
        r#"
let main = fn () {{
  Flex {{
    style: {{ width: "grow", height: "grow" }},
    children: [
      Slider {{
        value: 0,
        min: {min},
        max: {max},
        {extra}
        on_change: fn (v: float) {{ event("changed", v); }},
        on_commit: fn (v: float) {{ event("committed", v); }},
        style: {{ width: 200, height: 20 }},
      }}
    ],
  }}
}};
"#
    )
}

fn mounted(src: &str) -> (Ogham, Log) {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let mut config = RuntimeConfig::new();
    for name in ["changed", "committed"] {
        let log = log.clone();
        config = config.with_event_handler(name, move |args| {
            let value = match args.first() {
                Some(Value::Float(f)) => *f,
                other => panic!("a slider reports a float, got {other:?}"),
            };
            log.lock().unwrap().push((name.to_string(), value));
            Ok(Value::Void)
        });
    }
    let mut o = Ogham::from_source(src, config).expect("the document mounts");
    o.frame(W, H, DT).expect("frame");
    (o, log)
}

fn send(o: &mut Ogham, name: &str, x: f32, y: f32) -> bool {
    o.get_ui_mut()
        .call_event(&Event::with_point(name.to_string(), Point::new(x, y)))
}

fn slider_value(o: &Ogham) -> f32 {
    let child = o.get_ui().root.lock().unwrap().get_children()[0].clone();
    let g = child.lock().unwrap();
    g.downcast_ref::<SliderWidget>().expect("a Slider").value
}

#[test]
fn a_press_at_a_quarter_reports_a_quarter_of_the_range() {
    let (mut o, log) = mounted(&document(0.0, 100.0, ""));
    assert!(send(&mut o, "mouse_down", 50.0, 10.0));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [("changed".to_string(), 25.0)]
    );
    assert!(o.get_ui().captured().is_some(), "the press took capture");
}

#[test]
fn moving_while_held_keeps_reporting_and_the_release_commits() {
    let (mut o, log) = mounted(&document(0.0, 100.0, ""));
    send(&mut o, "mouse_down", 50.0, 10.0);
    // Well outside the slider's box, vertically and horizontally.
    send(&mut o, "mouse_move", 150.0, 300.0);
    send(&mut o, "mouse_move", 150.0, 300.0);
    send(&mut o, "mouse_up", 150.0, 300.0);
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            ("changed".to_string(), 25.0),
            ("changed".to_string(), 75.0),
            ("committed".to_string(), 75.0),
        ],
        "one change per change, one commit on the release"
    );
    assert!(
        o.get_ui().captured().is_none(),
        "the release ended the capture"
    );
}

#[test]
fn past_the_ends_the_value_clamps() {
    let (mut o, log) = mounted(&document(0.0, 100.0, ""));
    send(&mut o, "mouse_down", 50.0, 10.0);
    send(&mut o, "mouse_move", 900.0, 10.0);
    send(&mut o, "mouse_move", -50.0, 10.0);
    let log = log.lock().unwrap();
    assert_eq!(log[1], ("changed".to_string(), 100.0));
    assert_eq!(log[2], ("changed".to_string(), 0.0));
}

#[test]
fn a_step_snaps_what_is_reported() {
    let (mut o, log) = mounted(&document(0.0, 100.0, "step: 25,"));
    // 33% → 25; 45% → 50.
    send(&mut o, "mouse_down", 66.0, 10.0);
    send(&mut o, "mouse_move", 90.0, 10.0);
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [("changed".to_string(), 25.0), ("changed".to_string(), 50.0)]
    );
}

/// The document supplies the value. Here it never writes the reported
/// value back, so the next render snaps the thumb to what it did supply.
#[test]
fn the_value_is_the_documents() {
    let (mut o, _log) = mounted(&document(0.0, 100.0, ""));
    send(&mut o, "mouse_down", 100.0, 10.0);
    assert_eq!(slider_value(&o), 50.0, "reported and shown while held");
    o.with_runtime_mut(|rt| rt.request_rerender());
    o.frame(W, H, DT).expect("frame");
    assert_eq!(slider_value(&o), 0.0, "the document said 0");
}

#[test]
fn arrow_keys_step_and_commit_while_focused() {
    let (mut o, log) = mounted(&document(0.0, 100.0, "step: 10,"));
    send(&mut o, "mouse_down", 100.0, 10.0);
    send(&mut o, "mouse_up", 100.0, 10.0);
    log.lock().unwrap().clear();
    o.get_ui_mut()
        .call_event(&Event::keydown(39, None, KeyModifiers::default()));
    o.get_ui_mut()
        .call_event(&Event::keydown(37, None, KeyModifiers::default()));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            ("changed".to_string(), 60.0),
            ("committed".to_string(), 60.0),
            ("changed".to_string(), 50.0),
            ("committed".to_string(), 50.0),
        ]
    );
}

#[test]
fn a_missing_value_or_a_bad_step_is_refused() {
    let src = r#"let main = fn () { Slider { min: 0, max: 1 } };"#;
    let err = format!(
        "{:?}",
        Ogham::from_source(src, RuntimeConfig::new())
            .err()
            .expect("value is required")
    );
    assert!(err.contains("value"), "{err}");

    let src = document(0.0, 1.0, "step: 0,");
    let err = format!(
        "{:?}",
        Ogham::from_source(&src, RuntimeConfig::new())
            .err()
            .expect("step must be positive")
    );
    assert!(err.contains("step"), "{err}");
}
