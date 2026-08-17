//! A screen mounted by a route change plays its entry animation.
//!
//! The symptom this exists for: menus that "clip into existence" — the
//! incoming screen appearing at its final position and opacity with no
//! spring, while the *outgoing* one still faded out correctly. An
//! asymmetry like that reads as a `Presence` bug and is not one; it means
//! the incoming widget was **reused** rather than mounted, so the entry
//! transition armed at build time was thrown away.
//!
//! The load-bearing detail is that two different screens can carry the
//! *same* `key`. Every consumer has a shared panel helper, so its title,
//! its connect screen and its settings all render a `Flex { key:
//! "document" }`. If anything reconciles those two by key instead of
//! replacing the generation wholesale, the second screen inherits the
//! first one's widget — same key, same position — and never animates.

use ogham::runtime::config::RuntimeConfig;
use ogham::widget::flex_widget::FlexWidget;
use ogham::widget::presence_widget::PresenceWidget;
use ogham::Ogham;

/// Two screens whose documents share a key, which is what a shared panel
/// helper produces in every real consumer.
const SHARED_KEY: &str = r#"
screen "title" {
  view Flex {
    key: "document",
    initial: { opacity: 0 },
    exit: { opacity: 0 },
    style: { width: "grow", height: "grow", transition: { opacity: "spring" } },
    children: [ Text { text: "title" } ],
  }
};

screen "connect" {
  view Flex {
    key: "document",
    initial: { opacity: 0 },
    exit: { opacity: 0 },
    style: { width: "grow", height: "grow", transition: { opacity: "spring" } },
    children: [ Text { text: "connect" } ],
  }
};

let main = fn () { outlet() };
"#;

fn build(source: &str) -> Ogham {
    Ogham::from_source(source, RuntimeConfig::default()).expect("from_source")
}

/// The opacity the live document is currently rendering at.
///
/// `initial: { opacity: 0 }` means a freshly mounted widget starts at 0
/// and springs to 1. A reused widget is already at 1 and never moves.
fn document_opacity(ui: &mut ogham::widget::UI) -> f32 {
    let root = ui.root.clone();
    let guard = root.lock().expect("root lock");
    let presence = guard
        .downcast_ref::<PresenceWidget>()
        .expect("the outlet renders a Presence");
    let child = presence
        .inner
        .children
        .first()
        .expect("one live screen")
        .clone();
    drop(guard);
    let guard = child.lock().expect("child lock");
    let flex = guard
        .downcast_ref::<FlexWidget>()
        .expect("the screen is a Flex");
    flex.style.opacity.value()
}

fn frame(ogham: &mut Ogham) {
    ogham.frame(1280.0, 720.0, 1.0 / 60.0).expect("frame");
}

#[test]
fn the_first_screen_mounts_at_its_initial_style() {
    let mut ogham = build(SHARED_KEY);
    ogham.with_runtime_mut(|rt| rt.set_route_path(&["title"]));
    frame(&mut ogham);
    // Not exactly zero: one frame of spring has already run. What matters
    // is that it is nowhere near the declared 1.0.
    let opacity = document_opacity(ogham.get_ui_mut());
    assert!(
        opacity < 0.5,
        "a freshly mounted screen starts at its `initial` and springs up, got {opacity}"
    );
}

#[test]
fn a_route_change_remounts_rather_than_reusing_the_shared_key() {
    let mut ogham = build(SHARED_KEY);
    ogham.with_runtime_mut(|rt| rt.set_route_path(&["title"]));
    frame(&mut ogham);

    // Let the entrance settle, so a screen that failed to remount would
    // be sitting at full opacity and the assertion below would catch it.
    for _ in 0..240 {
        frame(&mut ogham);
    }
    assert!(
        document_opacity(ogham.get_ui_mut()) > 0.99,
        "the first screen finished its entrance"
    );

    ogham.with_runtime_mut(|rt| rt.set_route_path(&["connect"]));
    frame(&mut ogham);
    let opacity = document_opacity(ogham.get_ui_mut());
    assert!(
        opacity < 0.5,
        "the incoming screen mounts fresh and plays its entrance, even \
         though it carries the same `key` as the one it replaced; got {opacity}"
    );
}
