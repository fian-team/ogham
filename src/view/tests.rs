//! Tests for the view model's reconciler ([`ChildStack`]).
//!
//! The `Strict` policy is the spec UL's `OghamPresence` defined; the 13
//! presence-orchestrator scenarios are ported onto it here (a `ChildStack`
//! that owns its views rather than borrowing from a registry). The `Layered`
//! policy and the Tenet 10 failure path get their own tests below.

use super::*;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::value::Value;
use crate::widget::Widget;
use crate::Ogham;

/// Full exit/entry animation on opacity: `begin_exit` returns true, entry
/// replays on restart.
const ANIMATED: &str = r##"
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

/// No exit anywhere: `begin_exit` returns false; reconcile swaps immediately.
const STATIC: &str = r##"
    let main = fn () {
      Flex {
        style: { width: "grow", height: "grow" },
        children: [],
      }
    };
"##;

/// Root has no `exit:`; the only exit path is the cascade to an exiting child.
const ROOT_CASCADE: &str = r##"
    let main = fn () {
      Flex {
        style: { width: "grow", height: "grow", direction: "column" },
        children: [
          Flex {
            exit: { opacity: 0 },
            style: { width: 10, height: 10, opacity: 1, transition: { opacity: "spring" } },
            children: [],
          },
        ],
      }
    };
"##;

const DT: f32 = 1.0 / 60.0;

type Key = &'static str;

fn make_instance(src: &str) -> Instance {
    let mut o = Ogham::from_source(src, RuntimeConfig::default()).expect("from_source");
    // Settle the entry animation so a subsequent begin_exit has a stable start.
    for _ in 0..240 {
        o.get_ui_mut().tick_animations(DT);
    }
    o
}

fn make_view(key: Key, src: &str) -> View<Key> {
    View::leaf(key, make_instance(src))
}

/// Declares a *required* host_state field `wind: int`. The body does not read
/// it, so the instance constructs without a provider; resolution happens at
/// tick against the scope chain.
const WIND_REQUIRED: &str = r##"
    host_state { wind: int };
    let main = fn () {
      Flex { style: { width: "grow", height: "grow" }, children: [] }
    };
"##;

/// Declares an *optional* host_state field `wind: int?`.
const WIND_OPTIONAL: &str = r##"
    host_state { wind: int? };
    let main = fn () {
      Flex { style: { width: "grow", height: "grow" }, children: [] }
    };
"##;

/// *Reads* a required host_state field at top-level execution (`width: wind`).
/// Constructed eagerly without a `wind` provider this would crash at
/// `from_source`; the deferred (`leaf_pending`) path must resolve+seed first,
/// or fail *before* constructing.
const WIND_READER: &str = r##"
    host_state { wind: int };
    let main = fn () {
      Flex { style: { width: wind, height: "grow" }, children: [] }
    };
"##;

/// Read a host_state value back off the instance's runtime (post-injection).
fn host_state(inst: &Instance, name: &str) -> Option<Value> {
    inst.with_runtime_mut(|rt| rt.get_host_state(name))
}

fn opacity_of(inst: &Instance) -> f32 {
    use crate::widget::flex_widget::FlexWidget;
    let g = inst.get_ui().root.lock().expect("root lock");
    let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget root");
    f.style.opacity.0
}

fn is_exiting(inst: &Instance) -> bool {
    use crate::widget::flex_widget::FlexWidget;
    let g = inst.get_ui().root.lock().expect("root lock");
    let f = g.downcast_ref::<FlexWidget>().expect("FlexWidget root");
    f.is_exiting()
}

/// All-ANIMATED mint.
fn mint_animated(k: &Key) -> View<Key> {
    make_view(*k, ANIMATED)
}

fn drive_until_settled(cs: &mut ChildStack<Key>, desired: &[Key]) {
    for _ in 0..480 {
        cs.reconcile(desired, mint_animated);
        cs.tick(DT);
        if !cs.is_transitioning() {
            return;
        }
    }
    panic!("transition never completed within tick budget");
}

// ── Strict: ported OghamPresence scenarios ─────────────────────────────────

#[test]
fn strict_desired_equal_current_does_nothing() {
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], mint_animated);
    cs.tick(DT);
    cs.reconcile(&["A"], mint_animated);

    assert_eq!(cs.render_order(), vec!["A"]);
    assert!(!cs.is_transitioning());
    assert_eq!(cs.phase_of(&"A"), Some(Phase::Live));
}

#[test]
fn strict_fresh_transition_begins_exit_and_freezes_pending() {
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], mint_animated);
    cs.tick(DT);

    cs.reconcile(&["B"], mint_animated);

    // Current stays visible until exit settles; pending is frozen (not shown).
    assert_eq!(cs.render_order(), vec!["A"], "pending must not mount yet");
    assert_eq!(cs.phase_of(&"A"), Some(Phase::Exiting));
    assert_eq!(cs.phase_of(&"B"), Some(Phase::Mounting));
    assert!(cs.is_transitioning());
    assert!(
        is_exiting(cs.instance_of(&"A").unwrap()),
        "current began exiting"
    );
}

#[test]
fn strict_pending_promotes_after_exit_and_restarts_entry() {
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], mint_animated);
    cs.tick(DT);

    drive_until_settled(&mut cs, &["B"]);

    assert_eq!(cs.render_order(), vec!["B"]);
    assert!(cs.phase_of(&"A").is_none(), "outgoing dropped");
    // Just-promoted: entry restarted, so opacity sits near initial (0).
    let op = opacity_of(cs.instance_of(&"B").unwrap());
    assert!(
        op < 0.1,
        "promotion restarts entry; opacity near initial, got {op}"
    );
}

#[test]
fn strict_no_exit_animation_swaps_immediately() {
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], |_| make_view("A", STATIC));
    cs.tick(DT);

    cs.reconcile(&["B"], mint_animated);

    assert_eq!(
        cs.render_order(),
        vec!["B"],
        "no exit to wait for → immediate swap"
    );
    assert!(!cs.is_transitioning());
    assert_eq!(cs.phase_of(&"B"), Some(Phase::Live));
}

#[test]
fn strict_rapid_swaps_replace_pending_without_restarting_exit() {
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], mint_animated);
    cs.tick(DT);

    // A → B, then tick a few frames so A's exit actually makes progress.
    cs.reconcile(&["B"], mint_animated);
    for _ in 0..6 {
        cs.tick(DT);
    }
    let op_before = opacity_of(cs.instance_of(&"A").unwrap());
    assert!(
        op_before < 0.95,
        "setup: A's exit progressed, got {op_before}"
    );

    // A → C mid-exit. The exit must *continue* from where it was, not restart.
    cs.reconcile(&["C"], mint_animated);
    assert_eq!(
        cs.phase_of(&"C"),
        Some(Phase::Mounting),
        "pending replaced (latest-wins)"
    );
    assert!(
        cs.phase_of(&"B").is_none(),
        "abandoned pending never mounted"
    );
    assert_eq!(
        cs.phase_of(&"A"),
        Some(Phase::Exiting),
        "current unchanged, still exiting"
    );

    cs.tick(DT);
    let op_after = opacity_of(cs.instance_of(&"A").unwrap());
    assert!(
        op_after < op_before,
        "outgoing exit must keep decreasing across the swap, not jump back up: {op_before} → {op_after}"
    );

    // C mounts; B never appeared.
    drive_until_settled(&mut cs, &["C"]);
    assert_eq!(cs.render_order(), vec!["C"]);
}

#[test]
fn strict_revert_to_current_cancels_exit_and_clears_pending() {
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], mint_animated);
    cs.tick(DT);

    cs.reconcile(&["B"], mint_animated);
    for _ in 0..6 {
        cs.tick(DT);
    }
    let mid = opacity_of(cs.instance_of(&"A").unwrap());
    assert!(
        mid < 0.95 && mid > 0.05,
        "setup: A partway through exit, got {mid}"
    );

    cs.reconcile(&["A"], mint_animated);

    assert!(!cs.is_transitioning(), "revert clears the transition");
    assert!(cs.phase_of(&"B").is_none(), "pending dropped");
    assert_eq!(cs.phase_of(&"A"), Some(Phase::Live));
    assert!(
        !is_exiting(cs.instance_of(&"A").unwrap()),
        "cancel cleared the exiting flag"
    );
}

#[test]
fn strict_desired_equal_pending_is_a_noop() {
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], mint_animated);
    cs.tick(DT);

    cs.reconcile(&["B"], mint_animated);
    let before = cs.phase_of(&"B");
    cs.reconcile(&["B"], mint_animated);

    assert_eq!(cs.phase_of(&"B"), before, "pending unchanged");
    assert_eq!(
        cs.phase_of(&"A"),
        Some(Phase::Exiting),
        "current still exiting"
    );
}

#[test]
fn strict_empty_stack_mounts_directly() {
    // Analog of OghamPresence's "missing outgoing" defensive path: with no
    // current to exit, a desired key mounts in the same reconcile.
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], mint_animated);

    assert_eq!(cs.render_order(), vec!["A"]);
    assert!(!cs.is_transitioning());
    assert_eq!(cs.phase_of(&"A"), Some(Phase::Live));
}

#[test]
fn strict_cascade_exit_completes_and_promotes() {
    // Root has no exit; only the child cascade animates. The full
    // exit→drain→drop→promote path must still complete.
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], |_| make_view("A", ROOT_CASCADE));
    cs.tick(DT);

    drive_until_settled(&mut cs, &["B"]);

    assert_eq!(cs.render_order(), vec!["B"]);
    assert!(
        cs.phase_of(&"A").is_none(),
        "cascade-exiting view dropped after its child settled"
    );
}

#[test]
fn strict_triple_state_revert_clears_pending() {
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], mint_animated);
    cs.tick(DT);

    cs.reconcile(&["B"], mint_animated); // A → B (exit, pending B)
    cs.reconcile(&["C"], mint_animated); // A → C (pending replaced)
    assert_eq!(cs.phase_of(&"C"), Some(Phase::Mounting));

    cs.reconcile(&["A"], mint_animated); // revert to A mid-exit

    assert_eq!(cs.phase_of(&"A"), Some(Phase::Live));
    assert!(
        cs.phase_of(&"C").is_none(),
        "revert clears pending even after replacement"
    );
    assert!(!is_exiting(cs.instance_of(&"A").unwrap()));
}

#[test]
fn strict_round_trip_restarts_entry_on_both_legs() {
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], mint_animated);
    cs.tick(DT);

    // Leg 1: A → B.
    drive_until_settled(&mut cs, &["B"]);
    assert_eq!(cs.render_order(), vec!["B"]);
    assert!(
        opacity_of(cs.instance_of(&"B").unwrap()) < 0.1,
        "leg 1: B at initial"
    );

    // Settle B so leg 2 starts stable.
    for _ in 0..240 {
        cs.reconcile(&["B"], mint_animated);
        cs.tick(DT);
    }
    assert!(
        opacity_of(cs.instance_of(&"B").unwrap()) > 0.99,
        "leg 1: B settled"
    );

    // Leg 2: B → A. A must replay entry even though it was settled pre-leg-1.
    drive_until_settled(&mut cs, &["A"]);
    assert_eq!(cs.render_order(), vec!["A"]);
    assert!(
        opacity_of(cs.instance_of(&"A").unwrap()) < 0.1,
        "leg 2: A re-restarts entry"
    );
}

#[test]
fn input_target_gated_during_transition() {
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], mint_animated);
    cs.tick(DT);
    assert_eq!(
        cs.input_target(),
        Some("A"),
        "steady state routes input to the live child"
    );

    cs.reconcile(&["B"], mint_animated); // begin transition
    assert_eq!(
        cs.input_target(),
        None,
        "input gated while a swap is in flight"
    );

    drive_until_settled(&mut cs, &["B"]);
    assert_eq!(
        cs.input_target(),
        Some("B"),
        "input resumes to the promoted child"
    );
}

#[test]
fn layered_current_and_input_target_resolve_topmost() {
    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["hud", "modal"], mint_animated); // modal is on top
    cs.tick(DT);

    // Both layers are live; the *top* one is "current" and owns input.
    assert_eq!(cs.current(), Some("modal"), "top-most layer is current");
    assert_eq!(
        cs.input_target(),
        Some("modal"),
        "top-most layer receives input"
    );
    assert_eq!(
        cs.render_order(),
        vec!["hud", "modal"],
        "but both still paint, bottom→top"
    );
}

#[test]
fn instance_mut_some_for_mounted_none_otherwise() {
    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["leaf", "branch"], |k| match *k {
        "branch" => View::branch("branch", StackPolicy::Layered),
        other => make_view(other, ANIMATED),
    });
    cs.tick(DT);

    assert!(
        cs.instance_mut(&"leaf").is_some(),
        "a mounted leaf yields its instance"
    );
    assert!(
        cs.instance_mut(&"branch").is_none(),
        "a branch has no instance"
    );
    assert!(
        cs.instance_mut(&"absent").is_none(),
        "an absent key yields None"
    );
    assert!(
        cs.view(&"branch").unwrap().children().is_some(),
        "branch view exposes its child stack"
    );
}

#[test]
fn current_tracks_visible_child_through_transition() {
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], mint_animated);
    cs.tick(DT);
    assert_eq!(cs.current(), Some("A"));

    // Mid-transition: input is gated, but the *visible* (paint/layout) child is
    // still the outgoing one until the swap completes.
    cs.reconcile(&["B"], mint_animated);
    assert_eq!(cs.input_target(), None, "input gated");
    assert_eq!(
        cs.current(),
        Some("A"),
        "outgoing child is still the painted/laid-out one"
    );

    drive_until_settled(&mut cs, &["B"]);
    assert_eq!(cs.current(), Some("B"));
}

#[test]
fn strict_render_list_is_only_current() {
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], mint_animated);
    cs.tick(DT);
    assert_eq!(cs.render_order(), vec!["A"]);

    cs.reconcile(&["B"], mint_animated);
    assert_eq!(
        cs.render_order(),
        vec!["A"],
        "outgoing still the only thing rendered mid-transition"
    );
}

// ── Layered: coexisting z-ordered overlays ─────────────────────────────────

#[test]
fn layered_arrivals_coexist_in_z_order() {
    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["hud", "modal"], mint_animated);
    cs.tick(DT);

    assert_eq!(
        cs.render_order(),
        vec!["hud", "modal"],
        "both layers visible, bottom→top"
    );
    assert_eq!(cs.phase_of(&"hud"), Some(Phase::Live));
    assert_eq!(cs.phase_of(&"modal"), Some(Phase::Live));
}

#[test]
fn layered_departure_exits_then_drops_leaving_siblings() {
    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["hud", "modal"], mint_animated);
    cs.tick(DT);

    // Close the modal; HUD stays.
    cs.reconcile(&["hud"], mint_animated);
    assert_eq!(cs.phase_of(&"modal"), Some(Phase::Exiting));
    assert_eq!(cs.phase_of(&"hud"), Some(Phase::Live), "sibling unaffected");

    drive_until_settled(&mut cs, &["hud"]);
    assert_eq!(cs.render_order(), vec!["hud"]);
    assert!(cs.phase_of(&"modal").is_none(), "departed layer dropped");
}

#[test]
fn layered_reorder_follows_desired_z_order() {
    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["a", "b"], mint_animated);
    cs.tick(DT);
    assert_eq!(cs.render_order(), vec!["a", "b"]);

    // Same set, swapped z-order: render order tracks the desired order.
    cs.reconcile(&["b", "a"], mint_animated);
    cs.tick(DT);
    assert_eq!(
        cs.render_order(),
        vec!["b", "a"],
        "z-order follows the desired ordering"
    );
}

#[test]
fn layered_reconcile_empty_departs_all() {
    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["a", "b"], mint_animated);
    cs.tick(DT);

    cs.reconcile(&[], mint_animated);
    assert_eq!(cs.phase_of(&"a"), Some(Phase::Exiting));
    assert_eq!(cs.phase_of(&"b"), Some(Phase::Exiting));

    drive_until_settled(&mut cs, &[]);
    assert!(cs.render_order().is_empty(), "all layers departed");
}

#[test]
fn layered_revert_mid_exit_cancels_and_unwinds() {
    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["hud", "modal"], mint_animated);
    cs.tick(DT);

    cs.reconcile(&["hud"], mint_animated); // begin closing modal
    cs.tick(DT);
    assert_eq!(cs.phase_of(&"modal"), Some(Phase::Exiting));

    cs.reconcile(&["hud", "modal"], mint_animated); // reopen before it settled
    assert_eq!(
        cs.phase_of(&"modal"),
        Some(Phase::Live),
        "revert unwinds the exit"
    );
    assert!(!is_exiting(cs.instance_of(&"modal").unwrap()));
}

// ── Tenet 10: failure caught at a boundary ──────────────────────────────────

#[test]
fn failure_with_fallback_swaps_in_fallback_and_drops_failed() {
    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["screen"], mint_animated);
    cs.tick(DT);
    cs.set_fallback(make_view("fallback", ANIMATED));

    // Dormant fallback is not visible yet.
    assert_eq!(cs.render_order(), vec!["screen"]);

    // The screen instance faults.
    cs.force_fail(&"screen");
    let escaped = cs.tick(DT);

    assert!(!escaped, "failure caught by this stack's fallback");
    assert!(
        cs.phase_of(&"screen").is_none(),
        "failed normal child dropped"
    );
    assert_eq!(
        cs.phase_of(&"fallback"),
        Some(Phase::Live),
        "fallback surfaced"
    );
    assert_eq!(cs.role_of(&"fallback"), Some(Role::Fallback));
    assert_eq!(cs.render_order(), vec!["fallback"]);
}

#[test]
fn failure_without_fallback_escapes_upward() {
    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["screen"], mint_animated);
    cs.tick(DT);

    cs.force_fail(&"screen");
    let escaped = cs.tick(DT);

    assert!(
        escaped,
        "no boundary here → failure escapes to an ancestor (root fallback)"
    );
}

#[test]
fn application_root_fallback_bounds_escaped_failure() {
    let mut app: Application<Key> = Application::new(StackPolicy::Strict);
    app.reconcile_top(&["main"], mint_animated);
    app.tick(DT);
    app.set_root_fallback(make_view("root_fallback", ANIMATED));

    // The top view faults; Application's tick must not panic or blank — the
    // root boundary catches it.
    app.top_mut().force_fail(&"main");
    app.tick(DT);

    assert_eq!(
        app.render_order(),
        vec!["root_fallback"],
        "root fallback is the default boundary"
    );
}

// ── fault isolation: a panicking instance fails itself, not siblings ────────

// ── Step 3: the model in miniature (end-to-end through `Application`) ────────
//
// These exercise the whole stack — application → branch → leaves, scope-chain
// resolution, reconcile-by-key, and Tenet-10 failure bounding — with no UL.

/// Two leaves under one branch both read a single `wind` value provided once at
/// the branch (session) scope — neither declares it locally, both resolve it up
/// the chain, and both follow when it changes. This is Tenets 5–7 end to end.
#[test]
fn proof_two_leaves_share_a_session_scoped_value() {
    let mut app: Application<Key> = Application::new(StackPolicy::Strict);

    // A "session" branch provides wind; two panels live under it and read it.
    app.reconcile_top(&["session"], |_| {
        let mut session = View::branch("session", StackPolicy::Layered);
        session.scope_mut().provide("wind", Value::Integer(42));
        session
            .children_mut()
            .unwrap()
            .reconcile(&["panelA", "panelB"], |k| {
                View::leaf_pending(*k, WIND_READER, RuntimeConfig::default())
            });
        session
    });
    app.tick(DT); // mounts the branch; both panels construct, resolving wind=42

    let read = |app: &mut Application<Key>, panel: &Key| -> Option<Value> {
        let session = app.top_mut().child_view(&"session").unwrap();
        let panels = session.children().unwrap();
        host_state(panels.instance_of(panel).unwrap(), "wind")
    };
    assert_eq!(
        read(&mut app, &"panelA"),
        Some(Value::Integer(42)),
        "panelA resolved the shared value"
    );
    assert_eq!(
        read(&mut app, &"panelB"),
        Some(Value::Integer(42)),
        "panelB resolved the same provider"
    );

    // Update the single provider; both panels follow on the next frame.
    app.top_mut()
        .child_view_mut(&"session")
        .unwrap()
        .scope_mut()
        .provide("wind", Value::Integer(7));
    app.tick(DT);
    assert_eq!(read(&mut app, &"panelA"), Some(Value::Integer(7)));
    assert_eq!(
        read(&mut app, &"panelB"),
        Some(Value::Integer(7)),
        "one provider update reaches every reader"
    );
}

/// Top-level views swap by key through the application's strict stack.
#[test]
fn proof_top_level_views_swap_by_key() {
    let mut app: Application<Key> = Application::new(StackPolicy::Strict);
    app.reconcile_top(&["game"], |_| make_view("game", ANIMATED));
    app.tick(DT);
    assert_eq!(app.render_order(), vec!["game"]);

    // Reconcile the desired top to a different screen; drive the swap.
    app.reconcile_top(&["menu"], |_| make_view("menu", ANIMATED));
    for _ in 0..480 {
        app.tick(DT);
        if !app.top_mut().is_transitioning() {
            break;
        }
    }
    assert_eq!(
        app.render_order(),
        vec!["menu"],
        "key-driven swap completed"
    );
}

/// An unresolved required provider faults at mount and is bounded by the
/// application's root fallback — never a blank surface, never a host panic.
#[test]
fn proof_unresolved_requirement_bounded_by_root_fallback() {
    let mut app: Application<Key> = Application::new(StackPolicy::Strict);
    app.set_root_fallback(make_view("root_fallback", ANIMATED));

    // A top leaf that requires `wind`, with no provider anywhere in the chain.
    app.reconcile_top(&["broken"], |_| {
        View::leaf_pending("broken", WIND_READER, RuntimeConfig::default())
    });

    // Drive: the leaf faults at mount, the failure climbs to the root boundary.
    for _ in 0..10 {
        app.tick(DT);
    }

    assert_eq!(
        app.render_order(),
        vec!["root_fallback"],
        "uncaught failure resolves at Application's default boundary (Tenet 10)"
    );
}

#[test]
fn sibling_unaffected_when_one_layer_fails() {
    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["a", "b"], mint_animated);
    cs.tick(DT);

    cs.force_fail(&"a"); // 'a' faults; 'b' is healthy
    let escaped = cs.tick(DT);

    assert!(escaped, "no fallback → escapes");
    // 'b' kept ticking and remains live regardless of 'a's fault.
    assert_eq!(
        cs.phase_of(&"b"),
        Some(Phase::Live),
        "healthy sibling unaffected"
    );
}

// ── Step 2: scope-chain resolution into instances (Tenets 5–6, 10) ──────────

#[test]
fn required_host_state_resolves_from_chain() {
    let mut shell = Scope::new();
    shell.provide("wind", Value::Integer(7));
    let chain = ScopeChain::root(&shell);

    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["w"], |_| make_view("w", WIND_REQUIRED));
    let escaped = cs.tick_chained(DT, &chain);

    assert!(!escaped, "provider in scope → no failure");
    assert_eq!(cs.phase_of(&"w"), Some(Phase::Live));
    // The resolved value was injected into the instance's host state.
    assert_eq!(
        host_state(cs.instance_of(&"w").unwrap(), "wind"),
        Some(Value::Integer(7))
    );
}

#[test]
fn missing_required_provider_fails_the_leaf() {
    // Empty chain: no `wind` provider anywhere.
    let empty = Scope::new();
    let chain = ScopeChain::root(&empty);

    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["w"], |_| make_view("w", WIND_REQUIRED));
    let escaped = cs.tick_chained(DT, &chain);

    assert!(
        escaped,
        "unresolved required provider → Failed, escapes (no boundary)"
    );
    assert_eq!(cs.phase_of(&"w"), Some(Phase::Failed));
}

#[test]
fn host_managed_leaf_ignores_scope_and_keeps_host_injected_state() {
    // A host-managed leaf declares `host_state` but is NOT scope-driven: the
    // embedder injects host_state out-of-band (a UL controller calling
    // `Instance::tick` each frame). Even on an empty chain (no `wind`
    // provider — which would FAIL the equivalent `View::leaf`, see
    // `missing_required_provider_fails_the_leaf`), it must (a) never fault on
    // the missing provider, and (b) preserve its host-injected value across
    // the view tick (animate-only path: no scope resolution, no clobber).
    let empty = Scope::new();
    let chain = ScopeChain::root(&empty);

    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["w"], |k| {
        let mut inst = make_instance(WIND_REQUIRED);
        inst.with_runtime_mut(|rt| rt.inject_host_state("wind".into(), Value::Integer(42)));
        View::leaf_host_managed(k, inst)
    });
    let escaped = cs.tick_chained(DT, &chain);

    assert!(
        !escaped,
        "host-managed leaf resolves nothing from scope → never fails on a missing provider"
    );
    assert_eq!(cs.phase_of(&"w"), Some(Phase::Live));
    assert_eq!(
        host_state(cs.instance_of(&"w").unwrap(), "wind"),
        Some(Value::Integer(42)),
        "animate-only tick must not clobber host-injected host_state with a scope resolution"
    );
}

#[test]
fn missing_required_provider_caught_by_fallback() {
    let empty = Scope::new();
    let chain = ScopeChain::root(&empty);

    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["w"], |_| make_view("w", WIND_REQUIRED));
    cs.set_fallback(make_view("fb", ANIMATED));

    let escaped = cs.tick_chained(DT, &chain);

    assert!(
        !escaped,
        "boundary catches the unresolved-requirement failure (Tenet 10)"
    );
    assert!(cs.phase_of(&"w").is_none(), "failed leaf dropped");
    assert_eq!(cs.phase_of(&"fb"), Some(Phase::Live), "fallback surfaced");
}

#[test]
fn missing_optional_provider_resolves_to_void() {
    let empty = Scope::new();
    let chain = ScopeChain::root(&empty);

    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["w"], |_| make_view("w", WIND_OPTIONAL));
    let escaped = cs.tick_chained(DT, &chain);

    assert!(
        !escaped,
        "optional requirement does not fail when absent (Tenet 6)"
    );
    assert_eq!(cs.phase_of(&"w"), Some(Phase::Live));
    assert_eq!(
        host_state(cs.instance_of(&"w").unwrap(), "wind"),
        Some(Value::Void),
        "absent optional resolves to Void, the one explicit 'absence is acceptable'"
    );
}

#[test]
fn nearest_provider_shadows_farther_one() {
    // shell provides wind=1; an intervening branch provides wind=2.
    let mut shell = Scope::new();
    shell.provide("wind", Value::Integer(1));
    let chain = ScopeChain::root(&shell);

    let mut top = ChildStack::new(StackPolicy::Layered);
    top.reconcile(&["branch"], |_| {
        let mut b = View::branch("branch", StackPolicy::Layered);
        b.scope_mut().provide("wind", Value::Integer(2));
        b.children_mut()
            .unwrap()
            .reconcile(&["leaf"], |_| make_view("leaf", WIND_REQUIRED));
        b
    });
    top.tick_chained(DT, &chain);

    let leaf = top
        .child_view(&"branch")
        .unwrap()
        .children()
        .unwrap()
        .instance_of(&"leaf")
        .unwrap();
    assert_eq!(
        host_state(leaf, "wind"),
        Some(Value::Integer(2)),
        "nearest scope shadows the shell"
    );
}

#[test]
fn resolved_value_tracks_provider_across_ticks() {
    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["w"], |_| make_view("w", WIND_REQUIRED));

    {
        let mut shell = Scope::new();
        shell.provide("wind", Value::Integer(1));
        let chain = ScopeChain::root(&shell);
        cs.tick_chained(DT, &chain);
        assert_eq!(
            host_state(cs.instance_of(&"w").unwrap(), "wind"),
            Some(Value::Integer(1))
        );
    }
    {
        // Provider's value changes; the leaf re-resolves and re-injects.
        let mut shell = Scope::new();
        shell.provide("wind", Value::Integer(2));
        let chain = ScopeChain::root(&shell);
        cs.tick_chained(DT, &chain);
        assert_eq!(
            host_state(cs.instance_of(&"w").unwrap(), "wind"),
            Some(Value::Integer(2))
        );
    }
}

// ── Mount-time resolution: deferred construction (Tenet 6) ──────────────────

#[test]
fn pending_leaf_constructs_at_mount_with_resolved_value() {
    let mut shell = Scope::new();
    shell.provide("wind", Value::Integer(100));
    let chain = ScopeChain::root(&shell);

    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["w"], |_| {
        View::leaf_pending("w", WIND_READER, RuntimeConfig::default())
    });

    // Not constructed until mount (first tick): no instance yet.
    assert!(
        cs.instance_of(&"w").is_none(),
        "deferred leaf is unbuilt before its first tick"
    );

    let escaped = cs.tick_chained(DT, &chain);

    assert!(!escaped, "provider present → constructs cleanly");
    assert!(cs.instance_of(&"w").is_some(), "constructed at mount");
    assert_eq!(
        host_state(cs.instance_of(&"w").unwrap(), "wind"),
        Some(Value::Integer(100)),
        "resolved value was seeded before the module's first execution"
    );
}

#[test]
fn pending_leaf_missing_required_fails_without_executing() {
    // The crux: WIND_READER *reads* `wind`. Eager construction with no
    // provider would crash at from_source. The deferred path must detect the
    // missing requirement and fail BEFORE constructing — no crash, no instance.
    let empty = Scope::new();
    let chain = ScopeChain::root(&empty);

    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["w"], |_| {
        View::leaf_pending("w", WIND_READER, RuntimeConfig::default())
    });

    let escaped = cs.tick_chained(DT, &chain);

    assert!(
        escaped,
        "missing required provider → Failed at mount (Tenet 10)"
    );
    assert_eq!(cs.phase_of(&"w"), Some(Phase::Failed));
    assert!(
        cs.instance_of(&"w").is_none(),
        "module never executed against an unprovided scope — it cannot crash on an unset field"
    );
}

#[test]
fn pending_leaf_mount_failure_caught_by_fallback() {
    let empty = Scope::new();
    let chain = ScopeChain::root(&empty);

    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["w"], |_| {
        View::leaf_pending("w", WIND_READER, RuntimeConfig::default())
    });
    cs.set_fallback(make_view("fb", ANIMATED));

    let escaped = cs.tick_chained(DT, &chain);

    assert!(!escaped, "mount failure caught by the boundary (Tenet 10)");
    assert!(cs.phase_of(&"w").is_none(), "failed pending leaf dropped");
    assert_eq!(cs.phase_of(&"fb"), Some(Phase::Live));
}

#[test]
fn pending_leaf_optional_missing_constructs_with_void() {
    let empty = Scope::new();
    let chain = ScopeChain::root(&empty);

    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["w"], |_| {
        View::leaf_pending("w", WIND_OPTIONAL, RuntimeConfig::default())
    });
    let escaped = cs.tick_chained(DT, &chain);

    assert!(!escaped, "absent optional does not fail (Tenet 6)");
    assert!(
        cs.instance_of(&"w").is_some(),
        "constructs with Void seeded"
    );
    assert_eq!(
        host_state(cs.instance_of(&"w").unwrap(), "wind"),
        Some(Value::Void)
    );
}

#[test]
fn strict_pending_leaf_constructs_only_at_promotion() {
    // A strict frozen pending must not construct (or resolve) until it is
    // promoted — so resolution happens against the chain at the moment it
    // actually mounts.
    let mut shell = Scope::new();
    shell.provide("wind", Value::Integer(5));
    let chain = ScopeChain::root(&shell);

    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], |_| make_view("A", ANIMATED)); // eager current
    cs.tick_chained(DT, &chain);

    cs.reconcile(&["B"], |_| {
        View::leaf_pending("B", WIND_READER, RuntimeConfig::default())
    });
    assert!(
        cs.instance_of(&"B").is_none(),
        "frozen pending is not yet constructed"
    );

    for _ in 0..480 {
        cs.tick_chained(DT, &chain);
        if !cs.is_transitioning() {
            break;
        }
    }

    assert_eq!(cs.render_order(), vec!["B"]);
    assert!(
        cs.instance_of(&"B").is_some(),
        "constructed at promotion (same frame, no blank gap)"
    );
    assert_eq!(
        host_state(cs.instance_of(&"B").unwrap(), "wind"),
        Some(Value::Integer(5))
    );
}

#[test]
fn failed_child_without_fallback_persists_and_sibling_keeps_working() {
    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["a", "b"], mint_animated);
    cs.tick(DT);

    cs.force_fail(&"a"); // 'a' faults; no boundary to catch it.
    for _ in 0..4 {
        let escaped = cs.tick(DT);
        assert!(
            escaped,
            "no boundary → the failure keeps escaping each frame"
        );
    }

    // 'a' stays Failed (not re-ticked into recovery), is excluded from render,
    // and never starves its healthy sibling.
    assert_eq!(cs.phase_of(&"a"), Some(Phase::Failed));
    assert_eq!(cs.phase_of(&"b"), Some(Phase::Live));
    assert_eq!(
        cs.render_order(),
        vec!["b"],
        "a Failed child is not painted"
    );
}

#[test]
fn dormant_pending_fallback_is_not_constructed_until_needed() {
    let empty = Scope::new();
    let chain = ScopeChain::root(&empty);

    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["screen"], |_| make_view("screen", ANIMATED));
    // A deferred fallback: it must not be built while dormant.
    cs.set_fallback(View::leaf_pending("fb", ANIMATED, RuntimeConfig::default()));

    for _ in 0..3 {
        cs.tick_chained(DT, &chain);
    }
    assert!(
        cs.instance_of(&"fb").is_none(),
        "dormant fallback is never ticked into construction"
    );
    assert_eq!(
        cs.render_order(),
        vec!["screen"],
        "dormant fallback is not painted"
    );

    // Activate it: the screen faults, the fallback surfaces and then builds.
    cs.force_fail(&"screen");
    cs.tick_chained(DT, &chain); // resolve_failures activates the fallback (Live)
    cs.tick_chained(DT, &chain); // first tick as Live constructs it
    assert_eq!(cs.phase_of(&"fb"), Some(Phase::Live));
    assert!(
        cs.instance_of(&"fb").is_some(),
        "activated fallback constructs"
    );
    assert_eq!(cs.render_order(), vec!["fb"]);
}

#[test]
fn multiple_failures_all_resolve_to_one_fallback() {
    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["a", "b"], mint_animated);
    cs.tick(DT);
    cs.set_fallback(make_view("fb", ANIMATED));

    cs.force_fail(&"a");
    cs.force_fail(&"b");
    let escaped = cs.tick(DT);

    assert!(!escaped, "boundary catches all failures");
    assert!(cs.phase_of(&"a").is_none());
    assert!(cs.phase_of(&"b").is_none());
    assert_eq!(cs.phase_of(&"fb"), Some(Phase::Live));
    assert_eq!(cs.render_order(), vec!["fb"]);
}

// ── Regression: a dormant fallback must not be mistaken for live state ──────

#[test]
fn strict_dormant_fallback_does_not_register_as_transitioning() {
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], mint_animated);
    cs.tick(DT);
    cs.set_fallback(make_view("fb", ANIMATED));

    // A dormant fallback sits in `Mounting`, but the stack is at rest.
    assert!(
        !cs.is_transitioning(),
        "dormant fallback is not a transition in flight"
    );
    assert_eq!(
        cs.render_order(),
        vec!["A"],
        "dormant fallback is not rendered"
    );
}

#[test]
fn strict_empty_desired_exits_current_but_keeps_fallback() {
    let mut cs = ChildStack::new(StackPolicy::Strict);
    cs.reconcile(&["A"], mint_animated);
    cs.tick(DT);
    cs.set_fallback(make_view("fb", ANIMATED));

    // Empty desired set: the current view exits; the root fallback survives.
    cs.reconcile(&[], mint_animated);
    assert_eq!(
        cs.phase_of(&"A"),
        Some(Phase::Exiting),
        "current begins exiting"
    );
    assert_eq!(
        cs.role_of(&"fb"),
        Some(Role::Fallback),
        "fallback untouched"
    );

    drive_until_settled(&mut cs, &[]);
    assert!(cs.phase_of(&"A").is_none(), "current dropped");
    assert_eq!(
        cs.role_of(&"fb"),
        Some(Role::Fallback),
        "fallback still present after drain"
    );
}

// ── Layered: no-exit-animation departure drops immediately ──────────────────

#[test]
fn layered_no_exit_departure_drops_immediately() {
    let mut cs = ChildStack::new(StackPolicy::Layered);
    cs.reconcile(&["hud", "toast"], |k| match *k {
        "toast" => make_view("toast", STATIC), // no exit animation
        other => make_view(other, ANIMATED),
    });
    cs.tick(DT);
    assert_eq!(cs.render_order(), vec!["hud", "toast"]);

    // Removing the no-exit layer drops it in the same reconcile (no parked
    // Exiting that would never settle).
    cs.reconcile(&["hud"], mint_animated);
    assert!(
        cs.phase_of(&"toast").is_none(),
        "no-exit departure dropped immediately"
    );
    assert_eq!(cs.render_order(), vec!["hud"]);
}

// ── Branch recursion: lifecycle cascades into nested child stacks ───────────

#[test]
fn branch_ticks_recurse_into_nested_children() {
    let mut parent = ChildStack::new(StackPolicy::Layered);
    parent.reconcile(&["branch"], |_| {
        let mut b = View::branch("branch", StackPolicy::Layered);
        b.children_mut()
            .unwrap()
            .reconcile(&["x", "y"], mint_animated);
        b
    });
    parent.tick(DT);

    let inner = parent.child_view(&"branch").unwrap().children().unwrap();
    assert_eq!(
        inner.phase_of(&"x"),
        Some(Phase::Live),
        "grandchild ticked to live"
    );
    assert_eq!(inner.phase_of(&"y"), Some(Phase::Live));
}

#[test]
fn branch_exit_cascades_to_children_then_drops() {
    let mut parent = ChildStack::new(StackPolicy::Layered);
    parent.reconcile(&["branch"], |_| {
        let mut b = View::branch("branch", StackPolicy::Layered);
        b.children_mut()
            .unwrap()
            .reconcile(&["x", "y"], mint_animated);
        b
    });
    parent.tick(DT);

    // Remove the branch: its exit must cascade to the nested children.
    parent.reconcile(&[], mint_animated);
    assert_eq!(parent.phase_of(&"branch"), Some(Phase::Exiting));
    {
        let inner = parent.child_view(&"branch").unwrap().children().unwrap();
        assert_eq!(
            inner.phase_of(&"x"),
            Some(Phase::Exiting),
            "cascade reached grandchild x"
        );
        assert_eq!(
            inner.phase_of(&"y"),
            Some(Phase::Exiting),
            "cascade reached grandchild y"
        );
    }

    drive_until_settled(&mut parent, &[]);
    assert!(
        parent.phase_of(&"branch").is_none(),
        "branch dropped once its subtree drained"
    );
}
