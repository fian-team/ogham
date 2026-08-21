//! `Ogham::frame` — the single-call frame drive — and the
//! state-preserving hot reload it depends on. A reload rebuilds the
//! runtime from the config's *initial* host state; `frame`'s contract
//! is that hosts push state only when it changes, so the reload must
//! carry the live host-state map forward or the first reloaded frame
//! would render placeholders.

use std::collections::HashMap;

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::widget::text_widget::TextWidget;
use ogham::Ogham;

/// A strict-mode module whose whole tree is one host-state-driven Text,
/// so the root widget downcasts straight to `TextWidget` for assertions.
const LABELED_TREE: &str = r##"
host_state { label: string };
let main = fn () {
  Text { text: label, style: { size: 16 } }
};
"##;

fn build_with(label: &str) -> Ogham {
    let config = RuntimeConfig::new().with_host_state(HashMap::from([(
        "label".to_string(),
        Value::String(label.to_string()),
    )]));
    Ogham::from_source(LABELED_TREE, config).expect("Ogham::from_source")
}

fn root_text(ogham: &Ogham) -> String {
    let root = ogham.get_ui().root.clone();
    let g = root.lock().expect("widget lock poisoned");
    g.downcast_ref::<TextWidget>()
        .expect("root widget is Text")
        .text
        .clone()
}

#[test]
fn reload_carries_live_host_state_into_the_new_runtime() {
    let mut ogham = build_with("placeholder");
    assert_eq!(root_text(&ogham), "placeholder");

    ogham.with_runtime_mut(|rt| rt.set_host_state("label", "live"));
    ogham.update().expect("update");
    assert_eq!(root_text(&ogham), "live");

    ogham
        .recompile_from_source(LABELED_TREE)
        .expect("recompile");

    // The reloaded tree shows live state before any further update() —
    // the reload's own module execution already saw the carried map.
    assert_eq!(root_text(&ogham), "live");
    assert_eq!(
        ogham.with_runtime_mut(|rt| rt.get_host_state("label")),
        Some(Value::String("live".to_string()))
    );
}

#[test]
fn frame_reexecutes_only_when_host_state_changed() {
    let mut ogham = build_with("first");

    // Settle whatever construction left pending.
    ogham.frame(800.0, 600.0, 1.0 / 60.0).expect("frame");

    // Idle frame: nothing changed, no re-execute, and no watched file
    // means no frame can ever report a reload.
    let report = ogham.frame(800.0, 600.0, 1.0 / 60.0).expect("frame");
    assert!(!report.rerendered);
    assert!(!report.reloaded);

    // A changed value re-executes and the tree reflects it...
    ogham.with_runtime_mut(|rt| rt.set_host_state("label", "second"));
    assert!(
        ogham
            .frame(800.0, 600.0, 1.0 / 60.0)
            .expect("frame")
            .rerendered
    );
    assert_eq!(root_text(&ogham), "second");

    // ...an unchanged re-inject does not.
    ogham.with_runtime_mut(|rt| rt.set_host_state("label", "second"));
    assert!(
        !ogham
            .frame(800.0, 600.0, 1.0 / 60.0)
            .expect("frame")
            .rerendered
    );
}
