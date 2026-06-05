//! A host orchestrator that calls `begin_exit_root()` to animate an Ogham out
//! and later re-shows it MUST normally pair that with `restart_entry_animations`
//! (or `cancel_exit_root`). Forgetting that pairing used to strand the tree as a
//! blank exit-ghost forever — a sharp foot-gun for multi-Ogham hosts.
//!
//! `update()` now self-heals: when a reconcile is pending (the host is actively
//! pushing this UI's live content) and a prior root-exit has *fully completed*,
//! the stale exit is cleared so the live tree re-mounts. These tests lock that
//! in, and confirm it never disturbs an exit that is still in flight.

use ogham::widget::flex_widget::FlexWidget;
use ogham::Ogham;

fn build(source: &str) -> Ogham {
    Ogham::from_source(source, ogham::runtime::config::RuntimeConfig::default())
        .expect("Ogham::from_source")
}

fn root_child_count(ogham: &mut Ogham) -> usize {
    let root = ogham.get_ui_mut().root.clone();
    let g = root.lock().expect("root lock");
    g.downcast_ref::<FlexWidget>()
        .expect("root should be a FlexWidget")
        .children
        .len()
}

/// A passive-ghost root (no own exit) whose only child animates out. After the
/// child's exit settles it is dropped, leaving the root a stranded ghost. A
/// subsequent reconcile (host re-showing the UI) must clear the stale exit and
/// re-mount the child — without any `restart_entry_animations` call.
#[test]
fn stranded_root_exit_is_healed_on_next_reconcile() {
    let src = r#"
        let main = fn () {
          Flex {
            style: { width: "grow", height: "grow", direction: "column" },
            children: [
              Flex {
                exit: { opacity: 0 },
                style: {
                  width: 50, height: 50,
                  transition: { opacity: { stiffness: 680, damping: 52 } },
                },
                children: [],
              },
            ],
          }
        };
    "#;
    let mut ogham = build(src);
    ogham.set_screen_size(800.0, 600.0);
    let _ = ogham.tick(|_| {}).expect("initial mount");
    ogham.get_ui_mut().layout(800.0, 600.0);
    assert_eq!(root_child_count(&mut ogham), 1, "child mounts initially");

    // Host orchestrator begins the exit (child has an exit animation → true).
    assert!(ogham.begin_exit_root(), "root should begin exiting");

    // Settle the exit; the drained child is dropped from the tree.
    for _ in 0..600 {
        ogham.get_ui_mut().tick_animations(1.0 / 60.0);
        ogham.process_drain_queues();
        if ogham.is_exit_complete_root() {
            break;
        }
    }
    assert!(ogham.is_exit_complete_root(), "exit should complete");
    assert_eq!(root_child_count(&mut ogham), 0, "drained child is dropped");

    // Host re-shows the UI by pushing live content — but FORGETS
    // restart_entry_animations. A pending reconcile must self-heal.
    let _ = ogham.tick(|rt| rt.request_rerender()).expect("reconcile");

    assert!(
        !ogham.is_exit_complete_root(),
        "stale exit must be cleared on reconcile"
    );
    assert_eq!(
        root_child_count(&mut ogham),
        1,
        "live tree must re-mount, not stay a blank ghost"
    );
}

/// The self-heal must NOT fire while an exit is still animating: a reconcile
/// mid-exit (e.g. an unrelated host_state push) leaves the in-flight exit alone.
#[test]
fn in_flight_root_exit_is_not_disturbed_by_reconcile() {
    let src = r#"
        let main = fn () {
          Flex {
            exit: { opacity: 0 },
            style: {
              width: "grow", height: "grow",
              transition: { opacity: { stiffness: 40, damping: 26 } },
            },
            children: [ Flex { style: { width: 50, height: 50 }, children: [] } ],
          }
        };
    "#;
    let mut ogham = build(src);
    ogham.set_screen_size(800.0, 600.0);
    let _ = ogham.tick(|_| {}).expect("initial mount");
    ogham.get_ui_mut().layout(800.0, 600.0);

    assert!(ogham.begin_exit_root(), "root should begin exiting");
    // One small step — nowhere near settled (soft spring).
    ogham.get_ui_mut().tick_animations(1.0 / 240.0);
    assert!(
        !ogham.is_exit_complete_root(),
        "exit is still in flight after one tick"
    );

    // A reconcile mid-exit must not cancel the in-flight exit.
    let _ = ogham.tick(|rt| rt.request_rerender()).expect("reconcile");
    assert!(
        !ogham.is_exit_complete_root(),
        "an in-flight exit is left untouched"
    );
    let root = ogham.get_ui_mut().root.clone();
    assert!(
        root.lock().expect("root lock").is_exiting(),
        "root should still be exiting mid-flight"
    );
}
