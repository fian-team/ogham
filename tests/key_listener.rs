//! `keydown: { ctrl_k: fn () {…}, escape: fn () {…} }` on `Flex`.
//!
//! A chord with ctrl / alt / meta is a command and goes to the listeners
//! first, innermost on the focus chain winning; a bare key goes to the
//! tree first, and to the listeners only if the tree declined it and no
//! focused field would type it. The chain is the focused widget's
//! ancestors; an unfocused document dispatches from the root.

use std::sync::{Arc, Mutex};

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::widget::event::{Event, KeyModifiers};
use ogham::widget::point::Point;
use ogham::Ogham;

const W: f32 = 800.0;
const H: f32 = 600.0;
const DT: f32 = 1.0 / 60.0;

type Log = Arc<Mutex<Vec<String>>>;

/// A root with three chords, a nested panel with its own `ctrl+k` and a
/// field inside it, and a second panel with a field and no listener.
const DOCUMENT: &str = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow", direction: "column" },
    keydown: {
      ctrl_k: fn () { event("root_ctrl_k"); },
      k: fn () { event("root_k"); },
      escape: fn () { event("root_escape"); },
      shift_arrowdown: fn () { event("root_shift_down"); },
    },
    children: [
      Flex {
        style: { width: "grow", height: 100 },
        keydown: { ctrl_k: fn () { event("nested_ctrl_k"); } },
        children: [
          TextInput {
            value: "",
            style: { width: 200, height: 30 },
            on_change: fn (v: string) { event("typed", v); },
          }
        ],
      },
      Flex {
        style: { width: "grow", height: 100 },
        children: [ TextInput { value: "", style: { width: 200, height: 30 } } ],
      },
    ],
  }
};
"#;

fn mounted(src: &str) -> (Ogham, Log) {
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let mut config = RuntimeConfig::new();
    for name in [
        "root_ctrl_k",
        "root_k",
        "root_escape",
        "root_shift_down",
        "nested_ctrl_k",
        "typed",
    ] {
        let log = log.clone();
        config = config.with_event_handler(name, move |args| {
            let mut entry = name.to_string();
            if let Some(Value::String(s)) = args.first() {
                entry.push(':');
                entry.push_str(s);
            }
            log.lock().unwrap().push(entry);
            Ok(Value::Void)
        });
    }
    let mut o = Ogham::from_source(src, config).expect("the document mounts");
    o.frame(W, H, DT).expect("frame");
    (o, log)
}

fn focus(o: &mut Ogham, x: f32, y: f32) {
    o.get_ui_mut().call_event(&Event::with_point(
        "mouse_down".to_string(),
        Point::new(x, y),
    ));
    o.get_ui_mut()
        .call_event(&Event::with_point("mouse_up".to_string(), Point::new(x, y)));
    assert!(o.get_ui().get_focused().is_some());
}

fn ctrl() -> KeyModifiers {
    KeyModifiers {
        ctrl: true,
        ..KeyModifiers::default()
    }
}

fn key(o: &mut Ogham, code: u32, modifiers: KeyModifiers) -> bool {
    o.get_ui_mut()
        .call_event(&Event::keydown(code, None, modifiers))
}

#[test]
fn a_chord_fires_the_root_listener_while_a_field_in_a_plain_panel_is_focused() {
    let (mut o, log) = mounted(DOCUMENT);
    focus(&mut o, 10.0, 110.0);
    assert!(key(&mut o, 'k' as u32, ctrl()), "the chord is consumed");
    assert_eq!(log.lock().unwrap().as_slice(), ["root_ctrl_k"]);
}

#[test]
fn the_innermost_listener_on_the_focus_chain_wins() {
    let (mut o, log) = mounted(DOCUMENT);
    focus(&mut o, 10.0, 10.0);
    assert!(key(&mut o, 'k' as u32, ctrl()));
    assert_eq!(log.lock().unwrap().as_slice(), ["nested_ctrl_k"]);
}

#[test]
fn a_plain_key_goes_to_the_focused_field_and_not_to_the_listener() {
    let (mut o, log) = mounted(DOCUMENT);
    focus(&mut o, 10.0, 10.0);
    // The keydown carries no character; the field types on the keypress
    // that follows, which is why the keydown is the field's regardless.
    assert!(!key(&mut o, 'k' as u32, KeyModifiers::default()));
    assert!(
        log.lock().unwrap().is_empty(),
        "no listener for a key the field will type"
    );
    o.get_ui_mut().call_event(&Event::keypress(
        'k' as u32,
        Some('k'),
        KeyModifiers::default(),
    ));
    assert_eq!(log.lock().unwrap().as_slice(), ["typed:k"]);
}

#[test]
fn an_unfocused_document_dispatches_from_the_root() {
    let (mut o, log) = mounted(DOCUMENT);
    assert!(o.get_ui().get_focused().is_none());
    assert!(key(&mut o, 'k' as u32, KeyModifiers::default()));
    assert!(key(&mut o, 27, KeyModifiers::default()));
    // A host that shifts before reporting sends the capital; the chord
    // is the same. Shift on a named key is a modifier like any other.
    assert!(key(
        &mut o,
        40,
        KeyModifiers {
            shift: true,
            ..KeyModifiers::default()
        }
    ));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["root_k", "root_escape", "root_shift_down"]
    );
    // The nested listener is not on the chain of an unfocused document.
    log.lock().unwrap().clear();
    assert!(key(&mut o, 'k' as u32, ctrl()));
    assert_eq!(log.lock().unwrap().as_slice(), ["root_ctrl_k"]);
}

#[test]
fn escape_blurs_a_focused_field_before_any_listener_sees_it() {
    let (mut o, log) = mounted(DOCUMENT);
    focus(&mut o, 10.0, 10.0);
    assert!(key(&mut o, 27, KeyModifiers::default()));
    assert!(
        log.lock().unwrap().is_empty(),
        "the UI-level blur consumed it"
    );
    assert!(o.get_ui().get_focused().is_none());
    assert!(key(&mut o, 27, KeyModifiers::default()));
    assert_eq!(log.lock().unwrap().as_slice(), ["root_escape"]);
}

#[test]
fn a_key_no_listener_names_is_declined() {
    let (mut o, log) = mounted(DOCUMENT);
    assert!(!key(&mut o, 'j' as u32, ctrl()));
    assert!(
        !key(&mut o, 0, KeyModifiers::default()),
        "an unmapped key has no chord"
    );
    assert!(log.lock().unwrap().is_empty());
}

#[test]
fn a_chord_that_names_no_key_is_a_build_error() {
    // The tree is built on mount, so the bridge error is `from_source`'s.
    let src = r#"let main = fn () { Flex { keydown: { hyper_k: fn () { 1 } } } };"#;
    let err = format!(
        "{:?}",
        Ogham::from_source(src, RuntimeConfig::new())
            .err()
            .expect("refused")
    );
    assert!(err.contains("keydown"), "{err}");
    assert!(err.contains("hyper"), "{err}");

    let src = r#"let main = fn () { Flex { keydown: fn () { 1 } } };"#;
    let err = format!(
        "{:?}",
        Ogham::from_source(src, RuntimeConfig::new())
            .err()
            .expect("a bare closure is refused")
    );
    assert!(err.contains("map of chord"), "{err}");
}
