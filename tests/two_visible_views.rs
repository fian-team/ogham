//! WP-3.3: two visible views stack (`lorekeeper/docs/ROUTING.md` §13.5).
//!
//! The router's `visible_views` can return more than one id — an
//! [`Occlusion::None`] prompt over the workspace it is leaving — and the
//! outlet renders them as consecutive children of one `Presence`.
//! Consecutive children *flow*, so two of them sat side by side: two
//! half-width screens rather than a card over a workspace. The path shape
//! here is untold_lore's `PromptRoute` (`ul-client/src/routes.rs`), which
//! is the first consumer that needs it.
//!
//! Two things had to be true, and only one of them was obvious:
//!
//! - The two views occupy the **same box**, the deeper one over the
//!   shallower.
//! - The deeper one gets the **press**. A flow never has to decide that,
//!   because its children do not overlap, so the pointer walk was
//!   front-to-back — which with stacked children hands the click to the
//!   thing underneath. A prompt that drew on top and let the workspace
//!   take the click would be worse than the side-by-side bug, because it
//!   would look right.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::widget::event::Event;
use ogham::widget::point::Point;
use ogham::widget::presence_widget::PresenceWidget;
use ogham::widget::rect::Rect;
use ogham::Ogham;

const WIDTH: f32 = 800.0;
const HEIGHT: f32 = 600.0;

/// A document with the two screens untold_lore's editor has: a workspace,
/// and a prompt that sits over it rather than replacing it.
const WORKSPACE_AND_PROMPT: &str = r#"
screen "map-edit" {
  view Flex {
    key: "workspace",
    style: { width: "grow", height: "grow" },
    mouse_down: fn () { event("workspace_press"); },
  }
};

screen "exit-prompt" {
  view Flex {
    key: "prompt",
    style: { width: "grow", height: "grow" },
    mouse_down: fn () { event("prompt_press"); },
  }
};

let main = fn () { outlet() };
"#;

fn mounted(path: &[&str], presses: &Arc<AtomicUsize>, which: &'static str) -> Ogham {
    let counted = presses.clone();
    let config = RuntimeConfig::new()
        .with_event_handler("workspace_press", {
            let counted = counted.clone();
            move |_| {
                if which == "workspace" {
                    counted.fetch_add(1, Ordering::SeqCst);
                }
                Ok(Value::Boolean(true))
            }
        })
        .with_event_handler("prompt_press", move |_| {
            if which == "prompt" {
                counted.fetch_add(1, Ordering::SeqCst);
            }
            Ok(Value::Boolean(true))
        });
    let mut ogham = Ogham::from_source(WORKSPACE_AND_PROMPT, config).expect("the document mounts");
    ogham.with_runtime_mut(|rt| rt.set_route_path(path));
    ogham
        .frame(WIDTH, HEIGHT, 1.0 / 60.0)
        .expect("a frame with the path set");
    ogham
}

/// Every screen's layout rect, in the order the outlet rendered them.
fn views(ogham: &mut Ogham) -> Vec<Rect> {
    let root = ogham.get_ui_mut().root.clone();
    let guard = root.lock().expect("root lock");
    let presence = guard
        .downcast_ref::<PresenceWidget>()
        .expect("the outlet's root is a Presence");
    presence
        .inner
        .children
        .iter()
        .map(|child| {
            child
                .lock()
                .expect("child lock")
                .get_layout_rect()
                .expect("a laid-out screen")
                .clone()
        })
        .collect()
}

/// The gap the whole package exists to close: a path with two visible
/// views lays them **over** each other, not beside.
///
/// Both are `width: "grow"` under a column Presence, so before stacking
/// the second one sat at `y = 300` with half the height — the exit prompt
/// drawn in the bottom half of the window, with the workspace squeezed
/// into the top.
#[test]
fn two_visible_views_occupy_the_same_box() {
    let presses = Arc::new(AtomicUsize::new(0));
    let mut ogham = mounted(&["map-edit", "exit-prompt"], &presses, "prompt");
    let views = views(&mut ogham);

    assert_eq!(views.len(), 2, "the outlet rendered both ids");
    assert_eq!(views[0].x, views[1].x, "same origin");
    assert_eq!(views[0].y, views[1].y, "same origin");
    assert_eq!(views[0].width, views[1].width, "same box");
    assert_eq!(views[0].height, views[1].height, "same box");
    assert_eq!(views[0].width, WIDTH);
    assert_eq!(views[0].height, HEIGHT);
}

/// The press belongs to the view on top.
///
/// The two overlap exactly, so nothing about the point can distinguish
/// them; the only answer is the order, and the order is the path — the
/// deeper node is rendered last, so it paints over and is asked first.
#[test]
fn the_deeper_view_takes_the_press() {
    let presses = Arc::new(AtomicUsize::new(0));
    let mut ogham = mounted(&["map-edit", "exit-prompt"], &presses, "prompt");
    let handled = ogham.get_ui_mut().call_event(&Event::with_point(
        "mouse_down".to_string(),
        Point::new(WIDTH / 2.0, HEIGHT / 2.0),
    ));
    assert!(handled);
    assert_eq!(
        presses.load(Ordering::SeqCst),
        1,
        "the prompt is over the workspace, so the click is the prompt's"
    );

    // And the workspace, which is underneath, gets nothing.
    let workspace = Arc::new(AtomicUsize::new(0));
    let mut ogham = mounted(&["map-edit", "exit-prompt"], &workspace, "workspace");
    ogham.get_ui_mut().call_event(&Event::with_point(
        "mouse_down".to_string(),
        Point::new(WIDTH / 2.0, HEIGHT / 2.0),
    ));
    assert_eq!(workspace.load(Ordering::SeqCst), 0);
}

/// A one-view path is unchanged, which is the reason stacking could land
/// at all: every migrated consumer has exactly one visible view, and one
/// child on the whole content box is where a flow's first child already
/// was.
#[test]
fn one_visible_view_lays_out_exactly_as_it_did() {
    let presses = Arc::new(AtomicUsize::new(0));
    let mut ogham = mounted(&["map-edit"], &presses, "workspace");
    let views = views(&mut ogham);

    assert_eq!(views.len(), 1);
    let one = &views[0];
    assert_eq!(
        (one.x, one.y, one.width, one.height),
        (0.0, 0.0, WIDTH, HEIGHT)
    );
}

/// A `Presence` an author wrote still flows, because two things in one
/// generation are usually two things side by side. Stacking is the
/// outlet's declaration, not a change to what `Presence` means.
#[test]
fn an_ordinary_presence_still_flows_its_children() {
    let mut ogham = Ogham::from_source(
        r#"
        let main = fn () {
          Presence {
            key: "a",
            children: [
              Flex { key: "top", style: { width: "grow", height: 100 } },
              Flex { key: "bottom", style: { width: "grow", height: 100 } },
            ],
          }
        };
        "#,
        RuntimeConfig::new(),
    )
    .expect("the document mounts");
    ogham.frame(WIDTH, HEIGHT, 1.0 / 60.0).expect("a frame");

    let laid = views(&mut ogham);
    assert_eq!(laid.len(), 2);
    assert_eq!(laid[0].y, 0.0);
    assert_eq!(laid[1].y, 100.0, "flowed, not stacked");
}

/// And `stack: true` is what changes it, for an author who wants the
/// outlet's shape by hand.
#[test]
fn a_presence_that_asks_to_stack_stacks() {
    let mut ogham = Ogham::from_source(
        r#"
        let main = fn () {
          Presence {
            key: "a",
            stack: true,
            children: [
              Flex { key: "under", style: { width: "grow", height: 100 } },
              Flex { key: "over", style: { width: "grow", height: 100 } },
            ],
          }
        };
        "#,
        RuntimeConfig::new(),
    )
    .expect("the document mounts");
    ogham.frame(WIDTH, HEIGHT, 1.0 / 60.0).expect("a frame");

    let laid = views(&mut ogham);
    assert_eq!(laid.len(), 2);
    assert_eq!(laid[0].y, 0.0);
    assert_eq!(laid[1].y, 0.0, "stacked");
}
