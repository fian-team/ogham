//! Phase 3 M2 — contextmenu event tests.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ogham::widget::event::Event;
use ogham::widget::flex_widget::FlexWidget;
use ogham::widget::point::Point;
use ogham::widget::rect::Rect;
use ogham::widget::{WidgetRef, UI};

#[test]
fn contextmenu_fires_on_widget_at_cursor() {
    let fired = Arc::new(AtomicBool::new(false));

    let mut f = FlexWidget::new();
    f.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    let fired_c = fired.clone();
    f.event_listeners
        .entry("contextmenu".to_string())
        .or_default()
        .push(Box::new(move |_event: &Event| {
            fired_c.store(true, Ordering::SeqCst);
        }));
    let target: WidgetRef = Arc::new(Mutex::new(f));

    let mut ui = UI::new(target);
    let handled = ui.dispatch_contextmenu(Point::new(50.0, 50.0));
    assert!(handled, "contextmenu listener should fire");
    assert!(fired.load(Ordering::SeqCst));
}

#[test]
fn contextmenu_fires_on_deepest_widget_at_cursor() {
    // Parent and child both register a contextmenu listener.
    // The deepest match wins; the outer listener does NOT also
    // fire (the dispatch is targeted, not bubbled).
    let parent_count = Arc::new(AtomicUsize::new(0));
    let child_count = Arc::new(AtomicUsize::new(0));

    let mut child = FlexWidget::new();
    child.layout = Some(Rect::new(10.0, 10.0, 50.0, 50.0));
    let cc = child_count.clone();
    child
        .event_listeners
        .entry("contextmenu".to_string())
        .or_default()
        .push(Box::new(move |_| {
            cc.fetch_add(1, Ordering::SeqCst);
        }));
    let child_ref: WidgetRef = Arc::new(Mutex::new(child));

    let mut parent = FlexWidget::new();
    parent.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    parent.children = vec![child_ref];
    let pc = parent_count.clone();
    parent
        .event_listeners
        .entry("contextmenu".to_string())
        .or_default()
        .push(Box::new(move |_| {
            pc.fetch_add(1, Ordering::SeqCst);
        }));
    let parent_ref: WidgetRef = Arc::new(Mutex::new(parent));

    let mut ui = UI::new(parent_ref);
    // Point inside the child's rect (parent-relative 20,20
    // becomes child-local 10,10 after subtracting child origin
    // 10,10).
    ui.dispatch_contextmenu(Point::new(20.0, 20.0));
    assert_eq!(child_count.load(Ordering::SeqCst), 1, "child fires");
    assert_eq!(
        parent_count.load(Ordering::SeqCst),
        0,
        "parent does not also fire"
    );
}

#[test]
fn contextmenu_returns_false_when_no_widget_at_point() {
    let f = FlexWidget::new();
    // No layout — contains_point will be false everywhere.
    let target: WidgetRef = Arc::new(Mutex::new(f));
    let mut ui = UI::new(target);
    let handled = ui.dispatch_contextmenu(Point::new(50.0, 50.0));
    assert!(!handled);
}

#[test]
fn contextmenu_suppressed_under_block_policy_portal() {
    // Phase 2.5 M0 backdrop policy contract: an open
    // OverlayModal-layer portal blocks fall-through to the
    // base tree. dispatch_contextmenu uses the same hit-test
    // walker; verifies the suppression applies to right-click
    // dispatch too, not just left-click.
    use ogham::widget::portal_layer::{CursorPreference, PortalLayer};
    use ogham::widget::PortalEntry;
    use ogham::widget::rect::Rect;

    // Base-tree widget with a contextmenu listener that
    // SHOULD NOT fire when a Block-policy portal is open.
    let base_fired = Arc::new(AtomicBool::new(false));
    let mut base = FlexWidget::new();
    base.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    let base_fired_c = base_fired.clone();
    base.event_listeners
        .entry("contextmenu".to_string())
        .or_default()
        .push(Box::new(move |_| {
            base_fired_c.store(true, Ordering::SeqCst);
        }));
    let base_ref: WidgetRef = Arc::new(Mutex::new(base));

    let mut ui = UI::new(base_ref);
    // Push an OverlayModal-layer portal entry positioned
    // away from the click point. The Block policy still
    // suppresses fall-through regardless of physical
    // overlap (the modal "swallows" all input).
    let portal_holder: WidgetRef = Arc::new(Mutex::new(FlexWidget::new()));
    ui.portal_layers.push(PortalEntry {
        widget: portal_holder,
        focus_trap: false,
        viewport_rect: Rect::new(0.0, 0.0, 1.0, 1.0),
        layer: PortalLayer::OverlayModal,
        cursor: CursorPreference::Free,
    });

    let handled = ui.dispatch_contextmenu(Point::new(50.0, 50.0));
    assert!(!handled, "contextmenu should be suppressed by Block policy");
    assert!(
        !base_fired.load(Ordering::SeqCst),
        "base contextmenu listener should not fire under modal"
    );
}
