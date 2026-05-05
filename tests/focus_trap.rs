//! Phase 2 M4 — focus_trap + has_input_blocking_portal tests.
//!
//! Tests construct UI + portal_layer state directly (no Skia
//! surface needed) and exercise sync_focus_stack /
//! try_set_focus / has_input_blocking_portal.

use std::sync::{Arc, Mutex};

use ogham::widget::flex_widget::FlexWidget;
use ogham::widget::portal_widget::PortalWidget;
use ogham::widget::{
    FocusRestoration, PortalEntry, PortalInfo, Widget, WidgetRef, UI,
};

fn make_portal(focus_trap: bool, children: Vec<WidgetRef>) -> WidgetRef {
    let mut p = PortalWidget::new();
    p.open = true;
    p.focus_trap = focus_trap;
    p.inner.children = children;
    Arc::new(Mutex::new(p))
}

fn make_flex() -> WidgetRef {
    Arc::new(Mutex::new(FlexWidget::new()))
}

fn entry(portal: &WidgetRef) -> PortalEntry {
    let info = portal
        .lock()
        .unwrap()
        .as_portal()
        .expect("must be portal");
    PortalEntry {
        widget: portal.clone(),
        parent_rect: ogham::widget::rect::Rect::zero(),
        focus_trap: info.focus_trap,
    }
}

#[test]
fn has_input_blocking_portal_false_when_none_open() {
    let root = make_flex();
    let ui = UI::new(root);
    assert!(!ui.has_input_blocking_portal());
}

#[test]
fn has_input_blocking_portal_false_for_non_focus_trap_portal() {
    let root = make_flex();
    let mut ui = UI::new(root);
    let tooltip = make_portal(false, vec![]);
    ui.portal_layer.push(entry(&tooltip));
    assert!(
        !ui.has_input_blocking_portal(),
        "tooltip-style portal should not block input"
    );
}

#[test]
fn has_input_blocking_portal_true_for_focus_trap_portal() {
    let root = make_flex();
    let mut ui = UI::new(root);
    let modal = make_portal(true, vec![]);
    ui.portal_layer.push(entry(&modal));
    assert!(ui.has_input_blocking_portal());
}

#[test]
fn sync_focus_stack_pushes_on_first_appearance() {
    let root = make_flex();
    let mut ui = UI::new(root);
    let initial = make_flex();
    ui.try_set_focus(initial.clone());

    let modal = make_portal(true, vec![make_flex()]);
    ui.portal_layer.push(entry(&modal));
    ui.sync_focus_stack();

    assert_eq!(ui.focus_stack.len(), 1);
    let entry = &ui.focus_stack[0];
    assert!(Arc::ptr_eq(&entry.portal, &modal));
    let prev = entry.previous_focus.as_ref().expect("previous focus saved");
    assert!(Arc::ptr_eq(prev, &initial));
}

#[test]
fn sync_focus_stack_pops_and_restores_on_close() {
    let root = make_flex();
    let mut ui = UI::new(root);
    let initial = make_flex();
    ui.try_set_focus(initial.clone());

    let modal = make_portal(true, vec![make_flex()]);
    ui.portal_layer.push(entry(&modal));
    ui.sync_focus_stack();
    assert_eq!(ui.focus_stack.len(), 1);

    // Modal closes: portal_layer cleared (or just doesn't
    // include it next frame).
    ui.portal_layer.clear();
    ui.sync_focus_stack();
    assert!(ui.focus_stack.is_empty(), "stack should drain on close");
    let restored = ui.get_focused().expect("focus restored");
    assert!(
        Arc::ptr_eq(restored, &initial),
        "focus should restore to the pre-modal target"
    );
}

#[test]
fn try_set_focus_rejects_target_outside_trapped_subtree() {
    let root = make_flex();
    let mut ui = UI::new(root.clone());
    let modal_child = make_flex();
    let modal = make_portal(true, vec![modal_child.clone()]);
    ui.portal_layer.push(entry(&modal));
    ui.sync_focus_stack();

    // Target inside the modal: accepted.
    assert!(ui.try_set_focus(modal_child.clone()));
    let focused = ui.get_focused().unwrap();
    assert!(Arc::ptr_eq(focused, &modal_child));

    // Target outside the modal (sibling Flex): rejected.
    let outside = make_flex();
    assert!(!ui.try_set_focus(outside.clone()));
    // Focus should still be on modal_child (unchanged).
    let focused = ui.get_focused().unwrap();
    assert!(Arc::ptr_eq(focused, &modal_child));
}

#[test]
fn nested_focus_trap_portals_stack_correctly() {
    let root = make_flex();
    let mut ui = UI::new(root);

    // Outer modal's child can hold focus.
    let outer_child = make_flex();
    let outer = make_portal(true, vec![outer_child.clone()]);
    ui.portal_layer.push(entry(&outer));
    ui.sync_focus_stack();

    ui.try_set_focus(outer_child.clone());

    // Inner modal opens — push to portal_layer in order.
    let inner_child = make_flex();
    let inner = make_portal(true, vec![inner_child.clone()]);
    ui.portal_layer.push(entry(&inner));
    ui.sync_focus_stack();

    assert_eq!(ui.focus_stack.len(), 2);

    // Focus moves into inner.
    assert!(ui.try_set_focus(inner_child.clone()));

    // Try to focus outer_child — should be rejected (outside
    // inner's subtree).
    assert!(!ui.try_set_focus(outer_child.clone()));

    // Inner closes. Focus restores to outer_child (the
    // previous_focus saved when inner mounted).
    ui.portal_layer.retain(|e| !Arc::ptr_eq(&e.widget, &inner));
    ui.sync_focus_stack();
    assert_eq!(ui.focus_stack.len(), 1);
    let focused = ui.get_focused().unwrap();
    assert!(
        Arc::ptr_eq(focused, &outer_child),
        "popping inner trap restores to its previous_focus"
    );
}

#[test]
fn clear_lifecycle_state_resets_all_portal_state() {
    // Hot-reload safety: clearing should leave UI in a clean
    // state with no stale focus restoration.
    let root = make_flex();
    let mut ui = UI::new(root);
    let modal = make_portal(true, vec![make_flex()]);
    ui.portal_layer.push(entry(&modal));
    ui.sync_focus_stack();
    ui.try_set_focus(modal.clone());

    ui.clear_lifecycle_state();
    assert!(ui.portal_layer.is_empty());
    assert!(ui.focus_stack.is_empty());
    assert!(ui.get_focused().is_none());
}

#[test]
fn portal_info_carries_focus_trap_flag() {
    // PortalInfo is the signal Renderer/UI use to decide trap
    // behavior; verify the round-trip.
    let p = make_portal(true, vec![]);
    let info: PortalInfo = p.lock().unwrap().as_portal().unwrap();
    assert!(info.focus_trap);
}
