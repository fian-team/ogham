//! Host-side lifecycle API: `Ogham::{begin_exit_root,
//! cancel_exit_root, is_exit_complete_root,
//! restart_entry_animations, cancel_active_drag}`.
//!
//! These are the primitives that a host-side Presence-orchestrator
//! uses to sequence transitions between independent Ogham instances
//! (route swaps, modal stacks). Each test drives one slice of the
//! state machine end-to-end using a real Ogham built from source.

use ogham::widget::event::TickContext;
use ogham::widget::flex_widget::FlexWidget;
use ogham::widget::style::{Color, Size};
use ogham::widget::Widget;
use ogham::Ogham;

/// Tree with an exit_style + initial_style on a single Flex so the
/// host APIs have something to actually animate. Only opacity is
/// transitioned — enough to verify the lifecycle without dragging in
/// color-literal parsing concerns.
const ANIMATED_TREE: &str = r##"
    let main = fn () {
      Flex {
        initial: { opacity: 0 },
        exit: { opacity: 0 },
        style: {
          width: "grow",
          height: "grow",
          opacity: 1,
          transition: { opacity: "spring" },
        },
        children: [],
      }
    };
"##;

/// Tree with no exits anywhere — `begin_exit_root` should return false
/// and the host can drop this Ogham immediately.
const STATIC_TREE: &str = r##"
    let main = fn () {
      Flex {
        style: { width: "grow", height: "grow" },
        children: [],
      }
    };
"##;

fn build(src: &str) -> Ogham {
    Ogham::from_source(src, ogham::runtime::config::RuntimeConfig::default())
        .expect("Ogham::from_source")
}

fn tick(ogham: &mut Ogham, dt: f32) {
    ogham.get_ui_mut().tick_animations(dt);
}

#[test]
fn begin_exit_root_returns_true_when_tree_has_exit_animation() {
    let mut ogham = build(ANIMATED_TREE);
    // Settle from the entry animation first so begin_exit has a stable
    // starting point.
    for _ in 0..240 {
        tick(&mut ogham, 1.0 / 60.0);
    }
    assert!(
        ogham.begin_exit_root(),
        "tree with exit_style should accept begin_exit"
    );
    // Mirrored on the widget itself.
    let g = ogham.get_ui().root.lock().expect("root lock");
    let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget root");
    assert!(f.is_exiting());
}

#[test]
fn begin_exit_root_returns_false_when_tree_has_no_exits() {
    let mut ogham = build(STATIC_TREE);
    assert!(
        !ogham.begin_exit_root(),
        "tree with no exits anywhere should not start an exit"
    );
}

#[test]
fn is_exit_complete_root_tracks_in_flight_then_settled() {
    let mut ogham = build(ANIMATED_TREE);
    for _ in 0..240 {
        tick(&mut ogham, 1.0 / 60.0);
    }
    assert!(!ogham.is_exit_complete_root(), "no exit started yet");
    ogham.begin_exit_root();
    assert!(!ogham.is_exit_complete_root(), "exit just started; not settled");

    // Drive the exit spring all the way out.
    let mut settled = false;
    for _ in 0..480 {
        let mut ctx = TickContext::new(1.0 / 60.0);
        let mut g = ogham.get_ui_mut().root.lock().expect("root lock");
        g.tick_animations(&mut ctx);
        drop(g);
        if ogham.is_exit_complete_root() {
            settled = true;
            break;
        }
    }
    assert!(settled, "exit should settle within a reasonable tick budget");
}

#[test]
fn cancel_exit_root_unwinds_in_flight_exit() {
    let mut ogham = build(ANIMATED_TREE);
    for _ in 0..240 {
        tick(&mut ogham, 1.0 / 60.0);
    }
    ogham.begin_exit_root();
    {
        let g = ogham.get_ui().root.lock().expect("root lock");
        let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
        assert!(f.is_exiting());
    }

    ogham.cancel_exit_root();

    let g = ogham.get_ui().root.lock().expect("root lock");
    let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
    assert!(
        !f.is_exiting(),
        "cancel must drop the exiting flag at the root"
    );
}

#[test]
fn restart_entry_animations_reseeds_style_after_settled() {
    // Tree settles at declared opacity 1; restart_entry_animations
    // should snap style back to initial (opacity 0) and arm springs.
    let mut ogham = build(ANIMATED_TREE);
    for _ in 0..240 {
        tick(&mut ogham, 1.0 / 60.0);
    }
    {
        let g = ogham.get_ui().root.lock().expect("root lock");
        let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
        // Settled near declared.
        assert!(f.style.opacity.0 > 0.99);
    }

    ogham.restart_entry_animations();

    let g = ogham.get_ui().root.lock().expect("root lock");
    let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
    assert!(
        f.style.opacity.0 < 0.01,
        "restart should reseed opacity from initial (0); got {}",
        f.style.opacity.0
    );
    assert!(
        f.animations.opacity.is_some(),
        "restart should arm the opacity spring toward declared"
    );
}

#[test]
fn restart_entry_animations_clears_exit_state() {
    // Orchestrator may call restart on a tree that was mid-exit (rapid
    // revert during a transition). The widget must not stay marked
    // exiting — otherwise reconcile would treat it as a ghost.
    let mut ogham = build(ANIMATED_TREE);
    for _ in 0..240 {
        tick(&mut ogham, 1.0 / 60.0);
    }
    ogham.begin_exit_root();
    ogham.restart_entry_animations();

    let g = ogham.get_ui().root.lock().expect("root lock");
    let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
    assert!(!f.is_exiting(), "restart must clear the exiting flag");
}

#[test]
fn cancel_active_drag_is_idempotent_when_no_drag() {
    let mut ogham = build(ANIMATED_TREE);
    // No drag in flight; cancel should be a no-op (no panic).
    ogham.cancel_active_drag();
    ogham.cancel_active_drag();
    assert!(ogham.get_ui().active_drag_preview().is_none());
}

#[test]
fn full_route_swap_cycle_exits_then_restarts_entry() {
    // End-to-end: an orchestrator-style sequence on a single Ogham.
    // (1) Tree settles. (2) begin_exit_root; tick until is_exit_complete.
    // (3) restart_entry_animations to re-mount semantics.
    // (4) Tick again; widget should be back near declared.
    let mut ogham = build(ANIMATED_TREE);
    for _ in 0..240 {
        tick(&mut ogham, 1.0 / 60.0);
    }

    // Step 2 — exit.
    assert!(ogham.begin_exit_root());
    for _ in 0..480 {
        tick(&mut ogham, 1.0 / 60.0);
        if ogham.is_exit_complete_root() {
            break;
        }
    }
    assert!(ogham.is_exit_complete_root());

    // Step 3 — restart entry. Style snaps back to initial.
    ogham.restart_entry_animations();
    {
        let g = ogham.get_ui().root.lock().expect("root lock");
        let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
        assert!(
            f.style.opacity.0 < 0.01,
            "after restart, opacity should be at initial"
        );
        assert!(!f.is_exiting());
    }

    // Step 4 — let entry play out; we settle back near declared.
    for _ in 0..240 {
        tick(&mut ogham, 1.0 / 60.0);
    }
    let g = ogham.get_ui().root.lock().expect("root lock");
    let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
    assert!(
        f.style.opacity.0 > 0.99,
        "entry should re-play and settle back at declared opacity 1; got {}",
        f.style.opacity.0
    );
}

#[test]
fn restart_entry_animations_requests_rerender() {
    // After exit, widgets without exit_style get drained from the tree.
    // restart_entry_animations should request_rerender so the next host
    // tick rebuilds the tree from the module's declarative output —
    // otherwise the tree would render with gaps until something else
    // dirtied host_state.
    let mut ogham = build(ANIMATED_TREE);
    ogham.set_screen_size(800.0, 600.0);
    ogham.get_ui_mut().layout(800.0, 600.0);
    // First update consumes the initial rerender request.
    let _ = ogham.update();
    // Settled state — no rerender pending.
    assert!(
        !ogham
            .get_runtime()
            .lock()
            .expect("runtime lock")
            .needs_rerender(),
        "test setup: runtime should be quiescent before restart"
    );

    ogham.restart_entry_animations();

    assert!(
        ogham
            .get_runtime()
            .lock()
            .expect("runtime lock")
            .needs_rerender(),
        "restart_entry_animations must dirty the rerender flag so any \
         drained widgets get re-mounted on the next host tick"
    );
}

#[test]
fn restart_entry_animation_ticks_correctly_after_mid_exit_revert() {
    // Integration version of the spring-carryover regression: exit the
    // tree partially, restart, then tick. The opacity should rise from
    // initial (0) toward declared (1), not stay at the mid-exit value.
    let mut ogham = build(ANIMATED_TREE);
    for _ in 0..240 {
        tick(&mut ogham, 1.0 / 60.0);
    }
    ogham.begin_exit_root();
    // Drive a few frames of exit so opacity is partway out.
    for _ in 0..6 {
        tick(&mut ogham, 1.0 / 60.0);
    }
    let mid_opacity = {
        let g = ogham.get_ui().root.lock().expect("root");
        let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
        f.style.opacity.0
    };
    assert!(
        mid_opacity < 0.95 && mid_opacity > 0.05,
        "test setup: exit should be mid-flight, got opacity={}",
        mid_opacity,
    );

    ogham.restart_entry_animations();
    // One tick — rendered opacity should still be near initial (0),
    // not snap back to the mid-exit value.
    tick(&mut ogham, 1.0 / 60.0);
    let opacity = {
        let g = ogham.get_ui().root.lock().expect("root");
        let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
        f.style.opacity.0
    };
    assert!(
        opacity < 0.1,
        "after restart + one tick, opacity must be near initial (0); \
         got {} (carried its mid-exit value)",
        opacity,
    );
}

#[test]
fn restart_marks_layout_dirty() {
    // Style just jumped back to initial; the host must observe a
    // layout/repaint signal so the next frame redraws.
    let mut ogham = build(ANIMATED_TREE);
    ogham.set_screen_size(800.0, 600.0);
    ogham.get_ui_mut().layout(800.0, 600.0);
    for _ in 0..240 {
        tick(&mut ogham, 1.0 / 60.0);
    }
    // Quiescent — no pending layout.
    ogham.get_ui_mut().layout(800.0, 600.0);

    ogham.restart_entry_animations();

    assert!(
        ogham.get_ui().needs_layout(),
        "restart_entry_animations should flag needs_layout"
    );

    // Smoke the test that layout is well-defined post-restart.
    ogham.get_ui_mut().layout(800.0, 600.0);
    let g = ogham.get_ui().root.lock().expect("root lock");
    let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget");
    let rect = f.layout.as_ref().expect("laid out");
    assert!((rect.width - 800.0).abs() < 0.5);
    assert!((rect.height - 600.0).abs() < 0.5);
}

// Layout APIs used above need pub access; assert via compile if needed.
#[allow(dead_code)]
fn _size_check(s: Size) -> Size { s }
