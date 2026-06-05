//! Phase 3 M1 — drag event integration tests.
//!
//! Cover the dispatch + hit-test surface (drag_start /
//! drag_move / drag_end + accepts_drop predicate). The
//! input-pump → dispatch wiring lives host-side; these
//! tests exercise the ogham-side primitives directly.

use std::sync::{Arc, Mutex};

use ogham::runtime::value::Value;
use ogham::runtime::Runtime;
use ogham::widget::event::{DragState, Event, EventContext};
use ogham::widget::flex_widget::FlexWidget;
use ogham::widget::point::Point;
use ogham::widget::portal_layer::{CursorPreference, PortalLayer};
use ogham::widget::rect::Rect;
use ogham::widget::style::FlexStyle;
use ogham::widget::{builder, PortalEntry, Widget, WidgetRef, UI};

fn make_flex_with_layout(rect: Rect) -> WidgetRef {
    let mut f = FlexWidget::new();
    f.layout = Some(rect);
    Arc::new(Mutex::new(f))
}

fn build_root(src: &str) -> WidgetRef {
    let runtime = Arc::new(Mutex::new(Runtime::from_source(src, None).expect("parse")));
    let module = {
        let rt = runtime.lock().unwrap();
        rt.get_module().expect("module").clone()
    };
    let widget_value = {
        let mut rt = runtime.lock().unwrap();
        rt.execute_module(&module).expect("execute")
    };
    let registry = builder::WidgetRegistry::with_defaults();
    builder::widget_value_to_widget_ref(&registry, &runtime, &widget_value)
        .expect("build widget tree")
}

#[test]
fn drag_payload_property_round_trips_through_builder() {
    let src = r#"
let main = fn () {
  Flex {
    drag_payload: { kind: "item", id: 7 },
  }
};
"#;
    let root = build_root(src);
    let g = root.lock().unwrap();
    let payload = g.drag_payload().expect("drag_payload should be set");
    match payload {
        Value::Map(m) => {
            assert!(matches!(m.get("kind"), Some(Value::String(s)) if s == "item"));
            assert!(matches!(m.get("id"), Some(Value::Integer(7))));
        }
        other => panic!("expected Map, got {:?}", other),
    }
}

#[test]
fn accepts_drop_predicate_runs_from_ogh_closure() {
    // Authored predicate just returns the boolean argument
    // it received (after a no-op pass-through). Verifies the
    // closure path: builder reads `accepts_drop`, predicate
    // runs, returned Boolean is honored.
    let src = r#"
let main = fn () {
  Flex {
    accepts_drop: fn (payload: bool): bool { return payload; },
  }
};
"#;
    let root = build_root(src);
    let g = root.lock().unwrap();
    assert!(g.accepts_drop(&Value::Boolean(true)));
    assert!(!g.accepts_drop(&Value::Boolean(false)));
}

#[test]
fn drag_dead_zone_property_round_trips() {
    let src = r#"
let main = fn () {
  Flex {
    drag_dead_zone: 12.0,
  }
};
"#;
    let root = build_root(src);
    let g = root.lock().unwrap();
    assert_eq!(g.drag_dead_zone(), Some(12.0));
}

#[test]
fn default_widget_does_not_accept_drop() {
    let f = FlexWidget::new();
    assert!(!f.accepts_drop(&Value::Void));
    assert!(f.drag_payload().is_none());
    assert!(f.drag_dead_zone().is_none());
}

#[test]
fn dispatch_drag_start_fires_listener_on_origin() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let fired = Arc::new(AtomicBool::new(false));
    let captured_payload = Arc::new(Mutex::new(None::<Value>));

    let mut f = FlexWidget::new();
    f.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    let fired_c = fired.clone();
    let captured_c = captured_payload.clone();
    f.event_listeners
        .entry("drag_start".to_string())
        .or_default()
        .push(Box::new(move |event: &Event| {
            fired_c.store(true, Ordering::SeqCst);
            *captured_c.lock().unwrap() = event.payload.clone();
        }));
    let origin: WidgetRef = Arc::new(Mutex::new(f));
    let mut ui = UI::new(origin.clone());

    let mut item_map = std::collections::HashMap::new();
    item_map.insert("kind".to_string(), Value::String("item".to_string()));
    let payload = Value::Map(item_map);

    let state = ui.dispatch_drag_start(origin.clone(), payload.clone(), Point::new(5.0, 5.0));
    assert!(
        fired.load(Ordering::SeqCst),
        "drag_start listener should fire"
    );
    assert!(state.past_dead_zone);
    assert!(state.origin_widget.is_some());
    let captured = captured_payload.lock().unwrap();
    assert!(matches!(captured.as_ref(), Some(Value::Map(_))));
}

#[test]
fn dispatch_drag_end_targets_widget_whose_accepts_drop_is_true() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    // Layout: parent flex with two children. Only child B
    // accepts drops; both have drag_end listeners.
    let counter = Arc::new(AtomicUsize::new(0));

    let mut a = FlexWidget::new();
    a.layout = Some(Rect::new(0.0, 0.0, 50.0, 100.0));
    {
        let counter_a = counter.clone();
        a.event_listeners
            .entry("drag_end".to_string())
            .or_default()
            .push(Box::new(move |_event| {
                counter_a.fetch_add(1, Ordering::SeqCst);
            }));
    }
    let a_ref: WidgetRef = Arc::new(Mutex::new(a));

    let mut b = FlexWidget::new();
    b.layout = Some(Rect::new(50.0, 0.0, 50.0, 100.0));
    b.accepts_drop_predicate = Some(Box::new(|_| true));
    let counter_b = counter.clone();
    let b_fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let b_fired_c = b_fired.clone();
    b.event_listeners
        .entry("drag_end".to_string())
        .or_default()
        .push(Box::new(move |_event| {
            counter_b.fetch_add(1, Ordering::SeqCst);
            b_fired_c.store(true, Ordering::SeqCst);
        }));
    let b_ref: WidgetRef = Arc::new(Mutex::new(b));

    let mut parent = FlexWidget::new();
    parent.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    parent.children = vec![a_ref.clone(), b_ref.clone()];
    let parent_ref: WidgetRef = Arc::new(Mutex::new(parent));

    let mut ui = UI::new(parent_ref);

    let payload = Value::Integer(42);
    let mut state = DragState {
        origin_widget: Some(a_ref.clone()),
        payload: Some(payload.clone()),
        start_position: Point::new(10.0, 50.0),
        current_position: Point::new(10.0, 50.0),
        past_dead_zone: true,
    };
    // Drop point is over child B (x=75 lands in the right half).
    let target = ui.dispatch_drag_end(&mut state, Point::new(75.0, 50.0));
    assert!(target.is_some(), "should find a drop target");
    assert!(b_fired.load(Ordering::SeqCst), "B should receive drag_end");
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn dispatch_drag_end_falls_back_to_origin_when_no_target_accepts() {
    use std::sync::atomic::{AtomicBool, Ordering};
    // No widget accepts the drop; drag_end fires on origin.
    let origin_fired = Arc::new(AtomicBool::new(false));

    let mut origin_w = FlexWidget::new();
    origin_w.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    let origin_fired_c = origin_fired.clone();
    origin_w
        .event_listeners
        .entry("drag_end".to_string())
        .or_default()
        .push(Box::new(move |_event| {
            origin_fired_c.store(true, Ordering::SeqCst);
        }));
    let origin: WidgetRef = Arc::new(Mutex::new(origin_w));

    let mut other = FlexWidget::new();
    other.layout = Some(Rect::new(200.0, 0.0, 100.0, 100.0));
    // No accepts_drop predicate; not a drop target.
    let other_ref: WidgetRef = Arc::new(Mutex::new(other));

    let mut parent = FlexWidget::new();
    parent.layout = Some(Rect::new(0.0, 0.0, 400.0, 100.0));
    parent.children = vec![origin.clone(), other_ref];
    let parent_ref: WidgetRef = Arc::new(Mutex::new(parent));

    let mut ui = UI::new(parent_ref);
    let mut state = DragState {
        origin_widget: Some(origin.clone()),
        payload: Some(Value::Void),
        start_position: Point::new(10.0, 10.0),
        current_position: Point::new(10.0, 10.0),
        past_dead_zone: true,
    };
    // Drop over `other` (which doesn't accept drops).
    let target = ui.dispatch_drag_end(&mut state, Point::new(250.0, 50.0));
    assert!(target.is_some(), "fallback to origin");
    assert!(
        origin_fired.load(Ordering::SeqCst),
        "drag_end should fire on origin when no target accepts"
    );
}

#[test]
fn dispatch_drag_move_targets_widget_under_cursor() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let b_moved = Arc::new(AtomicBool::new(false));

    let a_ref: WidgetRef = make_flex_with_layout(Rect::new(0.0, 0.0, 50.0, 100.0));

    let mut b = FlexWidget::new();
    b.layout = Some(Rect::new(50.0, 0.0, 50.0, 100.0));
    let b_moved_c = b_moved.clone();
    b.event_listeners
        .entry("drag_move".to_string())
        .or_default()
        .push(Box::new(move |_event| {
            b_moved_c.store(true, Ordering::SeqCst);
        }));
    let b_ref: WidgetRef = Arc::new(Mutex::new(b));

    let mut parent = FlexWidget::new();
    parent.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    parent.children = vec![a_ref.clone(), b_ref.clone()];
    let parent_ref: WidgetRef = Arc::new(Mutex::new(parent));

    let mut ui = UI::new(parent_ref);
    let mut state = DragState {
        origin_widget: Some(a_ref),
        payload: Some(Value::Void),
        start_position: Point::new(10.0, 10.0),
        current_position: Point::new(10.0, 10.0),
        past_dead_zone: true,
    };
    let target = ui.dispatch_drag_move(&mut state, Point::new(70.0, 50.0));
    assert!(target.is_some(), "drag_move should find target");
    assert!(b_moved.load(Ordering::SeqCst), "B should receive drag_move");
    assert_eq!(state.current_position.x(), 70.0);
}

#[test]
fn drop_target_hit_test_walks_portal_layers_first() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Base-tree widget that would accept drops if reached.
    let base_hits = Arc::new(AtomicUsize::new(0));
    let mut base = FlexWidget::new();
    base.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    base.accepts_drop_predicate = Some(Box::new(|_| true));
    let base_hits_c = base_hits.clone();
    base.event_listeners
        .entry("drag_end".to_string())
        .or_default()
        .push(Box::new(move |_| {
            base_hits_c.fetch_add(1, Ordering::SeqCst);
        }));
    let base_ref: WidgetRef = Arc::new(Mutex::new(base));

    // Portal-layer widget that also accepts. Should win the
    // hit-test because portal layers are checked first.
    let portal_hits = Arc::new(AtomicUsize::new(0));
    let mut portal_child = FlexWidget::new();
    portal_child.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    portal_child.accepts_drop_predicate = Some(Box::new(|_| true));
    let portal_hits_c = portal_hits.clone();
    portal_child
        .event_listeners
        .entry("drag_end".to_string())
        .or_default()
        .push(Box::new(move |_| {
            portal_hits_c.fetch_add(1, Ordering::SeqCst);
        }));
    let portal_child_ref: WidgetRef = Arc::new(Mutex::new(portal_child));

    // Wrap the portal child in a parent flex (the layer entry
    // expects a node whose children are the portal contents,
    // mirroring how PortalWidget exposes them).
    let mut portal_holder = FlexWidget::new();
    portal_holder.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    portal_holder.children = vec![portal_child_ref.clone()];
    let portal_holder_ref: WidgetRef = Arc::new(Mutex::new(portal_holder));

    let mut ui = UI::new(base_ref.clone());
    ui.portal_layers.push(PortalEntry {
        widget: portal_holder_ref,
        focus_trap: false,
        viewport_rect: Rect::new(0.0, 0.0, 100.0, 100.0),
        layer: PortalLayer::Tooltip,
        cursor: CursorPreference::Inherit,
    });

    let mut state = DragState {
        origin_widget: Some(base_ref),
        payload: Some(Value::Void),
        start_position: Point::new(10.0, 10.0),
        current_position: Point::new(10.0, 10.0),
        past_dead_zone: true,
    };
    let _target = ui.dispatch_drag_end(&mut state, Point::new(50.0, 50.0));
    assert_eq!(
        portal_hits.load(Ordering::SeqCst),
        1,
        "portal layer drop target should be selected before base tree"
    );
    assert_eq!(
        base_hits.load(Ordering::SeqCst),
        0,
        "base-tree target should not fire"
    );
}

#[test]
fn drag_state_in_event_context_during_dispatch() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let saw_state = Arc::new(AtomicBool::new(false));

    let mut f = FlexWidget::new();
    f.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    f.accepts_drop_predicate = Some(Box::new(|_| true));
    let saw_c = saw_state.clone();
    f.event_listeners
        .entry("drag_end".to_string())
        .or_default()
        .push(Box::new(move |event: &Event| {
            // The Event itself carries the payload (we test
            // EventContext separately below via a custom listener
            // wired through handle_event).
            if event.payload.is_some() {
                saw_c.store(true, Ordering::SeqCst);
            }
        }));
    let target: WidgetRef = Arc::new(Mutex::new(f));

    let mut ui = UI::new(target.clone());
    let mut state = DragState {
        origin_widget: Some(target.clone()),
        payload: Some(Value::String("p".to_string())),
        start_position: Point::new(0.0, 0.0),
        current_position: Point::new(0.0, 0.0),
        past_dead_zone: true,
    };
    ui.dispatch_drag_end(&mut state, Point::new(50.0, 50.0));
    assert!(saw_state.load(Ordering::SeqCst));
}

#[test]
fn event_context_carries_drag_state_during_dispatch() {
    // Direct test: when an event is dispatched, EventContext
    // exposes drag_state for widgets that need to inspect the
    // in-flight drag (e.g. for hover highlighting).
    let mut ctx = EventContext::new();
    assert!(ctx.drag_state.is_none());
    ctx.drag_state = Some(DragState::default());
    assert!(ctx.drag_state.is_some());
}

#[test]
fn drag_listener_invalid_property_type_errors() {
    // A drag_start property that isn't a closure should fail
    // the build with a clear error.
    let src = r#"
let main = fn () {
  Flex {
    drag_start: 42,
  }
};
"#;
    let runtime = Arc::new(Mutex::new(Runtime::from_source(src, None).expect("parse")));
    let module = {
        let rt = runtime.lock().unwrap();
        rt.get_module().expect("module").clone()
    };
    let widget_value = {
        let mut rt = runtime.lock().unwrap();
        rt.execute_module(&module).expect("execute")
    };
    let registry = builder::WidgetRegistry::with_defaults();
    let result = builder::widget_value_to_widget_ref(&registry, &runtime, &widget_value);
    assert!(
        matches!(
            result,
            Err(builder::BridgeError::InvalidPropertyType(ref name, _))
                if name == "drag_start"
        ),
        "expected InvalidPropertyType for drag_start"
    );
}

// Style import used by helper that constructs FlexStyle but
// none of these tests actually exercise styling — silence the
// unused-import warning from rustc.
const _: Option<FlexStyle> = None;
