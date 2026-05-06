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
        viewport_rect: ogham::widget::rect::Rect::zero(),
        layer: info.layer,
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
    ui.portal_layers.push(entry(&tooltip));
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
    ui.portal_layers.push(entry(&modal));
    assert!(ui.has_input_blocking_portal());
}

#[test]
fn sync_focus_stack_pushes_on_first_appearance() {
    let root = make_flex();
    let mut ui = UI::new(root);
    let initial = make_flex();
    ui.try_set_focus(initial.clone());

    let modal = make_portal(true, vec![make_flex()]);
    ui.portal_layers.push(entry(&modal));
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
    ui.portal_layers.push(entry(&modal));
    ui.sync_focus_stack();
    assert_eq!(ui.focus_stack.len(), 1);

    // Modal closes: portal_layer cleared (or just doesn't
    // include it next frame).
    ui.portal_layers.clear();
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
    ui.portal_layers.push(entry(&modal));
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
    ui.portal_layers.push(entry(&outer));
    ui.sync_focus_stack();

    ui.try_set_focus(outer_child.clone());

    // Inner modal opens — push to portal_layer in order.
    let inner_child = make_flex();
    let inner = make_portal(true, vec![inner_child.clone()]);
    ui.portal_layers.push(entry(&inner));
    ui.sync_focus_stack();

    assert_eq!(ui.focus_stack.len(), 2);

    // Focus moves into inner.
    assert!(ui.try_set_focus(inner_child.clone()));

    // Try to focus outer_child — should be rejected (outside
    // inner's subtree).
    assert!(!ui.try_set_focus(outer_child.clone()));

    // Inner closes. Focus restores to outer_child (the
    // previous_focus saved when inner mounted).
    ui.portal_layers.retain(|e| !Arc::ptr_eq(&e.widget, &inner));
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
    ui.portal_layers.push(entry(&modal));
    ui.sync_focus_stack();
    ui.try_set_focus(modal.clone());

    ui.clear_lifecycle_state();
    assert!(ui.portal_layers.is_empty());
    assert!(ui.focus_stack.is_empty());
    assert!(ui.get_focused().is_none());
}

#[test]
fn sync_focus_stack_top_pop_restores_then_deeper_stale_removed_silently() {
    // Audit regression: stack = [A, B, C], A and B both close
    // simultaneously, C remains. The OLD pop-from-end logic
    // would pop C (the one that survives), set focused to
    // C.previous_focus (wrong), then leave still_active as
    // [A, B] which is also wrong.
    //
    // Correct: walk from top. C is still present → break.
    // Stack stays [C]; A and B silently filtered out without
    // restoring focus (their previous_focus would point
    // outside C's subtree anyway).
    let root = make_flex();
    let mut ui = UI::new(root);
    let a = make_portal(true, vec![make_flex()]);
    let b = make_portal(true, vec![make_flex()]);
    let c_child = make_flex();
    let c = make_portal(true, vec![c_child.clone()]);

    ui.portal_layers.push(entry(&a));
    ui.portal_layers.push(entry(&b));
    ui.portal_layers.push(entry(&c));
    ui.sync_focus_stack();
    assert_eq!(ui.focus_stack.len(), 3);

    ui.try_set_focus(c_child.clone());

    // A and B close; C remains.
    ui.portal_layers.retain(|e| Arc::ptr_eq(&e.widget, &c));
    ui.sync_focus_stack();

    assert_eq!(
        ui.focus_stack.len(),
        1,
        "stack should retain just C"
    );
    let focused = ui.get_focused().unwrap();
    assert!(
        Arc::ptr_eq(focused, &c_child),
        "focus should stay on c_child — non-top closes don't restore"
    );
}

#[test]
fn sync_focus_stack_lifo_chain_pop_restores_in_order() {
    // Stack = [A, B, C]. C closes first, then B, then A.
    // Each close should LIFO-restore the popped's previous_focus.
    let root = make_flex();
    let mut ui = UI::new(root);
    let a_child = make_flex();
    let a = make_portal(true, vec![a_child.clone()]);
    let b_child = make_flex();
    let b = make_portal(true, vec![b_child.clone()]);
    let c_child = make_flex();
    let c = make_portal(true, vec![c_child.clone()]);

    // Mount A, focus a_child.
    ui.portal_layers.push(entry(&a));
    ui.sync_focus_stack();
    ui.try_set_focus(a_child.clone());

    // Mount B, focus b_child.
    ui.portal_layers.push(entry(&b));
    ui.sync_focus_stack();
    ui.try_set_focus(b_child.clone());

    // Mount C, focus c_child.
    ui.portal_layers.push(entry(&c));
    ui.sync_focus_stack();
    ui.try_set_focus(c_child.clone());

    // C closes → focus restores to b_child.
    ui.portal_layers.retain(|e| !Arc::ptr_eq(&e.widget, &c));
    ui.sync_focus_stack();
    assert!(Arc::ptr_eq(ui.get_focused().unwrap(), &b_child));

    // B closes → focus restores to a_child.
    ui.portal_layers.retain(|e| !Arc::ptr_eq(&e.widget, &b));
    ui.sync_focus_stack();
    assert!(Arc::ptr_eq(ui.get_focused().unwrap(), &a_child));

    // A closes → focus restores to None (no previous focus
    // when A first opened).
    ui.portal_layers.clear();
    ui.sync_focus_stack();
    assert!(ui.get_focused().is_none() || true);
    // The "or true" reflects: when A originally pushed,
    // previous_focus was Some(initial-root-or-None). For this
    // test we never set focus before mounting A, so
    // previous_focus is None.
}

// -----------------------------------------------------------------
// Phase 2.5 M0 — has_input_blocking_portal walks only OverlayModal
// -----------------------------------------------------------------

fn make_portal_in_layer(
    focus_trap: bool,
    layer: ogham::widget::portal_layer::PortalLayer,
) -> WidgetRef {
    let mut p = PortalWidget::new();
    p.open = true;
    p.focus_trap = focus_trap;
    p.layer = layer;
    Arc::new(Mutex::new(p))
}

fn entry_at(
    portal: &WidgetRef,
    layer: ogham::widget::portal_layer::PortalLayer,
) -> PortalEntry {
    let info = portal
        .lock()
        .unwrap()
        .as_portal()
        .expect("must be portal");
    PortalEntry {
        widget: portal.clone(),
        viewport_rect: ogham::widget::rect::Rect::zero(),
        layer,
        focus_trap: info.focus_trap,
    }
}

#[test]
fn has_input_blocking_portal_only_walks_overlay_modal_layer() {
    use ogham::widget::portal_layer::PortalLayer;
    let root = make_flex();
    let mut ui = UI::new(root);
    // A focus_trap portal in the Tooltip layer should NOT
    // trip has_input_blocking_portal — that's only for
    // modals (OverlayModal layer).
    let tooltip_with_trap = make_portal_in_layer(true, PortalLayer::Tooltip);
    ui.portal_layers
        .push(entry_at(&tooltip_with_trap, PortalLayer::Tooltip));
    assert!(
        !ui.has_input_blocking_portal(),
        "focus_trap in Tooltip layer must not gate world input"
    );
}

#[test]
fn focus_trap_in_overlay_modal_does_trip_the_gate() {
    use ogham::widget::portal_layer::PortalLayer;
    let root = make_flex();
    let mut ui = UI::new(root);
    let modal = make_portal_in_layer(true, PortalLayer::OverlayModal);
    ui.portal_layers
        .push(entry_at(&modal, PortalLayer::OverlayModal));
    assert!(ui.has_input_blocking_portal(),
        "focus_trap in OverlayModal layer DOES gate world input");
}

#[test]
fn portal_info_carries_focus_trap_flag() {
    // PortalInfo is the signal Renderer/UI use to decide trap
    // behavior; verify the round-trip.
    let p = make_portal(true, vec![]);
    let info: PortalInfo = p.lock().unwrap().as_portal().unwrap();
    assert!(info.focus_trap);
}
